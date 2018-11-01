#[macro_use]
extern crate log;
extern crate clap;
extern crate env_logger;
extern crate futures;
extern crate grpcio;
extern crate protobuf;
extern crate socket2;

mod cli;
mod client;
mod schema;
mod server;

use self::schema::verfploeter::{PingV4, Task};
use clap::{App, Arg, ArgMatches, SubCommand};
use futures::*;
use protobuf::RepeatedField;
use std::net::Ipv4Addr;
use std::thread;
use std::time::Duration;

fn main() {
    // Setup logging
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "debug");
    env_logger::Builder::from_env(env).init();

    let matches = parse_cmd();

    info!("Starting verfploeter v{}", env!("CARGO_PKG_VERSION"));

    if let Some(server_matches) = matches.subcommand_matches("server") {
        let mut s = server::Server::new();
        s.start();

        let mut counter = 0;
        loop {
            {
                let hashmap = s.connection_list.lock().unwrap();
                for (_, v) in hashmap.iter() {
                    let mut t = Task::new();
                    t.taskId = counter;
                    counter += 1;
                    let mut p = PingV4::new();
                    p.source_address = Ipv4Addr::from([130, 89, 12, 10]).into();
                    p.destination_addresses = vec![
                        Ipv4Addr::from([8, 8, 8, 8]).into(),
                        Ipv4Addr::from([1, 1, 1, 1]).into(),
                    ];
                    t.set_ping_v4(p);
                    v.channel.clone().send(t).wait().unwrap();
                }
            }
            thread::sleep(Duration::from_secs(1));
        }
    } else if let Some(client_matches) = matches.subcommand_matches("client") {
        let mut c = client::Client::new();
        c.start();
    } else if let Some(cli_matches) = matches.subcommand_matches("cli") {
        cli::execute(cli_matches);
    } else {
        error!("run with --help to see options");
    }
}

fn parse_cmd<'a>() -> ArgMatches<'a> {
    App::new("Verfploeter")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Wouter B. de Vries <w.b.devries@utwente.nl")
        .about("Performs measurements")
        .subcommand(SubCommand::with_name("server").about("Launches the verfploeter server"))
        .subcommand(SubCommand::with_name("client").about("Launches the verfploeter client"))
        .subcommand(
            SubCommand::with_name("cli").about("Verfploeter CLI")
                .subcommand(SubCommand::with_name("client-list").about("retrieves a list of currently connected clients from the server"))
                .subcommand(SubCommand::with_name("do-verfploeter").about("performs verfploeter on the indicated client")
                    .arg(Arg::with_name("CLIENT_INDEX").help("Sets the client to run verfploeter from (i.e. the outbound ping)")
                    .required(true)
                    .index(1))
                    .arg(Arg::with_name("SOURCE_IP").help("The IP to send the pings from")
                        .required(true)
                        .index(2))
                    .arg(Arg::with_name("IP_FILE").help("A file that contains IP address to ping")
                    .required(true)
                    .index(3))

                )
        )
        .get_matches()
}
