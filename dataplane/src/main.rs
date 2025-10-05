mod certs;
mod grpc;
mod proxy;
mod snapshot;
mod utils;
mod client_pool;

use std::collections::HashMap;
use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use hyper_util::{rt::TokioExecutor, server::conn::auto};
use rustls::{ServerConfig};
use rustls::server::ResolvesServerCert;
use snapshot::RouteTable;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use arc_swap::ArcSwap;
use rustls::sign::CertifiedKey;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

mod argon_config {
    include!("argon.config.rs");
}
use crate::grpc::GrpcManager;
use crate::proxy::proxy_handler;
use argon_config::Snapshot;
use crate::client_pool::ClientPool;

#[derive(Clone, Default)]
struct AppState {
    client_pool: Arc<ArcSwap<ClientPool>>,
    ready: Arc<RwLock<bool>>,
    snapshot: Arc<RwLock<Snapshot>>,
    route_table: Arc<RwLock<Arc<RouteTable>>>,
    sni: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>,
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install ring CryptoProvider");
    // TLS provider Could not automatically determine the process-level CryptoProvider from Rustls crate features.
    let certs_dir = PathBuf::from("/certs");
    let thread_count = std::env::var("COUNT_THREADS").unwrap_or_else(|_| num_cpus::get().to_string());
    let thread_count = thread_count.parse::<usize>().unwrap_or_else(|_| 1);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(thread_count)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // env
            let http_port = std::env::var("HTTP_PORT").unwrap_or_else(|_| "8080".to_string());
            let https_port = std::env::var("HTTPS_PORT").unwrap_or_else(|_| "8443".to_string());
            let admin_port = std::env::var("ADMIN_PORT").unwrap_or_else(|_| "8181".to_string());
            let controller_addr =
                std::env::var("CONTROLLER_ADDR").unwrap_or_else(|_| "https://127.0.0.1:18000".into());
            let node_id = std::env::var("NODE_ID").unwrap_or_else(|_| "dp-axum".into());

            // log
            tracing_subscriber::fmt()
                .with_env_filter("info,tower_http=info,axum=info,tonic=info")
                .with_target(false)
                .compact()
                .init();

            // start not-ready; snap is empty (Default)
            let state = AppState {
                ready: Arc::new(RwLock::new(false)),
                snapshot: Arc::new(RwLock::new(Snapshot::default())),
                route_table: Arc::new(RwLock::new(Arc::new(RouteTable::default()))),
                sni: Arc::new(ArcSwap::new(Arc::new(HashMap::new()))),
                client_pool: Arc::new(ArcSwap::new(Arc::new(ClientPool::new_http_pool_connector(thread_count))))
            };

            // shutdown token
            let shutdown = CancellationToken::new();
            let shutdown_http = shutdown.clone();
            let shutdown_https = shutdown.clone();
            let shutdown_select = shutdown.clone();

            // Ctrl+C / SIGTERM -> cancel
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                shutdown.cancel();
            });

            // gRPC watcher
            let manager = GrpcManager::start(
                controller_addr,
                node_id,
                certs_dir,
                state.ready.clone(),
                state.snapshot.clone(),
                state.route_table.clone(),
                state.sni.clone(),
            );

            // healthcheck
            let addr = SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), admin_port.parse::<u16>()?));
            let admin_listener = TcpListener::bind(addr).await?;
            let admin_state = state.clone();

            tokio::spawn(async move {
                loop {
                    match admin_listener.accept().await {
                        Ok((stream_admin, _)) => {
                            let io_admin = TokioIo::new(stream_admin);
                            let state_for_conn = admin_state.clone();
                            let ab = auto::Builder::new(TokioExecutor::new());

                            tokio::spawn(async move {
                                let svc = service_fn(move |request: Request<Incoming>| {
                                    let ready = state_for_conn.ready.clone();
                                    async move { echo(request, ready).await }
                                });

                                if let Err(err) = ab.serve_connection(io_admin, svc).await {
                                    tracing::error!("admin serve_connection error: {err}");
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("admin accept error: {e}");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    }
                }
            });

            let http_addr  = SocketAddr::from((Ipv4Addr::UNSPECIFIED, http_port.parse()?));
            let https_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, https_port.parse()?));

            let dummy_cert = certs::make_dummy_cert()?;
            let server_cert_resolver: Arc<dyn ResolvesServerCert> =
                Arc::new(certs::DynResolver::new(dummy_cert, state.sni.clone()));
            let mut server_config = ServerConfig::builder()
                .with_no_client_auth()
                .with_cert_resolver(server_cert_resolver);
            server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec(), b"http/1.0".to_vec()];


            let http_handle  = tokio::spawn(run_http(http_addr,  state.clone(), shutdown_http.clone()));
            let https_handle = tokio::spawn(run_https(https_addr, state.clone(), server_config, shutdown_https.clone()));


            shutdown_select.cancelled().await;
            tracing::info!("shutdown requested; waiting servers to drain...");

            if let Err(e) = http_handle.await { tracing::error!("HTTP task join error: {e:?}"); }
            if let Err(e) = https_handle.await { tracing::error!("HTTPS task join error: {e:?}"); }

            // shutdown gRPC server after http server
            manager.shutdown().await;

            Ok(())
        })
}

