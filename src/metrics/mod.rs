extern crate hyper;
use hyper::{Request, Response, Server, Body};
use hyper::rt::Future;
use hyper::service::service_fn_ok;

use prometheus::{self, Counter, Encoder, Gauge, HistogramVec, TextEncoder};
use std::net::SocketAddr;

pub struct Prometheus {
    addr: SocketAddr
}

impl Prometheus {
    pub fn new(addr: SocketAddr) -> Prometheus {
        Prometheus{
            addr
        }
    }

    pub fn start(&self) {
        println!("Starting Prometheus on {:?}", self.addr);

        let prometheus_svc = || {
          service_fn_ok(|_req|{
              let metric_families = prometheus::gather();
              let mut buffer = vec![];
              let encoder = TextEncoder::new();
              encoder.encode(&metric_families, &mut buffer).unwrap();
                Response::new(Body::from(buffer))
          })
        };

        let server = Server::bind(&self.addr)
            .serve(prometheus_svc)
            .map_err(|e| error!("failed to start prometheus: {}", e));

        hyper::rt::run(server);
    }
}