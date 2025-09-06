use crate::AppState;
use crate::snapshot::{BackendProtocol, ClusterRule, RouteTable};
use crate::utils;
use bytes::Bytes;
use http::uri::{Authority, PathAndQuery};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, Version, header};
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use std::arch::x86_64::_mm256_hsub_epi16;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::future::FutureExt;
use std::convert::Infallible;
use http_body_util::{Full};

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
    HeaderName::from_static("proxy-connection"),
    HeaderName::from_static("keep-alive"),
];

pub async fn proxy_handler(
    mut req: Request<hyper::body::Incoming>,
    state: AppState,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let route_table = state.route_table.read().await;

    // <--Get host-->
    let host = if let Some(h) = req.headers().get(header::HOST) {
        match h.to_str() {
            Ok(s) if !s.is_empty() => match Authority::try_from(s.trim()) {
                Ok(a) => a
                    .host()
                    .to_ascii_lowercase()
                    .trim_end_matches('.')
                    .to_string(),
                Err(_) => return Ok(text(StatusCode::BAD_REQUEST, "Invalid Host header")),
            },
            _ => return Ok(text(StatusCode::BAD_REQUEST, "Invalid Host header")),
        }
    } else if let Some(h) = req.uri().host() {
        h.to_string()
    } else {
        return Ok(text(StatusCode::BAD_REQUEST, "Missing Host"));
    };

    // <--Get Path-->
    let path = req.uri().path();

    // <--Find rule for req-->
    let rule = match route_table.choose_route(&host, path) {
        Some(r) => r,
        None => {
            tracing::warn!(%host, %path, "route not found");
            return Ok(text(StatusCode::NOT_FOUND, "route not found"));
        }
    };

    // <--Get cluster rules-->
    let cluster_rules = match route_table.get_cluster_rules(rule.cluster.as_str()) {
        Some(r) => r,
        None => {
            tracing::error!(cluster=%rule.cluster, "cluster rule not found");
            return Ok(text(StatusCode::NOT_FOUND, "cluster rules not found"));
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

    let addr = format!("{}:{}", ep.address, ep.port);
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(%addr, error=%e, "connect failed");
            return Ok(text(
                StatusCode::BAD_GATEWAY,
                format!("connect to {addr} failed"),
            ));
        }
    };

    // handle req
    handle_req(
        &mut req,
        host.clone(),
        cluster_rules.clone(),
    );

    // lease read lock
    drop(route_table);

    let pool = state.client_pool.load();
    let req = pool.connector.request(req.map(|b| b.boxed())).await;
    
    let mut resp = match req { 
        Ok(r) => r,
        Err(e) => {
            return Ok(text(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
    };
    
    remove_hop_headers(resp.headers_mut());

    Ok(resp.map(|b| b.boxed()))
}

fn handle_req(
    req: &mut Request<Incoming>,
    host: String,
    cluster_rule: Arc<ClusterRule>,
) {
    let mut parts = req.uri().clone().into_parts();

    parts.scheme = Some(http::uri::Scheme::HTTP);
    if cluster_rule.backend_protocol == BackendProtocol::H2Ssl
        || cluster_rule.backend_protocol == BackendProtocol::H1Ssl
    {
        parts.scheme = Some(http::uri::Scheme::HTTPS);
    }

    match cluster_rule.backend_protocol {
        BackendProtocol::H2 | BackendProtocol::H2Ssl => {
            *req.version_mut() = Version::HTTP_2;
        }
        _ => {
            *req.version_mut() = Version::HTTP_11; // default H1
        }
    }

    remove_hop_headers(req.headers_mut());

    // check authority
    if parts.authority.is_none() {
        parts.authority = Some(Authority::from_str(host.as_str()).expect("invalid authority"));
    }
    

    if !req.headers().contains_key(header::HOST) {
        req.headers_mut().insert(
            header::HOST,
            HeaderValue::from_str(host.as_str()).expect("host is not a valid header value"),
        );
    }

    if parts.path_and_query.is_none() {
        parts.path_and_query = Some(PathAndQuery::from_static("/"));
    }

    let new_uri = Uri::from_parts(parts).expect("valid URI");
    *req.uri_mut() = new_uri;
}

fn text(status: StatusCode, s: impl Into<String>) -> Response<BoxBody<Bytes, hyper::Error>> {
    let body: BoxBody<Bytes, hyper::Error> = Full::new(Bytes::from(s.into()))
        .map_err(|| std::io::Error::new(std::io::ErrorKind::Other, status))
        .boxed();

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body)
        .unwrap()
}

fn remove_hop_headers(headers: &mut HeaderMap) {
    for header in HOP_HEADERS {
        headers.remove(header);
    }
}
