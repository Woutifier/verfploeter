use super::schema::verfploeter::{Empty, Metadata, PingV4};
use super::schema::verfploeter_grpc::VerfploeterClient;
use clap::ArgMatches;
use futures::sync::mpsc::{channel, Receiver, Sender};
use futures::*;
use grpcio::{ChannelBuilder, Environment};
use std::sync::Arc;

pub fn execute(args: &ArgMatches) {
    let env = Arc::new(Environment::new(1));
    let channel = ChannelBuilder::new(env).connect("127.0.0.1:50001");
    let grpc_client = VerfploeterClient::new(channel);

    if let Some(matches) = args.subcommand_matches("client-list") {
        let client_list = grpc_client.list_clients(&Empty::new()).unwrap();
        println!("Connected clients: {}", client_list.get_clients().len());
        println!("-------------------------------------");
        println!("Index\t\t\tHostname");
        for client in client_list.get_clients() {
            println!("{}\t\t\t{}", client.index, client.get_metadata().hostname);
        }
    } else {
        unimplemented!();
    }
}
