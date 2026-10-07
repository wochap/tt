//! HTTP surface: `/api/*` JSON endpoints, `/sync`, and static web hosting.

use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use automerge_repo::DocumentId;
use axum::{
    Json, Router,
    extract::{ConnectInfo, FromRequestParts, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn};
use tt_core::{Entry, View, Workspace, export, schema};

use crate::{
    acl::Access,
    app::App,
    auth::{random_secret, secret_hash, verify_password},
    db::{UserRecord, now_ms},
};

/// JSON error body `{"error": "..."}` with a status.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    #[must_use]
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized")
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
        let mut response = (self.status, Json(json!({"error": self.message}))).into_response();
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
    pub user: UserRecord,
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
    let user = app.db.run(move |db| db.token_user(&lookup)).await?;
    user.map(|user| AuthUser { user, token_hash })
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
    router.with_state(app)
}

async fn api_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not found")
}

async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "sessions": app.session_count(),
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
    let name = request.username.clone();
    let user = app.db.run(move |db| db.user_by_name(&name)).await?;
    let stored = user.as_ref().map(|user| user.password_hash.clone());
    let password = request.password.clone();
    let verified =
        tokio::task::spawn_blocking(move || verify_password(stored.as_deref(), &password))
            .await
            .map_err(anyhow::Error::from)?;
    let Some(user) = user.filter(|_| verified) else {
        audit(false, "invalid credentials").await?;
        warn!(%ip, username = %request.username, "login failed");
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid username or password",
        ));
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
    })))
}

async fn me(auth: AuthUser) -> Json<Value> {
    Json(json!({
        "user": {"id": auth.user.id, "name": auth.user.name},
        "index_doc": auth.user.index_doc,
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

/// The core JSON export of the caller's workspace and entries, read from the
/// documents the caller's index lists and the ACL grants.
async fn export_json(State(app): State<Arc<App>>, auth: AuthUser) -> ApiResult<Json<Value>> {
    let mut ids = app.index_listing(&auth.user.index_doc).await?;
    if ids.is_empty()
        && let Ok(workspace) = DocumentId::parse_any(&auth.user.workspace_doc)
    {
        ids.push(workspace);
    }
    let mut view = View::default();
    for id in ids {
        if !matches!(app.acl.access(id, &auth.user.id).await?, Access::Granted(_)) {
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
