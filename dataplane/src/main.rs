mod certs;
mod grpc;
mod proxy;
mod snapshot;
mod utils;

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use hyper_util::{rt::TokioExecutor, server::conn::auto};
use rustls::ServerConfig;
use rustls::server::ResolvesServerCert;
use snapshot::RouteTable;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io;
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

#[derive(Clone, Default)]
struct AppState {
    ready: Arc<RwLock<bool>>,
    snapshot: Arc<RwLock<Snapshot>>,
    route_table: Arc<RwLock<Arc<RouteTable>>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // env
    let http_port = std::env::var("HTTP_PORT").unwrap_or_else(|_| "8080".to_string());
    let https_port = std::env::var("HTTPS_PORT").unwrap_or_else(|_| "8443".to_string());
    let admin_port = std::env::var("ADMIN_PORT").unwrap_or_else(|_| "8181".to_string());
    let controller_addr =
        std::env::var("CONTROLLER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:18000".into());
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
    };

    // shutdown token
    let shutdown = CancellationToken::new();
    let shutdown_http = shutdown.clone();
    let shutdown_https = shutdown.clone();

    // Ctrl+C / SIGTERM -> cancel
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown.cancel();
    });

    // gRPC watcher
    let manager = GrpcManager::start(
        controller_addr,
        node_id,
        state.ready.clone(),
        state.snapshot.clone(),
        state.route_table.clone(),
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

    // track active connections HTTP
    let mut conns = JoinSet::new();

    // HTTP
    let socket = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        http_port.parse::<u16>()?,
    );
    let listener = TcpListener::bind(socket).await?;
    tracing::info!("listening on {}", socket);

    loop {
        tokio::select! {
            _ = shutdown_http.cancelled() => {
                tracing::info!("shutdown requested: stop accepting new connections");
                break;
            }

            accept_res = listener.accept() => {
                let (stream, _) = accept_res?;
                let io = TokioIo::new(stream);
                let state_cloned = state.clone();
                let mut builder = auto::Builder::new(TokioExecutor::new());

                // set http1 options
                builder
                .http1()
                .title_case_headers(true);

                // set http2 options
                builder
                .http2()
                .auto_date_header(true);

                conns.spawn(async move {
                    let svc = service_fn(move |request: Request<Incoming>| {
                        proxy_handler(request, state_cloned.clone())
                    });

                    if let Err(err) = builder
                        .serve_connection_with_upgrades(io, svc)
                        .await
                    {
                        eprintln!("Failed to serve connection: {:?}", err);
                    }
                });
            }
        }
    }

    // HTTPS
    // track active connections fot https
    let mut conns_https = JoinSet::new();

    let dummy_cert = certs::make_dummy_cert()?;
    let server_cert_resolver: Arc<dyn ResolvesServerCert> =
        Arc::new(certs::DynResolver::new(dummy_cert));


    let mut server_config = ServerConfig::builder() 
        .with_no_client_auth()
        .with_cert_resolver(server_cert_resolver);
    server_config.alpn_protocols = vec![
        b"h2".to_vec(),
        b"http/1.1".to_vec(),
        b"http/1.0".to_vec(),
    ];

    let socket = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        https_port.parse::<u16>()?,
    );
    
    let listener = TcpListener::bind(socket).await?;
    tracing::info!("listening on {}", socket);

    loop {
        tokio::select! {
            _ = shutdown_https.cancelled() => {
                tracing::info!("shutdown requested HTTPS: stop accepting new connections");
                break;
            }

            accept_res = listener.accept() => {
                let (stream, _) = accept_res?;

                let state_cloned = state.clone();
                
                let tls_acceptor = TlsAcceptor::from(Arc::new(server_config.clone()));

                let tls_acceptor = tls_acceptor.clone();

                conns_https.spawn(async move {
                    let tls_stream = match tls_acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(err) => {
                            tracing::error!("tls accept error: {err}");
                            return;
                        }
                    };

                    let io = TokioIo::new(tls_stream);

                    let builder_https = auto::Builder::new(
                        TokioExecutor::new()
                    );

                    let svc = service_fn(move |req: Request<Incoming>| {
                        proxy_handler(req, state_cloned.clone())
                    });

                    if let Err(err) = builder_https
                        .serve_connection_with_upgrades(io, svc)
                        .await
                    {
                        eprintln!("Failed to serve connection: {err:?}");
                    }
                });
            }
        }
    }

    // wait when connections will be finished
    while conns.join_next().await.is_some() {}
    while conns_https.join_next().await.is_some() {}

    // shutdown gRPC server after http server
    manager.shutdown().await;

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