async fn run_http(socket: SocketAddr, state: AppState, shutdown: CancellationToken) -> anyhow::Result<()> {
    let listener = TcpListener::bind(socket).await?;
    tracing::info!("HTTP listening on {}", socket);

    let mut conns = JoinSet::new();
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder.http1().title_case_headers(true);
    builder.http2().auto_date_header(true);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                tracing::info!("HTTP: stop accepting new connections");
                break;
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, _)) => {
                        let io = TokioIo::new(stream);
                        let state_cloned = state.clone();
                        let builder = builder.clone();
                        conns.spawn(async move {
                            let svc = service_fn(move |req: Request<Incoming>| proxy_handler(req, state_cloned.clone()));
                            if let Err(err) = builder.serve_connection_with_upgrades(io, svc).await {
                                tracing::error!("HTTP conn error: {err:?}");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("HTTP accept error: {e}; retry in 100ms");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }

    while conns.join_next().await.is_some() {}
    Ok(())
}

async fn run_https(socket: SocketAddr, state: AppState, server_config: ServerConfig, shutdown: CancellationToken) -> anyhow::Result<()> {
    let listener = TcpListener::bind(socket).await?;
    tracing::info!("HTTPS listening on {}", socket);

    let mut conns = JoinSet::new();
    let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                tracing::info!("HTTPS: stop accepting new connections");
                break;
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, _)) => {
                        let tls_acceptor = tls_acceptor.clone();
                        let state_cloned = state.clone();
                        conns.spawn(async move {
                            let tls_stream = match tls_acceptor.accept(stream).await {
                                Ok(s) => s,
                                Err(err) => { tracing::error!("TLS accept error: {err}"); return; }
                            };
                            let io = TokioIo::new(tls_stream);
                            let mut builder = auto::Builder::new(TokioExecutor::new());
                            builder.http1().title_case_headers(true);
                            builder
                            .http2()
                            .auto_date_header(true)
                            .enable_connect_protocol();

                            let svc = service_fn(move |req: Request<Incoming>| proxy_handler(req, state_cloned.clone()));
                            if let Err(err) = builder.serve_connection_with_upgrades(io, svc).await {
                                tracing::error!("HTTPS conn error: {err:?}");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("HTTPS accept error: {e}; retry in 100ms");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }

    while conns.join_next().await.is_some() {}
    Ok(())
}


pub async fn echo(
    req: Request<Incoming>,
    ready: Arc<RwLock<bool>>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let method = req.method();
    let path = req.uri().path();

    match (method, path) {
        (&Method::POST, "/echo") => Ok(Response::new(req.into_body().boxed())),
        (&Method::GET, "/healthz") => Ok(Response::new(utils::full("Ok"))),
        (&Method::GET, "/readyz") => {
            if *ready.read().await {
                let mut ok = Response::new(utils::empty());
                *ok.status_mut() = StatusCode::OK;
                Ok(ok)
            } else {
                let mut service_unavailable = Response::new(utils::empty());
                *service_unavailable.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                Ok(service_unavailable)
            }
        }
        _ => {
            let mut not_found = Response::new(utils::empty());
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            Ok(not_found)
        }
    }
}
