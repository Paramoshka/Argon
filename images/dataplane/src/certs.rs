use crate::argon_config::Snapshot;
use arc_swap::ArcSwap;
use rcgen::generate_simple_self_signed;
use rustls::crypto::aws_lc_rs::sign::any_supported_type;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::{CertifiedKey, SigningKey};
use rustls_pemfile::{Item, read_all};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::io::Cursor;
use std::sync::Arc;

pub struct DynResolver {
    map: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>,
    default: Arc<CertifiedKey>,
}

impl DynResolver {
    pub fn new(
        default: Arc<CertifiedKey>,
        map: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>,
    ) -> Self {
        Self { map, default }
    }
}

pub fn certificates_from_snap(snapshot: &Snapshot) -> HashMap<String, Arc<CertifiedKey>> {
    let mut map = HashMap::new();

    for sni in &snapshot.server_tls {
        let mut cert_reader = Cursor::new(&sni.cert_pem[..]);
        let mut chain_der: Vec<CertificateDer<'static>> = Vec::new();

        for item in read_all(&mut cert_reader) {
            match item {
                Ok(Item::X509Certificate(cert)) => {
                    chain_der.push(cert.into());
                }
                Ok(_other) => {
                    tracing::warn!("ignoring non-certificate PEM block in cert_pem");
                    continue;
                }
                Err(e) => {
                    tracing::error!("failed to decode cert from PEM: {}", e);
                    continue;
                }
            }
        }

        if chain_der.is_empty() {
            tracing::error!("no X.509 certificates found in cert_pem");
            continue;
        }

        let mut key_reader = Cursor::new(&sni.key_pem[..]);

        let key_der: PrivateKeyDer<'static> = match rustls_pemfile::read_one(&mut key_reader) {
            Ok(Some(Item::Pkcs8Key(der))) => PrivateKeyDer::from(der),
            Ok(Some(Item::Pkcs1Key(der))) => PrivateKeyDer::from(der), // RSA
            Ok(Some(Item::Sec1Key(der))) => PrivateKeyDer::from(der),  // EC
            Ok(Some(_other)) => {
                tracing::error!("unsupported private key type in key_pem");
                continue;
            }
            Ok(None) => {
                tracing::error!("no private key found in key_pem");
                continue;
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to parse key_pem");
                continue;
            }
        };

        let signing_key = match any_supported_type(&key_der) {
            Ok(k) => k,
            Err(e) => {
                tracing::error!(error = %e, "any_supported_type failed for key_pem");
                continue;
            }
        };

        let ck = Arc::new(CertifiedKey::new(chain_der, signing_key));

        for host in sni.sni.iter() {
            map.insert(host.clone(), ck.clone());
        }
    }

    tracing::info!(
        total = snapshot.server_tls.len(),
        unique_hosts = map.len(),
        "parsed certificates from snapshot"
    );
    map
}

impl Debug for DynResolver {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl ResolvesServerCert for DynResolver {
    fn resolve(&self, hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let name = hello.server_name()?.to_ascii_lowercase();
        if let Some(v) = self.map.load().get(&name) {
            // tracing::info!("resolved certificate for {}", name);
            return Some(v.clone());
        }

        // wildcard
        if let Some(pos) = name.find('.') {
            let star = format!("*.{}", &name[pos + 1..]);
            if let Some(v) = self.map.load().get(&star) {
                // tracing::info!("resolved single certificate for wildcard {}", name);
                return Some(v.clone());
            }
        }

        tracing::info!("not resolved single certificate for wildcard {}", name);
        println!("certs in resolve function: {:?}", self.map.load());
        Some(self.default.clone())
    }
}

pub fn make_dummy_cert() -> anyhow::Result<Arc<CertifiedKey>> {
    // self-signed SAN
    let cert = generate_simple_self_signed(vec![
        "hello.world.example".into(),
        "localhost".into(),
        "echo.local".into(),
    ])?;

    let cert_der = CertificateDer::from(cert.cert);

    let key_bytes = cert.signing_key.serialize_der(); // Vec<u8> (pkcs8)
    let key_der: PrivateKeyDer<'static> = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_bytes));

    let signing_key: Arc<dyn SigningKey> = any_supported_type(&key_der)?;

    let ck = CertifiedKey::new(vec![cert_der], signing_key);
    Ok(Arc::new(ck))
}
