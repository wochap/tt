//! Member-wide tokens: a token issued by one member is accepted by every
//! member of the root, a logout on one member reaches the others through
//! the registry, and tokens of a revoked or forged issuer are refused.

mod common;

use common::{Client, TestServer, eventually, get, post};
use ring::signature::Ed25519KeyPair;
use serde_json::json;
use tt_server::{
    admin,
    auth::{TokenClaims, encode_token},
};

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

/// A with alice, and B joined to A, linked.
async fn pair() -> (TestServer, TestServer) {
    let a = peer_server(Some("laptop-a")).await;
    a.add_user("alice").await;
    let b = peer_server(None).await;
    admin::join(&b.db(), &a.invite().await, Some("laptop-b"))
        .await
        .unwrap();
    a.wait_linked(&b, 10).await;
    b.wait_linked(&a, 10).await;
    (a, b)
}

async fn me(server: &TestServer, token: &str) -> u16 {
    get(&server.base(), "/api/me", Some(token)).await.0
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_from_one_member_is_accepted_by_another_while_the_issuer_is_offline() {
    let (a, b) = pair().await;
    let login = a.login("alice").await;
    let (status, body) = get(&b.base(), "/api/me", Some(&login.token)).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["user"]["id"], login.user_id);
    assert_eq!(body["server"]["name"], "laptop-b");

    // B records the token it accepted, with A as the issuer.
    let tokens = admin::list_tokens(&b.db()).await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].issuer, a.server_id());

    // A goes away: B still accepts the token, and gives tickets for it.
    let a_dir = a.stop().await;
    assert_eq!(me(&b, &login.token).await, 200);
    let client = Client::connect(&b, &login, "phone").await;
    drop(client);
    drop(a_dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_claiming_a_member_but_signed_by_another_key_is_refused() {
    let (a, b) = pair().await;
    let login = a.login("alice").await;
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let forged = encode_token(
        &TokenClaims::new(&a.server_id(), &login.user_id, 1_700_000_000),
        &key,
    )
    .unwrap();
    assert_eq!(me(&a, &forged).await, 401);
    assert_eq!(me(&b, &forged).await, 401);

    // An issuer that is not a member at all.
    let stranger = encode_token(
        &TokenClaims::new(
            &tt_server::identity::server_id(b"not a member"),
            &login.user_id,
            1,
        ),
        &key,
    )
    .unwrap();
    assert_eq!(me(&b, &stranger).await, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn logout_on_one_member_reaches_the_others_and_closes_their_sessions() {
    let (a, b) = pair().await;
    let login = a.login("alice").await;
    assert_eq!(me(&b, &login.token).await, 200);
    let on_a = Client::connect(&a, &login, "laptop-client").await;
    let on_b = Client::connect(&b, &login, "phone").await;
    assert_eq!(a.app().user_sessions(&login.user_id), 1);
    assert_eq!(b.app().user_sessions(&login.user_id), 1);

    let (status, body) = post(&b.base(), "/api/logout", Some(&login.token), json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(me(&b, &login.token).await, 401);
    eventually("b closes its session at once", 5, || async {
        b.app().user_sessions(&login.user_id) == 0
    })
    .await;
    eventually(
        "a rejects the token after the registry syncs",
        15,
        || async { me(&a, &login.token).await == 401 },
    )
    .await;
    eventually("a closes its session too", 5, || async {
        a.app().user_sessions(&login.user_id) == 0
    })
    .await;
    let tokens = admin::list_tokens(&a.db()).await.unwrap();
    assert!(tokens[0].revoked.is_some(), "{tokens:?}");
    drop((on_a, on_b));

    // `token revoke` on A reaches B the same way.
    let second = a.login("alice").await;
    assert_eq!(me(&b, &second.token).await, 200);
    let tokens = admin::list_tokens(&a.db()).await.unwrap();
    let live = tokens.iter().find(|token| token.revoked.is_none()).unwrap();
    admin::revoke_token(&a.db(), live.id()).await.unwrap();
    assert_eq!(me(&a, &second.token).await, 401);
    eventually("b rejects the revoked token", 15, || async {
        me(&b, &second.token).await == 401
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tokens_of_a_revoked_issuer_are_refused() {
    let (a, b) = pair().await;
    let from_a = a.login("alice").await;
    let from_b = b.login("alice").await;
    assert_eq!(me(&b, &from_a.token).await, 200);
    let on_b = Client::connect(&b, &from_a, "phone").await;
    admin::revoke_server(&b.db(), "laptop-a").await.unwrap();
    assert_eq!(me(&b, &from_a.token).await, 401);
    eventually("b closes the session of a's token", 5, || async {
        b.app().user_sessions(&from_a.user_id) == 0
    })
    .await;
    assert_eq!(
        me(&b, &from_b.token).await,
        200,
        "b's own tokens still work"
    );
    drop(on_b);
}
