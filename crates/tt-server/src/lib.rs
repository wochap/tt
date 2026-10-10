//! tt server: one root per server (a registry document holding every
//! account), password login with bearer tokens, per-user document access
//! derived from each user's index, the automerge-repo websocket sync
//! endpoint, a JSON export, an admin socket, and static hosting of the web
//! bundle. One binary, one SQLite file, one port.

pub mod access;
pub mod admin;
pub mod admin_socket;
pub mod api;
pub mod app;
pub mod auth;
pub mod db;
pub mod identity;
pub mod links;
pub mod peer;
pub mod registry;
pub mod serve;
mod sync;
pub mod tls;
pub mod transport;

pub use app::{App, RootState, Server, ServerOptions, UserRef};
