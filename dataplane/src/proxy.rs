use crate::AppState;
use crate::snapshot::RouteTable;
use crate::utils;
use bytes::Bytes;
use http::uri::{Authority, PathAndQuery};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, Version, header};
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::arch::x86_64::_mm256_hsub_epi16;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpStream;

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
    handle_req(&mut req, host.clone(), route_table.clone());

    // lease read lock
    drop(route_table);

    if req.version() == Version::HTTP_2 {
        // todo add h1-ssl,h2,h2-ssl for upstream, while h1

        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
            .preserve_header_case(true)
            .title_case_headers(false)
            .handshake(io)
            .await?;

        tokio::spawn(async move {
            if let Err(e) = conn.await {
                tracing::error!("upstream h1 connection error: {e}");
            }
        });

        let mut resp = sender.send_request(req).await?;
        remove_hop_headers(resp.headers_mut());
        return Ok(resp.map(|b| b.boxed()));
    } else {
        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
            .handshake(io)
            .await?;
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

fn handle_req(req: &mut Request<Incoming>, host: String, route_table: Arc<RouteTable>) {
    let mut parts = req.uri().clone().into_parts();
    parts.scheme = Some(http::uri::Scheme::HTTP);
    remove_hop_headers(req.headers_mut());

    // check authority
    if parts.authority.is_none() {
        parts.authority = Some(Authority::from_str(host.as_str()).expect("invalid authority"));
    }

    // check host if to h1
    // if req.version() == Version::HTTP_2 {
    //     *req.version_mut() = Version::HTTP_11;
    // }

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
