use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use dashmap::DashMap;
use rustls::crypto::aws_lc_rs::sign::any_supported_type;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::{CertifiedKey, SigningKey};
use rcgen::{generate_simple_self_signed};

pub(crate) struct DynResolver {
    map: DashMap<String, Arc<CertifiedKey>>, // hostname (lowercase, без конечной точки) -> key
    default: Arc<CertifiedKey>,
}

impl DynResolver {
    pub(crate) fn new(default: Arc<CertifiedKey>) -> Self {
        Self { map: DashMap::new(), default }
    }

    fn put_host(&self, host: &str, ck: Arc<CertifiedKey>) {
        self.map.insert(host.to_ascii_lowercase().trim_end_matches('.').to_string(), ck);
    }

    fn remove_host(&self, host: &str) {
        self.map.remove(&host.to_ascii_lowercase());
    }
}

impl Debug for DynResolver {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl ResolvesServerCert for DynResolver {
    fn resolve(&self, hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let name = hello.server_name()?.to_ascii_lowercase();
        if let Some(v) = self.map.get(&name) {
            return Some(v.clone());
        }

        // wildcard
        if let Some(pos) = name.find('.') {
            let star = format!("*.{}", &name[pos+1..]);
            if let Some(v) = self.map.get(&star) {
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
    ])?;
    
    let cert_der = CertificateDer::from(cert.signing_key.serialize_der());

    let key_bytes = cert.signing_key.serialize_der(); // Vec<u8> (pkcs8)
    let key_der: PrivateKeyDer<'static> =
        PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_bytes));
    
    let signing_key: Arc<dyn SigningKey> = any_supported_type(&key_der)?;
    
    let ck = CertifiedKey::new(vec![cert_der], signing_key);
    Ok(Arc::new(ck))
}
