//! HTTP surface: `/api/*` JSON endpoints, `/sync`, and static web hosting.

use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn};
use tt_core::{Entry, View, Workspace, export, schema};

use crate::{
    access::Access,
    app::{App, NOT_SET_UP, RootState},
    auth::{ParsedToken, TokenClaims, encode_token, random_secret, secret_hash, verify_password},
    db::now_ms,
    registry::{Account, RegistryView},
};

/// The client protocol version `/api/health` reports, bumped on every
/// incompatible change of what the web app relies on. 1: one server per
/// install with opaque tokens; 2: member-wide `tt2` tokens, CORS for member
/// origins, and the member list with client URLs.
pub const PROTOCOL: u32 = 2;

/// `scheme://host[:port]` of an `http`/`https` URL or `Origin` value,
/// lowercase and without a default port; `None` for anything else.
#[must_use]
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "https" => ":443",
        "http" => ":80",
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority.as_str(), |(_, host)| host);
    let authority = authority.strip_suffix(default_port).unwrap_or(authority);
    (!authority.is_empty()).then(|| format!("{scheme}://{authority}"))
}

/// Whether `origin` names the host the request was sent to (the page is
/// served by this server), from `Host`, or `X-Forwarded-Host` behind a
/// proxy.
fn same_origin(app: &App, origin: &str, headers: &HeaderMap) -> bool {
    let host = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let forwarded = app
        .options
        .behind_proxy
        .then(|| host("x-forwarded-host"))
        .flatten()
        .and_then(|value| value.split(',').next())
        .map(str::trim);
    let Some(host) = forwarded.or_else(|| host("host")) else {
        return false;
    };
    let Some(origin) = origin_of(origin) else {
        return false;
    };
    let scheme = origin
        .split_once("://")
        .map_or("https", |(scheme, _)| scheme);
    origin_of(&format!("{scheme}://{host}")).as_deref() == Some(origin.as_str())
}

/// Whether a websocket upgrade may come from `origin`: a member's client
/// URL or this server's own host. Browsers do not apply CORS to websockets,
/// so `/sync` checks it itself; the ticket remains the authentication.
pub(crate) fn sync_origin_allowed(app: &App, origin: &str, headers: &HeaderMap) -> bool {
    app.is_member_origin(origin) || same_origin(app, origin, headers)
}

/// JSON error body `{"error": "..."}` with a status.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
    state: RootState,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            state: RootState::NeedsDecision,
        }
    }
    #[must_use]
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized")
    }
    /// 503 while the server has no root, or is still fetching one.
    #[must_use]
    pub fn not_ready(state: RootState) -> Self {
        let message = if state == RootState::Joining {
            JOINING
        } else {
            NOT_SET_UP
        };
        Self {
            state,
            ..Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        warn!(error = %format!("{error:#}"), "request failed");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = if self.status == StatusCode::SERVICE_UNAVAILABLE {
            json!({"error": self.message, "state": self.state})
        } else {
            json!({"error": self.message})
        };
        let mut response = (self.status, Json(body)).into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
        }
        response
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// A request authenticated with `Authorization: Bearer <token>`.
pub struct AuthUser {
    pub user: Account,
    pub token: TokenRef,
}

/// Which token authenticated a request or a sync session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenRef {
    /// The token id (`tid`).
    pub id: String,
    /// The server id of the issuing member.
    pub issuer: String,
}

/// The bearer token of a request, if any.
pub(crate) fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|token| !token.is_empty())
}

/// The account a token grants, checked against the registry: the issuer is
/// a non-revoked member, the account is neither deleted nor conflicted, and
/// the token id is not in the replicated revocation set. The signature is
/// the caller's to check.
pub(crate) fn token_grant(
    view: &RegistryView,
    user_id: &str,
    issuer: &str,
    token_id: &str,
) -> Option<Account> {
    let issuer_ok = view.server(issuer).is_some_and(|entry| !entry.is_revoked());
    (issuer_ok && !view.is_token_revoked(token_id))
        .then(|| view.active(user_id))
        .flatten()
        .filter(|account| !account.conflicted)
        .cloned()
}

