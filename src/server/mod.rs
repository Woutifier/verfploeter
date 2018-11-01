use super::schema::verfploeter::{Metadata, Task};
use super::schema::verfploeter_grpc::{self, Verfploeter};
use futures::sync::mpsc::{channel, Sender};
use futures::*;
use grpcio::{Environment, RpcContext, Server as GrpcServer, ServerBuilder, ServerStreamingSink};
use std::collections::HashMap;
use std::ops::AddAssign;
use std::sync::{Arc, Mutex};

pub struct Server {
    pub connection_list: ConnectionList,
    grpc_server: GrpcServer,
}

impl Server {
    pub fn new() -> Server {
        let env = Arc::new(Environment::new(1));

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let connection_manager = Arc::new(ConnectionManager {
            connections: connections.clone(),
            connection_id: Arc::new(Mutex::new(0)),
        });

        let s = VerfploeterService { connection_manager };
        let service = verfploeter_grpc::create_verfploeter(s);
        let grpc_server = ServerBuilder::new(env)
            .register_service(service)
            .bind("127.0.0.1", 50001)
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
}

impl Verfploeter for VerfploeterService {
    fn connect(&mut self, ctx: RpcContext, metadata: Metadata, sink: ServerStreamingSink<Task>) {
        let (tx, rx) = channel(1);

        let connection_manager = self.connection_manager.clone();
        let connection_id = connection_manager.generate_connection_id();
        connection_manager.register_connection(
            connection_id,
            Connection {
                metadata,
                channel: tx,
            },
        );

        // Forward all tasks from the channel to the sink, and unregister from the connection
        // manager on error or completion.
        let f = rx
            .map(|item| (item, grpcio::WriteFlags::default()))
            .forward(sink.sink_map_err(|_| ()))
            .map({
                let cm = self.connection_manager.clone();
                move |_| cm.unregister_connection(connection_id)
            })
            .map_err({
                let cm = self.connection_manager.clone();
                move |_| cm.unregister_connection(connection_id)
            });

        ctx.spawn(f);
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
        counter.clone()
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
}
