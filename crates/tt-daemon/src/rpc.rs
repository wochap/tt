//! Newline-delimited JSON-RPC 2.0 over the unix socket, plus a blocking client.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tt_core::CoreError;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL: i64 = -32603;
/// Record not found (exit code 2).
pub const NOT_FOUND: i64 = -32002;
/// Workspace not available yet (exit code 3).
pub const UNAVAILABLE: i64 = -32003;
/// Invalid state or conflict (exit code 4).
pub const CONFLICT: i64 = -32004;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
    pub fn params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }
    pub fn internal(message: impl std::fmt::Display) -> Self {
        Self::new(INTERNAL, message.to_string())
    }
    /// CLI exit code: 1 usage, 2 not found, 3 daemon unavailable, 4 conflict.
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        match self.code {
            NOT_FOUND => 2,
            UNAVAILABLE => 3,
            CONFLICT | INTERNAL => 4,
            _ => 1,
        }
    }
}

impl From<CoreError> for RpcError {
    fn from(error: CoreError) -> Self {
        let code = match error {
            CoreError::NotFound(_) => NOT_FOUND,
            CoreError::Invalid(_) | CoreError::Conflict(_) | CoreError::Document(_) => CONFLICT,
        };
        Self::new(code, error.to_string())
    }
}

#[must_use]
pub fn response(id: Option<Value>, result: Result<Value, RpcError>) -> Value {
    match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    }
}

#[must_use]
pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// Blocking client used by the CLI.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("cannot reach daemon: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid response from daemon: {0}")]
    Protocol(String),
    #[error(transparent)]
    Rpc(#[from] RpcError),
}

impl Client {
    pub fn connect(socket: &Path) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next_id: 1,
        })
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.writer, "{request}")?;
        self.writer.flush()?;
        loop {
            let message = self.read_message()?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let error: RpcError = serde_json::from_value(error.clone())
                    .map_err(|e| ClientError::Protocol(e.to_string()))?;
                return Err(error.into());
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Reads the next line as JSON; used for subscription streams.
    pub fn read_message(&mut self) -> Result<Value, ClientError> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(ClientError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "daemon closed the connection",
            )));
        }
        serde_json::from_str(&line).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    /// Removes the read timeout for long-lived streams.
    pub fn stream_mode(&mut self) -> std::io::Result<()> {
        self.reader.get_ref().set_read_timeout(None)
    }
}
