//! End-to-end through tt-server: a server in a temp dir with two users,
//! real daemons logging in with `tt login`. Alice's daemon syncs (including a
//! yearly entries document it creates), Bob's daemon never sees Alice's
//! data, Alice's second daemon bootstraps from the index id returned at
//! login, `tt logout` stops sync but keeps local data, and a revoked token
//! makes `tt status` report that login is required.

mod support;

use std::time::Duration;

use serde_json::Value;
use support::*;
use tt_server::{Server, ServerOptions, admin};

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
        admin::add_user(&db, "alice", PASSWORD).await.unwrap();
        admin::add_user(&db, "bob", PASSWORD).await.unwrap();
        let mut options = ServerOptions::new(&db);
        options.revocation_poll = Duration::from_millis(200);
        let mut server = Server::open(options).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.spawn_http(listener);
        (server, format!("http://{address}"))
    });
    let alice_index = admin::list_users(&db)
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
    admin::revoke_user_tokens(&db, "alice").unwrap();
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
