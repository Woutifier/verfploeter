// This file is generated. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy)]

#![cfg_attr(rustfmt, rustfmt_skip)]

#![allow(box_pointers)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(trivial_casts)]
#![allow(unsafe_code)]
#![allow(unused_imports)]
#![allow(unused_results)]

const METHOD_VERFPLOETER_CONNECT: ::grpcio::Method<super::verfploeter::Metadata, super::verfploeter::Task> = ::grpcio::Method {
    ty: ::grpcio::MethodType::ServerStreaming,
    name: "/Verfploeter/connect",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

pub struct VerfploeterClient {
    client: ::grpcio::Client,
}

impl VerfploeterClient {
    pub fn new(channel: ::grpcio::Channel) -> Self {
        VerfploeterClient {
            client: ::grpcio::Client::new(channel),
        }
    }

    pub fn connect_opt(&self, req: &super::verfploeter::Metadata, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::verfploeter::Task>> {
        self.client.server_streaming(&METHOD_VERFPLOETER_CONNECT, req, opt)
    }

    pub fn connect(&self, req: &super::verfploeter::Metadata) -> ::grpcio::Result<::grpcio::ClientSStreamReceiver<super::verfploeter::Task>> {
        self.connect_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F) where F: ::futures::Future<Item = (), Error = ()> + Send + 'static {
        self.client.spawn(f)
    }
}

pub trait Verfploeter {
    fn connect(&mut self, ctx: ::grpcio::RpcContext, req: super::verfploeter::Metadata, sink: ::grpcio::ServerStreamingSink<super::verfploeter::Task>);
}

pub fn create_verfploeter<S: Verfploeter + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder.add_server_streaming_handler(&METHOD_VERFPLOETER_CONNECT, move |ctx, req, resp| {
        instance.connect(ctx, req, resp)
    });
    builder.build()
}
