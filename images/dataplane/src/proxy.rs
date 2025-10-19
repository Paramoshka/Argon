use crate::AppState;
use crate::snapshot::{BackendProtocol, HeaderRewriteMode, HeaderRewriteRule, SelectedEndpoint};
use bytes::Bytes;
use http::uri::{Authority, PathAndQuery};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, Version, header};
use http_body_util::Full;
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::{Request, Response};
use std::convert::Infallible;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio_util::future::FutureExt;

pub struct Proxy;

// hop-by-hop headers that cannot be proxied (RFC 7230)

static PROXY_CONNECTION: HeaderName = HeaderName::from_static("proxy-connection");
static KEEP_ALIVE: HeaderName = HeaderName::from_static("keep-alive");

static HOP_HEADERS_REF: &[&HeaderName] = &[
    &header::CONNECTION,
    &header::PROXY_AUTHENTICATE,
    &header::PROXY_AUTHORIZATION,
    &header::TE,
    &header::TRAILER,
    &header::TRANSFER_ENCODING,
    &header::UPGRADE,
    &PROXY_CONNECTION,
    &KEEP_ALIVE,
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
    let header_rewrites = cluster_rules.request_headers.clone();

    // <--Select LB algorithm-->
    let selection = match route_table.get_endpoint(rule.cluster.as_str()) {
        Some(e) => e,
        None => {
            tracing::error!(cluster=%rule.cluster, "endpoint not found");
            return Ok(text(StatusCode::BAD_GATEWAY, "endpoint not found"));
        }
    };

    let SelectedEndpoint {
        endpoint: ep,
        counter,
    } = selection;
    let _active_counter = ActiveConnGuard::new(counter);

    // handle req
    handle_req_upstream(
        &mut req,
        &host,
        &ep.address,
        ep.port as u16,
        cluster_rules.backend_protocol.clone(),
    );

    if !header_rewrites.is_empty() {
        apply_header_rewrites(req.headers_mut(), header_rewrites.as_ref());
    }

    // lease read lock
    drop(route_table);

    let addr = format!("{}:{}", ep.address, ep.port);
    let pool = state.client_pool.load();
    let timeout = Duration::from_millis(cluster_rules.timeout_ms as u64);
    let Ok(req) = pool
        .connector
        .request(req.map(|b| b.boxed()))
        .timeout(timeout)
        .await
    else {
        let err_text = format!("Upstream connector timed out: {:?}", addr);
        return Ok(text(StatusCode::BAD_GATEWAY, err_text));
    };

    let mut resp = match req {
        Ok(resp) => resp,
        Err(e) => {
            return Ok(text(StatusCode::BAD_GATEWAY, e.to_string()));
        }
    };

    remove_hop_headers(resp.headers_mut());

    Ok(resp.map(|b| b.boxed()))
}

fn handle_req_upstream(
    req: &mut Request<Incoming>,
    original_host: &str,
    upstream_host: &str,
    upstream_port: u16,
    proto: BackendProtocol,
) {
    let mut parts = req.uri().clone().into_parts();

    let is_tls = matches!(proto, BackendProtocol::H1Ssl | BackendProtocol::H2Ssl);
    parts.scheme = Some(if is_tls {
        http::uri::Scheme::HTTPS
    } else {
        http::uri::Scheme::HTTP
    });

    match proto {
        BackendProtocol::H2 | BackendProtocol::H2Ssl => *req.version_mut() = Version::HTTP_2,
        _ => *req.version_mut() = Version::HTTP_11,
    }

    remove_hop_headers(req.headers_mut());

    let default_port = if is_tls { 443 } else { 80 };
    let auth = if upstream_port == default_port {
        Authority::from_str(upstream_host)
    } else {
        Authority::from_str(&format!("{upstream_host}:{upstream_port}"))
    };
    if let Ok(a) = auth {
        parts.authority = Some(a);
    }

    if parts.path_and_query.is_none() {
        parts.path_and_query = Some(PathAndQuery::from_static("/"));
    }

    if let Ok(hv) = HeaderValue::from_str(original_host) {
        req.headers_mut().insert(header::HOST, hv);
    }

    add_forward_headers(req.headers_mut(), is_tls, original_host);

    if let Ok(new_uri) = Uri::from_parts(parts) {
        *req.uri_mut() = new_uri;
    }
}

fn text(status: StatusCode, s: impl Into<String>) -> http::Response<BoxBody<Bytes, hyper::Error>> {
    let body: BoxBody<Bytes, hyper::Error> = Full::new(Bytes::from(s.into()))
        .map_err(|never: Infallible| match never {})
        .boxed();

    http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body)
        .expect("failed to build response")
}

fn remove_hop_headers(headers: &mut HeaderMap) {
    for header in HOP_HEADERS_REF {
        headers.remove(*header);
    }
}

fn add_forward_headers(h: &mut http::HeaderMap, is_tls: bool, original_host: &str) {
    let proto = if is_tls { "https" } else { "http" };
    let _ = h.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static(proto),
    );

    if !h.contains_key(HeaderName::from_static("x-forwarded-host")) {
        if let Ok(v) = HeaderValue::from_str(original_host) {
            let _ = h.insert(HeaderName::from_static("x-forwarded-host"), v);
        }
    }
}

fn apply_header_rewrites(headers: &mut HeaderMap, rewrites: &[HeaderRewriteRule]) {
    for rule in rewrites {
        match rule.mode {
            HeaderRewriteMode::Remove => {
                headers.remove(&rule.name);
            }
            HeaderRewriteMode::Set | HeaderRewriteMode::Append => {
                let Some(value) = &rule.value else {
                    tracing::warn!(header = %rule.name, "missing value for header rewrite");
                    continue;
                };

                match HeaderValue::from_str(value) {
                    Ok(header_value) => {
                        if matches!(rule.mode, HeaderRewriteMode::Set) {
                            headers.insert(rule.name.clone(), header_value);
                        } else {
                            headers.append(rule.name.clone(), header_value);
                        }
                    }
                    Err(err) => {
                        tracing::warn!(header = %rule.name, %err, "invalid header value for rewrite");
                    }
                }
            }
        }
    }
}

struct ActiveConnGuard {
    counter: Option<Arc<AtomicUsize>>,
}

impl ActiveConnGuard {
    fn new(counter: Option<Arc<AtomicUsize>>) -> Self {
        if let Some(ref c) = counter {
            c.fetch_add(1, Ordering::Relaxed);
        }
        Self { counter }
    }
}

impl Drop for ActiveConnGuard {
    fn drop(&mut self) {
        if let Some(ref c) = self.counter {
            c.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
