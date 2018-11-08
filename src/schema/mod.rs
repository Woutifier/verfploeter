pub mod verfploeter;
pub mod verfploeter_grpc;

use self::verfploeter::{Address, TaskResult};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use std::fmt;

impl From<&Address> for IpAddr {
    fn from(address: &Address) -> Self {
        if address.has_v4() {
            IpAddr::V4(Ipv4Addr::from(address.get_v4()))
        } else {
            let mut buffer = [0; 16];
            for (i, e) in address.get_v6().iter().enumerate() {
                buffer[i] = *e;
            }
            IpAddr::V6(Ipv6Addr::from(buffer))
        }
    }
}

impl From<Ipv4Addr> for Address {
    fn from(addr: Ipv4Addr) -> Self {
        let mut address = Address::new();
        address.set_v4(addr.into());
        address
    }
}

impl fmt::Display for TaskResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let task_id = self.get_task_id();
        let client_id = self.get_client().get_metadata().get_hostname();
        for result in self.get_result_list() {
            if result.has_ping() {
                let ping = result.get_ping();
                write!(
                    f,
                    "{}|{}|{}|{}|{}|{}",
                    task_id,
                    client_id,
                    IpAddr::from(ping.get_source_address()),
                    IpAddr::from(ping.get_destination_address()),
                    IpAddr::from(ping.get_payload().get_source_address()),
                    IpAddr::from(ping.get_payload().get_destination_address()),
                );
            } else {
                write!(f, "{}|{}|unsupported-result", task_id, client_id);
            }
        }
        Ok(())
    }
}
