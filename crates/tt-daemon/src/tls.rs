//! Trust roots for client connections to the server: the bundled Mozilla
//! (webpki) roots plus, optionally, the certificates of one extra PEM file
//! (`server.ca_cert`, or `tt login --ca-cert`). The extra CA adds to the
//! bundled roots and never replaces them; hostname checks stay on.
//!
//! The root set is the single source of truth. Each client stack adapts it:
//! `ureq` (login, logout, ticket request) takes a root list, the websocket
//! client takes a `rustls::ClientConfig`.

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use rustls_pki_types::{CertificateDer, pem::PemObject};

/// Reads every certificate in the PEM file at `path`. Fails when the file
/// is unreadable or holds no certificate; errors name the path.
pub fn load_extra_roots(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let certs = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| anyhow::anyhow!("parsing {}: {error}", path.display()))?;
    if certs.is_empty() {
        bail!("{} contains no PEM certificate", path.display());
    }
    Ok(certs)
}

/// The bundled webpki roots plus `extra`.
#[must_use]
pub fn root_certs(extra: &[CertificateDer<'static>]) -> Vec<CertificateDer<'static>> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .cloned()
        .chain(extra.iter().cloned())
        .collect()
}

/// A `ureq` agent trusting [`root_certs`], with HTTP errors as responses.
#[must_use]
pub fn ureq_agent(extra: &[CertificateDer<'static>], timeout: Duration) -> ureq::Agent {
    let roots = root_certs(extra)
        .into_iter()
        .map(|cert| ureq::tls::Certificate::from_der(cert.as_ref()).to_owned())
        .collect();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::Specific(Arc::new(roots)))
                .build(),
        )
        .build()
        .into()
}

/// A rustls client config (ring provider) trusting [`root_certs`].
#[must_use]
pub fn rustls_client_config(extra: &[CertificateDer<'static>]) -> Arc<rustls::ClientConfig> {
    let mut store = rustls::RootCertStore::empty();
    store.add_parsable_certificates(root_certs(extra));
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports the default protocol versions")
    .with_root_certificates(store)
    .with_no_client_auth();
    Arc::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_a_ca_and_adds_it_to_the_webpki_roots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ca.pem");
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = params.self_signed(&key).unwrap();
        std::fs::write(&path, ca.pem()).unwrap();

        let extra = load_extra_roots(&path).unwrap();
        assert_eq!(extra, vec![ca.der().clone()]);
        let roots = root_certs(&extra);
        assert_eq!(
            roots.len(),
            webpki_root_certs::TLS_SERVER_ROOT_CERTS.len() + 1
        );
        assert!(roots.contains(ca.der()));
        assert!(
            webpki_root_certs::TLS_SERVER_ROOT_CERTS
                .iter()
                .all(|root| roots.contains(root))
        );
        let _ = rustls_client_config(&extra);
        let _ = ureq_agent(&extra, Duration::from_secs(1));
    }

    #[test]
    fn missing_or_empty_files_name_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.pem");
        let error = load_extra_roots(&missing).unwrap_err();
        assert!(format!("{error:#}").contains("nope.pem"), "{error:#}");

        let notes = dir.path().join("notes.txt");
        std::fs::write(&notes, "just some notes\n").unwrap();
        let error = load_extra_roots(&notes).unwrap_err();
        assert!(format!("{error:#}").contains("notes.txt"), "{error:#}");
    }
}
