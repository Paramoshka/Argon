# --- build stage ---
FROM rust:1.89-bookworm AS builder
ARG TARGETARCH

RUN apt-get update && apt-get install -y --no-install-recommends \
      musl-tools pkg-config file ca-certificates linux-perf \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY dataplane/Cargo.toml Cargo.toml
COPY dataplane/Cargo.lock Cargo.lock
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release || true && rm -rf src

COPY dataplane/src src/



RUN set -eux; \
  case "${TARGETARCH:-amd64}" in \
    amd64) T=x86_64-unknown-linux-musl ;; \
    arm64) T=aarch64-unknown-linux-musl ;; \
    *) echo "Unsupported TARGETARCH=$TARGETARCH" && exit 1 ;; \
  esac; \
  rustup target add "$T"; \
  export CARGO_PROFILE_RELEASE_DEBUG=true;  \
  cargo  build --release --target "$T" --bin dataplane; \
  install -Dm0755 "target/$T/release/dataplane" /workspace/dataplane; \
  file /workspace/dataplane

# install flamegraff
RUN cargo install flamegraph

ENTRYPOINT ["/usr/local/cargo/bin/flamegraph"]

CMD ["-o", "/usr/local/pprof/flamegraph.svg", "--", "/workspace/dataplane"]
