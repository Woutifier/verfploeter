# Verfploeter
[![Build Status](https://travis-ci.org/Woutifier/verfploeter.svg?branch=master)](https://travis-ci.org/Woutifier/verfploeter)

This project is an implementation of https://conferences.sigcomm.org/imc/2017/papers/imc17-final46.pdf

It is intended to replace the use of packetcapr and pinger, offering an integrated solution.

## Usage

Verfploeter has 3 components, all contained in the same binary:
 - Server (running somewhere centrally)
 - Client (running on the edges, e.g. the anycast nodes)
 - Cli (running on locally)

### Server

The server receives tasks from the cli component, and sends these to (one of) the connected clients
for execution. Client's receiving measurement data forward this to the server. Optionally, the server can stream back results to the cli.

```
$ verfploeter server --help
verfploeter-server
Launches the verfploeter server

USAGE:
    verfploeter server

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

### Client

On startup the client connects to the server and thereby registers itself. It is then possible
to schedule tasks on the client, via the server, from the cli.

```
$ verfploeter client --help
verfploeter-client
Launches the verfploeter client

USAGE:
    verfploeter client [OPTIONS] -h <hostname>

FLAGS:
        --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -h <hostname>        hostname for this client
    -s <server>          hostname/ip address:port of the server [default: 127.0.0.1:50001]
```

### Cli

The cli connects to the server and allows users to schedule tasks.

```
$ verfploeter cli --help
verfploeter-cli
Verfploeter CLI

USAGE:
    verfploeter cli [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -s <server>        hostname/ip address:port of the server [default: 127.0.0.1:50001]

SUBCOMMANDS:
    client-list       retrieves a list of currently connected clients from the server
    do-verfploeter    performs verfploeter on the indicated client
    help              Prints this message or the help of the given subcommand(s)
```

## Contributions

Issues and pull requests are welcome
