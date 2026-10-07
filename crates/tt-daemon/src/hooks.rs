//! Hook runner: forks executables in `hooks/<event>`, `hooks/<event>.d/*`
//! and `hooks/all.d/*` after each event, in that order. Each event's hooks
//! run in their own task, so a slow hook delays only later hooks of the same
//! event, never other events or the write.
//!
//! Each hook gets the event JSON on stdin and `TT_EVENT`, `TT_ORIGIN`,
//! `TT_SEQ` in its environment, runs with a 10 second timeout, and has its
//! output logged. Hooks run after the write has committed and never block it;
//! a failing or hanging hook only produces a log line.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{io::AsyncWriteExt, process::Command, sync::mpsc};
use tracing::{info, warn};

use crate::bus::Published;

pub const HOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Queue feeding the single hook worker.
#[derive(Clone)]
pub struct Hooks {
    queue: mpsc::UnboundedSender<Published>,
}

impl Hooks {
    /// Starts the worker for hooks under `dir`.
    #[must_use]
    pub fn start(dir: PathBuf) -> Self {
        let (queue, mut receiver) = mpsc::unbounded_channel::<Published>();
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                let dir = dir.clone();
                tokio::spawn(async move { run_hooks(&dir, &event).await });
            }
        });
        Self { queue }
    }

    pub fn dispatch(&self, event: &Published) {
        let _ = self.queue.send(event.clone());
    }
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn sorted_dir(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|read| read.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}

/// Executables that should run for `event`, in order.
#[must_use]
pub fn discover(dir: &Path, event: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let single = dir.join(event);
    if is_executable(&single) {
        found.push(single);
    }
    found.extend(sorted_dir(&dir.join(format!("{event}.d"))));
    found.extend(sorted_dir(&dir.join("all.d")));
    found.retain(|path| is_executable(path));
    found
}

async fn run_hooks(dir: &Path, event: &Published) {
    let kind = event.event.kind.clone();
    let hooks = discover(dir, &kind);
    if hooks.is_empty() {
        return;
    }
    let payload = event.to_json().to_string();
    for hook in hooks {
        run_one(&hook, &kind, event, &payload).await;
    }
}

async fn run_one(hook: &Path, kind: &str, event: &Published, payload: &str) {
    let child = Command::new(hook)
        .env("TT_EVENT", kind)
        .env("TT_ORIGIN", event.event.origin.as_str())
        .env("TT_SEQ", event.seq.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            warn!(hook = %hook.display(), %error, "hook failed to start");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let payload = payload.to_owned();
        tokio::spawn(async move {
            let _ = stdin.write_all(payload.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
        });
    }
    match tokio::time::timeout(HOOK_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success() {
                info!(hook = %hook.display(), event = kind, seq = event.seq, stdout = %stdout.trim(), stderr = %stderr.trim(), "hook ok");
            } else {
                warn!(hook = %hook.display(), event = kind, seq = event.seq, status = %output.status, stdout = %stdout.trim(), stderr = %stderr.trim(), "hook failed");
            }
        }
        Ok(Err(error)) => warn!(hook = %hook.display(), %error, "hook wait failed"),
        Err(_) => {
            // Dropping the future dropped the child, which kill_on_drop kills.
            warn!(hook = %hook.display(), event = kind, seq = event.seq, "hook timed out after 10 s and was killed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn discovery_order_and_executable_bit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        script(&root.join("entry.started"), "true");
        script(&root.join("entry.started.d/b"), "true");
        script(&root.join("entry.started.d/a"), "true");
        script(&root.join("all.d/z"), "true");
        std::fs::write(root.join("all.d/not-exec"), "x").unwrap();
        let found = discover(root, "entry.started");
        let names: Vec<_> = found
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "entry.started",
                "entry.started.d/a",
                "entry.started.d/b",
                "all.d/z"
            ]
        );
        assert_eq!(discover(root, "task.created").len(), 1);
    }
}
