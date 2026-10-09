//! Key-pinned mutual TLS 1.3 for peer links.
//!
//! Every server presents a self-signed certificate generated at start from
//! its identity key; only the certificate's ed25519 SubjectPublicKeyInfo
//! matters. The verifiers ignore names, issuers and validity dates, pin the
//! presented key against a trust callback (registry membership), and check
//! the TLS 1.3 handshake signature with that key, so a completed handshake
//! proves the peer holds the private key. TLS 1.2 and other key types are
//! refused.
//!
//! One listener serves two ALPN protocols: `tt-sync/1` (a sync link, both
//! keys must be trusted) and `tt-pair/1` (the pairing exchange, where the
//! inviter accepts any ed25519 key and checks the invite secret instead).
//! The protocol is read from the ClientHello before the server picks its
//! configuration, so an untrusted key fails the sync handshake itself.
//! Session resumption is off on both sides: a resumed session would skip
//! the key check, so a revoked member could come back.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, DistinguishedName, Error as TlsError,
    ServerConfig, SignatureScheme,
    client::ResolvesClientCert,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, WebPkiSupportedAlgorithms, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::{
        ResolvesServerCert,
        danger::{ClientCertVerified, ClientCertVerifier},
    },
    sign::CertifiedKey,
};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::identity::Identity;

pub const ALPN_SYNC: &[u8] = b"tt-sync/1";
pub const ALPN_PAIR: &[u8] = b"tt-pair/1";

/// Decides whether a raw ed25519 public key may link.
pub type Trust = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

/// DER of an ed25519 SubjectPublicKeyInfo up to the 32 key bytes.
const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The raw ed25519 public key of a certificate; an error for any other key
/// type or an unparsable certificate.
pub fn ed25519_key(cert: &CertificateDer<'_>) -> Result<Vec<u8>, TlsError> {
    let parsed = webpki::EndEntityCert::try_from(cert)
        .map_err(|_| TlsError::InvalidCertificate(CertificateError::BadEncoding))?;
    let spki = parsed.subject_public_key_info();
    match spki.as_ref().strip_prefix(&ED25519_SPKI_PREFIX[..]) {
        Some(key) if key.len() == 32 => Ok(key.to_vec()),
        _ => Err(TlsError::General("peer key is not ed25519".into())),
    }
}

/// The ed25519 key the other side of a completed handshake presented.
#[must_use]
pub fn peer_key(state: &rustls::CommonState) -> Option<Vec<u8>> {
    ed25519_key(state.peer_certificates()?.first()?).ok()
}

/// Both verifiers: pin the key, ignore everything else, check signatures.
#[derive(Clone)]
struct Pin {
    trust: Trust,
    algorithms: WebPkiSupportedAlgorithms,
}

impl std::fmt::Debug for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pin").finish_non_exhaustive()
    }
}

impl Pin {
    fn new(trust: Trust, provider: &CryptoProvider) -> Arc<Self> {
        Arc::new(Self {
            trust,
            algorithms: provider.signature_verification_algorithms,
        })
    }

