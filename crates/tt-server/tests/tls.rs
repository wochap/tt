//! `serve` over TLS (rustls), and the websocket client's `wss://` support:
//! it speaks TLS and verifies the server certificate.

use std::{sync::Arc, time::Duration};

use automerge_repo::{
    network::NetworkTransport,
    transport::{ConnectionState, WsJsClient, WsJsClientConfig},
};
use tokio::time::timeout;
use tokio_tungstenite::{Connector, tungstenite};
use tt_server::{ServerOptions, serve};

#[tokio::test(flavor = "multi_thread")]
async fn serves_tls_and_the_client_verifies_certificates() {
    let dir = tempfile::tempdir().unwrap();
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let (cert, key) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    std::fs::write(&cert, certified.cert.pem()).unwrap();
    std::fs::write(&key, certified.signing_key.serialize_pem()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let db = dir.path().join("server.db");
    tt_server::admin::init(&db, Some("tls")).await.unwrap();
    let options = ServerOptions::new(&db);
    let server = tokio::spawn(serve::run(
        options,
        listener,
        serve::Transport::Tls { cert, key },
    ));
    let url = format!("wss://localhost:{port}/sync");

    // A client trusting the certificate reaches the server: TLS works and
    // the upgrade without credentials is refused with 401.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certified.cert.der().clone()).unwrap();
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let result = timeout(Duration::from_secs(10), async {
        loop {
            let attempt = tokio_tungstenite::connect_async_tls_with_config(
                url.as_str(),
                None,
                false,
                Some(Connector::Rustls(Arc::new(config.clone()))),
            )
            .await;
            match attempt {
                Err(tungstenite::Error::Io(_)) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                other => return other,
            }
        }
    })
    .await
    .expect("server answers within 10 s");
    match result {
        Err(tungstenite::Error::Http(response)) => assert_eq!(response.status().as_u16(), 401),
        other => panic!("expected 401 over TLS, got {other:?}"),
    }

    // The stock client uses the webpki roots: a self-signed certificate is
    // rejected (proving `wss://` is compiled in and verifying).
    let client = WsJsClient::start(WsJsClientConfig::new(url, "verifier"));
    let mut state = client.subscribe_state();
    let error = timeout(Duration::from_secs(10), async {
        loop {
            if let ConnectionState::Disconnected { error, .. } = &*state.borrow_and_update() {
                return error.clone();
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .expect("client reports the failed attempt");
    assert!(
        error.to_lowercase().contains("certificate") || error.contains("UnknownIssuer"),
        "{error}"
    );
    server.abort();
}

/// Waits until the client's first attempt ends and returns its error. Past
/// TLS the server refuses the credential-less upgrade with 401.
async fn first_attempt(client: &WsJsClient) -> String {
    let mut state = client.subscribe_state();
    timeout(Duration::from_secs(10), async {
        loop {
            match &*state.borrow_and_update() {
                ConnectionState::Disconnected { error, .. } => return error.clone(),
                ConnectionState::Failed(error) => return error.to_string(),
                ConnectionState::Rejected(message) => return message.clone(),
                _ => {}
            }
            state.changed().await.unwrap();
        }
    })
    .await
    .expect("client reports the attempt")
}

#[tokio::test(flavor = "multi_thread")]
async fn ws_client_trusts_a_private_ca_through_its_tls_config() {
    let dir = tempfile::tempdir().unwrap();
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca = rcgen::CertifiedIssuer::self_signed(ca_params, rcgen::KeyPair::generate().unwrap())
        .unwrap();
    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf = rcgen::CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .signed_by(&leaf_key, &ca)
        .unwrap();
    let (cert, key) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    std::fs::write(&cert, leaf.pem()).unwrap();
    std::fs::write(&key, leaf_key.serialize_pem()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let db = dir.path().join("server.db");
    tt_server::admin::init(&db, Some("tls")).await.unwrap();
    let options = ServerOptions::new(&db);
    let server = tokio::spawn(serve::run(
        options,
        listener,
        serve::Transport::Tls { cert, key },
    ));
    let url = format!("wss://localhost:{port}/sync");

    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca.der().clone()).unwrap();
    let config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );

    // Trusting the CA: TLS succeeds and the server's 401 comes back. Retry
    // while the listener is still starting (plain I/O errors).
    let error = timeout(Duration::from_secs(10), async {
        loop {
            let client = WsJsClient::start(
                WsJsClientConfig::new(url.clone(), "trusting").tls(config.clone()),
            );
            let error = first_attempt(&client).await;
            let _ = client.close().await;
            if error.contains("401") || error.to_lowercase().contains("certificate") {
                return error;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("server answers within 10 s");
    assert!(error.contains("401"), "{error}");

    // Without the TLS config: the CA is unknown.
    let client = WsJsClient::start(WsJsClientConfig::new(url, "stock"));
    let error = first_attempt(&client).await;
    assert!(error.contains("UnknownIssuer"), "{error}");
    server.abort();
}
