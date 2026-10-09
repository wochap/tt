//! HTTP surface: login, tokens, logout, tickets, health, export, static
//! hosting, the root lifecycle, account conflicts, and the admin CLI.

mod common;

use std::{process::Stdio, time::Duration};

use common::{PASSWORD, TestServer, get, post};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite;
use tt_server::{
    RootState, UserRef, admin, admin::AdminError, auth::hash_password, db::Db, registry,
};

#[tokio::test(flavor = "multi_thread")]
async fn login_me_logout_and_audit() {
    let server = TestServer::start(|_| {}).await;
    let alice = server.add_user("alice").await;
    let base = server.base();

    let (status, body) = post(
        &base,
        "/api/login",
        None,
        json!({"username": "alice", "password": "nope nope"}),
    )
    .await;
    assert_eq!(status, 401);
    assert_eq!(body, json!({"error": "invalid username or password"}));
    let (status, unknown) = post(
        &base,
        "/api/login",
        None,
        json!({"username": "mallory", "password": "nope nope"}),
    )
    .await;
    assert_eq!(
        (status, unknown),
        (401, body),
        "unknown users are indistinguishable"
    );

    let (status, body) = post(
        &base,
        "/api/login",
        None,
        json!({"username": "alice", "password": PASSWORD}),
    )
    .await;
    assert_eq!(status, 200);
    let server_id = server.server().app().identity().server_id().to_owned();
    assert_eq!(
        body["server"],
        json!({"id": server_id, "name": "test-server"})
    );
    let login = server.login("alice").await;
    assert_eq!(login.user_id, alice.id);
    assert_eq!(login.index_doc, alice.index_doc);
    assert_eq!(login.token.len(), 43);

    let audit = Db::open(&server.db()).unwrap().login_audit(10).unwrap();
    assert_eq!(audit.len(), 4);
    assert!(audit[0].success && audit[1].success);
    assert!(!audit[2].success && audit[2].username == "mallory");
    assert!(!audit[3].success && audit[3].reason == "invalid credentials");

    // Only hashes are stored.
    let tokens = Db::open(&server.db()).unwrap().tokens().unwrap();
    assert_eq!(tokens.len(), 2);
    assert_ne!(tokens[0].token_hash, login.token);

    let (status, me) = get(&base, "/api/me", Some(&login.token)).await;
    assert_eq!(status, 200);
    assert_eq!(me["user"]["name"], "alice");
    assert_eq!(me["index_doc"], alice.index_doc);
    assert_eq!(me["server"]["name"], "test-server");
    assert_eq!(me["server"]["id"], server_id.as_str());
    assert_eq!(get(&base, "/api/me", None).await.0, 401);
    assert_eq!(get(&base, "/api/me", Some("forged")).await.0, 401);

    assert_eq!(
        post(&base, "/api/logout", Some(&login.token), json!({}))
            .await
            .0,
        200
    );
    assert_eq!(get(&base, "/api/me", Some(&login.token)).await.0, 401);
    assert_eq!(
        post(&base, "/api/ws-ticket", Some(&login.token), json!({}))
            .await
            .0,
        401
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sixth_login_attempt_in_a_minute_is_rate_limited() {
    let server = TestServer::start(|_| {}).await;
    server.add_user("alice").await;
    for _ in 0..5 {
        let (status, _) = post(
            &server.base(),
            "/api/login",
            None,
            json!({"username": "alice", "password": "wrong guess"}),
        )
        .await;
        assert_eq!(status, 401);
    }
    let (status, _) = post(
        &server.base(),
        "/api/login",
        None,
        json!({"username": "alice", "password": PASSWORD}),
    )
    .await;
    assert_eq!(
        status, 429,
        "even the right password is refused while limited"
    );
    let audit = Db::open(&server.db()).unwrap().login_audit(1).unwrap();
    assert_eq!(audit[0].reason, "rate limited");
}

#[tokio::test(flavor = "multi_thread")]
async fn revoked_tokens_are_refused() {
    let server = TestServer::start(|_| {}).await;
    server.add_user("alice").await;
    let login = server.login("alice").await;
    let tokens = admin::list_tokens(&server.db()).await.unwrap();
    admin::revoke_token(&server.db(), tokens[0].id())
        .await
        .unwrap();
    assert_eq!(
        get(&server.base(), "/api/me", Some(&login.token)).await.0,
        401
    );
    assert_eq!(
        get(&server.base(), "/api/export", Some(&login.token))
            .await
            .0,
        401
    );
    let second = server.login("alice").await;
    assert_eq!(
        admin::revoke_user_tokens(&server.db(), "alice")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        get(&server.base(), "/api/me", Some(&second.token)).await.0,
        401
    );
}

async fn upgrade(url: &str) -> Result<(), u16> {
    match tokio_tungstenite::connect_async(url).await {
        Ok(_) => Ok(()),
        Err(tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
        Err(error) => panic!("unexpected websocket error: {error}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn tickets_are_single_use_and_expire() {
    let server = TestServer::start(|options| options.ticket_ttl = Duration::from_millis(300)).await;
    server.add_user("alice").await;
    let login = server.login("alice").await;
    let ticket = |_: ()| async {
        let (status, body) = post(
            &server.base(),
            "/api/ws-ticket",
            Some(&login.token),
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        body["ticket"].as_str().unwrap().to_owned()
    };

    let first = ticket(()).await;
    let url = format!("{}?ticket={first}", server.ws());
    assert_eq!(upgrade(&url).await, Ok(()));
    assert_eq!(upgrade(&url).await, Err(401), "a ticket is single-use");

    let late = ticket(()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        upgrade(&format!("{}?ticket={late}", server.ws())).await,
        Err(401),
        "expired"
    );

    assert_eq!(
        upgrade(&format!("{}?ticket=made-up", server.ws())).await,
        Err(401)
    );
    assert_eq!(upgrade(&server.ws()).await, Err(401), "no credentials");
    assert_eq!(
        upgrade(&format!("{}?token={}", server.ws(), login.token)).await,
        Err(401),
        "the long-lived token is never accepted in a URL"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn health_and_unknown_api_paths() {
    let server = TestServer::start(|_| {}).await;
    let (status, body) = get(&server.base(), "/api/health", None).await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    assert_eq!(body["state"], "Ready");
    assert_eq!(body["server"]["name"], "test-server");
    let id = body["server"]["id"].as_str().unwrap();
    assert_eq!(id, server.server().app().identity().server_id());
    assert_eq!(id.len(), 26);
    let (status, body) = get(&server.base(), "/api/nope", None).await;
    assert_eq!((status, body), (404, json!({"error": "not found"})));
    assert_eq!(
        get(&server.base(), "/tasks/42", None).await.0,
        404,
        "no web dir"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn web_dir_is_served_with_spa_fallback() {
    let web = tempfile::tempdir().unwrap();
    std::fs::write(
        web.path().join("index.html"),
        "<!doctype html><title>tt</title>",
    )
    .unwrap();
    std::fs::write(web.path().join("app.js"), "console.log('tt')").unwrap();
    let dir = web.path().to_owned();
    let server = TestServer::start(move |options| options.web_dir = Some(dir)).await;
    let (status, body) = get(&server.base(), "/app.js", None).await;
    assert_eq!((status, body.as_str()), (200, Some("console.log('tt')")));
    for path in ["/", "/tasks/42", "/reports/week"] {
        let (status, body) = get(&server.base(), path, None).await;
        assert_eq!(status, 200, "{path}");
        assert!(
            body.as_str().unwrap().contains("<title>tt</title>"),
            "{path}"
        );
    }
    assert_eq!(
        get(&server.base(), "/api/tasks/42", None).await.0,
        404,
        "API paths never fall back"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_user_fails_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    let error = admin::add_user(&db, "alice", PASSWORD).await.unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<AdminError>(),
            Some(AdminError::NotSetUp)
        ),
        "{error:#}"
    );
    admin::init(&db, Some("laptop-a")).await.unwrap();
    let first = admin::add_user(&db, "alice", PASSWORD).await.unwrap();
    let error = admin::add_user(&db, "alice", "another password")
        .await
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<AdminError>(),
        Some(AdminError::Duplicate(_))
    ));
    assert_eq!(admin::list_users(&db).await.unwrap(), vec![first]);
    let users_table: bool = Db::open(&db)
        .unwrap()
        .with(|c| {
            c.query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'users')",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert!(!users_table, "accounts live in the registry only");
    assert!(
        admin::add_user(&db, "bob", "short").await.is_err(),
        "password too short"
    );
    assert!(
        admin::add_user(&db, "b o b", PASSWORD).await.is_err(),
        "bad name"
    );
}

async fn cli(db: &std::path::Path, args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_tt-server"))
        .args(args)
        .env("TT_SERVER_DB", db)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .await
        .unwrap();
    let output = child.wait_with_output().await.unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_cli_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    let (code, _, err) = cli(&db, &["user", "ls"], "").await;
    assert_eq!(code, 3, "no root yet: {err}");
    assert!(err.contains("tt-server init"), "{err}");
    let (code, out, err) = cli(&db, &["init", "--name", "laptop-a"], "").await;
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("initialized laptop-a"), "{out}");
    let (code, _, err) = cli(&db, &["init"], "").await;
    assert_eq!(code, 4, "init twice: {err}");
    let (code, out, err) = cli(&db, &["user", "add", "alice"], "correct horse battery\n").await;
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("created alice"));
    let (code, _, err) = cli(&db, &["user", "add", "alice"], "another password\n").await;
    assert_eq!(code, 4, "duplicate: {err}");
    let (code, out, _) = cli(&db, &["user", "ls"], "").await;
    assert_eq!((code, out.lines().count()), (0, 1));
    let (code, _, _) = cli(
        &db,
        &["user", "passwd", "nobody"],
        "correct horse battery\n",
    )
    .await;
    assert_eq!(code, 2);
    let (code, _, _) = cli(&db, &["token", "revoke", "abcdef"], "").await;
    assert_eq!(code, 2);

    let (code, _, err) = cli(&db, &["serve", "--listen", "127.0.0.1:0"], "").await;
    assert_eq!(code, 1, "plain HTTP by mistake");
    assert!(
        err.contains("--tls-cert") && err.contains("--insecure-http"),
        "{err}"
    );
    let (code, _, err) = cli(&db, &["serve", "--tls-cert", "x.pem"], "").await;
    assert_eq!(code, 1, "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_passwd_rename_del_and_ls() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    admin::init(&db, None).await.unwrap();
    let root = Db::open(&db).unwrap().root().unwrap().unwrap();
    assert_eq!(root.name, tt_server::identity::host_name(), "default name");
    for name in ["alice", "bob"] {
        let (code, _, err) = cli(&db, &["user", "add", name], "correct horse battery\n").await;
        assert_eq!(code, 0, "{err}");
    }
    let (code, _, err) = cli(&db, &["user", "passwd", "alice"], "a brand new password\n").await;
    assert_eq!(code, 0, "{err}");
    let alice = admin::list_users(&db).await.unwrap().remove(0);
    assert!(tt_server::auth::verify_password(
        Some(&alice.password_hash),
        "a brand new password"
    ));

    let (code, out, err) = cli(&db, &["user", "rename", "alice", "alicia"], "").await;
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("to alicia"), "{out}");
    let (code, _, _) = cli(&db, &["user", "rename", "bob", "alicia"], "").await;
    assert_eq!(code, 4, "name taken");
    let (code, _, _) = cli(&db, &["user", "rename", "nobody", "x"], "").await;
    assert_eq!(code, 2);

    let (code, _, err) = cli(&db, &["user", "del", "bob"], "").await;
    assert_eq!(code, 0, "{err}");
    let (code, out, _) = cli(&db, &["user", "ls"], "").await;
    assert_eq!(code, 0);
    let bob = out.lines().find(|line| line.starts_with("bob")).unwrap();
    assert!(bob.contains("deleted"), "{out}");
    let alicia = out.lines().find(|line| line.starts_with("alicia")).unwrap();
    assert!(alicia.contains("active"), "{out}");
    // The name of a deleted account is free again.
    let (code, _, err) = cli(&db, &["user", "add", "bob"], "correct horse battery\n").await;
    assert_eq!(code, 0, "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn old_format_database_is_refused_and_left_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(tt_server::db::V1_SCHEMA_FOR_TESTS)
        .unwrap();
    let before = std::fs::read(&db).unwrap();
    for args in [
        &["user", "ls"][..],
        &["init"],
        &["serve", "--insecure-http", "--listen", "127.0.0.1:0"],
    ] {
        let (code, _, err) = cli(&db, args, "").await;
        assert_eq!(code, 1, "{args:?}: {err}");
        assert!(
            err.contains("no longer supported") && err.contains("tt-server init"),
            "{err}"
        );
        assert_eq!(std::fs::read(&db).unwrap(), before, "{args:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_init_is_redone_cleanly() {
    use automerge_repo::{DocumentId, SqliteStorage, storage::StorageAdapter};
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    // A crash after the registry was stored and before the root row.
    drop(Db::open(&db).unwrap());
    let mut orphan = automerge::AutoCommit::new();
    registry::init(&mut orphan).unwrap();
    let storage = SqliteStorage::open(&db).unwrap();
    storage
        .store(DocumentId::new(), orphan.save())
        .await
        .unwrap();
    storage.flush().await.unwrap();
    drop(storage);
    assert!(Db::open(&db).unwrap().root().unwrap().is_none());

    let root = admin::init(&db, Some("laptop-a")).await.unwrap();
    assert_eq!(Db::open(&db).unwrap().root().unwrap(), Some(root));
    admin::add_user(&db, "alice", PASSWORD).await.unwrap();
    let error = admin::init(&db, Some("again")).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<AdminError>(),
        Some(AdminError::AlreadyInitialized)
    ));
    assert_eq!(
        admin::list_users(&db).await.unwrap().len(),
        1,
        "nothing changed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_without_root_runs_limited_until_init_arrives() {
    let web = tempfile::tempdir().unwrap();
    std::fs::write(web.path().join("index.html"), "<title>tt</title>").unwrap();
    let web_dir = web.path().to_owned();
    let server =
        TestServer::start_uninitialized(move |options| options.web_dir = Some(web_dir)).await;
    let base = server.base();
    assert_eq!(server.server().app().state(), RootState::NeedsDecision);
    let (status, health) = get(&base, "/api/health", None).await;
    assert_eq!(status, 200);
    assert_eq!(health["state"], "NeedsDecision");
    assert_eq!(
        health["server"]["id"],
        server.server().app().identity().server_id()
    );
    assert_eq!(health["server"]["name"], json!(null));

    let (status, body) = post(
        &base,
        "/api/login",
        None,
        json!({"username": "a", "password": "b"}),
    )
    .await;
    assert_eq!(status, 503);
    let message = body["error"].as_str().unwrap();
    assert!(
        message.contains("tt-server init") && message.contains("tt-server peer join"),
        "{message}"
    );
    assert_eq!(get(&base, "/api/me", Some("x")).await.0, 503);
    assert_eq!(upgrade(&server.ws()).await, Err(503));
    let (status, page) = get(&base, "/", None).await;
    assert_eq!(status, 503);
    assert!(page.as_str().unwrap().contains("not set up"), "{page}");

    // `init` through the admin socket: Ready without a restart.
    let root = admin::init(&server.db(), Some("laptop-a")).await.unwrap();
    assert_eq!(root.server_id, server.server().app().identity().server_id());
    assert_eq!(server.server().app().state(), RootState::Ready);
    let (_, health) = get(&base, "/api/health", None).await;
    assert_eq!(
        (health["state"].clone(), health["server"]["name"].clone()),
        (json!("Ready"), json!("laptop-a"))
    );
    server.add_user("alice").await;
    server.login("alice").await;
    let (status, page) = get(&base, "/", None).await;
    assert_eq!((status, page.as_str()), (200, Some("<title>tt</title>")));
}

#[tokio::test(flavor = "multi_thread")]
async fn deleted_accounts_cannot_log_in() {
    let server = TestServer::start(|_| {}).await;
    server.add_user("bob").await;
    let login = server.login("bob").await;
    admin::delete_user(&server.db(), &UserRef::Name("bob".into()))
        .await
        .unwrap();
    let (status, body) = post(
        &server.base(),
        "/api/login",
        None,
        json!({"username": "bob", "password": PASSWORD}),
    )
    .await;
    assert_eq!(
        (status, body),
        (401, json!({"error": "invalid username or password"}))
    );
    assert_eq!(
        get(&server.base(), "/api/me", Some(&login.token)).await.0,
        401
    );
    let tokens = admin::list_tokens(&server.db()).await.unwrap();
    assert!(tokens.iter().all(|token| token.revoked.is_some()));
}

#[tokio::test(flavor = "multi_thread")]
async fn conflicted_account_gets_409_until_renamed() {
    let server = TestServer::start(|_| {}).await;
    let bob = server.add_user("bob").await;
    // A second bob created later on another server, arriving by merge.
    let other = registry::NewAccount {
        id: "ffffffff-0000-4000-8000-000000000002".into(),
        name: "bob".into(),
        index_doc: automerge_repo::DocumentId::new().to_bs58check(),
        workspace_doc: automerge_repo::DocumentId::new().to_bs58check(),
        password_hash: hash_password("the other bob's password").unwrap(),
        created: bob.created + 1000,
    };
    let root = server.server().app().root().unwrap();
    let handle = server
        .server()
        .repo()
        .find(automerge_repo::DocumentId::parse_any(&root.registry_doc).unwrap())
        .await
        .unwrap();
    let added = other.clone();
    handle
        .change(move |tx| {
            registry::add_account(tx, &added)
                .map_err(|e| automerge_repo::Error::Change(e.to_string()))
        })
        .await
        .unwrap();
    common::eventually("the view flags the second bob", 5, || async {
        server
            .server()
            .app()
            .view()
            .get(&other.id)
            .is_some_and(|a| a.conflicted)
    })
    .await;

    let login = |password: &'static str| {
        let base = server.base();
        async move {
            post(
                &base,
                "/api/login",
                None,
                json!({"username": "bob", "password": password}),
            )
            .await
        }
    };
    assert_eq!(login(PASSWORD).await.0, 200, "the owner logs in");
    let (status, body) = login("the other bob's password").await;
    assert_eq!((status, body), (409, json!({"error": "account_conflict"})));
    assert_eq!(login("nobody's password").await.0, 401);
    let audit = Db::open(&server.db()).unwrap().login_audit(2).unwrap();
    assert_eq!(audit[1].reason, "account conflict");

    let users = admin::list_users(&server.db()).await.unwrap();
    assert_eq!(users.iter().filter(|u| u.conflicted).count(), 1);
    admin::rename_user(&server.db(), &UserRef::Id(other.id.clone()), "bob2")
        .await
        .unwrap();
    let (status, body) = post(
        &server.base(),
        "/api/login",
        None,
        json!({"username": "bob2", "password": "the other bob's password"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["user"]["id"], other.id.as_str());
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_commands_go_through_the_running_server() {
    let server = TestServer::start(|_| {}).await;
    // A separate process while `serve` holds the lock: through the socket.
    let (code, out, err) = cli(
        &server.db(),
        &["user", "add", "carol"],
        "correct horse battery\n",
    )
    .await;
    assert_eq!(code, 0, "{out}{err}");
    server.login("carol").await;
    let (code, out, _) = cli(&server.db(), &["user", "ls"], "").await;
    assert_eq!((code, out.lines().count()), (0, 1));
    let (code, _, _) = cli(
        &server.db(),
        &["user", "add", "carol"],
        "correct horse battery\n",
    )
    .await;
    assert_eq!(code, 4, "errors keep their exit codes over the socket");

    // The socket is private.
    use std::os::unix::fs::PermissionsExt;
    let socket = tt_server::admin_socket::socket_path(&server.db());
    let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);

    // A second server on the same database is refused.
    let (code, _, err) = cli(
        &server.db(),
        &["serve", "--insecure-http", "--listen", "127.0.0.1:0"],
        "",
    )
    .await;
    assert_eq!(code, 1);
    assert!(err.contains("in use by another tt-server"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn direct_admin_processes_serialize_on_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    admin::init(&db, Some("laptop-a")).await.unwrap();
    let names: Vec<String> = (0..4).map(|i| format!("user{i}")).collect();
    let runs = names.iter().map(|name| {
        let db = db.clone();
        let name = name.clone();
        async move { cli(&db, &["user", "add", &name], "correct horse battery\n").await }
    });
    for (code, out, err) in futures_join(runs).await {
        assert_eq!(code, 0, "{out}{err}");
    }
    let mut listed: Vec<String> = admin::list_users(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|u| u.name)
        .collect();
    listed.sort();
    assert_eq!(listed, names, "no registry write lost");
}

async fn futures_join<F: std::future::Future<Output = T> + Send + 'static, T: Send + 'static>(
    futures: impl Iterator<Item = F>,
) -> Vec<T> {
    let handles: Vec<_> = futures.map(tokio::spawn).collect();
    let mut out = Vec::new();
    for handle in handles {
        out.push(handle.await.unwrap());
    }
    out
}
