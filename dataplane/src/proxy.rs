use hyper::{Request, Response};
use bytes::Bytes;
use http::{header, HeaderMap, HeaderName, StatusCode, Version};
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper_util::rt::{TokioExecutor, TokioIo};
use http::uri::Authority;
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

pub async fn proxy_handler(
    mut req: Request<hyper::body::Incoming>,
    state: AppState
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let route_table = state.route_table.read().await;

    // <--Get host-->
    let host = if let Some(h) = req.headers().get(header::HOST) {
        match h.to_str() {
            Ok(s) if !s.is_empty() => {
                match Authority::try_from(s.trim()) {
                    Ok(a) => a.host().to_ascii_lowercase().trim_end_matches('.').to_string(),
                    Err(_) => return Ok(text(StatusCode::BAD_REQUEST, "Invalid Host header")),
                }
            }
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

    let addr = format!("{}:{}", ep.address, ep.port);
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(%addr, error=%e, "connect failed");
            return Ok(text(StatusCode::BAD_GATEWAY, format!("connect to {addr} failed")));
        }
    };

    // trim hop_by_hop headers
    remove_hop_headers(req.headers_mut());
    
    if req.version() == Version::HTTP_2 {
        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
            .handshake(io).await?;
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                tracing::error!("upstream h2 connection error: {e}");
            }
        });
        
        let mut resp = sender.send_request(req).await?;
        
        remove_hop_headers(resp.headers_mut());
        
        return Ok(resp.map(|b| b.boxed()));
        
    } else {
        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::Builder::new().handshake(io).await?;
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                tracing::error!("upstream h1 connection error: {e}");
            }
        });

        let mut resp = sender.send_request(req).await?;

        remove_hop_headers(resp.headers_mut());
        
        return Ok(resp.map(|b| b.boxed()));
    }
}

// todo need add text
fn text(status: StatusCode, s: impl Into<String>) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(utils::empty())
        .unwrap()
}

fn remove_hop_headers(headers: &mut HeaderMap) {

    for header in &*HOP_HEADERS {
        headers.remove(header);
    }
}