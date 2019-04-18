#!/bin/bash
GRPC_PLUGIN=$(which grpc_rust_plugin)
protoc --grpc_out=src/schema --plugin=protoc-gen-grpc=$GRPC_PLUGIN src/schema/verfploeter.proto

