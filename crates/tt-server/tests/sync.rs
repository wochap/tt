//! Sync endpoint: identity, per-user isolation derived from the indexes,
//! the new-document rule, the private registry, revocation and deletion
//! closing sessions, restart persistence, and the export.

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
use tt_server::{UserRef, admin};

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

fn owner(server: &TestServer, doc: DocumentId) -> Option<String> {
    server.server().app().owner_of(doc)
}

/// Asks for `id` and expects `doc-unavailable`.
async fn refused(client: &Client, id: &str) {
    let handle = client
        .repo
        .find(DocumentId::parse_any(id).unwrap())
        .await
        .unwrap();
    let result = timeout(Duration::from_secs(10), handle.ready())
        .await
        .expect("answered within 10 s");
    assert!(result.is_err(), "must not receive {id}");
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
        owner(
            &server,
            DocumentId::parse_any(&alice.workspace_doc).unwrap()
        ),
        Some(alice.id.clone())
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
    assert_eq!(owner(&server, entries.id()), None, "held until listed");
    let id = entries.id().to_bs58check();
    index
        .change(move |tx| {
            schema::index_set_year(tx, 2027, &id).map_err(|e| Error::Change(e.to_string()))
        })
        .await
        .unwrap();

    eventually("alice owns the new document", 10, || async {
        owner(&server, entries.id()) == Some(alice.id.clone())
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
    assert_eq!(owner(&server, rogue.id()), None);
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
    let tokens = admin::list_tokens(&server.db()).await.unwrap();
    admin::revoke_token(&server.db(), tokens[0].id())
        .await
        .unwrap();
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

#[tokio::test(flavor = "multi_thread")]
async fn registry_and_documents_listed_by_another_index_stay_private() {
    let server = TestServer::start(|_| {}).await;
    let alice = server.add_user("alice").await;
    let bob = server.add_user("bob").await;
    let bob_login = server.login("bob").await;
    let b = Client::connect(&server, &bob_login, "bob-device").await;

    // The registry is never sent to a client.
    let registry = server.server().app().root().unwrap().registry_doc;
    refused(&b, &registry).await;

    // Bob lists Alice's workspace in his own index: still refused.
    let index = b.find_ready(&bob.index_doc).await;
    let stolen = alice.workspace_doc.clone();
    index
        .change(move |tx| {
            schema::index_set_year(tx, 2030, &stolen).map_err(|e| Error::Change(e.to_string()))
        })
        .await
        .unwrap();
    let listed = DocumentId::parse_any(&alice.workspace_doc).unwrap();
    eventually("the server sees bob's index edit", 10, || async {
        match server
            .server()
            .repo()
            .open_document(DocumentId::parse_any(&bob.index_doc).unwrap())
            .await
        {
            Ok(handle) => handle
                .read(schema::read_index)
                .await
                .is_ok_and(|view| view.entries.values().any(|id| *id == alice.workspace_doc)),
            Err(_) => false,
        }
    })
    .await;
    let other = Client::connect(&server, &bob_login, "bob-device-2").await;
    refused(&other, &alice.workspace_doc).await;
    assert_eq!(
        owner(&server, listed),
        Some(alice.id.clone()),
        "alice keeps it"
    );
    // Bob's export does not include it either.
    let (status, body) = get(&server.base(), "/api/export", Some(&bob_login.token)).await;
    assert_eq!((status, body["tasks"].clone()), (200, json!([])));
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_an_account_closes_its_websockets_and_rejects_its_token() {
    let server =
        TestServer::start(|options| options.revocation_poll = Duration::from_secs(3600)).await;
    let bob = server.add_user("bob").await;
    let login = server.login("bob").await;
    let client = Client::connect(&server, &login, "device").await;
    client.find_ready(&bob.workspace_doc).await;
    admin::delete_user(&server.db(), &UserRef::Name("bob".into()))
        .await
        .unwrap();
    client
        .wait_state("rejected", |state| {
            matches!(state, ConnectionState::Rejected(_))
        })
        .await;
    assert_eq!(
        get(&server.base(), "/api/me", Some(&login.token)).await.0,
        401
    );
    eventually("sessions closed", 5, || async {
        server.server().app().user_sessions(&bob.id) == 0
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_through_the_socket_closes_sessions_without_polling() {
    // The poller would take an hour: only the socket path can be this fast.
    let server =
        TestServer::start(|options| options.revocation_poll = Duration::from_secs(3600)).await;
    let alice = server.add_user("alice").await;
    let login = server.login("alice").await;
    let client = Client::connect(&server, &login, "device").await;
    client.find_ready(&alice.workspace_doc).await;
    let tokens = admin::list_tokens(&server.db()).await.unwrap();
    assert_eq!(tokens[0].user_name, "alice");
    admin::revoke_token(&server.db(), tokens[0].id())
        .await
        .unwrap();
    client
        .wait_state("rejected", |state| {
            matches!(state, ConnectionState::Rejected(_))
        })
        .await;
}
