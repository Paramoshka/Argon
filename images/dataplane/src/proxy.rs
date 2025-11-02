use crate::AppState;
use crate::snapshot::{BackendProtocol, HeaderRewriteMode, HeaderRewriteRule, SelectedEndpoint};
use bytes::Bytes;
use http::uri::{Authority, PathAndQuery};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, Version, header};
use http_body_util::Full;
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body::{Body, Incoming};
use hyper::{Request, Response};
use std::convert::Infallible;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio_util::future::FutureExt;

#[derive(Clone, Copy, Debug)]
pub struct FrontendTls(pub bool);

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
    let frontend_is_tls = req
        .extensions()
        .get::<FrontendTls>()
        .map(|f| f.0)
        .unwrap_or(false);
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

    // subrequest if DEX AUTH enabled
    if let Some(auth) = cluster_rules.auth.as_deref() {
        let path = req.uri().path();
        if auth.skip_paths.iter().any(|p| path.starts_with(p)) {
            // skip
        } else {
            // Optional fast-path: if a cookie name is configured and the cookie
            // is absent, redirect to signin immediately.
            if let Some(cookie_name) = &auth.cookie_name {
                if let Some(cookies_val) = req.headers().get(header::COOKIE) {
                    if let Ok(cookies) = cookies_val.to_str() {
                        let needle = format!("{}=", cookie_name);
                        if !cookies.contains(&needle) {
                            if let Some(signin) = &auth.signin {
                                let location = build_signin_location(
                                    signin,
                                    &host,
                                    req.uri(),
                                    frontend_is_tls,
                                );
                                return Ok(redirect(StatusCode::FOUND, &location));
                            }
                        }
                    }
                } else if let Some(signin) = &auth.signin {
                    let location = build_signin_location(signin, &host, req.uri(), frontend_is_tls);
                    return Ok(redirect(StatusCode::FOUND, &location));
                }
            }

            // check auth via subrequest to auth-url
            let pool = state.client_pool.load();
            let client = if cluster_rules.backend_tls_insecure_skip_verify {
                &pool.connector_insecure
            } else {
                &pool.connector
            };

            if let Some(auth_url) = &auth.url {
                if let Ok(uri) = Uri::from_str(auth_url.as_str()) {
                    let auth_req = build_auth_request(&req, uri, &host, frontend_is_tls);
                    let auth_resp = match client.request(auth_req).await {
                        Ok(r) => r,
                        Err(e) => {
                            let err_text = format!(
                                "Authorization subrequest failed: {:?} : {:?}",
                                auth_url, e
                            );
                            return Ok(text(StatusCode::BAD_GATEWAY, err_text));
                        }
                    };

                    let status = auth_resp.status();
                    if status.is_success() {
                        // Copy configured headers from auth response into the upstream request
                        for name in auth.response_headers.iter() {
                            if let Ok(hn) = HeaderName::from_bytes(name.as_bytes()) {
                                if let Some(val) = auth_resp.headers().get(&hn) {
                                    req.headers_mut().insert(hn, val.clone());
                                }
                            }
                        }
                    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
                    {
                        if let Some(signin) = &auth.signin {
                            let location =
                                build_signin_location(signin, &host, req.uri(), frontend_is_tls);
                            return Ok(redirect(StatusCode::FOUND, &location));
                        }
                        return Ok(text(StatusCode::UNAUTHORIZED, "unauthorized"));
                    } else {
                        let err_text = format!("authorization service returned {}", status);
                        return Ok(text(StatusCode::BAD_GATEWAY, err_text));
                    }
                } else {
                    let err_text = format!("Invalid authorization URL: {}", auth_url);
                    return Ok(text(StatusCode::BAD_GATEWAY, err_text));
                }
            } else {
                let err_text = format!("Authorization URL not found for: {:?}", req.uri());
                return Ok(text(StatusCode::BAD_GATEWAY, err_text));
            }
        }
    }

    // handle req (prepare headers/authority for selected endpoint)
    handle_req_upstream(
        &mut req,
        &host,
        &ep.address,
        ep.port as u16,
        cluster_rules.backend_protocol.clone(),
        frontend_is_tls,
    );

    if !header_rewrites.is_empty() {
        apply_header_rewrites(req.headers_mut(), header_rewrites.as_ref());
    }

    let method = req.method().clone();
    let uri = req.uri().clone();
    let version = req.version();
    let headers_snapshot = req.headers().clone();
    let body_is_reusable = req.body().is_end_stream();

    // release read lock before IO
    drop(route_table);

    let addr = format!("{}:{}", ep.address, ep.port);
    let pool = state.client_pool.load();
    let timeout = Duration::from_millis(cluster_rules.timeout_ms as u64);
    let client = if cluster_rules.backend_tls_insecure_skip_verify {
        &pool.connector_insecure
    } else {
        &pool.connector
    };

    // Retries: at least 1 attempt
    let retries = cluster_rules.retries.max(1) as usize;
    let mut last_err: Option<String> = None;
    let mut response: Option<Response<Incoming>> = None;
    let mut first_request = Some(req.map(|b| b.boxed()));

    for attempt in 0..retries {
        let request = if let Some(req) = first_request.take() {
            req
        } else if body_is_reusable {
            let body = Full::new(Bytes::new())
                .map_err(|never: Infallible| match never {})
                .boxed();
            let mut builder = Request::builder()
                .method(method.clone())
                .uri(uri.clone())
                .version(version);
            let mut retry_req = builder
                .body(body)
                .expect("retry request build must succeed");
            *retry_req.headers_mut() = headers_snapshot.clone();
            retry_req
        } else {
            break;
        };

        match client.request(request).timeout(timeout).await {
            Ok(Ok(resp)) => {
                response = Some(resp);
                break;
            }
            Ok(Err(e)) => {
                last_err = Some(format!("Upstream connector error ({}): {:?}", e, addr));
                if attempt + 1 == retries {
                    break;
                }
            }
            Err(e) => {
                last_err = Some(format!("Upstream connector timeout ({}): {:?}", e, addr));
                if attempt + 1 == retries {
                    break;
                }
            }
        }
    }

    let mut resp = match response {
        Some(resp) => resp,
        None => {
            let err_text =
                last_err.unwrap_or_else(|| format!("Upstream connector timed out: {:?}", addr));
            return Ok(text(StatusCode::BAD_GATEWAY, err_text));
        }
    };

    remove_hop_headers(resp.headers_mut());

    Ok(resp.map(|b| b.boxed()))
}

