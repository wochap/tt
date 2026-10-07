//! HTTP surface: login, tokens, logout, tickets, health, export, static
//! hosting, and the admin CLI exit codes.

mod common;

use std::{process::Stdio, time::Duration};

use common::{PASSWORD, TestServer, get, post};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite;
use tt_server::{admin, admin::AdminError, db::Db};

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

    let login = server.login("alice").await;
    assert_eq!(login.user_id, alice.id);
    assert_eq!(login.index_doc, alice.index_doc);
    assert_eq!(login.token.len(), 43);

    let audit = Db::open(&server.db()).unwrap().login_audit(10).unwrap();
    assert_eq!(audit.len(), 3);
    assert!(audit[0].success);
    assert!(!audit[1].success && audit[1].username == "mallory");
    assert!(!audit[2].success && audit[2].reason == "invalid credentials");

    // Only a hash is stored.
    let tokens = Db::open(&server.db()).unwrap().tokens().unwrap();
    assert_eq!(tokens.len(), 1);
    assert_ne!(tokens[0].token_hash, login.token);

    let (status, me) = get(&base, "/api/me", Some(&login.token)).await;
    assert_eq!(status, 200);
    assert_eq!(me["user"]["name"], "alice");
    assert_eq!(me["index_doc"], alice.index_doc);
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
    let tokens = admin::list_tokens(&server.db()).unwrap();
    admin::revoke_token(&server.db(), tokens[0].id()).unwrap();
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
    assert_eq!(admin::revoke_user_tokens(&server.db(), "alice").unwrap(), 1);
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
    let first = admin::add_user(&db, "alice", PASSWORD).await.unwrap();
    let error = admin::add_user(&db, "alice", "another password")
        .await
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<AdminError>(),
        Some(AdminError::Duplicate(_))
    ));
    assert_eq!(admin::list_users(&db).unwrap(), vec![first]);
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
