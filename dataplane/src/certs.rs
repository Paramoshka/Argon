use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use rustls::crypto::aws_lc_rs::sign::any_supported_type;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::{CertifiedKey, SigningKey};
use rcgen::{generate_simple_self_signed};
use crate::argon_config::Snapshot;
use arc_swap::ArcSwap;

pub struct DynResolver {
    map: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>,
    default: Arc<CertifiedKey>,
}

impl DynResolver {
    pub fn new(default: Arc<CertifiedKey>, map: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>) -> Self {
        Self { map, default }
    }
}

pub fn certificates_from_snap(
    snapshot: &Snapshot,
) -> HashMap<String, Arc<CertifiedKey>> {
    let mut map = HashMap::new();

    for sni in &snapshot.server_tls {
        let name = sni.name.to_ascii_lowercase();

        let cert_der = CertificateDer::from(sni.cert_pem.clone());
        let key_der: PrivateKeyDer<'static> =
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(sni.key_pem.clone()));

        let signing_key = match any_supported_type(&key_der) {
            Ok(key) => key,
            Err(e) => {
                tracing::error!(
                    host = %sni.name,
                    error = %e,
                    "failed to parse signing key"
                );
                continue;
            }
        };

        let ck = Arc::new(CertifiedKey::new(vec![cert_der], signing_key));
        map.insert(name, ck);

    }

    tracing::info!(
        total = snapshot.server_tls.len(),
        "parsed certificates from snapshot"
    );

    map
}

impl Debug for DynResolver {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl ResolvesServerCert for DynResolver {
    fn resolve(&self, hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let name = hello.server_name()?.to_ascii_lowercase();
        if let Some(v) = self.map.load().get(&name) {
            return Some(v.clone());
        }

        // wildcard
        if let Some(pos) = name.find('.') {
            let star = format!("*.{}", &name[pos+1..]);
            if let Some(v) = self.map.load().get(&star) {
                return Some(v.clone());
            }
        }
        
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
    let key_der: PrivateKeyDer<'static> =
        PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_bytes));

    let signing_key: Arc<dyn SigningKey> = any_supported_type(&key_der)?;

    let ck = CertifiedKey::new(vec![cert_der], signing_key);
    Ok(Arc::new(ck))
}