/// Verifies a `tt2` token issued by any member of this server's root,
/// without contacting the issuer.
pub(crate) async fn authenticate(app: &App, token: &str) -> ApiResult<AuthUser> {
    let parsed = ParsedToken::parse(token).ok_or_else(ApiError::unauthorized)?;
    let view = app.view();
    let claims = &parsed.claims;
    let signed = view
        .server(&claims.iss)
        .and_then(|entry| entry.public_key())
        .is_some_and(|key| parsed.verify(&key));
    if !signed {
        return Err(ApiError::unauthorized());
    }
    let user = token_grant(&view, &claims.uid, &claims.iss, &claims.tid)
        .ok_or_else(ApiError::unauthorized)?;
    let token = TokenRef {
        id: claims.tid.clone(),
        issuer: claims.iss.clone(),
    };
    let (id, user_id, issuer, created) = (
        token.id.clone(),
        user.id.clone(),
        token.issuer.clone(),
        claims.iat.saturating_mul(1000),
    );
    let live = app
        .db
        .run(move |db| db.touch_token(&id, &user_id, &issuer, created))
        .await?;
    if !live {
        return Err(ApiError::unauthorized());
    }
    Ok(AuthUser { user, token })
}

impl FromRequestParts<Arc<App>> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> ApiResult<Self> {
        let token = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        authenticate(app, token).await
    }
}

/// The client address used for rate limiting and the audit log.
fn client_ip(app: &App, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
    if app.options.behind_proxy {
        let forwarded = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .or_else(|| {
                headers
                    .get("x-real-ip")
                    .and_then(|value| value.to_str().ok())
            })
            .and_then(|value| value.trim().parse().ok());
        if let Some(ip) = forwarded {
            return ip;
        }
    }
    peer.ip()
}

pub fn router(app: Arc<App>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/peers", get(peers))
        .route("/ws-ticket", post(ws_ticket))
        .route("/export", get(export_json))
        .fallback(api_not_found);
    let router = Router::new()
        .nest("/api", api)
        .route("/sync", get(crate::sync::upgrade));
    let router = match &app.options.web_dir {
        Some(dir) => router.fallback_service(
            ServeDir::new(dir)
                .append_index_html_on_directories(true)
                .fallback(ServeFile::new(dir.join("index.html"))),
        ),
        None => router.fallback(api_not_found),
    };
    router
        .layer(middleware::from_fn_with_state(app.clone(), root_gate))
        .layer(middleware::from_fn_with_state(app.clone(), cors))
        .with_state(app)
}

/// CORS on `/api/*` for the client URL origins of non-revoked members: the
/// app loaded from one member calls the API of another. No cookies are
/// involved (the token is a bearer header), so credentials stay disallowed.
/// Other origins get no CORS headers, and their preflights fall through to
/// the router.
async fn cors(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if !(path.starts_with("/api/") || path == "/api") {
        return next.run(request).await;
    }
    let allowed = request
        .headers()
        .get(header::ORIGIN)
        .filter(|origin| {
            origin
                .to_str()
                .is_ok_and(|origin| app.is_member_origin(origin))
        })
        .cloned();
    let preflight = request.method() == Method::OPTIONS
        && request
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    let mut response = if allowed.is_some() && preflight {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    if let Some(origin) = allowed {
        let headers = response.headers_mut();
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.append(header::VARY, HeaderValue::from_static("Origin"));
        if preflight {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("Authorization, Content-Type"),
            );
            headers.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_static("600"),
            );
        }
    }
    response
}

