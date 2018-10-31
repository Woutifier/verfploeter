#[macro_use]
extern crate log;
extern crate env_logger;

extern crate clap;
extern crate futures;
extern crate grpcio;

mod schema;

use self::schema::verfploeter::{SchedulingResult, Task};
use self::schema::verfploeter_grpc::{self, Verfploeter, VerfploeterClient};
use clap::{App, ArgMatches, SubCommand};
use futures::sync::mpsc::{channel, Sender};
use futures::*;
use grpcio::{ChannelBuilder, Environment, RpcContext, Server, ServerBuilder, ServerStreamingSink};
use std::collections::HashMap;
use std::ops::AddAssign;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

type ConnectionList = Arc<Mutex<HashMap<u32, Sender<Task>>>>;

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

    fn register_connection(&self, connection_id: u32, sender: Sender<Task>) {
        let mut hashmap = self.connections.lock().unwrap();
        hashmap.insert(connection_id, sender);
        debug!(
            "(connection manager) added connection to list with id {}",
            connection_id
        );
    }

    fn unregister_connection(&self, connection_id: u32) {
        let mut hashmap = self.connections.lock().unwrap();
        hashmap.remove(&connection_id);
        debug!(
            "(connection manager) removed connection from list with id {}",
            connection_id
        );
    }
}

#[derive(Clone)]
struct VerfploeterService {
    connection_manager: Arc<ConnectionManager>,
}

impl Verfploeter for VerfploeterService {
    fn connect(&mut self, ctx: RpcContext, _: SchedulingResult, sink: ServerStreamingSink<Task>) {
        let (tx, rx) = channel(1);

        let connection_manager = self.connection_manager.clone();
        let connection_id = connection_manager.generate_connection_id();
        connection_manager.register_connection(connection_id, tx);

        // Forward all tasks from the channel to the sink, and unregister from the connection
        // manager on error or completion.
        let connection_manager1 = self.connection_manager.clone();
        let connection_manager2 = self.connection_manager.clone();
        let f = rx
            .map(|item| (item, grpcio::WriteFlags::default()))
            .forward(sink.sink_map_err(|_| ()))
            .map(move |_| connection_manager1.unregister_connection(connection_id))
            .map_err(move |_| connection_manager2.unregister_connection(connection_id));

        ctx.spawn(f);
    }
}

fn main() {
    // Setup logging
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "debug");
    env_logger::Builder::from_env(env).init();

    let matches = parse_cmd();

    info!("Starting verfploeter v{}", env!("CARGO_PKG_VERSION"));

    if let Some(server_matches) = matches.subcommand_matches("server") {
        let (_, connections) = start_server();

        let mut counter = 0;
        loop {
            {
                let hashmap = connections.lock().unwrap();
                for (_, v) in hashmap.iter() {
                    let mut t = Task::new();
                    t.taskId = counter;
                    counter += 1;
                    v.clone().send(t).wait().unwrap();
                }
            }
            thread::sleep(Duration::from_secs(1));
        }
    } else if let Some(client_matches) = matches.subcommand_matches("client") {
        start_client();
    } else {
        error!("run with --help to see options");
    }
}

fn start_server() -> (Server, ConnectionList) {
    let env = Arc::new(Environment::new(1));

    let connections = Arc::new(Mutex::new(HashMap::new()));
    let connection_manager = Arc::new(ConnectionManager {
        connections: connections.clone(),
        connection_id: Arc::new(Mutex::new(0)),
    });
    let s = VerfploeterService { connection_manager };
    let service = verfploeter_grpc::create_verfploeter(s);
    let mut server = ServerBuilder::new(env)
        .register_service(service)
        .bind("127.0.0.1", 50001)
        .build()
        .unwrap();
    server.start();

    for &(ref host, port) in server.bind_addrs() {
        info!("Listening on {}:{}", host, port);
    }

    (server, connections)
}

fn start_client() {
    let env = Arc::new(Environment::new(2));
    let channel = ChannelBuilder::new(env).connect("127.0.0.1:50001");
    let client = VerfploeterClient::new(channel);

    let res = client.connect(&SchedulingResult::new());
    if let Ok(b) = res {
        let mut stream = b;
        loop {
            match stream.into_future().wait() {
                Ok((Some(t), s)) => {
                    println!("Task: {}", t.taskId);
                    stream = s;
                }
                Ok((None, s)) => {
                    stream = s;
                }
                Err((e, _)) => {
                    error!("disconnected ({})", e);
                    break;
                }
            }
        }
    }
}

fn parse_cmd<'a>() -> ArgMatches<'a> {
    App::new("Verfploeter")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Wouter B. de Vries <w.b.devries@utwente.nl")
        .about("Performs measurements")
        .subcommand(SubCommand::with_name("server").about("Launches the verfploeter server"))
        .subcommand(SubCommand::with_name("client").about("Launches the verfploeter client"))
        .get_matches()
}
