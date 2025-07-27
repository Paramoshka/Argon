FROM rust:1.78-slim as builder
WORKDIR /build
COPY src/ /build
RUN cargo install bpf-linker
RUN cargo build --release