/// Shown instead of the web app while the server has no root.
const NOT_SET_UP_PAGE: &str = "<!doctype html>
<html lang=\"en\"><meta charset=\"utf-8\"><title>tt: not set up</title>
<body style=\"font-family: system-ui, sans-serif; max-width: 36rem; margin: 4rem auto; padding: 0 1rem\">
<h1>This tt server is not set up yet</h1>
<p>On the server host, run <code>tt-server init --name &lt;name&gt;</code> to create a new server,
or <code>tt-server peer join</code> to join an existing one. This page reloads into the app once it is ready.</p>
</body></html>";

/// Answered while a join fetches the root.
const JOINING: &str =
    "this server is joining another server and fetching its data; try again in a moment";

/// In `NeedsDecision` and `Joining` only `/api/health` and the web app
/// (which shows its own not-set-up page from `/api/health`), or without a
/// web bundle a static not-set-up page, are served; every other API route
/// and `/sync` answer 503.
async fn root_gate(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let state = app.state();
    if state == RootState::Ready {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if path == "/api/health" {
        next.run(request).await
    } else if path.starts_with("/api/") || path == "/api" || path == "/sync" {
        ApiError::not_ready(state).into_response()
    } else if app.options.web_dir.is_some() {
        next.run(request).await
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Html(NOT_SET_UP_PAGE)).into_response()
    }
}

async fn api_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not found")
}

/// `setup` is `ready` once the server has a root and `needs-decision`
/// otherwise (also while a join fetches one); until then the server name is
/// the host name `init` would default to.
async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    let ready = app.state() == RootState::Ready;
    let mut server = app.server_json();
    if !ready {
        server["name"] = json!(crate::identity::host_name());
    }
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL,
        "public_url": app.options.public_url,
        "sessions": app.session_count(),
        "state": app.state(),
        "setup": if ready { "ready" } else { "needs-decision" },
        "server": server,
    }))
}

/// Every other non-revoked member with its link state, as `peer ls` shows
/// it. Membership holds no user data, so any signed-in user may read it.
async fn peers(State(app): State<Arc<App>>, _auth: AuthUser) -> ApiResult<Json<Value>> {
    let peers = app.peer_status().await?;
    Ok(Json(json!({
        "server": app.server_json(),
        "peers": peers,
    })))
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

async fn login(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> ApiResult<Json<Value>> {
    let ip = client_ip(&app, peer, &headers);
    let audit = |success: bool, reason: &'static str| {
        let (ip, username) = (ip.to_string(), request.username.clone());
        app.db
            .run(move |db| db.audit_login(&ip, &username, success, reason))
    };
    if !app.limiter.attempt(ip) {
        audit(false, "rate limited").await?;
        warn!(%ip, username = %request.username, "login rate limited");
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts; wait a minute",
        ));
    }
    // Every non-deleted account with the name, the owner first. The owner
    // logs in; a conflicted account with a matching password gets 409, which
    // needs the right password and so reveals nothing about which names exist.
    let candidates: Vec<Account> = app
        .view()
        .named(&request.username)
        .into_iter()
        .cloned()
        .collect();
    let password = request.password.clone();
    let hashes: Vec<String> = candidates.iter().map(|a| a.password_hash.clone()).collect();
    let matched = tokio::task::spawn_blocking(move || {
        if hashes.is_empty() {
            let _ = verify_password(None, &password);
            return None;
        }
        hashes
            .iter()
            .position(|hash| verify_password(Some(hash), &password))
    })
    .await
    .map_err(anyhow::Error::from)?;
    let user = match matched {
        Some(0) => candidates[0].clone(),
        Some(_) => {
            audit(false, "account conflict").await?;
            warn!(%ip, username = %request.username, "login to a conflicted account");
            return Err(ApiError::new(StatusCode::CONFLICT, "account_conflict"));
        }
        None => {
            audit(false, "invalid credentials").await?;
            warn!(%ip, username = %request.username, "login failed");
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid username or password",
            ));
        }
    };
    let claims = TokenClaims::new(
        app.identity().server_id(),
        &user.id,
        now_ms().div_euclid(1000),
    );
    let token = encode_token(&claims, &app.identity().key_pair()?)?;
    app.db
        .run(move |db| db.insert_token(&claims.tid, &claims.uid, &claims.iss))
        .await?;
    audit(true, "ok").await?;
    info!(%ip, user = %user.name, "login");
    Ok(Json(json!({
        "token": token,
        "index_doc": user.index_doc,
        "user": {"id": user.id, "name": user.name},
        "server": app.server_json(),
    })))
}

