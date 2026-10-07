//! tt server: password accounts with bearer tokens, a per-user ACL over
//! Automerge documents, the automerge-repo websocket sync endpoint, a JSON
//! export, and static hosting of the web bundle. One binary, one SQLite
//! file, one port.

pub mod acl;
pub mod admin;
pub mod api;
pub mod app;
pub mod auth;
pub mod db;
pub mod serve;
mod sync;

pub use app::{App, Server, ServerOptions};
