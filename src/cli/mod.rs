use super::schema::verfploeter::{Client, Empty, Metadata, PingV4, ScheduleTask};
use super::schema::verfploeter_grpc::VerfploeterClient;
use clap::ArgMatches;
use futures::sync::mpsc::{channel, Receiver, Sender};
use futures::*;
use grpcio::{ChannelBuilder, Environment};
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::net::Ipv4Addr;
use std::str::FromStr;
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
    } else if let Some(matches) = args.subcommand_matches("do-verfploeter") {
        // Get parameters
        let client_index: u32 = matches.value_of("CLIENT_INDEX").unwrap().parse().unwrap();
        let source_ip: u32 =
            u32::from(Ipv4Addr::from_str(matches.value_of("SOURCE_IP").unwrap()).unwrap());
        let ip_file = matches.value_of("IP_FILE").unwrap();

        // Read IP Addresses from given file
        let file = File::open(ip_file).expect(&format!("Unable to open file {}", ip_file));
        let mut buf_reader = BufReader::new(file);

        let ips = buf_reader
            .lines()
            .map(|l| u32::from(Ipv4Addr::from_str(&l.unwrap()).unwrap()))
            .collect::<Vec<u32>>();

        // Construct appropriate structs
        let mut ping_v4 = PingV4::new();
        ping_v4.source_address = source_ip;
        ping_v4.destination_addresses = ips;
        let mut client = Client::new();
        client.index = client_index;
        let mut schedule_task = ScheduleTask::new();
        schedule_task.set_ping_v4(ping_v4);
        schedule_task.set_client(client);

        // Send task to server
        grpc_client.do_task(&schedule_task).unwrap();
    } else {
        unimplemented!();
    }
}
