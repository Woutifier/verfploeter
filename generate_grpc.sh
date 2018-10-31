#!/bin/bash
GRPC_PLUGIN=$(which grpc_rust_plugin)
protoc --rust_out=src/schema --grpc_out=src/schema --plugin=protoc-gen-grpc=$GRPC_PLUGIN schema/verfploeter.proto

