use super::{ChannelType, TaskHandler};
use crate::net::{ICMP4Packet, IPv4Packet, PacketPayload};
use crate::schema::verfploeter::{
    Client, Metadata, PingPayload, PingResult, Result, Task, TaskId, TaskResult,
};
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
use ratelimit_meter::{DirectRateLimiter, LeakyBucket};
use std::net::{Ipv4Addr, Shutdown, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
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

pub struct PingOutbound {
    tx: Sender<Task>,
    rx: Option<Receiver<Task>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    shutdown_rx: Option<oneshot::Receiver<()>>,
    handle: Option<JoinHandle<()>>,
    grpc_client: Arc<VerfploeterClient>,
}

impl TaskHandler for PingOutbound {
    fn start(&mut self) {
        let handle = thread::spawn({
            let grpc_client = self.grpc_client.clone();
            let rx = self.rx.take().unwrap();
            let shutdown_rx = self.shutdown_rx.take().unwrap();
            move || {
                let handler = rx
                    .for_each(|i| {
                        // Perform the ping
                        PingOutbound::perform_ping(&i);

                        // Wait for a timeout
                        thread::sleep(Duration::from_secs(10));
                        // After finishing notify the server that the task is finished
                        let mut task_id = TaskId::new();
                        task_id.task_id = i.task_id;
                        grpc_client
                            .task_finished(&task_id.clone())
                            .expect("Could not deliver task finished notification");
                        futures::future::ok(())
                    })
                    .map_err(|_| error!("exiting outbound thread"));
                let poison = shutdown_rx.map_err(|_| ());
                handler.select(poison).map_err(|_| ()).wait().unwrap();
                debug!("Exiting outbound thread");
            }
        });
        self.handle = Some(handle);
    }

    fn exit(&mut self) {
        self.shutdown_tx.take().unwrap().send(()).unwrap();
        if self.handle.is_some() {
            self.handle.take().unwrap().join().unwrap();
        }
    }

    fn get_channel(&mut self) -> ChannelType {
        ChannelType::Task {
            sender: Some(self.tx.clone()),
            receiver: None,
        }
    }
}

impl PingOutbound {
    pub fn new(grpc_client: Arc<VerfploeterClient>) -> PingOutbound {
        let (tx, rx): (Sender<Task>, Receiver<Task>) = channel(10);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        PingOutbound {
            tx,
            rx: Some(rx),
            shutdown_tx: Some(shutdown_tx),
            shutdown_rx: Some(shutdown_rx),
            handle: None,
            grpc_client,
        }
    }

    fn perform_ping(task: &Task) {
        info!(
            "performing outbound ping from {}, to {} addresses, task id: {}",
            Ipv4Addr::from(task.get_ping().get_source_address().get_v4()),
            task.get_ping().get_destination_addresses().len(),
            task.get_task_id()
        );
        let bindaddress = format!(
            "{}:0",
            Ipv4Addr::from(task.get_ping().get_source_address().get_v4()).to_string()
        );
        let socket = Socket::new(Domain::ipv4(), Type::raw(), Some(Protocol::icmpv4())).unwrap();
        socket
            .bind(
                &bindaddress
                    .to_string()
                    .parse::<SocketAddr>()
                    .unwrap()
                    .into(),
            )
            .unwrap();

        let mut lb = DirectRateLimiter::<LeakyBucket>::per_second(NonZeroU32::new(10000).unwrap());
        for ip in task.get_ping().get_destination_addresses() {
            // Create payload that will be transmitted inside the ICMP echo request
            let mut payload = PingPayload::new();
            payload.set_source_address(task.get_ping().get_source_address().clone());
            payload.set_destination_address(ip.clone());
            payload.set_task_id(task.get_task_id());

            // Get the current time
            payload.set_transmit_time(current_timestamp());

            let bindaddress = format!("{}:0", Ipv4Addr::from(ip.get_v4()).to_string());
            // Todo: make the secret configurable
            let icmp =
                ICMP4Packet::echo_request(1, 2, payload.to_signed_bytes("test-secret").unwrap());

            // Rate limiting
            while let Err(_) = lb.check() {
                thread::sleep(Duration::from_millis(1));
                //thread::sleep(v.wait_time_from(Instant::now()));
            }

            if let Err(e) = socket.send_to(
                &icmp,
                &bindaddress
                    .to_string()
                    .parse::<SocketAddr>()
                    .unwrap()
                    .into(),
            ) {
                error!("Failed to send packet to socket: {:?}", e);
            }
        }
        debug!("finished ping");
    }
}

fn current_timestamp() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32
}
