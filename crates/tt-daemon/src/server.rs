//! Unix socket server: one JSON-RPC request per line; `subscribe` turns the
//! connection into a stream of `event` notifications.

use std::{path::Path, sync::Arc};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream, unix::OwnedWriteHalf},
    sync::broadcast::error::RecvError,
};
use tracing::{debug, warn};
use tt_core::events::EventFilter;

use crate::{
    Daemon, bus,
    rpc::{self, PARSE_ERROR, Request, RpcError},
};

/// Binds the socket, replacing a stale file left by a dead daemon.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("another daemon answers on {}", path.display()),
            ));
        }
        std::fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(listener)
}

pub async fn serve(listener: UnixListener, daemon: Arc<Daemon>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let daemon = daemon.clone();
                tokio::spawn(async move {
                    if let Err(error) = connection(stream, daemon).await {
                        debug!(%error, "connection ended");
                    }
                });
            }
            Err(error) => warn!(%error, "accept failed"),
        }
    }
}

async fn write_line(writer: &mut OwnedWriteHalf, value: &Value) -> std::io::Result<()> {
    let mut line = value.to_string();
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await
}

async fn connection(stream: UnixStream, daemon: Arc<Daemon>) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                let reply = rpc::response(None, Err(RpcError::new(PARSE_ERROR, error.to_string())));
                write_line(&mut writer, &reply).await?;
                continue;
            }
        };
        if request.method == "subscribe" {
            return subscribe(request, writer, lines, daemon).await;
        }
        let id = request.id.clone();
        let result = daemon.handle(&request.method, request.params).await;
        if id.is_some() {
            write_line(&mut writer, &rpc::response(id, result)).await?;
        }
    }
    Ok(())
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SubscribeParams {
    since: Option<u64>,
    #[serde(flatten)]
    filter: EventFilter,
}

async fn subscribe(
    request: Request,
    mut writer: OwnedWriteHalf,
    mut lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    daemon: Arc<Daemon>,
) -> std::io::Result<()> {
    let params: SubscribeParams = if request.params.is_null() {
        SubscribeParams::default()
    } else {
        match serde_json::from_value(request.params) {
            Ok(params) => params,
            Err(error) => {
                let reply = rpc::response(request.id, Err(RpcError::params(error.to_string())));
                return write_line(&mut writer, &reply).await;
            }
        }
    };
    let filter = params.filter;
    // Holding the engine lock orders the snapshot against concurrent writes.
    let (snapshot, replay) = match daemon.engine_ready().await {
        Ok(engine) => {
            let engine = engine.lock().await;
            (engine.snapshot(&filter), engine.bus.subscribe(params.since))
        }
        Err(error) => {
            return write_line(&mut writer, &rpc::response(request.id, Err(error))).await;
        }
    };
    let reply = rpc::response(
        request.id,
        Ok(json!({"subscribed": true, "last_seq": replay.last_seq})),
    );
    write_line(&mut writer, &reply).await?;
    write_line(&mut writer, &rpc::notification("event", snapshot)).await?;
    if let Some((from, to)) = replay.gap {
        write_line(&mut writer, &rpc::notification("event", bus::gap(from, to))).await?;
    }
    let mut last = params.since.unwrap_or(replay.last_seq);
    for event in replay.events {
        last = event.seq;
        if filter.matches(&event.event) {
            write_line(&mut writer, &rpc::notification("event", event.to_json())).await?;
        }
    }
    let mut live = replay.live;
    loop {
        tokio::select! {
            received = live.recv() => match received {
                Ok(event) => {
                    if event.seq <= last {
                        continue;
                    }
                    last = event.seq;
                    if filter.matches(&event.event) {
                        write_line(&mut writer, &rpc::notification("event", event.to_json())).await?;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    let gap = bus::gap(last + 1, last + skipped);
                    last += skipped;
                    write_line(&mut writer, &rpc::notification("event", gap)).await?;
                }
                Err(RecvError::Closed) => return Ok(()),
            },
            line = lines.next_line() => {
                // Input after `subscribe` is ignored; EOF means the client went away.
                if line?.is_none() {
                    return Ok(());
                }
            }
        }
    }
}
