use super::schema::verfploeter::Metadata;
use super::schema::verfploeter_grpc::VerfploeterClient;
use futures::*;
use grpcio::{ChannelBuilder, Environment};
use std::sync::Arc;

pub struct Client {
    grpc_client: VerfploeterClient,
}

impl Client {
    pub fn new() -> Client {
        let env = Arc::new(Environment::new(1));
        let channel = ChannelBuilder::new(env).connect("127.0.0.1:50001");
        Client {
            grpc_client: VerfploeterClient::new(channel),
        }
    }

    pub fn start(&self) {
        let res = self.grpc_client.connect(&Metadata::new());
        if let Ok(b) = res {
            let mut stream = b;
            loop {
                match stream.into_future().wait() {
                    Ok((Some(t), s)) => {
                        println!("Task: {:#?}", t);
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
}
