//! HTTP surface: `/api/*` JSON endpoints, `/sync`, and static web hosting.

use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
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
    auth::{random_secret, secret_hash, verify_password},
    db::now_ms,
    registry::Account,
};

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
    pub token_hash: String,
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

pub(crate) async fn authenticate(app: &App, token: &str) -> ApiResult<AuthUser> {
    let token_hash = secret_hash(token);
    let lookup = token_hash.clone();
    let user_id = app.db.run(move |db| db.token_user(&lookup)).await?;
    // Deleted (or unknown) accounts are refused even with a live token.
    user_id
        .and_then(|id| app.view().active(&id).cloned())
        .map(|user| AuthUser { user, token_hash })
        .ok_or_else(ApiError::unauthorized)
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
        .with_state(app)
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

/// In `NeedsDecision` and `Joining` only `/api/health` and the not-set-up
/// page are served; every other API route and `/sync` answer 503.
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
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Html(NOT_SET_UP_PAGE)).into_response()
    }
}

async fn api_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not found")
}

async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "sessions": app.session_count(),
        "state": app.state(),
        "server": app.server_json(),
    }))
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
    let token = random_secret();
    let (token_hash, user_id) = (secret_hash(&token), user.id.clone());
    app.db
        .run(move |db| db.insert_token(&token_hash, &user_id))
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

async fn logout(State(app): State<Arc<App>>, auth: AuthUser) -> ApiResult<Json<Value>> {
    let hash = auth.token_hash.clone();
    app.db.run(move |db| db.revoke_token(&hash)).await?;
    let closed = app.close_token_sessions(&auth.token_hash);
    info!(user = %auth.user.name, closed, "logout");
    Ok(Json(json!({"ok": true})))
}

async fn ws_ticket(State(app): State<Arc<App>>, auth: AuthUser) -> ApiResult<Json<Value>> {
    let ticket = random_secret();
    let ttl = app.options.ticket_ttl;
    let expires = now_ms() + i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX);
    let (ticket_hash, user_id, token_hash) =
        (secret_hash(&ticket), auth.user.id.clone(), auth.token_hash);
    app.db
        .run(move |db| db.insert_ticket(&ticket_hash, &user_id, &token_hash, expires))
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
