use super::schema::verfploeter::{
    Metadata, Task
};
use super::schema::verfploeter_grpc::VerfploeterClient;

use futures::*;
use futures::sync::oneshot;
use grpcio::{ChannelBuilder, Environment};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use clap::ArgMatches;
use std::collections::HashMap;

mod handlers;
use self::handlers::ping::{PingInbound,PingOutbound};
use self::handlers::{TaskHandler, ChannelType};

pub struct Client {
    grpc_client: Arc<VerfploeterClient>,
    task_handlers: HashMap<String, Box<dyn TaskHandler>>,
    metadata: Metadata,
}


impl Client {
    pub fn new(args: &ArgMatches) -> Client {
        let host = args.value_of("server").unwrap();
        let env = Arc::new(Environment::new(1));
        let channel = ChannelBuilder::new(env).connect(host);
        let grpc_client = Arc::new(VerfploeterClient::new(channel));

        let mut task_handlers: HashMap<String, Box<dyn TaskHandler>> = HashMap::new();

        let mut metadata = Metadata::new();
        metadata.set_hostname(args.value_of("hostname").unwrap().to_string());

        // Setup task_handlers
        task_handlers.insert("ping_outbound".to_string(), Box::new(PingOutbound::new()));
        task_handlers.insert("ping_inbound".to_string(), Box::new(PingInbound::new(metadata.clone(), grpc_client.clone())));

        Client {
            grpc_client,
            task_handlers,
            metadata
        }
    }

    pub fn start(mut self) {
        let res = self.grpc_client.connect(&self.metadata);
        if let Ok(stream) = res {

            // Get tx channel for ping_outbound
            let tx = match self.task_handlers.get_mut("ping_outbound").unwrap().get_channel() {
                ChannelType::Task {sender, receiver: _} => sender.unwrap(),
                _ => panic!("ping_outbound has wrong tx channel type")
            };

            // Signal finish
            let (finish_tx, finish_rx) = oneshot::channel();

            // For now we only have a ping task, in the future we can have a match here
            // that sends tasks to different threads for processing
            let f = stream
                .map({
                    let tx = tx.clone();
                    move |i| {
                        if i.has_ping() {
                            tx.clone().send(i).unwrap();
                        }
                    }
                })
                .map_err(|_| ())
                .collect()
                .map(|_| ())
                .map_err(|_| {finish_tx.send(()).unwrap()});

            self.grpc_client.spawn(f);

            // Start all task handlers
            for (i, v) in &mut self.task_handlers {
                v.start();
                debug!("started {} task handler", i);
            }

            // Wait for process to finish
            finish_rx.map(|_| ()).wait().unwrap();

            // Stop all task handlers
            for (i, v) in &mut self.task_handlers {
                v.exit();
                debug!("exited {} task handler", i);
            }
        }
    }
}


