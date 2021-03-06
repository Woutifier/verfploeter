use crate::schema::verfploeter::TaskResult;
use maxminddb::geoip2::{Country, Isp};
use maxminddb::Reader;
use serde::{Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;

pub trait Columnizable {
    fn get_data(&self) -> Vec<HashMap<String, RowData>>;
    fn get_headers() -> Vec<String>;
}

#[derive(Debug, Clone)]
pub enum RowData {
    String(String),
    Integer(u64),
    Float(f64),
    IpAddress(IpAddr),
}

impl Serialize for RowData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            RowData::String(d) => serializer.serialize_str(d),
            RowData::Integer(i) => serializer.serialize_u64(*i),
            RowData::Float(i) => serializer.serialize_f64(*i),
            RowData::IpAddress(i) => serializer.serialize_str(&i.to_string()),
        }
    }
}

impl fmt::Display for RowData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RowData::String(d) => write!(f, "{}", d),
            RowData::Integer(i) => write!(f, "{}", i),
            RowData::Float(i) => write!(f, "{}", i),
            RowData::IpAddress(i) => write!(f, "{}", i),
        }
    }
}

impl From<String> for RowData {
    fn from(data: String) -> RowData {
        RowData::String(data)
    }
}

impl From<&str> for RowData {
    fn from(data: &str) -> RowData {
        RowData::String(data.to_string())
    }
}

impl From<u32> for RowData {
    fn from(data: u32) -> RowData {
        RowData::Integer(data as u64)
    }
}

impl From<IpAddr> for RowData {
    fn from(data: IpAddr) -> RowData {
        RowData::IpAddress(data)
    }
}

impl From<u64> for RowData {
    fn from(data: u64) -> RowData {
        RowData::Integer(data)
    }
}

impl From<f64> for RowData {
    fn from(data: f64) -> RowData {
        RowData::Float(data)
    }
}

impl Columnizable for TaskResult {
    fn get_data(&self) -> Vec<HashMap<String, RowData>> {
        let task_id = self.get_task_id();
        let client_id = self.get_client().get_metadata().get_hostname();
        let mut results: Vec<HashMap<String, RowData>> = Vec::new();
        for result in self.get_result_list().iter() {
            if result.has_ping() {
                let ping = result.get_ping();
                let mut row: HashMap<String, RowData> = HashMap::new();
                row.insert("task_id".to_string(), task_id.into());
                row.insert("client_id".to_string(), client_id.into());
                row.insert(
                    "transmit_time".to_string(),
                    ping.get_payload().get_transmit_time().into());
                row.insert("receive_time".to_string(), ping.get_receive_time().into());
                row.insert(
                    "send_receive_time_diff".to_string(),
                    (((ping.get_receive_time() - ping.get_payload().get_transmit_time()) as f64)
                        / 1_000_000f64)
                        .into(),
                );
                row.insert(
                    "source_address".to_string(),
                    IpAddr::from(ping.get_source_address()).into(),
                );
                row.insert(
                    "destination_address".to_string(),
                    IpAddr::from(ping.get_destination_address()).into(),
                );
                row.insert(
                    "meta_source_address".to_string(),
                    IpAddr::from(ping.get_payload().get_source_address()).into(),
                );
                row.insert(
                    "meta_destination_address".to_string(),
                    IpAddr::from(ping.get_payload().get_destination_address()).into(),
                );
                row.insert("ttl".to_string(), ping.ttl.into());
                results.push(row);
            }
        }
        results
    }

    fn get_headers() -> Vec<String> {
        vec![
            "task_id",
            "client_id",
            "transmit_time",
            "receive_time",
            "send_receive_time_diff",
            "source_address",
            "destination_address",
            "meta_source_address",
            "meta_destination_address",
            "ttl",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
    }
}

pub trait Transformer {
    fn new(source: &str, destination: &str, data: &str) -> Box<Self>
    where
        Self: Sized;
    fn transform(&self, data: HashMap<String, RowData>) -> HashMap<String, RowData>;
    fn add_header(&self, header: &mut Vec<String>);
}

pub struct TransformPipeline {
    pub pipeline: Vec<Box<dyn Transformer>>,
}

pub struct IP2CountryTransformer {
    source: String,
    destination: String,
    mmreader: Reader<Vec<u8>>,
}

impl Transformer for IP2CountryTransformer {
    fn new(source: &str, destination: &str, data: &str) -> Box<Self>
    where
        Self: Sized,
    {
        let reader =
            maxminddb::Reader::open_readfile(data).expect("Could not open IP2Country file");
        Box::new(IP2CountryTransformer {
            source: source.to_string(),
            destination: destination.to_string(),
            mmreader: reader,
        })
    }

    fn transform(&self, mut data: HashMap<String, RowData>) -> HashMap<String, RowData> {
        if let Some(RowData::IpAddress(source_data)) = data.get(&self.source) {
            if let Ok(country) = self.mmreader.lookup::<Country>(*source_data) {
                if let Some(country) = country.country {
                    if let Some(iso_code) = country.iso_code {
                        data.insert(self.destination.to_string(), iso_code.into());
                        return data;
                    }
                }
            }
        }
        data.insert(self.destination.to_string(), "Unknown".into());
        data
    }

    fn add_header(&self, header: &mut Vec<String>) {
        header.push(self.destination.to_string());
    }
}

pub struct IP2ASNTransformer {
    source: String,
    destination: String,
    mmreader: Reader<Vec<u8>>,
}

impl Transformer for IP2ASNTransformer {
    fn new(source: &str, destination: &str, data: &str) -> Box<Self>
    where
        Self: Sized,
    {
        let reader = maxminddb::Reader::open_readfile(data).expect("Could not open IP2ASN file");
        Box::new(IP2ASNTransformer {
            source: source.to_string(),
            destination: destination.to_string(),
            mmreader: reader,
        })
    }

    fn transform(&self, mut data: HashMap<String, RowData>) -> HashMap<String, RowData> {
        if let Some(RowData::IpAddress(source_data)) = data.get(&self.source) {
            if let Ok(isp) = self.mmreader.lookup::<Isp>(*source_data) {
                if let Some(autonomous_system_number) = isp.autonomous_system_number {
                    data.insert(
                        self.destination.to_string(),
                        autonomous_system_number.into(),
                    );
                    return data;
                }
            }
        }
        data.insert(self.destination.to_string(), "Unknown".into());
        data
    }

    fn add_header(&self, header: &mut Vec<String>) {
        header.push(self.destination.to_string());
    }
}
