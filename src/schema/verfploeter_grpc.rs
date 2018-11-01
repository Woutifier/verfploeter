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

const METHOD_VERFPLOETER_DO_TASK: ::grpcio::Method<super::verfploeter::ScheduleTask, super::verfploeter::Ack> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/Verfploeter/do_task",
    req_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
    resp_mar: ::grpcio::Marshaller { ser: ::grpcio::pb_ser, de: ::grpcio::pb_de },
};

const METHOD_VERFPLOETER_LIST_CLIENTS: ::grpcio::Method<super::verfploeter::Empty, super::verfploeter::ClientList> = ::grpcio::Method {
    ty: ::grpcio::MethodType::Unary,
    name: "/Verfploeter/list_clients",
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

    pub fn do_task_opt(&self, req: &super::verfploeter::ScheduleTask, opt: ::grpcio::CallOption) -> ::grpcio::Result<super::verfploeter::Ack> {
        self.client.unary_call(&METHOD_VERFPLOETER_DO_TASK, req, opt)
    }

    pub fn do_task(&self, req: &super::verfploeter::ScheduleTask) -> ::grpcio::Result<super::verfploeter::Ack> {
        self.do_task_opt(req, ::grpcio::CallOption::default())
    }

    pub fn do_task_async_opt(&self, req: &super::verfploeter::ScheduleTask, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::verfploeter::Ack>> {
        self.client.unary_call_async(&METHOD_VERFPLOETER_DO_TASK, req, opt)
    }

    pub fn do_task_async(&self, req: &super::verfploeter::ScheduleTask) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::verfploeter::Ack>> {
        self.do_task_async_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_clients_opt(&self, req: &super::verfploeter::Empty, opt: ::grpcio::CallOption) -> ::grpcio::Result<super::verfploeter::ClientList> {
        self.client.unary_call(&METHOD_VERFPLOETER_LIST_CLIENTS, req, opt)
    }

    pub fn list_clients(&self, req: &super::verfploeter::Empty) -> ::grpcio::Result<super::verfploeter::ClientList> {
        self.list_clients_opt(req, ::grpcio::CallOption::default())
    }

    pub fn list_clients_async_opt(&self, req: &super::verfploeter::Empty, opt: ::grpcio::CallOption) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::verfploeter::ClientList>> {
        self.client.unary_call_async(&METHOD_VERFPLOETER_LIST_CLIENTS, req, opt)
    }

    pub fn list_clients_async(&self, req: &super::verfploeter::Empty) -> ::grpcio::Result<::grpcio::ClientUnaryReceiver<super::verfploeter::ClientList>> {
        self.list_clients_async_opt(req, ::grpcio::CallOption::default())
    }
    pub fn spawn<F>(&self, f: F) where F: ::futures::Future<Item = (), Error = ()> + Send + 'static {
        self.client.spawn(f)
    }
}

pub trait Verfploeter {
    fn connect(&mut self, ctx: ::grpcio::RpcContext, req: super::verfploeter::Metadata, sink: ::grpcio::ServerStreamingSink<super::verfploeter::Task>);
    fn do_task(&mut self, ctx: ::grpcio::RpcContext, req: super::verfploeter::ScheduleTask, sink: ::grpcio::UnarySink<super::verfploeter::Ack>);
    fn list_clients(&mut self, ctx: ::grpcio::RpcContext, req: super::verfploeter::Empty, sink: ::grpcio::UnarySink<super::verfploeter::ClientList>);
}

pub fn create_verfploeter<S: Verfploeter + Send + Clone + 'static>(s: S) -> ::grpcio::Service {
    let mut builder = ::grpcio::ServiceBuilder::new();
    let mut instance = s.clone();
    builder = builder.add_server_streaming_handler(&METHOD_VERFPLOETER_CONNECT, move |ctx, req, resp| {
        instance.connect(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_VERFPLOETER_DO_TASK, move |ctx, req, resp| {
        instance.do_task(ctx, req, resp)
    });
    let mut instance = s.clone();
    builder = builder.add_unary_handler(&METHOD_VERFPLOETER_LIST_CLIENTS, move |ctx, req, resp| {
        instance.list_clients(ctx, req, resp)
    });
    builder.build()
}
