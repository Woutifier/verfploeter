use super::schema::verfploeter::{Address, Client, Empty, Ping, ScheduleTask, TaskId};
use super::schema::verfploeter_grpc::VerfploeterClient;
use clap::ArgMatches;
use futures::Stream;
use grpcio::{ChannelBuilder, Environment};
use protobuf::RepeatedField;
use std::error::Error;
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

    if args.subcommand_matches("client-list").is_some() {
        match grpc_client.list_clients(&Empty::new()) {
            Ok(client_list) => {
                println!("Connected clients: {}", client_list.get_clients().len());
                println!("-------------------------------------");
                println!("Index\t\t\tHostname");
                for client in client_list.get_clients() {
                    println!("{}\t\t\t{}", client.index, client.get_metadata().hostname);
                }
            }
            Err(e) => error!("unable to obtain client list: {}", e),
        }
    } else if let Some(matches) = args.subcommand_matches("do-verfploeter") {
        // Get parameters
        let client_index: u32 = matches.value_of("CLIENT_INDEX").unwrap().parse().unwrap();
        let source_ip: u32 =
            u32::from(Ipv4Addr::from_str(matches.value_of("SOURCE_IP").unwrap()).unwrap());
        let ip_file = matches.value_of("IP_FILE").unwrap();

        // Read IP Addresses from given file
        let file =
            File::open(ip_file).unwrap_or_else(|_| panic!("Unable to open file {}", ip_file));
        let buf_reader = BufReader::new(file);

        let ips = buf_reader
            .lines()
            .map(|l| {
                let mut address = Address::new();
                address.set_v4(u32::from(Ipv4Addr::from_str(&l.unwrap()).unwrap()));
                address
            })
            .collect::<Vec<Address>>();

        // Construct appropriate structs
        let mut ping = Ping::new();
        let mut address = Address::new();
        address.set_v4(source_ip);
        ping.set_source_address(address);
        ping.set_destination_addresses(RepeatedField::from(ips));
        let mut client = Client::new();
        client.index = client_index;
        let mut schedule_task = ScheduleTask::new();
        schedule_task.set_ping(ping);
        schedule_task.set_client(client);

        let mut scheduled_task_id = None;
        // Send task to server
        match grpc_client.do_task(&schedule_task) {
            Ok(ack) => { println!("successfully scheduled task, id: {}", ack.get_task_id()); scheduled_task_id = Some(ack.get_task_id()); }
            Err(e) => error!("unable to schedule task: {} ({})", e.description(), e),
        }

        if scheduled_task_id.is_some() {
            let mut request_task_id = TaskId::new();
            request_task_id.set_task_id(scheduled_task_id.unwrap());
            let result = grpc_client.subscribe_result(&request_task_id).unwrap();
            result
                .map(|i| println!("{}", i))
                .map_err(|_| ())
                .wait()
                .for_each(drop);
        }
    } else {
        unimplemented!();
    }
}
