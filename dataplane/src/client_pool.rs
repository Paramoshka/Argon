use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use std::time::Duration;
use tokio;

#[derive(Clone, Debug)]
pub struct ClientPool {
    pub connector: Client<HttpsConnector<HttpConnector>, BoxBody<Bytes, hyper::Error>>,
}

impl ClientPool {
    pub fn new_http_pool_connector() -> Self {
        // Build HTTPS connector with native root certificates.
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("no native root CA certificates found")
            .https_or_http()
            .enable_all_versions()
            .build();

        // Construct a Hyper client with the HTTPS connector.
        let connector = Client::builder(TokioExecutor::new())
            .pool_timer(TokioTimer::new())
            .pool_idle_timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(1024)
            .build(https);

        ClientPool { connector }
    }
}

impl Default for ClientPool {
    fn default() -> Self {
        Self::new_http_pool_connector()
    }
}
