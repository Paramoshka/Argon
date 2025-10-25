use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use rustls::DigitallySignedStruct;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, SignatureScheme};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ClientPool {
    pub connector: Client<HttpsConnector<HttpConnector>, BoxBody<Bytes, hyper::Error>>,
    pub connector_insecure: Client<HttpsConnector<HttpConnector>, BoxBody<Bytes, hyper::Error>>,
}

const DEFAULT_COUNT_POOL: usize = 1024;
const DEFAULT_IDLE_TIMEOUT: u64 = 60;

impl ClientPool {
    pub fn new_http_pool_connector(count_thread: usize) -> Self {
        // Secure HTTPS connector with native root certificates.
        let https_secure = HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("no native root CA certificates found")
            .https_or_http()
            .enable_all_versions()
            .build();

        // Insecure HTTPS connector that skips certificate verification (for self-signed backends).
        let insecure_tls_config: ClientConfig = {
            // Build a rustls ClientConfig with a custom certificate verifier that accepts any cert.
            #[derive(Debug)]
            struct NoCertVerifier;
            impl ServerCertVerifier for NoCertVerifier {
                fn verify_server_cert(
                    &self,
                    _end_entity: &CertificateDer<'_>,
                    _intermediates: &[CertificateDer<'_>],
                    _server_name: &ServerName<'_>,
                    _ocsp_response: &[u8],
                    _now: UnixTime,
                ) -> Result<ServerCertVerified, rustls::Error> {
                    Ok(ServerCertVerified::assertion())
                }

                fn verify_tls12_signature(
                    &self,
                    _message: &[u8],
                    _cert: &CertificateDer<'_>,
                    _dss: &DigitallySignedStruct,
                ) -> Result<HandshakeSignatureValid, rustls::Error> {
                    Ok(HandshakeSignatureValid::assertion())
                }

                fn verify_tls13_signature(
                    &self,
                    _message: &[u8],
                    _cert: &CertificateDer<'_>,
                    _dss: &DigitallySignedStruct,
                ) -> Result<HandshakeSignatureValid, rustls::Error> {
                    Ok(HandshakeSignatureValid::assertion())
                }

                fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                    vec![
                        SignatureScheme::RSA_PSS_SHA256,
                        SignatureScheme::ECDSA_NISTP256_SHA256,
                        SignatureScheme::RSA_PKCS1_SHA256,
                    ]
                }
            }

            let mut cfg = ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertVerifier))
                .with_no_client_auth();
            cfg
        };

        let https_insecure = HttpsConnectorBuilder::new()
            .with_tls_config(insecure_tls_config)
            .https_or_http()
            .enable_all_versions()
            .build();

        // Construct Hyper clients with the respective HTTPS connectors.
        let connector = Client::builder(TokioExecutor::new())
            .pool_timer(TokioTimer::new())
            .pool_idle_timeout(Duration::from_secs(DEFAULT_IDLE_TIMEOUT))
            .pool_max_idle_per_host(DEFAULT_COUNT_POOL * count_thread)
            .build(https_secure);

        let connector_insecure = Client::builder(TokioExecutor::new())
            .pool_timer(TokioTimer::new())
            .pool_idle_timeout(Duration::from_secs(DEFAULT_IDLE_TIMEOUT))
            .pool_max_idle_per_host(DEFAULT_COUNT_POOL * count_thread)
            .build(https_insecure);

        ClientPool {
            connector,
            connector_insecure,
        }
    }
}

impl Default for ClientPool {
    fn default() -> Self {
        Self::new_http_pool_connector(DEFAULT_COUNT_POOL)
    }
}
