use super::schema::verfploeter::{
    Address, Client, Empty, Metadata, Ping, ScheduleTask, TaskId, TaskResult,
};

use super::schema::verfploeter_grpc::VerfploeterClient;
use clap::ArgMatches;
use futures::Stream;
use grpcio::{ChannelBuilder, Environment};
use prettytable::{color, format, Attr, Cell, Row, Table};
//use protobuf::RepeatedField;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::sync::Arc;

mod enrichment;
use crate::cli::enrichment::{
    Columnizable, IP2ASNTransformer, IP2CountryTransformer, TransformPipeline, Transformer,
};

pub fn execute(args: &ArgMatches) {
    let server = args.value_of("server").unwrap();
    let env = Arc::new(Environment::new(1));
    let channel = ChannelBuilder::new(env).connect(server);
    let grpc_client = VerfploeterClient::new(channel);

    if args.subcommand_matches("client-list").is_some() {
        print_client_list(&grpc_client)
    } else if let Some(matches) = args.subcommand_matches("start") {
        perform_verfploeter_measurement(matches, &grpc_client, matches)
    } else {
        unimplemented!();
    }
}

fn print_client_list(grpc_client: &VerfploeterClient) {
    match grpc_client.list_clients(&Empty::new()) {
        Ok(client_list) => {
            let mut table = Table::new();
            table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
            table.add_row(Row::new(vec![
                Cell::new("Index")
                    .with_style(Attr::Bold)
                    .with_style(Attr::ForegroundColor(color::GREEN)),
                Cell::new("Hostname")
                    .with_style(Attr::Bold)
                    .with_style(Attr::ForegroundColor(color::GREEN)),
                Cell::new("Version")
                    .with_style(Attr::Bold)
                    .with_style(Attr::ForegroundColor(color::GREEN)),
            ]));
            for client in client_list.get_clients() {
                table.add_row(row!(
                    client.index,
                    client.get_metadata().hostname,
                    client.get_metadata().version
                ));
            }
            table.printstd();
            println!("Connected clients: {}", client_list.get_clients().len());
        }
        Err(e) => println!("unable to obtain client list: {}", e),
    }
}

fn perform_verfploeter_measurement(
    args: &ArgMatches,
    grpc_client: &VerfploeterClient,
    matches: &ArgMatches,
) {
    // Get parameters
    let client_hostname = matches.value_of("CLIENT_HOSTNAME").unwrap();
    let source_ip: u32 =
        u32::from(Ipv4Addr::from_str(matches.value_of("SOURCE_IP").unwrap()).unwrap());
    let ip_file = matches.value_of("IP_FILE").unwrap();
    // Read IP Addresses from given file
    let file = File::open(ip_file).unwrap_or_else(|_| panic!("Unable to open file {}", ip_file));
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
    ping.set_destination_addresses(ips);

    let mut client = Client::new();
    let mut metadata = Metadata::new();
    metadata.hostname = client_hostname.to_string();
    client.set_metadata(metadata);

    let mut schedule_task = ScheduleTask::new();
    schedule_task.set_ping(ping);
    schedule_task.set_client(client);
    let mut scheduled_task_id = None;
    let mut error_message = None;
    let mut success = false;
    // Send task to server
    match grpc_client.do_task(&schedule_task) {
        Ok(ack) => {
            info!("successfully connected, id: {}", ack.get_task_id());
            success = ack.get_success();
            scheduled_task_id = Some(ack.get_task_id());
            if !ack.get_success() {
                error_message = Some(ack.get_error_message().to_string());
            }
        }
        Err(e) => error!("unable to connect: {} ({})", e.description(), e),
    }
    if success {
        let mut transform_pipeline = TransformPipeline { pipeline: vec![] };

        if let Some(ip2country_db_path) = args.value_of("ip2country") {
            transform_pipeline.pipeline.push(IP2CountryTransformer::new(
                "source_address",
                "source_address_country",
                ip2country_db_path,
            ));
            info!("added ip2country transformer");
        }

        if let Some(ip2asn_db_path) = args.value_of("ip2asn") {
            transform_pipeline.pipeline.push(IP2ASNTransformer::new(
                "source_address",
                "source_address_asn",
                ip2asn_db_path,
            ));
            info!("added ip2asn transformer");
        }

        // Determine headers and print them if we are outputting CSV
        let mut headers = TaskResult::get_headers();
        transform_pipeline
            .pipeline
            .iter()
            .for_each(|t| t.add_header(&mut headers));
        if !matches.is_present("json") {
            println!("{}", headers.join(","));
        }

        let mut request_task_id = TaskId::new();
        request_task_id.set_task_id(scheduled_task_id.unwrap());
        let result = grpc_client.subscribe_result(&request_task_id).unwrap();
        result
            .map(move |i| {
                let data = i.get_data();
                for mut entry in data {
                    for transformer in &transform_pipeline.pipeline {
                        entry = transformer.transform(entry);
                    }
                    if matches.is_present("json") {
                        println!("{}", serde_json::to_string(&entry).unwrap());
                    } else {
                        for (idx, header) in headers.iter().enumerate() {
                            if idx != 0 {
                                print!(",");
                            }
                            print!("{}", entry[header]);
                        }
                        println!();
                    }
                }
            })
            .map_err(|_| ())
            .wait()
            .for_each(drop);
    } else {
        error!("failed to schedule task");
        error!("Message: {}", error_message.unwrap());
    }
}
