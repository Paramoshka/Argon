# Argon

**Argon** is an educational, high-performance, modular web server written in pure C.  
The goal of this project is to understand how modern web servers like Nginx or HAProxy work under the hood — by building one from scratch.

> Noble by name, fast by design.

---

## Features

- 🧩 **Modular architecture** — dynamic module loading via `dlopen()`.
- ⚡ **High performance** — event-driven, asynchronous I/O.
- 🖥️ **HTTP server** — basic GET support, static file serving.
- 🔩 **Reverse proxy (planned)** — simple proxying to upstream servers.
- 🛠️ **Configurable** — load settings from config files.
- 📊 **Logging** — multiple log levels: INFO, DEBUG, ERROR.

---

## Roadmap

### MVP v0.1 — Minimal Working Server

- [x] Socket initialization, bind, listen.
- [x] Accept incoming connections.
- [x] Basic HTTP GET request handling.
- [x] Send "Hello, World!" response.
- [ ] Basic routing support.
- [ ] Graceful client disconnects.

### v0.2 — Asynchronous I/O

- [ ] Implement `epoll` (Linux) or `kqueue` (BSD/macOS).
- [ ] Handle multiple clients concurrently.
- [ ] Basic connection timeout handling.

### v0.3 — Module System

- [ ] Implement dynamic module loading with `dlopen()`.
- [ ] Define API for modules (`init()`, `handle_request()`, `cleanup()`).
- [ ] Build simple static file serving module.
- [ ] Create reverse proxy module (simple HTTP forwarding).

### v0.4 — Configuration System

- [ ] Parse configuration file (INI or TOML).
- [ ] Support port, address, module paths from config.
- [ ] Hot-reload (optional).

### v0.5 — Logging & Diagnostics

- [ ] Implement basic logging to stdout/stderr.
- [ ] Add configurable log levels.
- [ ] Track and display simple connection metrics.

### Future Ideas

- [ ] TLS/SSL support (via OpenSSL or LibreSSL).
- [ ] HTTP/2 or HTTP/3 support.
- [ ] Modular plugin system for rate limiting, caching, etc.
- [ ] Embedded benchmarking tool.
- [ ] Containerized deployment example.
- [ ] GitHub Actions CI pipeline.

---

## Build & Run

### Prerequisites

- GCC or Clang
- CMake or Make
- Linux / BSD / macOS

### Build

```bash
mkdir build
cd build
make

