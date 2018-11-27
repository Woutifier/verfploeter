pub mod verfploeter;
pub mod verfploeter_grpc;

use self::verfploeter::{Address, TaskResult, PingPayload};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use std::fmt;

use sha2::Sha256;
use hmac::{Hmac, Mac};

use protobuf;
use protobuf::Message;
use std::error::Error;

type HmacSha256 = Hmac<Sha256>;

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
        for (idx, result) in self.get_result_list().iter().enumerate() {
            if idx != 0 {
                writeln!(f).unwrap();
            }
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
                )
                .unwrap();
            } else {
                writeln!(f, "{}|{}|unsupported-result", task_id, client_id).unwrap();
            }
        }
        Ok(())
    }
}

pub trait Signable<T> {
    fn to_signed_bytes(&self, secret: &str) -> Result<Vec<u8>, Box<Error>>;
    fn from_signed_bytes(secret: &str, buffer: &[u8]) -> Result<T, Box<Error>>;
}

impl Signable<PingPayload> for PingPayload {
    fn to_signed_bytes(&self, secret: &str) -> Result<Vec<u8>, Box<Error>>{
        // Create HMAC-SHA256 instance which implements `Mac` trait
        let mut mac = HmacSha256::new_varkey(secret.as_ref())?;

        let mut payload_bytes = self.write_to_bytes()?;
        mac.input(&payload_bytes);

        // `result` has type `MacResult` which is a thin wrapper around array of
        // bytes for providing constant time equality check
        let result = mac.result().code();
        payload_bytes.extend(result);

        Ok(payload_bytes)
    }

    fn from_signed_bytes(secret: &str, buffer: &[u8]) -> Result<PingPayload, Box<Error>> {
        // Create HMAC-SHA256 instance which implements `Mac` trait
        let mut mac = HmacSha256::new_varkey(secret.as_ref())?;

        let signature = &buffer[buffer.len()-32..];
        let value = &buffer[..buffer.len()-32];

        mac.input(value);
        mac.verify(signature)?;

        Ok(protobuf::parse_from_bytes::<PingPayload>(value)?)
    }
}

#[cfg(test)]
mod signable_pingpayload {
    use super::*;

    #[test]
    fn valid_signature_length() {
        let pp = PingPayload::new();
        let pp_bytes = pp.write_to_bytes().unwrap();
        let pp_bytes_signed = pp.to_signed_bytes("abc123").unwrap();
        assert_eq!(pp_bytes.len(), pp_bytes_signed.len() - 32);
    }

    #[test]
    fn validates_with_correct_secret() {
        let mut pp = PingPayload::new();
        pp.set_task_id(1234);

        let pp_bytes_signed = pp.to_signed_bytes("abc123").unwrap();
        let pp2 = PingPayload::from_signed_bytes("abc123", &pp_bytes_signed).unwrap();

        assert_eq!(pp, pp2);
    }

    #[test]
    fn does_not_validate_with_incorrect_secret() {
        let mut pp = PingPayload::new();
        pp.set_task_id(1234);

        let pp_bytes_signed = pp.to_signed_bytes("abc123").unwrap();
        let pp2 = PingPayload::from_signed_bytes("abc124", &pp_bytes_signed);

        assert!(pp2.is_err(), "payload should not pass validation with incorrect secret");
    }

    #[test]
    fn does_not_validate_with_incorrect_payload() {
        let mut pp = PingPayload::new();
        pp.set_task_id(1234);

        let mut pp_bytes_signed = pp.to_signed_bytes("abc123").unwrap();

        pp_bytes_signed[2] = pp_bytes_signed[2] + 1;
        let pp2 = PingPayload::from_signed_bytes("abc123", &pp_bytes_signed);

        assert!(pp2.is_err(), "payload should not pass validation with incorrect payload");
    }
}