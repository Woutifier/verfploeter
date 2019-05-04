pub mod ping_inbound;
pub mod ping_outbound;
use super::{Receiver, Sender, Task};
use std::time::{SystemTime, UNIX_EPOCH};

pub enum ChannelType {
    Task {
        sender: Option<Sender<Task>>,
        receiver: Option<Receiver<Task>>,
    },
    None,
}

pub trait TaskHandler {
    fn start(&mut self);
    fn exit(&mut self);
    fn get_channel(&mut self) -> ChannelType;
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}