async fn me(State(app): State<Arc<App>>, auth: AuthUser) -> Json<Value> {
    Json(json!({
        "user": {"id": auth.user.id, "name": auth.user.name},
        "index_doc": auth.user.index_doc,
        "server": app.server_json(),
    }))
}

/// Revokes the token on every member: its id goes into the registry's
/// revocation set, and open sessions using it here close at once.
async fn logout(State(app): State<Arc<App>>, auth: AuthUser) -> ApiResult<Json<Value>> {
    let closed = app.revoke_token_ids(vec![auth.token.id]).await?;
    info!(user = %auth.user.name, closed, "logout");
    Ok(Json(json!({"ok": true})))
}

async fn ws_ticket(State(app): State<Arc<App>>, auth: AuthUser) -> ApiResult<Json<Value>> {
    let ticket = random_secret();
    let ttl = app.options.ticket_ttl;
    let expires = now_ms() + i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX);
    let (ticket_hash, user_id, token_id) =
        (secret_hash(&ticket), auth.user.id.clone(), auth.token.id);
    app.db
        .run(move |db| db.insert_ticket(&ticket_hash, &user_id, &token_id, expires))
        .await?;
    Ok(Json(json!({
        "ticket": ticket,
        "expires_in": ttl.as_secs(),
    })))
}

/// The core JSON export of the caller's workspace and entries: the
/// documents the caller's index lists and the caller owns.
async fn export_json(State(app): State<Arc<App>>, auth: AuthUser) -> ApiResult<Json<Value>> {
    app.refresh_listing(&auth.user.id).await?;
    let ids = app.access.owned(&auth.user.id);
    let mut view = View::default();
    for id in ids {
        if app.access.access(id, &auth.user.id) != Access::Granted {
            continue;
        }
        let Some(handle) = app.stored(id).await? else {
            continue;
        };
        let part = handle
            .read(|doc| match schema::kind(doc).as_deref() {
                Some(schema::KIND_WORKSPACE) => Part::Workspace(schema::read_workspace(doc)),
                Some(schema::KIND_ENTRIES) => Part::Entries(schema::read_entries(doc)),
                _ => Part::Other,
            })
            .await
            .map_err(anyhow::Error::from)?;
        match part {
            Part::Workspace(workspace) => view.workspace = workspace,
            Part::Entries(entries) => view.entries.extend(entries),
            Part::Other => {}
        }
    }
    let data = export::export(&view, None, chrono::Utc::now());
    Ok(Json(
        serde_json::to_value(data).map_err(anyhow::Error::from)?,
    ))
}

enum Part {
    Workspace(Workspace),
    Entries(BTreeMap<uuid::Uuid, Entry>),
    Other,
}

#[cfg(test)]
mod tests {
    use super::origin_of;

    #[test]
    fn origins_are_normalized() {
        assert_eq!(
            origin_of("https://Laptop-B.example.ts.net/app?x#y").as_deref(),
            Some("https://laptop-b.example.ts.net")
        );
        assert_eq!(
            origin_of("https://tt.example:443").as_deref(),
            Some("https://tt.example")
        );
        assert_eq!(
            origin_of("http://127.0.0.1:8080/").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            origin_of("https://user@tt.example:8443").as_deref(),
            Some("https://tt.example:8443")
        );
        assert_eq!(
            origin_of("http://[::1]:80").as_deref(),
            Some("http://[::1]")
        );
        assert_eq!(origin_of("ftp://tt.example"), None);
        assert_eq!(origin_of("null"), None);
        assert_eq!(origin_of("https://"), None);
    }
}
