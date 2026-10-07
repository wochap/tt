//! Starts the real `@automerge/automerge-repo-sync-server` under node.
//!
//! The package is installed with npm into `target/js-interop` (pinned
//! version) on first use. When node or npm is missing, or the install fails,
//! [`SyncServer::start`] prints a loud `SKIPPED` banner naming the missing
//! component and returns `None`; set `TT_REQUIRE_NODE=1` to turn that into a
//! test failure instead (CI does).

use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::process::{Child, Command};

pub const SYNC_SERVER_PACKAGE: &str = "@automerge/automerge-repo-sync-server@0.3.0";

pub struct SyncServer {
    pub port: u16,
    _child: Child,
    _data: tempfile::TempDir,
}

pub fn skip_or_fail(component: &str, detail: &str) -> Option<std::convert::Infallible> {
    let message = format!(
        "\n==================== SKIPPED ====================\n\
         JS interop test skipped: {component} unavailable ({detail}).\n\
         This is NOT a pass. Install node + npm or set TT_REQUIRE_NODE=1 to fail.\n\
         =================================================\n"
    );
    if std::env::var("TT_REQUIRE_NODE").is_ok_and(|value| value == "1") {
        panic!("{message}");
    }
    eprintln!("{message}");
    None
}

fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    })
}

pub fn interop_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/js-interop")
        .canonicalize()
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/js-interop"))
}

/// Installs the pinned package once; returns the scratch dir.
pub async fn install() -> Option<PathBuf> {
    if which("node").is_none() {
        skip_or_fail("node", "not found on PATH");
        return None;
    }
    if which("npm").is_none() {
        skip_or_fail("npm", "not found on PATH");
        return None;
    }
    let dir = interop_dir();
    let marker = dir.join("node_modules/@automerge/automerge-repo-sync-server/package.json");
    if marker.is_file() {
        return Some(dir);
    }
    std::fs::create_dir_all(&dir).unwrap();
    if !dir.join("package.json").is_file() {
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"tt-js-interop","private":true,"type":"module"}"#,
        )
        .unwrap();
    }
    let output = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund", SYNC_SERVER_PACKAGE])
        .current_dir(&dir)
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() && marker.is_file() => Some(dir),
        Ok(output) => {
            skip_or_fail(
                SYNC_SERVER_PACKAGE,
                &format!(
                    "npm install failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            );
            None
        }
        Err(error) => {
            skip_or_fail("npm", &error.to_string());
            None
        }
    }
}

pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

impl SyncServer {
    pub async fn start() -> Option<Self> {
        let dir = install().await?;
        let port = free_port();
        let data = tempfile::tempdir().unwrap();
        let child = Command::new("node")
            .arg("node_modules/@automerge/automerge-repo-sync-server/src/index.js")
            .current_dir(&dir)
            .env("PORT", port.to_string())
            .env("DATA_DIR", data.path())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                skip_or_fail("node", &error.to_string());
                return None;
            }
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "sync server did not listen on port {port} within 20 s"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Some(Self {
            port,
            _child: child,
            _data: data,
        })
    }

    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }
}
