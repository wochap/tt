//! `tt-server serve`: TLS (rustls) or, behind a local reverse proxy, plain
//! HTTP; graceful shutdown on SIGINT/SIGTERM.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use axum_server::{Handle, tls_rustls::RustlsConfig};
use tracing::{info, warn};

use crate::app::{Server, ServerOptions};

/// How the listener is secured.
#[derive(Clone, Debug)]
pub enum Transport {
    Tls {
        cert: PathBuf,
        key: PathBuf,
    },
    /// Plain HTTP, only for a TLS-terminating reverse proxy on localhost.
    InsecureHttp,
}

pub const NO_TLS_MESSAGE: &str = "refusing to serve passwords over plain HTTP.\n\
    Either pass --tls-cert <cert.pem> --tls-key <key.pem> to terminate TLS here,\n\
    or pass --insecure-http when a TLS-terminating reverse proxy on this host\n\
    forwards to --listen (bind it to 127.0.0.1).";

/// Picks the transport from the flags; `Err` carries the message for exit 1.
pub fn transport(
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    insecure_http: bool,
) -> Result<Transport, String> {
    match (cert, key, insecure_http) {
        (Some(_), Some(_), true) => {
            Err("--insecure-http cannot be combined with --tls-cert/--tls-key".into())
        }
        (Some(cert), Some(key), false) => Ok(Transport::Tls { cert, key }),
        (Some(_), None, _) | (None, Some(_), _) => {
            Err("--tls-cert and --tls-key must be given together".into())
        }
        (None, None, true) => Ok(Transport::InsecureHttp),
        (None, None, false) => Err(NO_TLS_MESSAGE.into()),
    }
}

async fn signal() {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                terminate.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("interrupted"),
        () = terminate => info!("terminated"),
    }
}

/// Serves until a signal, then closes sessions and flushes the repository.
pub async fn run(
    mut options: ServerOptions,
    listener: std::net::TcpListener,
    transport: Transport,
) -> Result<()> {
    listener.set_nonblocking(true)?;
    let listen = listener.local_addr()?;
    if let Some(dir) = &options.web_dir
        && !dir.join("index.html").is_file()
    {
        // The bundle is optional: serve the API alone until it is installed.
        warn!(dir = %dir.display(), "--web-dir has no index.html; not serving the web app");
        options.web_dir = None;
    }
    let server = Server::open(options.clone()).await?;
    let service = server
        .router()
        .into_make_service_with_connect_info::<SocketAddr>();
    let handle = Handle::new();
    let shutdown = handle.clone();
    let app = server.app().clone();
    tokio::spawn(async move {
        signal().await;
        // Websocket sessions never end on their own; close them first.
        app.close_all_sessions().await;
        shutdown.graceful_shutdown(Some(Duration::from_secs(5)));
    });
    match transport {
        Transport::Tls { cert, key } => {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let config = RustlsConfig::from_pem_file(&cert, &key)
                .await
                .with_context(|| {
                    format!(
                        "loading TLS key pair {} / {}",
                        cert.display(),
                        key.display()
                    )
                })?;
            info!(%listen, db = %options.db.display(), "serving https");
            axum_server::from_tcp_rustls(listener, config)
                .handle(handle)
                .serve(service)
                .await?;
        }
        Transport::InsecureHttp => {
            info!(%listen, db = %options.db.display(), "serving plain http (--insecure-http)");
            axum_server::from_tcp(listener)
                .handle(handle)
                .serve(service)
                .await?;
        }
    }
    server.shutdown().await?;
    info!("stopped");
    Ok(())
}
