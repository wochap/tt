//! Three servers in a full mesh: A initializes, B and C join A; changes
//! relay through A, then flow between B and C directly once A is gone, and
//! offline edits on B and C converge when they link again.

mod common;

use std::time::Duration;

use automerge_repo::{DocHandle, Error};
use common::{Client, Proxy, TestServer, eventually};
use tt_core::{Task, schema};
use tt_server::admin;

async fn peer_server(init: Option<&str>) -> TestServer {
    let mut server = match init {
        Some(name) => {
            let dir = tempfile::tempdir().unwrap();
            admin::init(&dir.path().join("server.db"), Some(name))
                .await
                .unwrap();
            TestServer::reopen(dir, |_| {}).await
        }
        None => TestServer::start_uninitialized(|_| {}).await,
    };
    server.listen_peers().await;
    server
}

fn task(seq: u64, title: &str) -> Task {
    serde_json::from_value(serde_json::json!({
        "id": uuid::Uuid::now_v7(), "seq": seq, "title": title,
        "created": "2026-10-09T10:00:00Z", "updated": "2026-10-09T10:00:00Z"
    }))
    .unwrap()
}

async fn add_task(workspace: &DocHandle, seq: u64, title: &str) {
    let task = task(seq, title);
    workspace
        .change(move |tx| schema::write_task(tx, &task).map_err(|e| Error::Change(e.to_string())))
        .await
        .unwrap();
}

async fn titles(workspace: &DocHandle) -> Vec<String> {
    let mut titles: Vec<String> = workspace
        .read(schema::read_workspace)
        .await
        .unwrap()
        .tasks
        .into_values()
        .map(|task| task.title)
        .collect();
    titles.sort();
    titles
}

async fn sees(workspace: &DocHandle, title: &str) -> bool {
    titles(workspace).await.iter().any(|t| t == title)
}

#[tokio::test(flavor = "multi_thread")]
async fn three_members_relay_survive_the_first_and_converge_after_offline_edits() {
    let a = peer_server(Some("laptop-a")).await;
    let alice = a.add_user("alice").await;
    let b = peer_server(None).await;
    let c = peer_server(None).await;
    admin::join(&b.db(), &a.invite().await, Some("laptop-b"))
        .await
        .unwrap();
    admin::join(&c.db(), &a.invite().await, Some("laptop-c"))
        .await
        .unwrap();

    let on_b = Client::connect(&b, &b.login("alice").await, "phone-b").await;
    let on_c = Client::connect(&c, &c.login("alice").await, "phone-c").await;
    let workspace_b = on_b.find_ready(&alice.workspace_doc).await;
    let workspace_c = on_c.find_ready(&alice.workspace_doc).await;

    // B and C only know A: a task created through B relays through A.
    assert!(b.app().linked_servers() == [a.server_id()]);
    add_task(&workspace_b, 1, "relayed through a").await;
    eventually("c's client sees the relayed task", 15, || async {
        sees(&workspace_c, "relayed through a").await
    })
    .await;
    // B learned C's membership through A.
    eventually("b lists c", 10, || async {
        admin::list_servers(&b.db())
            .await
            .unwrap()
            .iter()
            .any(|entry| entry.name == "laptop-c")
    })
    .await;

    // A goes away; C dials B directly (through a proxy the test can cut).
    a.stop().await;
    let proxy = Proxy::start(b.peer_addr.unwrap(), -1).await;
    c.server().add_peer(proxy.addr.to_string());
    c.wait_linked(&b, 10).await;
    add_task(&workspace_b, 2, "direct to c").await;
    eventually("c's client sees b's task without a", 15, || async {
        sees(&workspace_c, "direct to c").await
    })
    .await;

    // Cut the link, edit on both sides, link again: both converge.
    proxy.cut();
    eventually("b and c are unlinked", 10, || async {
        b.app().linked_servers().is_empty() && c.app().linked_servers().is_empty()
    })
    .await;
    add_task(&workspace_b, 3, "offline on b").await;
    add_task(&workspace_c, 4, "offline on c").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!sees(&workspace_c, "offline on b").await);
    proxy.open();
    eventually("both sides converge", 20, || async {
        titles(&workspace_b).await == titles(&workspace_c).await
            && sees(&workspace_b, "offline on c").await
            && sees(&workspace_c, "offline on b").await
    })
    .await;
    assert_eq!(
        titles(&workspace_b).await,
        [
            "direct to c",
            "offline on b",
            "offline on c",
            "relayed through a"
        ]
    );
}