fn build_auth_request(
    original_req: &Request<Incoming>,
    auth_uri: Uri,
    original_host: &str,
    frontend_is_tls: bool,
) -> Request<BoxBody<Bytes, hyper::Error>> {
    // Copy Cookie and Authorization from original request if present.
    let mut builder = http::Request::builder().method("GET").uri(auth_uri);

    let headers = builder.headers_mut().expect("headers_mut");

    if let Some(v) = original_req.headers().get(header::COOKIE) {
        headers.insert(header::COOKIE, v.clone());
    }
    if let Some(v) = original_req.headers().get(header::AUTHORIZATION) {
        headers.insert(header::AUTHORIZATION, v.clone());
    }

    // Forwarding headers for auth service to understand original request context
    let scheme = if frontend_is_tls { "https" } else { "http" };
    let path_q = original_req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let redirect_url = format!("{}://{}{}", scheme, original_host, path_q);

    headers.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static(scheme),
    );
    if let Ok(v) = HeaderValue::from_str(original_host) {
        headers.insert(HeaderName::from_static("x-forwarded-host"), v);
    }
    if let Ok(v) = HeaderValue::from_str(path_q) {
        headers.insert(HeaderName::from_static("x-forwarded-uri"), v);
    }
    if let Ok(v) = HeaderValue::from_str(path_q) {
        headers.insert(HeaderName::from_static("x-original-uri"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&redirect_url) {
        headers.insert(HeaderName::from_static("x-auth-request-redirect"), v);
    }

    let body: BoxBody<Bytes, hyper::Error> = Full::new(Bytes::new())
        .map_err(|never: Infallible| match never {})
        .boxed();
    builder.body(body).expect("build auth request")
}

fn redirect(status: StatusCode, location: &str) -> http::Response<BoxBody<Bytes, hyper::Error>> {
    let body: BoxBody<Bytes, hyper::Error> = Full::new(Bytes::from_static(b"redirect"))
        .map_err(|never: Infallible| match never {})
        .boxed();
    let mut resp = http::Response::builder()
        .status(status)
        .body(body)
        .expect("failed to build redirect");
    if let Ok(loc) = HeaderValue::from_str(location) {
        resp.headers_mut().insert(header::LOCATION, loc);
    }
    resp
}

fn build_signin_location(
    signin_tmpl: &str,
    host: &str,
    uri: &Uri,
    frontend_is_tls: bool,
) -> String {
    let scheme = if frontend_is_tls { "https" } else { "http" };
    let path_q = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let escaped = percent_encode(path_q);
    // Replace $host and $escaped_request_uri if present in template
    let mut out = signin_tmpl.replace("$host", host);
    out = out.replace("$escaped_request_uri", &escaped);
    out = out.replace("$scheme", scheme);
    out
}

fn percent_encode(input: &str) -> String {
    const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut out = String::with_capacity(input.len() * 3);
    for b in input.as_bytes() {
        if UNRESERVED.contains(b) {
            out.push(*b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
}

fn handle_req_upstream(
    req: &mut Request<Incoming>,
    original_host: &str,
    upstream_host: &str,
    upstream_port: u16,
    proto: BackendProtocol,
    frontend_is_tls: bool,
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

    add_forward_headers(req.headers_mut(), frontend_is_tls, original_host);

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

fn add_forward_headers(h: &mut http::HeaderMap, frontend_is_tls: bool, original_host: &str) {
    let proto = if frontend_is_tls { "https" } else { "http" };
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
