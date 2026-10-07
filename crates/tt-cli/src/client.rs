//! Daemon connection with auto-spawn and readiness wait.

use std::{
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tt_daemon::{
    paths::{Paths, ensure_private_dir},
    rpc::{Client, ClientError},
};

use crate::Failure;

const READY_TIMEOUT: Duration = Duration::from_secs(5);

fn unreachable(paths: &Paths, detail: &str) -> Failure {
    Failure::new(
        3,
        format!(
            "daemon unreachable ({detail}); see log {}",
            paths.log_file().display()
        ),
    )
}

fn try_connect(paths: &Paths) -> Option<Client> {
    let mut client = Client::connect(&paths.socket).ok()?;
    client.call("ping", json!({})).ok()?;
    Some(client)
}

/// Spawns `tt daemon --spawned` detached, logging to the daemon log.
fn spawn(paths: &Paths) -> Result<std::process::Child, Failure> {
    let exe = std::env::current_exe().map_err(|e| unreachable(paths, &e.to_string()))?;
    ensure_private_dir(&paths.state_dir).map_err(|e| unreachable(paths, &e.to_string()))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log_file())
        .map_err(|e| unreachable(paths, &e.to_string()))?;
    let err = log
        .try_clone()
        .map_err(|e| unreachable(paths, &e.to_string()))?;
    Command::new(exe)
        .args(["daemon", "--spawned"])
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err)
        .process_group(0)
        .spawn()
        .map_err(|e| unreachable(paths, &format!("spawn failed: {e}")))
}

/// Connects, spawning the daemon when the socket is absent or dead.
pub fn connect(paths: &Paths) -> Result<Client, Failure> {
    if let Some(client) = try_connect(paths) {
        return Ok(client);
    }
    let mut child = spawn(paths)?;
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(client) = try_connect(paths) {
            // Reap the child in the background so it never lingers as a zombie
            // of this short-lived process; the daemon itself keeps running.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(client);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(unreachable(paths, &format!("daemon exited with {status}")));
        }
        if Instant::now() >= deadline {
            return Err(unreachable(paths, "not ready within 5 s"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Calls a method, mapping errors to exit codes.
pub fn call(
    client: &mut Client,
    paths: &Paths,
    method: &str,
    params: Value,
) -> Result<Value, Failure> {
    client.call(method, params).map_err(|error| match error {
        ClientError::Rpc(error) => Failure::rpc(error),
        other => unreachable(paths, &other.to_string()),
    })
}
