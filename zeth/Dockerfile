# syntax=docker/dockerfile:1.4

# Use ubuntu:20.04 as the build environment
# https://github.com/risc0/risc0/blob/main/risc0/cargo-risczero/docker/Dockerfile.release
FROM ubuntu:20.04@sha256:3246518d9735254519e1b2ff35f95686e4a5011c90c85344c1f38df7bae9dd37 as build-environment

# Set non-interactive mode for apt so it doesn't ask for user input during the build
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y clang lld curl build-essential linux-headers-generic git libssl-dev pkg-config  \
    && curl --proto '=https' --tlsv1.2 --retry 10 --retry-connrefused -fsSL 'https://sh.rustup.rs' | sh -s -- -y 
ENV PATH="/root/.cargo/bin:${PATH}"

ARG TARGETARCH
WORKDIR /opt

WORKDIR /opt/zeth
COPY . .

# TODO: Consider creating a base image with the risc zero dependencies installed
RUN . $HOME/.profile && cargo install cargo-risczero && cargo risczero install && cargo build --bin zeth --release --locked \
    && mkdir out \
    && mv target/release/zeth  out/zeth \
    && strip out/zeth 

# Use debian:bookworm-slim for the client
FROM ubuntu:20.04@sha256:3246518d9735254519e1b2ff35f95686e4a5011c90c85344c1f38df7bae9dd37 as zeth-client

ENV DEBIAN_FRONTEND=noninteractive

# Install required dependencies
RUN apt-get update && apt-get install -y  ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the build environment
COPY --from=build-environment /opt/zeth/out/zeth /usr/local/bin/zeth

# Add a user for zeth
RUN useradd -ms /bin/bash zeth

ENTRYPOINT ["zeth"]