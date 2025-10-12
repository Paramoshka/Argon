# --- build stage ---
FROM rust:1.89-bookworm AS builder
ARG TARGETARCH

RUN apt-get update && apt-get install -y --no-install-recommends \
      musl-tools pkg-config file ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY images/dataplane/Cargo.toml Cargo.toml
COPY images/dataplane/Cargo.lock Cargo.lock
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release || true && rm -rf src

COPY images/dataplane/src src/

RUN set -eux; \
  case "${TARGETARCH:-amd64}" in \
    amd64) T=x86_64-unknown-linux-musl ;; \
    arm64) T=aarch64-unknown-linux-musl ;; \
    *) echo "Unsupported TARGETARCH=$TARGETARCH" && exit 1 ;; \
  esac; \
  rustup target add "$T"; \
  cargo build --release --target "$T" --bin dataplane; \
  install -Dm0755 "target/$T/release/dataplane" /workspace/dataplane; \
  strip /workspace/dataplane || true; \
  file /workspace/dataplane

# --- final stage ---
# 'static' не содержит динамического линковщика, идеально для MUSL-бинарей
FROM gcr.io/distroless/static-debian12
COPY --from=builder /workspace/dataplane /usr/local/bin/dataplane
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/dataplane"]
