use hyper::{Method, Request, Response};
use bytes::Bytes;
use http::{header, Error, HeaderName, StatusCode, Uri};
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use crate::AppState;
use crate::utils;

pub struct Proxy;

// hop-by-hop headers that cannot be proxied (RFC 7230)
static HOP_HEADERS: &[HeaderName] = &[
    header::CONNECTION,
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
];

type ClientBuilder = hyper::client::conn::http1::Builder;

pub async fn proxy_handler(
    req: Request<hyper::body::Incoming>,
    state: AppState
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let route_table = state.route_table.read().await;

    // <--Get host-->
    let host = if let Some(h) = req.headers().get(hyper::header::HOST) {
        match h.to_str() {
            Ok(s) if !s.is_empty() => s.to_string(),
            _ => return Ok(text(StatusCode::BAD_REQUEST, "Invalid Host header")),
        }
    } else if let Some(h) = req.uri().host() {
        h.to_string()
    } else {
        return Ok(text(StatusCode::BAD_REQUEST, "Missing Host"));
    };

    // <--Get Path and Query-->
    let path = req.uri().path();
    let _path_and_query = req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    // <--Find rule for req-->
    let rule = match route_table.choose_route(&host, path) {
        Some(r) => r,
        None => {
            tracing::warn!(%host, %path, "route not found");
            return Ok(text(StatusCode::NOT_FOUND, "route not found"));
        }
    };

    // <--Select LB algorithm-->
    let ep = match route_table.get_endpoint(rule.cluster.as_str()) {
        Some(e) => e,
        None => {
            tracing::error!(cluster=%rule.cluster, "endpoint not found");
            return Ok(text(StatusCode::BAD_GATEWAY, "endpoint not found"));
        }
    };

    drop(route_table); // lease read lock

    let addr = format!("{}:{}", ep.address, ep.port); // <-- ДОЛЖЕН быть двоеточие, не пробел
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(%addr, error=%e, "connect failed");
            return Ok(text(StatusCode::BAD_GATEWAY, format!("connect to {addr} failed")));
        }
    };
    let io = TokioIo::new(stream);

    let (mut sender, conn) = match ClientBuilder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .handshake(io)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error=%e, "handshake failed");
            return Ok(text(StatusCode::BAD_GATEWAY, "upstream handshake failed"));
        }
    };

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            tracing::error!("upstream connection error: {e}");
        }
    });

    let resp = sender.send_request(req).await?;
    Ok(resp.map(|b| b.boxed()))
}

// todo need add text
fn text(status: StatusCode, s: impl Into<String>) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(utils::empty())
        .unwrap()
}