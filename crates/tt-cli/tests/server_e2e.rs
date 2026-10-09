//! End-to-end through tt-server: a server in a temp dir with two users,
//! real daemons logging in with `tt login`. Alice's daemon syncs (including a
//! yearly entries document it creates), Bob's daemon never sees Alice's
//! data, Alice's second daemon bootstraps from the index id returned at
//! login, `tt logout` stops sync but keeps local data, and a revoked token
//! makes `tt status` report that login is required. A second server behind
//! a private CA is reached with `tt login --ca-cert`.

mod support;

use std::time::Duration;

use serde_json::Value;
use support::*;
use tt_server::{Server, ServerOptions, admin, serve};

const PASSWORD: &str = "correct horse battery";

fn export(url: &str, token: &str) -> Value {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&format!("{url}/api/export"))
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    response.body_mut().read_json().unwrap()
}

fn titles(export: &Value) -> Vec<String> {
    export["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn two_users_and_two_devices_through_tt_server() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    let (server, url) = runtime.block_on(async {
        admin::init(&db, Some("e2e")).await.unwrap();
        admin::add_user(&db, "alice", PASSWORD).await.unwrap();
        admin::add_user(&db, "bob", PASSWORD).await.unwrap();
        let mut options = ServerOptions::new(&db);
        options.revocation_poll = Duration::from_millis(200);
        let mut server = Server::open(options).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.spawn_http(listener);
        (server, format!("http://{address}"))
    });
    let alice_index = runtime
        .block_on(admin::list_users(&db))
        .unwrap()
        .into_iter()
        .find(|user| user.name == "alice")
        .unwrap()
        .index_doc;

    // Alice's first device: login, work, and a new entries document.
    let a = Env::new();
    a.login(&url, "alice", PASSWORD);
    assert_eq!(
        a.config("user.index_doc").as_deref(),
        Some(alice_index.as_str())
    );
    assert!(a.config("user.id").is_some());
    a.ok(&["task", "add", "Shared task", "+work"]);
    a.json(&["start", "1", "--at", "-10m"]);
    wait_for("A to sync", Duration::from_secs(20), || synced_with(&a, 3));
    let alice_token = a.config("server.token").unwrap();
    let alice_export = export(&url, &alice_token);
    assert_eq!(titles(&alice_export), vec!["Shared task"]);
    assert_eq!(alice_export["entries"].as_array().unwrap().len(), 1);

    // Bob: his own workspace, none of Alice's data anywhere.
    let b = Env::new();
    b.login(&url, "bob", PASSWORD);
    b.ok(&["task", "add", "Bob's task"]);
    wait_for("B to sync", Duration::from_secs(20), || synced_with(&b, 2));
    let tasks = b.json(&["task", "ls", "--state", "all"]);
    assert_eq!(titles_of_tasks(&tasks), vec!["Bob's task"]);
    assert_ne!(
        b.config("user.index_doc").as_deref(),
        Some(alice_index.as_str())
    );
    let bob_export = export(&url, &b.config("server.token").unwrap());
    assert_eq!(titles(&bob_export), vec!["Bob's task"]);
    assert_eq!(bob_export["entries"], Value::Array(vec![]));

    // Alice's second device bootstraps everything from the index id.
    let a2 = Env::new();
    a2.login(&url, "alice", PASSWORD);
    let tasks = a2.json(&["task", "ls"]);
    assert_eq!(titles_of_tasks(&tasks), vec!["Shared task"]);
    wait_for("A2 sees the running entry", Duration::from_secs(20), || {
        a2.json(&["status"])["workspace"]["running"]
            .as_array()
            .is_some_and(|running| running.len() == 1)
    });

    // Logout: token revoked server-side, sync stopped, local data kept.
    let a2_token = a2.config("server.token").unwrap();
    let out = a2.json(&["logout"]);
    assert_eq!(out["revoked"], true);
    assert_eq!(a2.config("server.token"), None);
    assert_eq!(a2.json(&["status"])["sync"]["configured"], false);
    assert_eq!(
        titles_of_tasks(&a2.json(&["task", "ls"])),
        vec!["Shared task"]
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let status = agent
        .get(&format!("{url}/api/me"))
        .header("Authorization", &format!("Bearer {a2_token}"))
        .call()
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 401);

    // Revoked on the server: the daemon stops and asks for a login.
    runtime
        .block_on(admin::revoke_user_tokens(&db, "alice"))
        .unwrap();
    wait_for("A to require login", Duration::from_secs(20), || {
        a.json(&["status"])["sync"]["state"] == "login_required"
    });
    assert!(a.ok(&["status"]).contains("login required"));
    // Offline work still succeeds.
    a.ok(&["task", "add", "Offline after revocation"]);

    runtime.block_on(server.shutdown()).unwrap();
}

fn titles_of_tasks(tasks: &Value) -> Vec<String> {
    tasks
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_owned())
        .collect()
}

