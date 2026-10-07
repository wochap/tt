//! `serve` over TLS (rustls), and the websocket client's `wss://` support:
//! it speaks TLS and verifies the server certificate.

use std::{sync::Arc, time::Duration};

use automerge_repo::transport::{ConnectionState, WsJsClient, WsJsClientConfig};
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
    let options = ServerOptions::new(dir.path().join("server.db"));
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