    fn check(&self, cert: &CertificateDer<'_>) -> Result<(), TlsError> {
        let key = ed25519_key(cert)?;
        if (self.trust)(&key) {
            Ok(())
        } else {
            Err(TlsError::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn tls13(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        if dss.scheme != SignatureScheme::ED25519 {
            return Err(TlsError::General("peer must sign with ed25519".into()));
        }
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }
}

fn no_tls12() -> TlsError {
    TlsError::General("peer links require TLS 1.3".into())
}

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        self.check(end_entity)
            .map(|()| ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Err(no_tls12())
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.tls13(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}

impl ClientCertVerifier for Pin {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, TlsError> {
        self.check(end_entity)
            .map(|()| ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Err(no_tls12())
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.tls13(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}

/// Always presents one certificate. Unlike `with_single_cert` it does not
/// check that the key matches the certificate, which only tests exploit.
#[derive(Debug)]
struct Fixed(Arc<CertifiedKey>);

impl ResolvesServerCert for Fixed {
    fn resolve(&self, _: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}

impl ResolvesClientCert for Fixed {
    fn resolve(&self, _: &[&[u8]], _: &[SignatureScheme]) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
    fn has_certs(&self) -> bool {
        true
    }
}

/// This server's certificate and key, and config builders around them.
pub struct PeerTls {
    certified: Option<Arc<CertifiedKey>>,
    chain: Vec<CertificateDer<'static>>,
    provider: Arc<CryptoProvider>,
}

impl std::fmt::Debug for PeerTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerTls").finish_non_exhaustive()
    }
}

/// A self-signed certificate for the identity key. Names and dates do not
/// matter; it is regenerated at every start.
pub fn certificate(identity: &Identity) -> Result<CertificateDer<'static>> {
    let pkcs8 = PrivatePkcs8KeyDer::from(identity.pkcs8().to_vec());
    let key = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&pkcs8, &rcgen::PKCS_ED25519)
        .context("loading the server key for its certificate")?;
    let mut params = rcgen::CertificateParams::new(vec!["tt-peer".to_owned()])?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, identity.server_id());
    Ok(params.self_signed(&key)?.der().clone())
}

impl PeerTls {
    pub fn new(identity: &Identity) -> Result<Self> {
        let certificate = certificate(identity)?;
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.pkcs8().to_vec()));
        Ok(Self::from_parts(vec![certificate], key))
    }

    /// Any certificate and key (tests build impostors and other key types).
    #[must_use]
    pub fn from_parts(chain: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) -> Self {
        let provider = provider();
        let certified = provider
            .key_provider
            .load_private_key(key)
            .ok()
            .map(|key| Arc::new(CertifiedKey::new(chain.clone(), key)));
        Self {
            certified,
            chain,
            provider,
        }
    }

    fn resolver(&self) -> Result<Arc<Fixed>> {
        self.certified
            .clone()
            .map(|certified| Arc::new(Fixed(certified)))
            .ok_or_else(|| anyhow!("unsupported private key"))
    }

    #[must_use]
    pub fn certificate(&self) -> &CertificateDer<'static> {
        &self.chain[0]
    }

    /// Client side: presents our certificate, accepts a server whose key
    /// `trust` accepts.
    pub fn client(&self, alpn: &[u8], trust: Trust) -> Result<Arc<ClientConfig>> {
        let mut config = ClientConfig::builder_with_provider(self.provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(Pin::new(trust, &self.provider))
            .with_client_cert_resolver(self.resolver()?);
        config.alpn_protocols = vec![alpn.to_vec()];
        // A resumed session skips certificate verification: every link
        // must prove its key against current membership.
        config.resumption = rustls::client::Resumption::disabled();
        Ok(Arc::new(config))
    }

    /// Server side: requires a client certificate whose key `trust` accepts.
    pub fn server(&self, alpn: &[u8], trust: Trust) -> Result<Arc<ServerConfig>> {
        let mut config = ServerConfig::builder_with_provider(self.provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(Pin::new(trust, &self.provider))
            .with_cert_resolver(self.resolver()?);
        config.alpn_protocols = vec![alpn.to_vec()];
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        config.send_tls13_tickets = 0;
        Ok(Arc::new(config))
    }
}

/// What an inbound peer connection is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Sync,
    Pair,
}

/// The peer listener's TLS side: picks the configuration by ALPN.
pub struct PeerAcceptor {
    sync: Arc<ServerConfig>,
    pair: Arc<ServerConfig>,
}

impl PeerAcceptor {
    /// `trust` decides sync links; pairing accepts any ed25519 key.
    pub fn new(tls: &PeerTls, trust: Trust) -> Result<Self> {
        Ok(Self {
            sync: tls.server(ALPN_SYNC, trust)?,
            pair: tls.server(ALPN_PAIR, Arc::new(|_: &[u8]| true))?,
        })
    }

    /// Completes the handshake; returns the stream, its purpose and the
    /// client's verified key.
    pub async fn accept<S>(
        &self,
        stream: S,
    ) -> Result<(tokio_rustls::server::TlsStream<S>, Purpose, Vec<u8>)>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let acceptor =
            tokio_rustls::LazyConfigAcceptor::new(rustls::server::Acceptor::default(), stream);
        let start = acceptor.await.context("reading the client hello")?;
        let offered: Vec<Vec<u8>> = start
            .client_hello()
            .alpn()
            .map(|protocols| protocols.map(<[u8]>::to_vec).collect())
            .unwrap_or_default();
        let (config, purpose) = if offered.iter().any(|p| p == ALPN_SYNC) {
            (self.sync.clone(), Purpose::Sync)
        } else if offered.iter().any(|p| p == ALPN_PAIR) {
            (self.pair.clone(), Purpose::Pair)
        } else {
            bail!("the client offered no tt peer protocol");
        };
        let stream = start.into_stream(config).await.context("peer handshake")?;
        let key = peer_key(stream.get_ref().1).ok_or_else(|| anyhow!("no peer key"))?;
        Ok((stream, purpose, key))
    }
}

