use super::schema::verfploeter::{
    Ack, Client, ClientList, Empty, Metadata, ScheduleTask, Task, TaskId, TaskResult,
};
use super::schema::verfploeter_grpc::{self, Verfploeter};
use futures::sync::mpsc::{channel, Sender};
use futures::*;
use grpcio::ChannelCredentialsBuilder;
use grpcio::ServerCredentialsBuilder;
use grpcio::{
    ChannelBuilder, Environment, RpcContext, Server as GrpcServer, ServerBuilder,
    ServerStreamingSink, UnarySink,
};
use protobuf::RepeatedField;
use std::collections::HashMap;
use std::ops::AddAssign;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::timer::Interval;

pub struct Server {
    grpc_server: GrpcServer,
}

pub struct ServerConfig {
    pub certificate: Option<Vec<u8>>,
    pub private_key: Option<Vec<u8>>,
    pub port: u16,
}

impl Server {
    pub fn new(config: &ServerConfig) -> Server {
        let connection_manager = Arc::new(ConnectionManager::new());
        let s = VerfploeterService {
            connection_manager,
            subscription_list: Arc::new(RwLock::new(HashMap::new())),
            current_task_id: Arc::new(Mutex::new(0)),
            runtime: Arc::new(Runtime::new().unwrap()),
        };

        if config.certificate.is_some() && config.private_key.is_some() {
            Server::create_secure_server(s, config)
        } else {
            Server::create_insecure_server(s, config)
        }
    }

    fn create_server_builder(s: VerfploeterService) -> ServerBuilder {
        let env = Arc::new(Environment::new(10));

        let channel_args = ChannelBuilder::new(Arc::clone(&env))
            .max_receive_message_len(100 * 1024 * 1024)
            .max_send_message_len(100 * 1024 * 1024)
            .build_args();

        let service = verfploeter_grpc::create_verfploeter(s);

        ServerBuilder::new(env)
            .channel_args(channel_args)
            .register_service(service)
    }

    fn create_secure_server(s: VerfploeterService, config: &ServerConfig) -> Server {
        info!("creating a secure server (SSL enabled)");
        // Setup credentials
        let credentials = ServerCredentialsBuilder::new()
            .add_cert(
                config.certificate.clone().unwrap(),
                config.private_key.clone().unwrap(),
            )
            .build();
        Server {
            grpc_server: Server::create_server_builder(s)
                .bind_secure("0.0.0.0", config.port, credentials)
                .build()
                .unwrap(),
        }
    }

    fn create_insecure_server(s: VerfploeterService, config: &ServerConfig) -> Server {
        info!("creating an insecure server (SSL disabled)");
        Server {
            grpc_server: Server::create_server_builder(s)
                .bind("0.0.0.0", config.port)
                .build()
                .unwrap(),
        }
    }

    pub fn start(&mut self) {
        self.grpc_server.start();

        for &(ref host, port) in self.grpc_server.bind_addrs() {
            info!("Listening on {}:{}", host, port);
        }
    }
}

#[derive(Debug)]
pub struct Connection {
    pub channel: Sender<Task>,
    pub metadata: Metadata,
}

#[derive(Clone)]
struct VerfploeterService {
    connection_manager: Arc<ConnectionManager>,
    subscription_list: Arc<RwLock<HashMap<u32, Vec<Sender<TaskResult>>>>>,
    current_task_id: Arc<Mutex<u32>>, // todo: replace this with AtomicU32 when it stabilizes
    runtime: Arc<Runtime>,
}

impl VerfploeterService {
    fn register_subscriber(&mut self, task_id: u32, tx: Sender<TaskResult>) {
        debug!("registering subscriber for task id {}", task_id);
        let mut list = self.subscription_list.write().unwrap();
        if let Some(subscribers) = list.get_mut(&task_id) {
            subscribers.push(tx);
        } else {
            list.insert(task_id, vec![tx]);
        }
    }

    fn get_subscribers(&self, task_id: u32) -> Option<Vec<Sender<TaskResult>>> {
        let list = self.subscription_list.read().unwrap();
        if let Some(subscribers) = list.get(&task_id) {
            debug!(
                "returning {} subscribers for task {}",
                subscribers.len(),
                task_id
            );
            return Some(subscribers.to_vec());
        }
        None
    }

