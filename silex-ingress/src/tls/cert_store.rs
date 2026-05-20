use dashmap::DashMap;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::sync::Arc;

pub struct SilexCertResolver {
    store: DashMap<String, Arc<CertifiedKey>>,
    fallback_cert: Arc<CertifiedKey>,
}

impl SilexCertResolver {
    pub fn new(fallback_cert: Arc<CertifiedKey>) -> Self {
        Self {
            store: DashMap::new(),
            fallback_cert,
        }
    }

    pub fn inject_cert(&self, sni: String, cert: Arc<CertifiedKey>) {
        self.store.insert(sni, cert);
    }
}

impl ResolvesServerCert for SilexCertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        if let Some(sni) = client_hello.server_name() {
            if let Some(cert_ref) = self.store.get(sni) {
                return Some(cert_ref.value().clone());
            }
        }
        Some(self.fallback_cert.clone())
    }
}