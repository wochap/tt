//! Client URLs and cross-origin access: `/api/health` reports the protocol
//! version and `--public-url`, `/api/*` answers CORS for member origins
//! only, and `/sync` refuses upgrades from unknown origins.

mod common;

use std::time::Duration;

use common::{TestServer, eventually, get, post};
use serde_json::json;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use tt_server::admin;

const URL_A: &str = "https://tt.laptop-a.example.ts.net";
const URL_B: &str = "https://tt.laptop-b.example.ts.net";
const EVIL: &str = "https://evil.example";

async fn peer_server(init: Option<&str>, public_url: &str) -> TestServer {
    let url = public_url.to_owned();
    let configure = move |options: &mut tt_server::ServerOptions| {
        options.public_url = Some(url);
    };
    let mut server = match init {
        Some(name) => {
            let dir = tempfile::tempdir().unwrap();
            admin::init(&dir.path().join("server.db"), Some(name))
                .await
                .unwrap();
            TestServer::reopen(dir, configure).await
        }
        None => TestServer::start_uninitialized(configure).await,
    };
    server.listen_peers().await;
    server
}

/// The response headers (lowercase names) and status of a request.
async fn request(
    method: &'static str,
    url: String,
    headers: Vec<(&'static str, String)>,
) -> (u16, std::collections::HashMap<String, String>) {
    tokio::task::spawn_blocking(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        let mut builder = ureq::http::Request::builder().method(method).uri(&url);
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        let response = agent.run(builder.body(()).unwrap()).unwrap();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    value.to_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        (response.status().as_u16(), headers)
    })
    .await
    .unwrap()
}

async fn preflight(
    server: &TestServer,
    path: &str,
    origin: &str,
) -> (u16, std::collections::HashMap<String, String>) {
    request(
        "OPTIONS",
        format!("{}{path}", server.base()),
        vec![
            ("Origin", origin.to_owned()),
            ("Access-Control-Request-Method", "POST".into()),
            (
                "Access-Control-Request-Headers",
                "authorization,content-type".into(),
            ),
        ],
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn health_reports_the_protocol_and_the_client_url() {
    let server = TestServer::start(|options| options.public_url = Some(URL_A.into())).await;
    let (status, body) = get(&server.base(), "/api/health", None).await;
    assert_eq!(status, 200);
    assert_eq!(body["protocol"], tt_server::api::PROTOCOL);
    assert_eq!(body["public_url"], URL_A);
    assert_eq!(body["server"]["public_url"], URL_A);
}

#[tokio::test(flavor = "multi_thread")]
async fn api_answers_cors_for_member_origins_only() {
    let a = peer_server(Some("laptop-a"), URL_A).await;
    a.add_user("alice").await;
    let b = peer_server(None, URL_B).await;
    admin::join(&b.db(), &a.invite().await, Some("laptop-b"))
        .await
        .unwrap();
    a.wait_linked(&b, 10).await;
    eventually("a learns b's client URL", 10, || async {
        a.app().is_member_origin(URL_B)
    })
    .await;

    // The app loaded from B asks A for a ticket.
    let (status, headers) = preflight(&a, "/api/ws-ticket", URL_B).await;
    assert_eq!(status, 204);
    assert_eq!(headers["access-control-allow-origin"], URL_B);
    assert!(headers["access-control-allow-headers"].contains("Authorization"));
    assert!(headers["access-control-allow-headers"].contains("Content-Type"));
    assert!(headers["access-control-allow-methods"].contains("POST"));
    assert!(!headers.contains_key("access-control-allow-credentials"));
    let login = a.login("alice").await;
    let (status, headers) = request(
        "POST",
        format!("{}/api/ws-ticket", a.base()),
        vec![
            ("Origin", URL_B.into()),
            ("Authorization", format!("Bearer {}", login.token)),
        ],
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(headers["access-control-allow-origin"], URL_B);
    assert!(headers["vary"].contains("Origin"));

    // A's own client URL is a member origin too, on A and on B.
    let (status, headers) = preflight(&a, "/api/health", URL_A).await;
    assert_eq!(
        (status, headers["access-control-allow-origin"].as_str()),
        (204, URL_A)
    );
    eventually("b learns a's client URL", 10, || async {
        b.app().is_member_origin(URL_A)
    })
    .await;

    // An unknown origin gets no CORS headers at all.
    let (status, headers) = preflight(&a, "/api/ws-ticket", EVIL).await;
    assert_ne!(status, 204);
    assert!(
        !headers.contains_key("access-control-allow-origin"),
        "{headers:?}"
    );
    let (status, headers) = request(
        "GET",
        format!("{}/api/me", a.base()),
        vec![
            ("Origin", EVIL.into()),
            ("Authorization", format!("Bearer {}", login.token)),
        ],
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        !headers.contains_key("access-control-allow-origin"),
        "{headers:?}"
    );

    // A revoked member's origin is no longer allowed.
    admin::revoke_server(&a.db(), "laptop-b").await.unwrap();
    assert!(!a.app().is_member_origin(URL_B));
    let (_, headers) = preflight(&a, "/api/ws-ticket", URL_B).await;
    assert!(!headers.contains_key("access-control-allow-origin"));
}

async fn ticket(server: &TestServer, token: &str) -> String {
    let (status, body) = post(&server.base(), "/api/ws-ticket", Some(token), json!({})).await;
    assert_eq!(status, 200, "{body}");
    body["ticket"].as_str().unwrap().to_owned()
}

async fn upgrade(server: &TestServer, ticket: &str, origin: Option<&str>) -> Result<(), u16> {
    let mut request = format!("{}?ticket={ticket}", server.ws())
        .into_client_request()
        .unwrap();
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("Origin", origin.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok(_) => Ok(()),
        Err(tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
        Err(error) => panic!("unexpected websocket error: {error}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_refuses_upgrades_from_unknown_origins() {
    let server = TestServer::start(|options| options.public_url = Some(URL_A.into())).await;
    server.add_user("alice").await;
    let login = server.login("alice").await;

    let first = ticket(&server, &login.token).await;
    assert_eq!(upgrade(&server, &first, Some(EVIL)).await, Err(403));
    // The refused upgrade did not use up the ticket.
    assert_eq!(upgrade(&server, &first, Some(URL_A)).await, Ok(()));

    let none = ticket(&server, &login.token).await;
    assert_eq!(
        upgrade(&server, &none, None).await,
        Ok(()),
        "no Origin: not a browser"
    );

    // A page served by this server itself (same host) is allowed even when
    // the client URL differs, e.g. without a proxy in development.
    let own = ticket(&server, &login.token).await;
    let host = format!("http://{}", server.addr);
    assert_eq!(upgrade(&server, &own, Some(&host)).await, Ok(()));
}
