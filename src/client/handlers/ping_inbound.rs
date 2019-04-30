use super::{current_timestamp, ChannelType, TaskHandler};
use crate::net::{IPv4Packet, PacketPayload};
use crate::schema::verfploeter::{Client, Metadata, PingPayload, PingResult, Result, TaskResult};
use crate::schema::verfploeter_grpc::VerfploeterClient;
use crate::schema::Signable;
use socket2::{Domain, Protocol, Socket, Type};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use futures::sync::mpsc::{channel, Receiver, Sender};
use futures::sync::oneshot;
use futures::Future;
use futures::Sink;
use futures::Stream;
use std::net::Shutdown;
use std::sync::Mutex;
use std::time::Duration;
use std::u32;

pub struct PingInbound {
    handles: Vec<JoinHandle<()>>,
    socket: Arc<Socket>,
    grpc_client: Arc<VerfploeterClient>,
    metadata: Metadata,
    result_queue: Arc<Mutex<Option<Vec<Result>>>>,
    poison_rx: oneshot::Receiver<()>,
    poison_tx: Option<oneshot::Sender<()>>,
}

impl TaskHandler for PingInbound {
    fn start(&mut self) {
        let (tx, rx): (Sender<IPv4Packet>, Receiver<IPv4Packet>) = channel(1024);

        // The packet receiver thread takes the packets from the actual socket
        // and puts them in a channel to be processed
        let packet_receiver_handle = thread::spawn({
            let socket = self.socket.clone();
            move || {
                let mut buffer: Vec<u8> = vec![0; 1500];
                while let Ok(result) = socket.recv(&mut buffer) {
                    if result == 0 {
                        break;
                    }

                    let packet = IPv4Packet::from(&buffer[..result]);
                    tx.clone()
                        .send(packet)
                        .wait()
                        .expect("unable to send packet to tx channel");
                }
            }
        });

        // The packet processor thread takes the packets from the packet receiver thread channel
        // processes them (check the payload, create the protobuf struct) and puts them in a
        // buffer for transmission to the server
        let packet_processor_handle = thread::spawn({
            let result_queue = self.result_queue.clone();
            move || {
                rx.for_each(|packet| {
                    // Note the receive time
                    let receive_time = current_timestamp();

                    // Extract payload
                    let mut ping_payload = None;
                    if let PacketPayload::ICMPv4 { value } = packet.payload {
                        // Todo: make the secret configurable
                        let payload = PingPayload::from_signed_bytes("test-secret", &value.body);
                        if let Ok(payload) = payload {
                            ping_payload = Some(payload);
                        }
                    }

                    // Don't do anything if we don't have a proper payload
                    if ping_payload.is_none() {
                        return futures::future::ok(());
                    }
                    let ping_payload = ping_payload.unwrap();

                    let mut result = Result::new();
                    let mut pr = PingResult::new();

                    pr.set_payload(ping_payload);
                    pr.set_source_address(packet.source_address.into());
                    pr.set_destination_address(packet.destination_address.into());
                    pr.set_receive_time(receive_time);
                    pr.set_ttl(packet.ttl.into());
                    result.set_ping(pr);

                    // Put result in transmission queue
                    {
                        let mut rq_opt = result_queue.lock().unwrap();
                        if let Some(ref mut x) = *rq_opt {
                            x.push(result);
                        }
                    }

                    futures::future::ok(())
                })
                .map_err(|_| ())
                .wait()
                .unwrap();
            }
        });

        // The packet transmitter thread periodically swaps out the buffer the packet processor
        // writes its results to and starts transmitting the data
        let packet_transmitter_handle = thread::spawn({
            let grpc_client = self.grpc_client.clone();
            let result_queue = self.result_queue.clone();
            let poison_tx = self.poison_tx.take().unwrap();
            let metadata = self.metadata.clone();
            move || {
                loop {
                    thread::sleep(Duration::from_secs(5));

                    // Check if this thread is still supposed to be running
                    if poison_tx.is_canceled() {
                        break;
                    }

                    // Get the current result queue, and replace it with an empty one
                    let mut rq;
                    {
                        let mut result_queue = result_queue.lock().unwrap();
                        rq = result_queue.replace(Vec::new()).unwrap();
                    }

                    // Sort the result queue by task id
                    let mut rq_ping = rq.drain_filter(|x| x.has_ping()).collect::<Vec<Result>>();
                    rq_ping.sort_by_key(|x| x.get_ping().get_payload().get_task_id());

                    // Transmit the results, grouped by task id
                    let mut tr = TaskResult::new();
                    tr.set_task_id(u32::MAX);
                    for result in rq_ping {
                        let result_taskid = result.get_ping().get_payload().get_task_id();
                        if tr.get_task_id() != result_taskid {
                            // If the current 'result container' has some results, send it
                            if !tr.get_result_list().is_empty() {
                                if let Err(e) = grpc_client.send_result(&tr) {
                                    error!("failed to send result to server: {}", e);
                                }
                            }
                            tr = TaskResult::new();
                            tr.set_task_id(result_taskid);
                            let mut client = Client::new();
                            client.set_metadata(metadata.clone());
                            tr.set_client(client);
                        }
                        tr.mut_result_list().push(result);
                    }
                    if !tr.get_result_list().is_empty() {
                        if let Err(e) = grpc_client.send_result(&tr) {
                            error!("failed to send result to server: {}", e);
                        }
                    }
                }
            }
        });
        self.handles.push(packet_receiver_handle);
        self.handles.push(packet_processor_handle);
        self.handles.push(packet_transmitter_handle);
    }

    fn exit(&mut self) {
        self.socket.shutdown(Shutdown::Both).unwrap_err();
        self.poison_rx.close();
        for handle in self.handles.drain(..) {
            handle.join().unwrap();
        }
    }

    fn get_channel(&mut self) -> ChannelType {
        ChannelType::None
    }
}

impl PingInbound {
    pub fn new(metadata: Metadata, grpc_client: Arc<VerfploeterClient>) -> PingInbound {
        let socket =
            Arc::new(Socket::new(Domain::ipv4(), Type::raw(), Some(Protocol::icmpv4())).unwrap());
        let (poison_tx, poison_rx): (oneshot::Sender<()>, oneshot::Receiver<()>) =
            oneshot::channel();

        PingInbound {
            handles: Vec::new(),
            socket,
            grpc_client,
            metadata,
            result_queue: Arc::new(Mutex::new(Some(Vec::new()))),
            poison_tx: Some(poison_tx),
            poison_rx,
        }
    }
}