/// Completes a client handshake; returns the stream and the server's
/// verified key.
pub async fn connect<S>(
    stream: S,
    config: Arc<ClientConfig>,
) -> Result<(tokio_rustls::client::TlsStream<S>, Vec<u8>)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let name = ServerName::try_from("tt-peer").expect("valid name");
    let stream = tokio_rustls::TlsConnector::from(config)
        .connect(name, stream)
        .await
        .context("peer handshake")?;
    let key = peer_key(stream.get_ref().1).ok_or_else(|| anyhow!("no peer key"))?;
    Ok((stream, key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{KEY_FILE, server_id};

    fn identity(dir: &tempfile::TempDir, name: &str) -> Identity {
        Identity::load_or_create(&dir.path().join(name).join(KEY_FILE)).unwrap()
    }

    fn only(key: Vec<u8>) -> Trust {
        Arc::new(move |presented: &[u8]| presented == key.as_slice())
    }

    /// Runs one handshake over an in-memory pipe; returns (client, server)
    /// outcomes with the key each side saw.
    async fn handshake(
        client: Arc<ClientConfig>,
        acceptor: &PeerAcceptor,
    ) -> (Result<Vec<u8>>, Result<(Purpose, Vec<u8>)>) {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (client, server) = tokio::join!(connect(a, client), acceptor.accept(b));
        (
            client.map(|(_, key)| key),
            server.map(|(_, purpose, key)| (purpose, key)),
        )
    }

    #[test]
    fn the_certificate_carries_the_server_key() {
        let dir = tempfile::tempdir().unwrap();
        let identity = identity(&dir, "a");
        let tls = PeerTls::new(&identity).unwrap();
        assert_eq!(
            ed25519_key(tls.certificate()).unwrap(),
            identity.public_key()
        );
        // Regenerated certificates differ but pin the same key.
        let again = certificate(&identity).unwrap();
        assert_eq!(ed25519_key(&again).unwrap(), identity.public_key());
        assert_eq!(
            server_id(&ed25519_key(&again).unwrap()),
            identity.server_id()
        );
    }

    #[tokio::test]
    async fn members_link_and_unknown_or_revoked_keys_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, stranger) = (
            identity(&dir, "a"),
            identity(&dir, "b"),
            identity(&dir, "c"),
        );
        let (a_tls, b_tls, stranger_tls) = (
            PeerTls::new(&a).unwrap(),
            PeerTls::new(&b).unwrap(),
            PeerTls::new(&stranger).unwrap(),
        );
        let trusted = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let b_key = b.public_key().to_vec();
        let flag = trusted.clone();
        let acceptor = PeerAcceptor::new(
            &a_tls,
            Arc::new(move |key: &[u8]| {
                flag.load(std::sync::atomic::Ordering::SeqCst) && key == b_key.as_slice()
            }),
        )
        .unwrap();

        // Member accepted, both sides see the other's key.
        let config = b_tls
            .client(ALPN_SYNC, only(a.public_key().to_vec()))
            .unwrap();
        let (client, server) = handshake(config.clone(), &acceptor).await;
        assert_eq!(client.unwrap(), a.public_key());
        assert_eq!(server.unwrap(), (Purpose::Sync, b.public_key().to_vec()));

        // Unknown key refused by the server during the handshake.
        let unknown = stranger_tls
            .client(ALPN_SYNC, only(a.public_key().to_vec()))
            .unwrap();
        let (_, server) = handshake(unknown.clone(), &acceptor).await;
        assert!(server.is_err());

        // ... but it may pair (the secret decides there).
        let pairing = stranger_tls
            .client(ALPN_PAIR, only(a.public_key().to_vec()))
            .unwrap();
        let (client, server) = handshake(pairing, &acceptor).await;
        assert_eq!(client.unwrap(), a.public_key());
        assert_eq!(
            server.unwrap(),
            (Purpose::Pair, stranger.public_key().to_vec())
        );

        // Revoked: the same member is now refused, also with the client
        // config that completed a handshake before (no resumption).
        trusted.store(false, std::sync::atomic::Ordering::SeqCst);
        let (_, server) = handshake(config, &acceptor).await;
        assert!(server.is_err());

        // A client that does not trust the server's key aborts.
        trusted.store(true, std::sync::atomic::Ordering::SeqCst);
        let wrong_pin = b_tls
            .client(ALPN_SYNC, only(stranger.public_key().to_vec()))
            .unwrap();
        let (client, _) = handshake(wrong_pin, &acceptor).await;
        assert!(client.is_err());
    }

    #[tokio::test]
    async fn names_and_dates_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (identity(&dir, "a"), identity(&dir, "b"));
        // B presents an expired certificate for an unrelated name.
        let pkcs8 = PrivatePkcs8KeyDer::from(b.pkcs8().to_vec());
        let key =
            rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&pkcs8, &rcgen::PKCS_ED25519).unwrap();
        let mut params = rcgen::CertificateParams::new(vec!["elsewhere.example".into()]).unwrap();
        params.not_before = rcgen::date_time_ymd(2001, 1, 1);
        params.not_after = rcgen::date_time_ymd(2002, 1, 1);
        let expired = params.self_signed(&key).unwrap().der().clone();
        let b_tls = PeerTls::from_parts(vec![expired], PrivateKeyDer::Pkcs8(pkcs8));
        let acceptor =
            PeerAcceptor::new(&PeerTls::new(&a).unwrap(), only(b.public_key().to_vec())).unwrap();
        let config = b_tls
            .client(ALPN_SYNC, only(a.public_key().to_vec()))
            .unwrap();
        let (client, server) = handshake(config, &acceptor).await;
        client.unwrap();
        server.unwrap();
    }

    #[tokio::test]
    async fn impersonation_without_the_private_key_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, impostor) = (
            identity(&dir, "a"),
            identity(&dir, "b"),
            identity(&dir, "c"),
        );
        // The impostor presents A's real (public) certificate but can only
        // sign with its own key.
        let a_certificate = PeerTls::new(&a).unwrap().certificate().clone();
        let fake = PeerTls::from_parts(
            vec![a_certificate],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(impostor.pkcs8().to_vec())),
        );
        // As a server towards B, which trusts A.
        let acceptor = PeerAcceptor::new(&fake, Arc::new(|_: &[u8]| true)).unwrap();
        let config = PeerTls::new(&b)
            .unwrap()
            .client(ALPN_SYNC, only(a.public_key().to_vec()))
            .unwrap();
        let (client, _) = handshake(config, &acceptor).await;
        assert!(client.is_err(), "B must not accept the impostor as A");
        // As a client towards B, which trusts A.
        let acceptor =
            PeerAcceptor::new(&PeerTls::new(&b).unwrap(), only(a.public_key().to_vec())).unwrap();
        let config = fake.client(ALPN_SYNC, Arc::new(|_: &[u8]| true)).unwrap();
        let (_, server) = handshake(config, &acceptor).await;
        assert!(
            server.is_err(),
            "B must not accept the impostor client as A"
        );
    }

    #[tokio::test]
    async fn other_key_types_and_tls_1_2_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let a = identity(&dir, "a");
        let acceptor =
            PeerAcceptor::new(&PeerTls::new(&a).unwrap(), Arc::new(|_: &[u8]| true)).unwrap();

        // A P-256 client certificate.
        let p256 = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let p256_certificate = rcgen::CertificateParams::new(vec!["tt-peer".into()])
            .unwrap()
            .self_signed(&p256)
            .unwrap();
        assert!(ed25519_key(p256_certificate.der()).is_err());
        let ecdsa = PeerTls::from_parts(
            vec![p256_certificate.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(p256.serialize_der())),
        );
        let config = ecdsa.client(ALPN_SYNC, Arc::new(|_: &[u8]| true)).unwrap();
        let (_, server) = handshake(config, &acceptor).await;
        assert!(server.is_err(), "a non-ed25519 client key is refused");

        // A client limited to TLS 1.2.
        let b = identity(&dir, "b");
        let pkcs8 = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(b.pkcs8().to_vec()));
        let mut config = ClientConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Pin::new(Arc::new(|_: &[u8]| true), &provider()))
            .with_client_auth_cert(vec![certificate(&b).unwrap()], pkcs8)
            .unwrap();
        config.alpn_protocols = vec![ALPN_SYNC.to_vec()];
        let (client, server) = handshake(Arc::new(config), &acceptor).await;
        assert!(client.is_err() && server.is_err(), "TLS 1.2 is refused");

        // No tt protocol offered.
        let mut config = (*PeerTls::new(&b)
            .unwrap()
            .client(ALPN_SYNC, Arc::new(|_: &[u8]| true))
            .unwrap())
        .clone();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let (_, server) = handshake(Arc::new(config), &acceptor).await;
        assert!(server.is_err());
    }
}