/// Asks the daemon to stop and waits until its socket is gone.
fn stop_daemon(env: &Env) {
    use std::io::{BufRead, BufReader, Write};
    if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(env.socket()) {
        let _ = writeln!(stream, r#"{{"jsonrpc":"2.0","id":1,"method":"shutdown"}}"#);
        let mut line = String::new();
        let _ = BufReader::new(stream).read_line(&mut line);
    }
    wait_for("the daemon to stop", Duration::from_secs(10), || {
        std::os::unix::net::UnixStream::connect(env.socket()).is_err()
    });
}

#[test]
fn private_ca_through_login_ca_cert() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");

    // A private CA and a server certificate for localhost it signed.
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca = rcgen::CertifiedIssuer::self_signed(ca_params, rcgen::KeyPair::generate().unwrap())
        .unwrap();
    let leaf_key = rcgen::KeyPair::generate().unwrap();
    let leaf = rcgen::CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .signed_by(&leaf_key, &ca)
        .unwrap();
    let ca_pem = dir.path().join("ca.pem");
    let (cert, key) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    std::fs::write(&ca_pem, ca.pem()).unwrap();
    std::fs::write(&cert, leaf.pem()).unwrap();
    std::fs::write(&key, leaf_key.serialize_pem()).unwrap();

    runtime
        .block_on(admin::init(&db, Some("private-ca")))
        .unwrap();
    runtime
        .block_on(admin::add_user(&db, "alice", PASSWORD))
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    );
    let server = runtime.spawn(serve::run(
        ServerOptions::new(&db),
        listener,
        serve::Transport::Tls { cert, key },
    ));
    let extra = tt_daemon::tls::load_extra_roots(&ca_pem).unwrap();
    let agent = tt_daemon::tls::ureq_agent(&extra, Duration::from_secs(10));
    wait_for("the TLS server", Duration::from_secs(10), || {
        agent.get(&format!("{url}/api/me")).call().is_ok()
    });

    // Without --ca-cert: a certificate error (exit 4) with the hint.
    let c = Env::new();
    let output = c.run_with_input(
        &["login", &url, "--username", "alice"],
        &format!("{PASSWORD}\n"),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(4), "{stderr}");
    assert!(stderr.contains("UnknownIssuer"), "{stderr}");
    assert!(stderr.contains("--ca-cert"), "{stderr}");
    assert_eq!(c.config("server.token"), None);

    // A missing CA file: usage error naming it, config unchanged.
    let output = c.run_with_input(
        &[
            "login",
            &url,
            "--username",
            "alice",
            "--ca-cert",
            "/nope.pem",
        ],
        &format!("{PASSWORD}\n"),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("/nope.pem"), "{stderr}");
    assert_eq!(c.config("server.url"), None);

    // With --ca-cert: login, the absolute path stored, sync over wss://.
    let a = Env::new();
    let a_ca = a.path("ca.pem");
    std::fs::copy(&ca_pem, &a_ca).unwrap();
    let mut command = a.command(&["login", &url, "--username", "alice", "--ca-cert", "ca.pem"]);
    command.current_dir(a.dir.path());
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{PASSWORD}\n").as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        a.config("server.ca_cert").map(std::path::PathBuf::from),
        Some(std::fs::canonicalize(&a_ca).unwrap())
    );
    a.ok(&["task", "add", "Over a private CA"]);
    wait_for("A to sync over wss", Duration::from_secs(20), || {
        synced_with(&a, 2)
    });
    let token = a.config("server.token").unwrap();
    let mut response = agent
        .get(&format!("{url}/api/export"))
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .unwrap();
    let exported: Value = response.body_mut().read_json().unwrap();
    assert_eq!(titles(&exported), vec!["Over a private CA"]);

    // Logout reaches the private-CA server and revokes the token.
    let a2 = Env::new();
    let output = a2.run_with_input(
        &[
            "login",
            &url,
            "--username",
            "alice",
            "--ca-cert",
            ca_pem.to_str().unwrap(),
        ],
        &format!("{PASSWORD}\n"),
    );
    assert!(output.status.success());
    let a2_token = a2.config("server.token").unwrap();
    assert_eq!(a2.json(&["logout"])["revoked"], true);
    let status = agent
        .get(&format!("{url}/api/me"))
        .header("Authorization", &format!("Bearer {a2_token}"))
        .call()
        .unwrap()
        .status();
    assert_eq!(status.as_u16(), 401);

    // The CA file removed and the daemon restarted: no sync, local work goes on.
    stop_daemon(&a);
    std::fs::remove_file(&a_ca).unwrap();
    let status = a.json(&["status"]);
    assert_eq!(status["sync"]["state"], "ca_cert_invalid", "{status}");
    let path = status["sync"]["detail"]["path"].as_str().unwrap();
    assert!(path.ends_with("ca.pem"), "{status}");
    assert!(
        status["sync"]["detail"]["error"]
            .as_str()
            .unwrap()
            .contains("ca.pem"),
        "{status}"
    );
    assert_eq!(
        titles_of_tasks(&a.json(&["task", "ls"])),
        vec!["Over a private CA"]
    );

    server.abort();
}
