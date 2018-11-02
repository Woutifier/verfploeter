use super::schema::verfploeter::{Metadata, PingV4};
use super::schema::verfploeter_grpc::VerfploeterClient;
use futures::sync::mpsc::{channel, Receiver, Sender};
use futures::*;
use grpcio::{ChannelBuilder, Environment};
use std::sync::Arc;
//use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread::JoinHandle;

use socket2::{Domain, Protocol, Socket, Type};

use byteorder::{LittleEndian, NetworkEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use std::io::Write;
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
    ping_inbound: PingInbound,
}

impl Client {
    pub fn new() -> Client {
        let env = Arc::new(Environment::new(1));
        let channel = ChannelBuilder::new(env).connect("127.0.0.1:50001");
        let grpc_client = VerfploeterClient::new(channel);
        Client {
            ping_outbound: PingOutbound::new(),
            ping_inbound: PingInbound::new(),
            grpc_client,
        }
    }

    pub fn start(self) {
        // Setup outbound ping
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

            // Wait for ping_outbound thread to exit
            self.ping_outbound.handle.join().unwrap();
            debug!("outbound ping thread exited");

            // Wait for ping_inbound thread to exit
            self.ping_inbound.handle.join().unwrap();

            debug!("exiting");
        }
    }
}

struct PingInbound {
    pub handle: JoinHandle<()>,
}

impl PingInbound {
    pub fn new() -> PingInbound {
        let handle = thread::spawn(move || {
            let socket =
                Socket::new(Domain::ipv4(), Type::raw(), Some(Protocol::icmpv4())).unwrap();
            socket
                .bind(&"0.0.0.0:0".parse::<SocketAddr>().unwrap().into())
                .unwrap();
            socket.listen(128);

            let mut buffer: Vec<u8> = vec![0; 1500];
            loop {
                if let Ok(result) = socket.recv(&mut buffer) {
                    let packet = IPv4Packet::from(&buffer[..result]);
                    info!("packet: {:?}", packet);
                }
            }
            debug!("exited pinginbound loop");
        });
        PingInbound { handle }
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
                debug!(
                    "starting outbound ping from {}, to {} addresses",
                    value.source_address,
                    value.destination_addresses.len()
                );
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

                for ip in value.destination_addresses {
                    let bindaddress = format!("{}:0", Ipv4Addr::from(ip).to_string());
                    let icmp = ICMP4Packet::echo_request(1, 2, Vec::new());
                    socket
                        .send_to(
                            &icmp,
                            &bindaddress
                                .to_string()
                                .parse::<SocketAddr>()
                                .unwrap()
                                .into(),
                        )
                        .expect("unable to call send_to on socket");
                }
                debug!("finished ping");
            }
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub struct IPv4Packet {
    pub source_address: Ipv4Addr,
    pub destination_address: Ipv4Addr,
    pub payload: PacketPayload,
}

#[derive(Debug)]
pub enum PacketPayload {
    ICMPv4 { value: ICMP4Packet },
    Unimplemented,
}

impl From<&[u8]> for IPv4Packet {
    fn from(data: &[u8]) -> Self {
        let mut cursor = Cursor::new(data);
        // Get header length, which is the 4 right bits in the first byte (hence & 0xF)
        // header length is in number of 32 bits i.e. 4 bytes (hence *4)
        let header_length: usize = ((cursor.read_u8().unwrap() & 0xF)*4).into();

        cursor.set_position(9);
        let packet_type = cursor.read_u8().unwrap();

        cursor.set_position(12);
        let source_address = Ipv4Addr::from(cursor.read_u32::<NetworkEndian>().unwrap());
        let destination_address = Ipv4Addr::from(cursor.read_u32::<NetworkEndian>().unwrap());

        let payload_bytes = &cursor.into_inner()[header_length..];
        let payload = match packet_type {
            1 => PacketPayload::ICMPv4{value: ICMP4Packet::from(payload_bytes)},
            _ => PacketPayload::Unimplemented,
        };

        IPv4Packet {
            source_address,
            destination_address,
            payload,
        }
    }
}

#[derive(Debug)]
pub struct ICMP4Packet {
    pub icmp_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub identifier: u16,
    pub sequence_number: u16,
    pub body: Vec<u8>,
}

impl From<&[u8]> for ICMP4Packet {
    fn from(data: &[u8]) -> Self {
        let mut data = Cursor::new(data);
        ICMP4Packet {
            icmp_type: data.read_u8().unwrap(),
            code: data.read_u8().unwrap(),
            checksum: data.read_u16::<NetworkEndian>().unwrap(),
            identifier: data.read_u16::<NetworkEndian>().unwrap(),
            sequence_number: data.read_u16::<NetworkEndian>().unwrap(),
            body: data.into_inner()[8..].iter().cloned().collect()
        }
    }
}

impl Into<Vec<u8>> for &ICMP4Packet {
    fn into(self) -> Vec<u8> {
        let mut wtr = vec![];
        wtr.write_u8(self.icmp_type)
            .expect("Unable to write to byte buffer for ICMP packet");
        wtr.write_u8(self.code)
            .expect("Unable to write to byte buffer for ICMP packet");
        wtr.write_u16::<NetworkEndian>(self.checksum)
            .expect("Unable to write to byte buffer for ICMP packet");
        wtr.write_u16::<NetworkEndian>(self.identifier)
            .expect("Unable to write to byte buffer for ICMP packet");
        wtr.write_u16::<NetworkEndian>(self.sequence_number)
            .expect("Unable to write to byte buffer for ICMP packet");
        wtr.write_all(&self.body)
            .expect("Unable to write to byte buffer for ICMP packet");
        wtr
    }
}

impl ICMP4Packet {
    pub fn echo_request(identifier: u16, sequence_number: u16, body: Vec<u8>) -> Vec<u8> {
        let mut packet = ICMP4Packet {
            icmp_type: 8,
            code: 0,
            checksum: 0,
            identifier,
            sequence_number,
            body,
        };

        // Turn everything into a vec of bytes and calculate checksum
        let bytes: Vec<u8> = (&packet).into();
        let checksum = ICMP4Packet::calc_checksum(&bytes);
        packet.checksum = checksum;

        // Put the checksum at the right position in the packet (calling into() again is also
        // possible but is likely slower).
        let mut cursor = Cursor::new(bytes);
        cursor.set_position(2); // Skip icmp_type (1 byte) and code (1 byte)
        cursor.write_u16::<NetworkEndian>(checksum);

        // Return the vec
        cursor.into_inner()
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
