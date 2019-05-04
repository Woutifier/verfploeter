use super::{current_timestamp, ChannelType, TaskHandler};
use crate::net::ICMP4Packet;
use crate::schema::verfploeter::{PingPayload, Task, TaskId};
use crate::schema::verfploeter_grpc::VerfploeterClient;
use crate::schema::Signable;

use futures::sync::mpsc::{channel, Receiver, Sender};
use futures::sync::oneshot;
use futures::{Future, Stream};
use lazy_static::lazy_static;
use prometheus::{opts, register_counter, register_int_counter, IntCounter};
use ratelimit_meter::{DirectRateLimiter, LeakyBucket};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::u32;

// Define Prometheus metrics
lazy_static! {
    static ref PACKETS_TRANSMITTED_OK: IntCounter = register_int_counter!(
        "client_ping_outbound_packets_transmitted_ok",
        "Number of packets transmitted successfully"
    )
    .unwrap();
    static ref PACKETS_TRANSMITTED_ERROR: IntCounter = register_int_counter!(
        "client_ping_outbound_packets_transmitted_error",
        "Number of packets transmitted which failed"
    )
    .unwrap();
}

pub struct PingOutbound {
    tx: Sender<Task>,
    rx: Option<Receiver<Task>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    shutdown_rx: Option<oneshot::Receiver<()>>,
    handle: Option<JoinHandle<()>>,
    grpc_client: Arc<VerfploeterClient>,
    outbound_mutex: Arc<Mutex<u32>>,
}

impl TaskHandler for PingOutbound {
    fn start(&mut self) {
        let handle = thread::spawn({
            let grpc_client = Arc::clone(&self.grpc_client);
            let rx = self.rx.take().unwrap();
            let shutdown_rx = self.shutdown_rx.take().unwrap();
            let outbound_mutex = Arc::clone(&self.outbound_mutex);
            move || {
                let handler = rx
                    .for_each(|i| {
                        // Start the actual pinging process in a different thread
                        // otherwise the GRPC stream will die if it takes too long
                        debug!("starting ping thread");
                        PingOutbound::start_ping_thread(
                            Arc::clone(&grpc_client),
                            Arc::clone(&outbound_mutex),
                            i,
                        );

                        futures::future::ok(())
                    })
                    .map_err(|_| error!("exiting outbound thread"));
                let poison = shutdown_rx.map_err(|_| warn!("error on shutdown_rx"));
                handler
                    .select(poison)
                    .map_err(|_| warn!("error in handler select"))
                    .wait()
                    .unwrap();
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
            outbound_mutex: Arc::new(Mutex::new(0)),
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
                PACKETS_TRANSMITTED_ERROR.inc();
            } else {
                PACKETS_TRANSMITTED_OK.inc();
            }
        }
        debug!("finished ping");
    }

    fn start_ping_thread(
        grpc_client: Arc<VerfploeterClient>,
        outbound_mutex: Arc<Mutex<u32>>,
        task: Task,
    ) {
        thread::spawn({
            move || {
                debug!("ping thread started");
                // Perform the ping, locking the outbound_mutex, we only
                // want one outbound ping action going at a given time
                let guard = outbound_mutex.lock().unwrap();
                debug!("start pinging (task: {})", task.task_id);
                PingOutbound::perform_ping(&task);
                debug!("stop pinging (task: {})", task.task_id);
                drop(guard);

                // Wait for a timeout
                debug!("sleeping for duration to wait for final packets");
                thread::sleep(Duration::from_secs(10));
                debug!("slept for duration to wait for final packets");

                // After finishing notify the server that the task is finished
                let mut task_id = TaskId::new();
                task_id.task_id = task.task_id;
                grpc_client
                    .task_finished(&task_id.clone())
                    .expect("Could not deliver task finished notification");

                debug!("finished entire ping process");
            }
        });
    }
}
