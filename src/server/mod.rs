use super::schema::verfploeter::{
    Ack, Client, ClientList, Empty, Metadata, ScheduleTask, Task, TaskId, TaskResult,
};
use super::schema::verfploeter_grpc::{self, Verfploeter};
use futures::sync::mpsc::{channel, Sender};
use futures::*;
use grpcio::{
    ChannelBuilder, Environment, RpcContext, Server as GrpcServer, ServerBuilder,
    ServerStreamingSink, UnarySink,
};
use protobuf::RepeatedField;
use std::collections::HashMap;
use std::ops::AddAssign;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::timer::Interval;

pub struct Server {
    pub connection_list: ConnectionList,
    grpc_server: GrpcServer,
}

impl Server {
    pub fn new() -> Server {
        let env = Arc::new(Environment::new(10));

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let connection_manager = Arc::new(ConnectionManager {
            connections: connections.clone(),
            connection_id: Arc::new(Mutex::new(0)),
        });

        let s = VerfploeterService {
            connection_manager,
            subscription_list: Arc::new(RwLock::new(HashMap::new())),
            current_task_id: Arc::new(Mutex::new(0)),
            runtime: Arc::new(Runtime::new().unwrap()),
        };
        let service = verfploeter_grpc::create_verfploeter(s);

        let channel_args = ChannelBuilder::new(Arc::clone(&env))
            .max_receive_message_len(100 * 1024 * 1024)
            .max_send_message_len(100 * 1024 * 1024)
            .build_args();

        let grpc_server = ServerBuilder::new(env)
            .channel_args(channel_args)
            .register_service(service)
            .bind("0.0.0.0", 50001)
            .build()
            .unwrap();

        Server {
            connection_list: connections,
            grpc_server,
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
}

impl Verfploeter for VerfploeterService {
    fn connect(&mut self, ctx: RpcContext, metadata: Metadata, sink: ServerStreamingSink<Task>) {
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
                  return t;
              }).forward(tx.clone().sink_map_err(|_| ()))
              .map_err(|_| ()).map(|_| ())
        );
    }

    fn do_task(&mut self, ctx: RpcContext, mut req: ScheduleTask, sink: UnarySink<Ack>) {
        debug!("received do_task request");
        let mut ack = Ack::new();
        ack.set_success(false);

        // Handle a ping task
        if req.has_ping() {
            let tx = self
                .connection_manager
                .get_client_tx(req.get_client().index);
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

        let connections = self.connection_manager.connections.lock().unwrap();
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
        ctx: RpcContext,
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
}

type ConnectionList = Arc<Mutex<HashMap<u32, Connection>>>;

#[derive(Debug)]
struct ConnectionManager {
    connections: ConnectionList,
    connection_id: Arc<Mutex<u32>>,
}

impl ConnectionManager {
    fn generate_connection_id(&self) -> u32 {
        let mut counter = self.connection_id.lock().unwrap();
        counter.add_assign(1);
        *counter
    }

    fn register_connection(&self, connection_id: u32, connection: Connection) {
        let mut hashmap = self.connections.lock().unwrap();
        hashmap.insert(connection_id, connection);
        debug!(
            "added connection to list with id {}, connection count: {}",
            connection_id,
            hashmap.len()
        );
    }

    fn unregister_connection(&self, connection_id: u32) {
        let mut hashmap = self.connections.lock().unwrap();
        hashmap.remove(&connection_id);
        debug!(
            "removed connection from list with id {}, connection count: {}",
            connection_id,
            hashmap.len()
        );
    }

    fn get_client_tx(&self, connection_id: u32) -> Option<Sender<Task>> {
        let hashmap = self.connections.lock().unwrap();
        if let Some(v) = hashmap.get(&connection_id) {
            return Some(v.channel.clone());
        }
        None
    }
}
