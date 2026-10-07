//! Shared end-to-end harness: real `tt` binary, real daemon, temp XDG dirs.
#![allow(dead_code)]

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

use serde_json::Value;

pub struct Env {
    pub dir: tempfile::TempDir,
    pub editor: Option<PathBuf>,
}

impl Env {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("tte2e")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        Self { dir, editor: None }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tt"));
        command
            .args(args)
            .env("HOME", self.dir.path())
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("TZ", "UTC")
            .env_remove("TT_SOCKET")
            .env_remove("VISUAL")
            .stdin(Stdio::null());
        match &self.editor {
            Some(editor) => command.env("EDITOR", editor),
            None => command.env("EDITOR", "true"),
        };
        command
    }

    /// Runs with `input` on stdin.
    pub fn run_with_input(&self, args: &[&str], input: &str) -> Output {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// `tt login <url> --username <user>` with the password on stdin.
    pub fn login(&self, url: &str, user: &str, password: &str) {
        let output = self.run_with_input(
            &["login", url, "--username", user],
            &format!("{password}\n"),
        );
        assert!(
            output.status.success(),
            "tt login failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A value from this environment's config.toml.
    pub fn config(&self, key: &str) -> Option<String> {
        let output = self.run(&["config", "get", key]);
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Runs and asserts success, returning stdout.
    pub fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "tt {args:?} failed with {}:\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    pub fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["-j"];
        all.extend_from_slice(args);
        let text = self.ok(&all);
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("tt {args:?}: {e}: {text}"))
    }

    pub fn code(&self, args: &[&str]) -> (i32, String, String) {
        let output = self.run(args);
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    pub fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    pub fn socket(&self) -> PathBuf {
        self.path("run/tt.sock")
    }

    pub fn set(&self, key: &str, value: &str) {
        self.ok(&["config", "set", key, value]);
    }

    pub fn watch(&self, args: &[&str]) -> Watch {
        let mut all = vec!["watch"];
        all.extend_from_slice(args);
        let mut child = self.command(&all).stdout(Stdio::piped()).spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx
                    .send(serde_json::from_str::<Value>(&line).unwrap())
                    .is_err()
                {
                    break;
                }
            }
        });
        Watch { child, rx }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        if let Ok(mut stream) = UnixStream::connect(self.socket()) {
            let _ = writeln!(stream, r#"{{"jsonrpc":"2.0","id":1,"method":"shutdown"}}"#);
            let mut line = String::new();
            let _ = BufReader::new(stream).read_line(&mut line);
        }
    }
}

pub struct Watch {
    child: Child,
    rx: mpsc::Receiver<Value>,
}

impl Watch {
    pub fn next(&self, timeout: Duration) -> Option<Value> {
        self.rx.recv_timeout(timeout).ok()
    }
    /// Next event of `kind`, skipping others.
    pub fn expect(&self, kind: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(event) = self.next(Duration::from_millis(200))
                && event["type"] == kind
            {
                return event;
            }
        }
        panic!("no {kind} event within 10 s");
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn wait_for(what: &str, timeout: Duration, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

pub fn titles_of(entries: &Value) -> Vec<String> {
    entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["task"]["title"].as_str().unwrap_or_default().to_owned())
        .collect()
}

pub fn sync_progress_done(env: &Env) -> bool {
    synced_with(env, 3)
}

/// Connected and every relationship with the server synced, over at least
/// `documents` documents.
pub fn synced_with(env: &Env, documents: u64) -> bool {
    let status = env.json(&["status"]);
    if std::env::var("TT_E2E_DEBUG").is_ok() {
        eprintln!("{}", status["workspace"]["sync"]);
    }
    status["sync"]["state"] == "connected"
        && status["workspace"]["sync"].as_array().is_some_and(|peers| {
            peers.iter().any(|peer| {
                peer["state"] == "synced" && peer["documents"].as_u64().unwrap_or(0) >= documents
            })
        })
}
