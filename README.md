# Argon Ingress Controller

## Short version: a minimal, fast ingress controller split into a Go control-plane (discovers config) and a Rust data-plane (does the actual proxying). The two talk over gRPC streaming.

---

### Quick start

1) helm repo add argon https://raw.githubusercontent.com/Paramoshka/Argon/main/.deploy
2) helm repo update
3) helm install argon argon/argon --version 0.2.0 --set-string image.tag=v0.2.0

## Architecture

### Control-plane (Go)
* Watches your cluster (Ingresses/Services/etc.).
* Builds immutable Snapshots: { routes[], clusters[] }.
* Streams updates with Watch(WatchRequest{ node_id }) → stream Snapshot

### Data-plane (Rust)
* Async reverse proxy built on Tokio + hyper v1 + hyper-util + tonic.
* Maintains in-memory RouteTable and Clusters from the Snapshot.
* Load-balancing: RoundRobin (more to come).
* Zero-copy hot updates (reads guarded by RwLock, no restarts).
* Graceful shutdown: stops accepting, waits active conns, then exits.

---
### Routing model (from Snapshot)

* Route: { host, path, path_type: "Prefix"|"Exact", cluster, priority }
* Cluster: { name, lb_policy: "RoundRobin", endpoints[], timeout_ms, retries }
* Endpoint: { address, port, weight, zone, region }

### Data-plane flow:
* Extract Host (prefer header, fallback to absolute URI).
* Match Route by (host, path).
* Pick an Endpoint via the cluster’s LB policy.
* Proxy request to address:port.

### Ingress configuration

How to configure Ingress (annotations, timeouts, backend protocols) — see **docs/settings.md**.

👉 [Configuration guide](./docs/settings.md)


---
### Status & roadmap

* ✅ Go control-plane → streaming Snapshots

* ✅ Rust data-plane → proxy, health/readiness, RR LB

* ✅ HTTP/2|TLS upstreams, per-route timeouts

* ⏳ mTLS between planes

* ⏳ Metrics & OpenTelemetry

* ⏳ Canary/weights, headers/rewrites

---
## License

TBD (MIT/Apache-2.0 suggested).

---
## Contributing

### PRs welcome.