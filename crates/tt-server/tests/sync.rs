//! Sync endpoint: identity, per-user isolation, the new-document rule,
//! revocation closing sessions, restart persistence, and the export.

mod common;

use std::time::Duration;

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use automerge_repo::{
    DocHandle, DocumentId, Error, error::ProtocolError, network::NetworkTransport,
    transport::ConnectionState,
};
use common::{Client, TestServer, eventually, get, post};
use serde_json::json;
use tokio::time::timeout;
use tt_core::schema;
use tt_server::{acl::Role, db::Db};

async fn text(handle: &DocHandle, key: &'static str) -> Option<String> {
    handle
        .read(move |doc| match doc.get(ROOT, key).unwrap()? {
            (automerge::Value::Object(automerge::ObjType::Text), id) => doc.text(id).ok(),
            (value, _) => value.into_string().ok(),
        })
        .await
        .unwrap()
}

async fn put(handle: &DocHandle, key: &'static str, value: &'static str) {
    handle
        .change(move |tx| {
            tx.put(ROOT, key, value)
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();
}

fn acl(server: &TestServer, doc: DocumentId) -> Vec<(String, Role)> {
    Db::open(&server.db()).unwrap().acl_rows(doc).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn users_sync_their_own_workspace_and_never_see_others() {
    let server = TestServer::start(|_| {}).await;
    let alice = server.add_user("alice").await;
    let bob = server.add_user("bob").await;
    let alice_login = server.login("alice").await;
    let bob_login = server.login("bob").await;

    let a1 = Client::connect(&server, &alice_login, "device-1").await;
    let index = a1.find_ready(&alice.index_doc).await;
    let view = index.read(schema::read_index).await.unwrap();
    assert_eq!(
        view.workspace.as_deref(),
        Some(alice.workspace_doc.as_str())
    );
    let workspace = a1.find_ready(&alice.workspace_doc).await;
    assert_eq!(
        workspace.read(schema::kind).await.unwrap().as_deref(),
        Some(schema::KIND_WORKSPACE)
    );
    put(&workspace, "note", "from device 1").await;

    // A second device of the same user (same senderId even) gets the change.
    let a2 = Client::connect(&server, &alice_login, "device-1").await;
    let mirror = a2.find_ready(&alice.workspace_doc).await;
    eventually("device 2 sees the note", 10, || async {
        text(&mirror, "note").await.as_deref() == Some("from device 1")
    })
    .await;
    assert_eq!(server.server().app().user_sessions(&alice.id), 2);

    // Bob asks for Alice's documents by id: unavailable, nothing synced.
    let b = Client::connect(&server, &bob_login, "bob-device").await;
    for id in [&alice.workspace_doc, &alice.index_doc] {
        let handle = b
            .repo
            .find(DocumentId::parse_any(id).unwrap())
            .await
            .unwrap();
        let result = timeout(Duration::from_secs(10), handle.ready())
            .await
            .expect("answered within 10 s");
        assert!(result.is_err(), "bob must not receive {id}");
        assert_eq!(text(&handle, "note").await, None);
    }
    // Bob's own workspace still works.
    b.find_ready(&bob.workspace_doc).await;
    assert_eq!(
        acl(
            &server,
            DocumentId::parse_any(&alice.workspace_doc).unwrap()
        ),
        vec![(alice.id.clone(), Role::Owner)]
    );

    // The export only contains the caller's data.
    let task = json!({
        "id": "0190f7a4-0000-7000-8000-000000000001", "seq": 1, "title": "Alice's task",
        "created": "2026-10-07T10:00:00Z", "updated": "2026-10-07T10:00:00Z"
    });
    let task: tt_core::Task = serde_json::from_value(task).unwrap();
    workspace
        .change(move |tx| schema::write_task(tx, &task).map_err(|e| Error::Change(e.to_string())))
        .await
        .unwrap();
    eventually("alice's export has her task", 10, || async {
        let (status, body) = get(&server.base(), "/api/export", Some(&alice_login.token)).await;
        status == 200 && body["tasks"][0]["title"] == "Alice's task"
    })
    .await;
    let (status, body) = get(&server.base(), "/api/export", Some(&bob_login.token)).await;
    assert_eq!(status, 200);
    assert_eq!(body["format"], "tt-export");
    assert_eq!(body["tasks"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn new_yearly_entries_document_is_accepted_once_the_index_lists_it() {
    let server = TestServer::start(|_| {}).await;
    let alice = server.add_user("alice").await;
    let login = server.login("alice").await;
    let client = Client::connect(&server, &login, "device").await;
    let index = client.find_ready(&alice.index_doc).await;

    // Created offline-style: the new document reaches the server before the
    // index edit that lists it.
    let entries = client
        .repo
        .create_with(|tx| schema::init_entries(tx, 2027).map_err(|e| Error::Change(e.to_string())))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(acl(&server, entries.id()).is_empty(), "held until listed");
    let id = entries.id().to_bs58check();
    index
        .change(move |tx| {
            schema::index_set_year(tx, 2027, &id).map_err(|e| Error::Change(e.to_string()))
        })
        .await
        .unwrap();

    eventually("owner ACL row for the new document", 10, || async {
        acl(&server, entries.id()) == vec![(alice.id.clone(), Role::Owner)]
    })
    .await;
    eventually("server stores the new document", 10, || async {
        match server.server().repo().open_document(entries.id()).await {
            Ok(handle) => handle
                .read(schema::entries_year)
                .await
                .is_ok_and(|year| year == Some(2027)),
            Err(_) => false,
        }
    })
    .await;
    assert!(
        matches!(client.transport.state(), ConnectionState::Connected { .. }),
        "the connection stays open"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unlisted_push_is_discarded_and_the_connection_closed() {
    let server =
        TestServer::start(|options| options.pending_timeout = Duration::from_millis(300)).await;
    server.add_user("alice").await;
    let login = server.login("alice").await;
    let client = Client::connect(&server, &login, "device").await;
    let mut errors = client.repo.subscribe_errors();
    let mut state = client.transport.subscribe_state();
    state.borrow_and_update();

    let rogue = client
        .repo
        .create_with(|tx| {
            tx.put(ROOT, "rogue", true)
                .map_err(|e| Error::Change(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let message = timeout(Duration::from_secs(5), async {
        loop {
            if let Error::Protocol(ProtocolError::Remote(message)) = errors.recv().await.unwrap() {
                return message;
            }
        }
    })
    .await
    .expect("server reports the protocol error");
    assert!(message.contains("not listed"), "{message}");
    timeout(
        Duration::from_secs(5),
        state.wait_for(|state| !matches!(state, ConnectionState::Connected { .. })),
    )
    .await
    .expect("connection closed")
    .unwrap();
    assert!(acl(&server, rogue.id()).is_empty());
    assert!(
        !server
            .server()
            .repo()
            .document_ids()
            .await
            .unwrap()
            .contains(&rogue.id()),
        "data discarded"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn revocation_closes_open_websockets() {
    let server = TestServer::start(|_| {}).await;
    let alice = server.add_user("alice").await;

    // Revoked by the admin CLI (another process writing the database).
    let login = server.login("alice").await;
    let client = Client::connect(&server, &login, "device").await;
    client.find_ready(&alice.workspace_doc).await;
    let tokens = tt_server::admin::list_tokens(&server.db()).unwrap();
    tt_server::admin::revoke_token(&server.db(), tokens[0].id()).unwrap();
    client
        .wait_state("rejected", |state| {
            matches!(state, ConnectionState::Rejected(_))
        })
        .await;

    // Revoked by logout.
    let login = server.login("alice").await;
    let client = Client::connect(&server, &login, "device").await;
    assert_eq!(
        post(&server.base(), "/api/logout", Some(&login.token), json!({}))
            .await
            .0,
        200
    );
    client
        .wait_state("rejected", |state| {
            matches!(state, ConnectionState::Rejected(_))
        })
        .await;
    eventually("sessions closed", 5, || async {
        server.server().app().user_sessions(&alice.id) == 0
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_loses_nothing_and_idle_documents_are_evicted() {
    let server =
        TestServer::start(|options| options.idle_eviction = Duration::from_millis(100)).await;
    let alice = server.add_user("alice").await;
    let login = server.login("alice").await;
    let workspace_id = DocumentId::parse_any(&alice.workspace_doc).unwrap();
    {
        let client = Client::connect(&server, &login, "device").await;
        let workspace = client.find_ready(&alice.workspace_doc).await;
        put(&workspace, "note", "before restart").await;
        eventually("server has the note", 10, || async {
            match server.server().repo().open_document(workspace_id).await {
                Ok(handle) => text(&handle, "note").await.as_deref() == Some("before restart"),
                Err(_) => false,
            }
        })
        .await;
        client.transport.close().await.ok();
    }
    // Disconnected and idle: flushed and dropped from memory.
    eventually("workspace evicted", 10, || async {
        server
            .server()
            .repo()
            .get(workspace_id)
            .await
            .unwrap()
            .is_none()
    })
    .await;

    let dir = server.stop().await;
    let server = TestServer::start_in(dir, |_| {}).await;
    let login = server.login("alice").await;
    let client = Client::connect(&server, &login, "other-device").await;
    let workspace = client.find_ready(&alice.workspace_doc).await;
    assert_eq!(
        text(&workspace, "note").await.as_deref(),
        Some("before restart")
    );
}