    fn disconnect_subscribers(&self, task_id: u32) {
        let mut list = self.subscription_list.write().unwrap();
        if let Some(subscribers) = list.get(&task_id) {
            debug!(
                "disconnecting {} subscribers for task {}",
                subscribers.len(),
                task_id
            );
            subscribers.iter().for_each(drop);
            list.remove(&task_id);
        }
    }
}

impl Verfploeter for VerfploeterService {
    fn connect(&mut self, _ctx: RpcContext, metadata: Metadata, sink: ServerStreamingSink<Task>) {
        let (tx, rx) = channel(1);

        let connection_manager = self.connection_manager.clone();
        let connection_id = connection_manager.generate_connection_id();
        let hostname = metadata.get_hostname().to_string();
        connection_manager.register_connection(
            connection_id,
            Connection {
                metadata,
                channel: tx.clone(),
            },
        );

        // Forward all tasks from the channel to the sink, and unregister from the connection
        // manager on error or completion.
        let f = rx
            .map(|item| (item, grpcio::WriteFlags::default()))
            .forward(sink.sink_map_err({
                let hostname = hostname.clone();
                move |e| {
                    debug!("exiting task forwarder ({}), with error {}", hostname, e);
                }
            }))
            .map({
                let cm = self.connection_manager.clone();
                let hostname = hostname.clone();
                move |_| {
                    cm.unregister_connection(connection_id);
                    debug!("exiting task forwarder ({})", hostname);
                }
            })
            .map_err({
                let cm = self.connection_manager.clone();
                let hostname = hostname.clone();
                move |_| {
                    cm.unregister_connection(connection_id);
                    debug!("exiting task forwarder ({}), with error", hostname);
                }
            });
        self.runtime.executor().spawn(f);

        // Send keepalives
        self.runtime.executor().spawn(
            Interval::new_interval(Duration::from_secs(5))
                .map_err(|_| ())
                .map(|_| {
                    let mut t = Task::new();
                    t.set_empty(Empty::new());
                    t
                })
                .forward(tx.clone().sink_map_err(|_| ()))
                .map_err(|_| ())
                .map(|_| ()),
        );
    }

    fn do_task(&mut self, ctx: RpcContext, mut req: ScheduleTask, sink: UnarySink<Ack>) {
        debug!("received do_task request");
        let mut ack = Ack::new();
        ack.set_success(false);

        // Handle a ping task
        if req.has_ping() {
            let tx=
            // Get a connection to the client, either by hostname (if provided) or by index
            if !req.get_client().get_metadata().hostname.is_empty() {
                self
                    .connection_manager
                    .get_client_tx_by_hostname(&req.get_client().get_metadata().hostname)
            } else {
                self
                    .connection_manager
                    .get_client_tx_by_idx(req.get_client().index)
            };

            if let Some(tx) = tx {
                let mut t = Task::new();

                // obtain task id
                let task_id: u32;
                {
                    let mut current_task_id = self.current_task_id.lock().unwrap();
                    task_id = *current_task_id;
                    current_task_id.add_assign(1);
                }
                ack.set_task_id(task_id);

                t.set_task_id(task_id);
                t.set_ping(req.take_ping());

                debug!("sending task to client");
                if tx.send(t).wait().is_ok() {
                    ack.set_success(true);
                } else {
                    ack.set_error_message("client exists, but was unable to send task".to_string());
                }
                debug!("task sent");
            } else {
                ack.set_error_message("client does not exist".to_string());
            }
        }

        let f = sink.success(ack).map_err(|_| ());
        ctx.spawn(f);
    }

    fn list_clients(&mut self, ctx: RpcContext, _: Empty, sink: UnarySink<ClientList>) {
        debug!("received list_clients request");

        let connections = self.connection_manager.connections.read().unwrap();
        let mut list = ClientList::new();
        list.set_clients(RepeatedField::from_vec(
            connections
                .iter()
                .map(|(k, v)| {
                    let mut c = Client::new();
                    c.index = *k;
                    c.set_metadata(v.metadata.clone());
                    c
                })
                .collect::<Vec<Client>>(),
        ));
        ctx.spawn(
            sink.success(list)
                .map(|_| ())
                .map_err(|e| error!("could not send client list: {}", e)),
        );
    }

    fn send_result(&mut self, ctx: RpcContext, req: TaskResult, sink: UnarySink<Ack>) {
        let task_id = req.get_task_id();
        if let Some(subscribers) = self.get_subscribers(task_id) {
            subscribers
                .iter()
                .map(|s| s.clone().send(req.clone()).wait())
                .for_each(drop);
        }
        ctx.spawn(sink.success(Ack::new()).map_err(|_| ()));
    }

