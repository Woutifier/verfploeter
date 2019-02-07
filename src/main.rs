#![feature(drain_filter)]
#[macro_use]
extern crate log;
extern crate byteorder;
extern crate clap;
extern crate env_logger;
extern crate futures;
extern crate grpcio;
extern crate protobuf;
extern crate ratelimit_meter;
extern crate socket2;
extern crate tokio;
#[macro_use]
extern crate prettytable;
extern crate hmac;
extern crate sha2;
extern crate maxminddb;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

mod cli;
mod client;
mod net;
mod schema;
mod server;

use clap::{App, Arg, ArgMatches, SubCommand};

use std::thread;
use std::time::Duration;

fn main() {
    // Setup logging
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info");
    env_logger::Builder::from_env(env).init();

    let matches = parse_cmd();

    info!("Starting verfploeter v{}", env!("CARGO_PKG_VERSION"));

    if matches.subcommand_matches("server").is_some() {
        let mut s = server::Server::new();
        s.start();

        // todo: come up with a smarter way to keep the program alive
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    } else if let Some(client_matches) = matches.subcommand_matches("client") {
        let grpc_host = client_matches.value_of("server").unwrap().to_string();
        let client_hostname = client_matches.value_of("hostname").unwrap().to_string();
        let c = client::Client::new(grpc_host, client_hostname);
        c.start();
    } else if let Some(cli_matches) = matches.subcommand_matches("cli") {
        cli::execute(cli_matches);
    } else {
        error!("run with --help to see options");
    }
    debug!("exiting");
}

fn parse_cmd<'a>() -> ArgMatches<'a> {
    App::new("Verfploeter")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Wouter B. de Vries <w.b.devries@utwente.nl")
        .about("Performs measurements")
        .subcommand(SubCommand::with_name("server").about("Launches the verfploeter server"))
        .subcommand(
            SubCommand::with_name("client").about("Launches the verfploeter client")
                .arg(
                    Arg::with_name("hostname")
                        .short("h")
                        .takes_value(true)
                        .help("hostname for this client")
                        .required(true)
                )
                .arg(
                    Arg::with_name("server")
                        .short("s")
                        .takes_value(true)
                        .help("hostname/ip address:port of the server")
                        .default_value("127.0.0.1:50001")
                )
        )
        .subcommand(
            SubCommand::with_name("cli").about("Verfploeter CLI")
                .arg(
                    Arg::with_name("server")
                        .short("s")
                        .takes_value(true)
                        .help("hostname/ip address:port of the server")
                        .default_value("127.0.0.1:50001")
                )
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
                    .arg(Arg::with_name("stream")
                        .short("s")
                        .multiple(false)
                        .help("Stream results to stdout"))
                    .arg(Arg::with_name("json")
                        .short("j")
                        .multiple(false)
                        .help("Output results in JSON format"))
                )
        )
        .get_matches()
}
