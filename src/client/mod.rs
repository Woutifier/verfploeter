use super::schema::verfploeter::{Metadata, PingV4};
use super::schema::verfploeter_grpc::VerfploeterClient;
use futures::sync::mpsc::{channel, Receiver, Sender};
use futures::*;
use grpcio::{ChannelBuilder, Environment};
use std::sync::Arc;
//use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread::JoinHandle;

use socket2::{Domain, Protocol, Socket, Type};

use std::net::Ipv4Addr;
use std::net::SocketAddr;

#[derive(Debug)]
enum PingTask {
    V4 { value: PingV4 },
}

use std::thread;

pub struct Client {
    grpc_client: VerfploeterClient,
    ping_outbound: PingOutbound,
}

impl Client {
    pub fn new() -> Client {
        let env = Arc::new(Environment::new(1));
        let channel = ChannelBuilder::new(env).connect("127.0.0.1:50001");
        let grpc_client = VerfploeterClient::new(channel);
        Client {
            ping_outbound: PingOutbound::new(),
            grpc_client,
        }
    }

    pub fn start(self) {
        let res = self.grpc_client.connect(&Metadata::new());
        if let Ok(stream) = res {
            let f = stream
                .map({
                    let tx = self.ping_outbound.tx;
                    move |mut i| {
                        if i.has_ping_v4() {
                            tx.clone()
                                .send(PingTask::V4 {
                                    value: i.take_ping_v4(),
                                })
                                .wait()
                                .unwrap();
                        }
                    }
                })
                .map_err(|_| ())
                .collect()
                .map(|_| ())
                .map_err(|_| ());

            self.grpc_client.spawn(f);
            self.ping_outbound.handle.join().unwrap();
            debug!("exiting");
        }
    }
}

struct PingOutbound {
    pub tx: Sender<PingTask>,
    pub handle: JoinHandle<()>,
}

impl PingOutbound {
    pub fn new() -> PingOutbound {
        let (tx, rx): (Sender<PingTask>, Receiver<PingTask>) = channel(1);

        let handle = thread::spawn(move || {
            rx.map(|i| PingOutbound::perform_ping(i))
                .map_err(|_| ())
                .wait()
                .for_each(drop);
        });

        PingOutbound { tx, handle }
    }

    fn perform_ping(task: PingTask) {
        match task {
            PingTask::V4 { value } => {
                let bindaddress = format!("{}:0", Ipv4Addr::from(value.source_address).to_string());
                let socket =
                    Socket::new(Domain::ipv4(), Type::raw(), Some(Protocol::icmpv4())).unwrap();
                socket
                    .bind(
                        &bindaddress
                            .to_string()
                            .parse::<SocketAddr>()
                            .unwrap()
                            .into(),
                    )
                    .unwrap();
                socket.listen(128);

                for ip in value.destination_addresses {
                    let bindaddress = format!("{}:0", Ipv4Addr::from(ip).to_string());
                    let icmp = ICMP4Header::echo_request(1, 2);
                    socket
                        .send_to(
                            &icmp.to_byte_array(),
                            &bindaddress
                                .to_string()
                                .parse::<SocketAddr>()
                                .unwrap()
                                .into(),
                        )
                        .expect("unable to call send_to on socket");
                }
            }
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub struct ICMP4Header {
    pub icmp_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub header: u32,
}

impl ICMP4Header {
    pub fn echo_request(identifier: u16, sequence_number: u16) -> ICMP4Header {
        let header = ((identifier as u32) << 16) | (sequence_number as u32);
        let mut icmp4_header = ICMP4Header {
            icmp_type: 8,
            code: 0,
            checksum: 0,
            header: header,
        };
        let checksum = ICMP4Header::calc_checksum(&icmp4_header.to_byte_array());
        icmp4_header.checksum = checksum;
        icmp4_header
    }

    pub fn to_byte_array(&self) -> [u8; 8] {
        let mut buffer = [0; 8];
        buffer[0] = self.icmp_type;
        buffer[1] = self.code;
        buffer[2] = (self.checksum >> 8 & 0xFF) as u8;
        buffer[3] = (self.checksum & 0xFF) as u8;
        buffer[4] = (self.header >> 24 & 0xFF) as u8;
        buffer[5] = (self.header >> 16 & 0xFF) as u8;
        buffer[6] = (self.header >> 8 & 0xFF) as u8;
        buffer[7] = (self.header & 0xFF) as u8;
        buffer
    }

    fn calc_checksum(buffer: &[u8]) -> u16 {
        let mut size = buffer.len();
        let mut checksum: u32 = 0;
        while size > 0 {
            let word = (buffer[buffer.len() - size] as u16) << 8
                | (buffer[buffer.len() - size + 1]) as u16;
            checksum += word as u32;
            size -= 2;
        }
        let remainder = checksum >> 16;
        checksum &= 0xFFFF;
        checksum += remainder;
        checksum ^= 0xFFFF;
        checksum as u16
    }
}