    fn subscribe_result(
        &mut self,
        _ctx: RpcContext,
        req: TaskId,
        sink: ServerStreamingSink<TaskResult>,
    ) {
        let (tx, rx) = channel(1);

        let f = rx
            .map(|i| (i, grpcio::WriteFlags::default()))
            .forward(sink.sink_map_err(|_| ()))
            .map(|_| ())
            .map_err(|_| error!("closed result stream"));

        self.register_subscriber(req.get_task_id(), tx);

        self.runtime.executor().spawn(f);
    }

    fn task_finished(&mut self, ctx: RpcContext, req: TaskId, sink: UnarySink<Ack>) {
        let task_id = req.get_task_id();
        self.disconnect_subscribers(task_id);
        ctx.spawn(sink.success(Ack::new()).map_err(|_| ()));
    }
}

type ConnectionList = Arc<RwLock<HashMap<u32, Connection>>>;

#[derive(Debug)]
struct ConnectionManager {
    connections: ConnectionList,
    connection_id: Arc<Mutex<u32>>,
}

impl ConnectionManager {
    fn new() -> ConnectionManager {
        ConnectionManager {
            connections: Arc::new(RwLock::new(HashMap::new())),
            connection_id: Arc::new(Mutex::new(0)),
        }
    }

    fn generate_connection_id(&self) -> u32 {
        let mut counter = self.connection_id.lock().unwrap();
        counter.add_assign(1);
        *counter
    }

    fn register_connection(&self, connection_id: u32, connection: Connection) {
        let mut hashmap = self.connections.write().unwrap();
        hashmap.insert(connection_id, connection);
        debug!(
            "added connection to list with id {}, connection count: {}",
            connection_id,
            hashmap.len()
        );
    }

    fn unregister_connection(&self, connection_id: u32) {
        let mut hashmap = self.connections.write().unwrap();
        hashmap.remove(&connection_id);
        debug!(
            "removed connection from list with id {}, connection count: {}",
            connection_id,
            hashmap.len()
        );
    }

    fn get_client_tx_by_idx(&self, connection_id: u32) -> Option<Sender<Task>> {
        let hashmap = self.connections.read().unwrap();
        if let Some(v) = hashmap.get(&connection_id) {
            return Some(v.channel.clone());
        }
        None
    }

    fn get_client_tx_by_hostname(&self, hostname: &str) -> Option<Sender<Task>> {
        let hashmap = self.connections.read().unwrap();
        hashmap
            .iter()
            .find(|f| f.1.metadata.hostname == hostname)
            .map(|f| f.1.channel.clone())
    }
}

#[cfg(test)]
mod connection_manager {
    use super::*;

    #[test]
    fn ids_increase() {
        let manager = ConnectionManager::new();
        let mut prev_id = None;
        for _ in 0..100 {
            let new_id = manager.generate_connection_id();
            if prev_id.is_some() {
                assert_eq!(prev_id.unwrap() + 1, new_id);
            }
            prev_id = Some(new_id);
        }
    }

    #[test]
    fn connections_can_be_registered() {
        let manager = ConnectionManager::new();

        let mut registered_ids = Vec::new();
        for _ in 0..5 {
            let connection_id = manager.generate_connection_id();
            let (channel_tx, _) = channel(0);
            let connection = Connection {
                channel: channel_tx,
                metadata: Metadata::default(),
            };
            manager.register_connection(connection_id, connection);
            registered_ids.push(connection_id);
        }

        for id in registered_ids {
            assert!(
                manager.get_client_tx_by_idx(id).is_some(),
                "registered connection should be retrievable from connection manager"
            );
        }
    }

    #[test]
    fn connections_can_be_retrieved_by_hostname() {
        let manager = ConnectionManager::new();

        for i in 0..5 {
            let connection_id = manager.generate_connection_id();
            let (channel_tx, _) = channel(0);
            let mut connection = Connection {
                channel: channel_tx,
                metadata: Metadata::default(),
            };
            connection.metadata.hostname = format!("host{}", i);
            manager.register_connection(connection_id, connection);
        }

        assert!(
            manager.get_client_tx_by_hostname("host4").is_some(),
            "registered connection should be retrievable from connection manager"
        );
    }
}
