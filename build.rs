fn main() {
    prost_build::compile_protos(&["src/schema/verfploeter.proto"],
                                &["src/"]).unwrap();
}
