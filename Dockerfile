# syntax=docker/dockerfile:1.4

# Use rust:bookworm as the build environment
FROM ubuntu:22.04 as build-environment

# Set non-interactive mode for apt so it doesn't ask for user input during the build
ENV DEBIAN_FRONTEND=noninteractive


RUN apt-get update && apt-get install cmake -y

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
FROM debian:bookworm-slim as zeth-client

ENV DEBIAN_FRONTEND=noninteractive

# Install required dependencies

RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the build environment
COPY --from=build-environment /opt/zeth/out/zeth /usr/local/bin/zeth

# Add a user for zeth
RUN useradd -ms /bin/bash zeth

ENTRYPOINT ["zeth"]