FROM debian:stable
RUN apt-get update && apt-get install -y curl build-essential cmake golang
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > /tmp/rustup.sh && chmod +x /tmp/rustup.sh && /tmp/rustup.sh -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup toolchain install nightly && rustup default nightly
VOLUME /source
VOLUME /target
ENTRYPOINT [ "sh", "-c", "cp -r /source /source_copy && cd /source_copy && cargo build --release --target-dir /target" ]
