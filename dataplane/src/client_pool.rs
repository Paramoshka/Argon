use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ClientPool {
    pub connector: Client<HttpsConnector<HttpConnector>, BoxBody<Bytes, hyper::Error>>,
}

const DEFAULT_COUNT_POOL: usize = 1024;
const DEFAULT_IDLE_TIMEOUT:u64 = 60;

impl ClientPool {
    pub fn new_http_pool_connector(count_thread: usize) -> Self {
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
            .pool_idle_timeout(Duration::from_secs(DEFAULT_IDLE_TIMEOUT))
            .pool_max_idle_per_host(DEFAULT_COUNT_POOL * count_thread)
            .build(https);

        ClientPool { connector }
    }
}

impl Default for ClientPool {
    fn default() -> Self {
        Self::new_http_pool_connector(DEFAULT_COUNT_POOL)
    }
}
