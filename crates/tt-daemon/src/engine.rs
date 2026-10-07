//! The daemon's single writer: owns document handles and the current view,
//! applies commands, and turns every change into domain events.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use automerge::transaction::Transaction;
use automerge_repo::{ChangeOrigin, DocHandle, DocumentId, DocumentStatus, Repo};
use chrono::{DateTime, Datelike, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::{info, warn};
use tt_core::{
    CoreError, CoreResult, Entry, Index, Task, TaskState, View,
    events::{self, EventFilter, Origin},
    export,
    model::User,
    ops, report, round, schema, search, text,
    time::Range,
};
use uuid::Uuid;

use crate::{
    bus::{self, Bus},
    hooks::Hooks,
    rpc::{CONFLICT, RpcError, UNAVAILABLE},
};

pub type RpcResult<T> = Result<T, RpcError>;

/// How long to wait for a document that must come from the server.
const REMOTE_DOC_TIMEOUT: Duration = Duration::from_secs(60);

fn doc_error(error: impl std::fmt::Display) -> RpcError {
    RpcError::new(CONFLICT, error.to_string())
}

fn params<T: for<'de> Deserialize<'de>>(value: Value) -> RpcResult<T> {
    let value = if value.is_null() { json!({}) } else { value };
    serde_json::from_value(value).map_err(|error| RpcError::params(error.to_string()))
}

fn tz_of(name: Option<&str>) -> Tz {
    tt_core::time::resolve_tz(name, Some("UTC"))
}

/// Waits until a handle has content; `Unavailable` keeps waiting because a
/// peer that has it may still connect.
pub async fn wait_ready(handle: &DocHandle, timeout: Duration) -> RpcResult<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match handle.status() {
            DocumentStatus::Ready => return Ok(()),
            DocumentStatus::Closed => {
                return Err(RpcError::new(
                    UNAVAILABLE,
                    format!("document {} closed", handle.id()),
                ));
            }
            DocumentStatus::Loading | DocumentStatus::Unavailable => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(RpcError::new(
                UNAVAILABLE,
                format!(
                    "document {} is not available locally and was not received from the server",
                    handle.id().to_bs58check()
                ),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Forwards remote changes of `handle` to the engine's refresh queue.
fn watch_remote(handle: &DocHandle, queue: mpsc::UnboundedSender<DocumentId>) {
    let mut events = handle.subscribe();
    let id = handle.id();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if matches!(event.origin, ChangeOrigin::Remote(_)) && queue.send(id).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if queue.send(id).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub struct Engine {
    pub repo: Repo,
    index: DocHandle,
    workspace: DocHandle,
    entries: BTreeMap<i32, DocHandle>,
    loaded: BTreeSet<i32>,
    index_view: Index,
    pub view: View,
    pub bus: Bus,
    hooks: Hooks,
    remote: mpsc::UnboundedSender<DocumentId>,
}

/// Result of [`Engine::init`].
pub struct Init {
    pub engine: Engine,
    /// Set when a fresh index was created and must be saved to config.
    pub created_index: Option<String>,
}

async fn change<T, F>(handle: &DocHandle, apply: F) -> RpcResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Transaction<'_>) -> CoreResult<T> + Send + 'static,
{
    let failure: Arc<Mutex<Option<CoreError>>> = Arc::default();
    let slot = failure.clone();
    let result = handle
        .change(move |tx| {
            apply(tx).map_err(|error| {
                *slot.lock().unwrap() = Some(error);
                automerge_repo::Error::Change("domain validation failed".into())
            })
        })
        .await;
    if let Some(error) = failure.lock().unwrap().take() {
        return Err(error.into());
    }
    result.map(|change| change.value).map_err(doc_error)
}

impl Engine {
    /// Loads (or on first run creates) the index and workspace documents.
    pub async fn init(
        repo: Repo,
        index_doc: Option<&str>,
        user: User,
        bus: Bus,
        hooks: Hooks,
        remote: mpsc::UnboundedSender<DocumentId>,
    ) -> anyhow::Result<Init> {
        let mut created_index = None;
        let (index, workspace) = if let Some(id) = index_doc {
            let id = DocumentId::parse_any(id)?;
            let index = repo.find(id).await?;
            wait_ready(&index, REMOTE_DOC_TIMEOUT)
                .await
                .map_err(|e| anyhow::anyhow!("index document: {e}"))?;
            index
                .change(|tx| {
                    schema::migrate(tx).map_err(|e| automerge_repo::Error::Change(e.to_string()))
                })
                .await
                .ok();
            let view = index.read(schema::read_index).await?;
            let workspace_id = view
                .workspace
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("index document lists no workspace"))?;
            let workspace = repo.find(DocumentId::parse_any(workspace_id)?).await?;
            // Ask for every listed document so a fresh device fetches them all.
            for doc in view.entries.values() {
                if let Ok(id) = DocumentId::parse_any(doc) {
                    let _ = repo.find(id).await;
                }
            }
            wait_ready(&workspace, REMOTE_DOC_TIMEOUT)
                .await
                .map_err(|e| anyhow::anyhow!("workspace document: {e}"))?;
            (index, workspace)
        } else {
            let workspace = repo
                .create_with(move |tx| {
                    schema::init_workspace(tx, &user)
                        .map_err(|e| automerge_repo::Error::Change(e.to_string()))
                })
                .await?;
            let workspace_id = workspace.id().to_bs58check();
            let index = repo
                .create_with(move |tx| {
                    schema::init_index(tx, &workspace_id)
                        .map_err(|e| automerge_repo::Error::Change(e.to_string()))
                })
                .await?;
            repo.flush().await?;
            created_index = Some(index.id().to_bs58check());
            info!(index = %index.id().to_bs58check(), "created index and workspace documents");
            (index, workspace)
        };
        let index_view = index.read(schema::read_index).await?;
        let workspace_view = workspace.read(schema::read_workspace).await?;
        watch_remote(&index, remote.clone());
        watch_remote(&workspace, remote.clone());
        let mut engine = Self {
            repo,
            index,
            workspace,
            entries: BTreeMap::new(),
            loaded: BTreeSet::new(),
            index_view,
            view: View {
                workspace: workspace_view,
                entries: BTreeMap::new(),
            },
            bus,
            hooks,
            remote,
        };
        let year = Utc::now().year();
        for year in [year - 1, year] {
            if engine.index_view.entries.contains_key(&year)
                && let Err(error) = engine.load_year(year).await
            {
                warn!(year, %error, "entries document not available yet");
            }
        }
        Ok(Init {
            engine,
            created_index,
        })
    }

    #[must_use]
    pub fn index_id(&self) -> DocumentId {
        self.index.id()
    }

    // ---------- documents ----------

    async fn load_year(&mut self, year: i32) -> RpcResult<()> {
        if self.loaded.contains(&year) {
            return Ok(());
        }
        let Some(id) = self.index_view.entries.get(&year) else {
            return Ok(());
        };
        let id = DocumentId::parse_any(id).map_err(doc_error)?;
        let handle = self.repo.find(id).await.map_err(doc_error)?;
        wait_ready(&handle, REMOTE_DOC_TIMEOUT).await?;
        let entries = handle.read(schema::read_entries).await.map_err(doc_error)?;
        // Loading is not a change: entries join the view without events.
        self.view.entries.extend(entries);
        watch_remote(&handle, self.remote.clone());
        self.entries.insert(year, handle);
        self.loaded.insert(year);
        Ok(())
    }

    /// Loads every entries document overlapping `range` (all with `None`).
    pub async fn ensure_range(&mut self, range: Option<Range>) -> RpcResult<()> {
        let years: Vec<i32> = self
            .index_view
            .entries
            .keys()
            .copied()
            .filter(|year| {
                range.is_none_or(|range| {
                    range.from == DateTime::<Utc>::MIN_UTC
                        || (*year >= range.from.year() - 1 && *year <= range.to.year())
                })
            })
            .collect();
        for year in years {
            self.load_year(year).await?;
        }
        Ok(())
    }

    async fn entries_doc(&mut self, year: i32) -> RpcResult<DocHandle> {
        if let Some(handle) = self.entries.get(&year) {
            return Ok(handle.clone());
        }
        if self.index_view.entries.contains_key(&year) {
            self.load_year(year).await?;
            return Ok(self.entries[&year].clone());
        }
        let handle = self
            .repo
            .create_with(move |tx| {
                schema::init_entries(tx, year)
                    .map_err(|e| automerge_repo::Error::Change(e.to_string()))
            })
            .await
            .map_err(doc_error)?;
        let id = handle.id().to_bs58check();
        let stored = id.clone();
        change(&self.index, move |tx| {
            schema::index_set_year(tx, year, &stored).map_err(CoreError::from)
        })
        .await?;
        self.index_view.entries.insert(year, id);
        watch_remote(&handle, self.remote.clone());
        self.entries.insert(year, handle.clone());
        self.loaded.insert(year);
        Ok(handle)
    }

    async fn put_entry(&mut self, entry: Entry) -> RpcResult<()> {
        ops::validate_entry(&entry)?;
        let year = ops::entry_year(entry.start);
        if let Some(previous) = self.view.entries.get(&entry.id) {
            let old_year = ops::entry_year(previous.start);
            if old_year != year {
                let old = self.entries_doc(old_year).await?;
                let id = entry.id;
                change(&old, move |tx| {
                    schema::delete_item(tx, "entries", id).map_err(CoreError::from)
                })
                .await?;
            }
        }
        let doc = self.entries_doc(year).await?;
        change(&doc, move |tx| {
            schema::write_entry(tx, &entry).map_err(CoreError::from)
        })
        .await
    }

    async fn delete_entry(&mut self, entry: &Entry) -> RpcResult<()> {
        let doc = self.entries_doc(ops::entry_year(entry.start)).await?;
        let id = entry.id;
        change(&doc, move |tx| {
            schema::delete_item(tx, "entries", id).map_err(CoreError::from)
        })
        .await
    }

    async fn read_view(&self) -> RpcResult<View> {
        let workspace = self
            .workspace
            .read(schema::read_workspace)
            .await
            .map_err(doc_error)?;
        let mut entries = BTreeMap::new();
        for handle in self.entries.values() {
            entries.extend(handle.read(schema::read_entries).await.map_err(doc_error)?);
        }
        Ok(View { workspace, entries })
    }

    /// Re-reads every loaded document, emits events for the difference, and
    /// hands them to hooks and watchers.
    pub async fn refresh(&mut self, origin: Origin) -> RpcResult<usize> {
        let next = self.read_view().await?;
        let changes = events::diff(&self.view, &next, origin, Utc::now());
        self.view = next;
        for event in &changes {
            let published = self.bus.publish(event.clone());
            self.hooks.dispatch(&published);
        }
        Ok(changes.len())
    }

    /// Handles changes that arrived through sync.
    pub async fn on_remote(&mut self, docs: &BTreeSet<DocumentId>) -> RpcResult<()> {
        if docs.contains(&self.index.id()) {
            self.index_view = self
                .index
                .read(schema::read_index)
                .await
                .map_err(doc_error)?;
            let year = Utc::now().year();
            let wanted: Vec<i32> = self
                .index_view
                .entries
                .keys()
                .copied()
                .filter(|y| *y >= year - 1 && !self.loaded.contains(y))
                .collect();
            for year in wanted {
                if let Err(error) = self.load_year(year).await {
                    warn!(year, %error, "remote entries document not loaded");
                }
            }
        }
        self.refresh(Origin::Remote).await?;
        if !ops::seq_collisions(&self.view.workspace).is_empty() {
            let now = Utc::now();
            let renumbered = change(&self.workspace, move |tx| ops::repair_seqs(tx, now)).await?;
            info!(?renumbered, "repaired task seq collisions after sync");
            self.refresh(Origin::Local).await?;
        }
        Ok(())
    }

    // ---------- lookups ----------

    fn task(&self, key: &str) -> RpcResult<Task> {
        Ok(self.view.resolve_task(key)?.clone())
    }

    fn entry(&self, key: &str) -> RpcResult<Entry> {
        Ok(self.view.resolve_entry(key)?.clone())
    }

    fn task_json(&self, task: &Task) -> Value {
        json!(self.view.task_view(task))
    }

    fn entries_in(&self, range: &Range, now: DateTime<Utc>) -> Vec<Entry> {
        let mut entries: Vec<Entry> = self
            .view
            .entries
            .values()
            .filter(|entry| range.overlaps(entry.start, entry.end.unwrap_or(now)))
            .cloned()
            .collect();
        entries.sort_by_key(|entry| (entry.start, entry.id));
        entries
    }

    // ---------- commands ----------

    pub async fn status(&self) -> Value {
        let now = Utc::now();
        json!({
            "index_doc": self.index.id().to_bs58check(),
            "workspace_doc": self.workspace.id().to_bs58check(),
            "entries_docs": self.index_view.entries,
            "loaded_years": self.loaded,
            "tasks": self.view.workspace.tasks.len(),
            "event_seq": self.bus.last_seq(),
            "running": self.view.running(now),
            "peers": self.repo.connected_peers().iter().map(ToString::to_string).collect::<Vec<_>>(),
            "sync": self.repo.peer_sync_progress().values().map(|progress| json!({
                "peer": progress.peer.to_string(),
                "state": match progress.state {
                    automerge_repo::PeerSyncState::Connected => "connected",
                    automerge_repo::PeerSyncState::Syncing => "syncing",
                    automerge_repo::PeerSyncState::Synced => "synced",
                },
                "documents": progress.documents,
                "syncing": progress.syncing_documents.iter().map(|d| d.to_bs58check()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    pub async fn dispatch(&mut self, method: &str, p: Value) -> RpcResult<Value> {
        match method {
            "task.add" => self.task_add(p).await,
            "task.list" => self.task_list(p),
            "task.show" => self.task_show(p).await,
            "task.modify" => self.task_modify(p).await,
            "task.done" => {
                #[derive(Deserialize)]
                struct P {
                    task: String,
                }
                let P { task } = params(p)?;
                self.modify_task_key(
                    &task,
                    ops::TaskPatch {
                        state: Some(TaskState::Done),
                        ..ops::TaskPatch::default()
                    },
                )
                .await
            }
            "task.remove" => self.task_remove(p).await,
            "task.find" => self.task_find(p),
            "project.add" | "tag.add" => self.named_add(method, p).await,
            "project.list" => Ok(json!(self.sorted_projects())),
            "tag.list" => Ok(json!(self.sorted_tags())),
            "project.modify" | "tag.modify" => self.named_modify(method, p).await,
            "project.remove" | "tag.remove" => self.named_remove(method, p).await,
            "entry.start" => self.entry_start(p).await,
            "entry.stop" => self.entry_stop(p).await,
            "entry.list" => self.entry_list(p).await,
            "entry.modify" => self.entry_modify(p).await,
            "entry.move" => {
                #[derive(Deserialize)]
                struct P {
                    entry: String,
                    task: String,
                }
                let P { entry, task } = params(p)?;
                self.entry_modify(json!({"entry": entry, "task": task}))
                    .await
            }
            "entry.split" => self.entry_split(p).await,
            "entry.remove" => self.entry_remove(p).await,
            "entry.table" => self.entry_table(p).await,
            "entry.apply_table" => self.entry_apply_table(p).await,
            "report" => self.report(p).await,
            "export" => self.export(p).await,
            "import" => self.import(p).await,
            _ => Err(RpcError::new(
                crate::rpc::METHOD_NOT_FOUND,
                format!("unknown method {method}"),
            )),
        }
    }

    async fn task_add(&mut self, p: Value) -> RpcResult<Value> {
        let input: ops::NewTask = params(p)?;
        let now = Utc::now();
        let task = change(&self.workspace, move |tx| ops::add_task(tx, &input, now)).await?;
        self.refresh(Origin::Local).await?;
        Ok(self.task_json(&task))
    }

    fn task_list(&self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct P {
            state: Option<String>,
            tag: Option<String>,
            project: Option<String>,
        }
        let p: P = params(p)?;
        let state = match p.state.as_deref() {
            None => Some(TaskState::Open),
            Some("all") => None,
            Some(state) => Some(state.parse::<TaskState>()?),
        };
        let tag = p
            .tag
            .as_deref()
            .map(|t| self.view.resolve_tag(t))
            .transpose()?;
        let project = p
            .project
            .as_deref()
            .map(|t| self.view.resolve_project(t))
            .transpose()?;
        let mut tasks: Vec<&Task> = self
            .view
            .workspace
            .tasks
            .values()
            .filter(|task| state.is_none_or(|state| task.state == state))
            .filter(|task| tag.is_none_or(|tag| task.tags.contains(&tag.id)))
            .filter(|task| project.is_none_or(|project| task.project == Some(project.id)))
            .collect();
        tasks.sort_by_key(|task| task.seq);
        Ok(json!(
            tasks
                .into_iter()
                .map(|task| self.view.task_view(task))
                .collect::<Vec<_>>()
        ))
    }

    async fn task_show(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            task: String,
        }
        let P { task } = params(p)?;
        let task = self.task(&task)?;
        self.ensure_range(None).await?;
        let now = Utc::now();
        let mut entries: Vec<_> = self
            .view
            .entries
            .values()
            .filter(|entry| entry.task == task.id)
            .map(|entry| self.view.entry_view(entry, now))
            .collect();
        entries.sort_by_key(|entry| entry.start);
        let total: i64 = entries.iter().map(|entry| entry.duration).sum();
        let mut value = self.task_json(&task);
        value["entries"] = json!(entries);
        value["total"] = json!(total);
        Ok(value)
    }

    async fn modify_task_key(&mut self, key: &str, patch: ops::TaskPatch) -> RpcResult<Value> {
        let id = self.task(key)?.id;
        let now = Utc::now();
        let task = change(&self.workspace, move |tx| {
            ops::modify_task(tx, id, &patch, now)
        })
        .await?;
        self.refresh(Origin::Local).await?;
        Ok(self.task_json(&task))
    }

    async fn task_modify(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            task: String,
            patch: ops::TaskPatch,
        }
        let P { task, patch } = params(p)?;
        self.modify_task_key(&task, patch).await
    }

    async fn task_remove(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            task: String,
            #[serde(default)]
            force: bool,
        }
        let P { task, force } = params(p)?;
        let task = self.task(&task)?;
        self.ensure_range(None).await?;
        let entries: Vec<Entry> = self
            .view
            .entries
            .values()
            .filter(|entry| entry.task == task.id)
            .cloned()
            .collect();
        if !entries.is_empty() && !force {
            return Err(RpcError::new(
                CONFLICT,
                format!(
                    "task #{} has {} time entries; mark it done, or remove with --force (deletes them too)",
                    task.seq,
                    entries.len()
                ),
            ));
        }
        for entry in &entries {
            self.delete_entry(entry).await?;
        }
        let id = task.id;
        let removed = change(&self.workspace, move |tx| ops::delete_task(tx, id)).await?;
        let value = self.task_json(&removed);
        self.refresh(Origin::Local).await?;
        Ok(value)
    }

    fn task_find(&self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            query: String,
            #[serde(default)]
            state: Option<String>,
            #[serde(default)]
            limit: Option<usize>,
        }
        let p: P = params(p)?;
        let state = match p.state.as_deref() {
            None | Some("all") => None,
            Some(state) => Some(state.parse::<TaskState>()?),
        };
        let hits = search::search(&self.view, &p.query, |task| {
            state.is_none_or(|state| task.state == state)
        });
        Ok(json!(
            hits.into_iter()
                .take(p.limit.unwrap_or(50))
                .map(|hit| {
                    let mut value = self.task_json(hit.task);
                    value["score"] = json!(hit.score);
                    value
                })
                .collect::<Vec<_>>()
        ))
    }

    fn sorted_projects(&self) -> Vec<tt_core::Project> {
        let mut projects: Vec<_> = self.view.workspace.projects.values().cloned().collect();
        projects.sort_by_key(|project| project.name.to_lowercase());
        projects
    }

    fn sorted_tags(&self) -> Vec<tt_core::Tag> {
        let mut tags: Vec<_> = self.view.workspace.tags.values().cloned().collect();
        tags.sort_by_key(|tag| tag.name.to_lowercase());
        tags
    }

    async fn named_add(&mut self, method: &str, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            name: String,
            #[serde(default)]
            color: Option<String>,
        }
        let P { name, color } = params(p)?;
        let now = Utc::now();
        let value = if method == "project.add" {
            json!(
                change(&self.workspace, move |tx| ops::add_project(
                    tx, &name, color, now
                ))
                .await?
            )
        } else {
            json!(
                change(&self.workspace, move |tx| ops::add_tag(
                    tx, &name, color, now
                ))
                .await?
            )
        };
        self.refresh(Origin::Local).await?;
        Ok(value)
    }

    async fn named_modify(&mut self, method: &str, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            key: String,
            patch: ops::NamedPatch,
        }
        let P { key, patch } = params(p)?;
        let now = Utc::now();
        let value = if method == "project.modify" {
            let id = self.view.resolve_project(&key)?.id;
            json!(
                change(&self.workspace, move |tx| ops::modify_project(
                    tx, id, &patch, now
                ))
                .await?
            )
        } else {
            let id = self.view.resolve_tag(&key)?.id;
            json!(
                change(&self.workspace, move |tx| ops::modify_tag(
                    tx, id, &patch, now
                ))
                .await?
            )
        };
        self.refresh(Origin::Local).await?;
        Ok(value)
    }

    async fn named_remove(&mut self, method: &str, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            key: String,
        }
        let P { key } = params(p)?;
        let now = Utc::now();
        let value = if method == "project.remove" {
            let id = self.view.resolve_project(&key)?.id;
            json!(change(&self.workspace, move |tx| ops::delete_project(tx, id, now)).await?)
        } else {
            let id = self.view.resolve_tag(&key)?.id;
            json!(change(&self.workspace, move |tx| ops::delete_tag(tx, id, now)).await?)
        };
        self.refresh(Origin::Local).await?;
        Ok(value)
    }

    /// Resolves a task by id/seq, else by fuzzy search over open tasks.
    fn task_for_start(&self, key: &str) -> RpcResult<(Task, &'static str)> {
        match self.view.resolve_task(key) {
            Ok(task) => return Ok((task.clone(), "id")),
            Err(CoreError::Conflict(message)) => return Err(RpcError::new(CONFLICT, message)),
            Err(_) => {}
        }
        let hits = search::search(&self.view, key, |task| task.state == TaskState::Open);
        hits.first()
            .map(|hit| (hit.task.clone(), "search"))
            .ok_or_else(|| CoreError::NotFound(format!("no open task matches {key:?}")).into())
    }

    async fn entry_start(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            task: String,
            #[serde(default)]
            at: Option<DateTime<Utc>>,
            #[serde(default)]
            note: Option<String>,
        }
        let p: P = params(p)?;
        let (task, matched) = self.task_for_start(&p.task)?;
        let now = Utc::now();
        let entry = ops::new_entry(task.id, p.at.unwrap_or(now), None, p.note, now);
        self.put_entry(entry.clone()).await?;
        self.refresh(Origin::Local).await?;
        Ok(json!({"entry": self.view.entry_view(&entry, now), "matched_by": matched}))
    }

    async fn entry_stop(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct P {
            target: Option<String>,
            all: bool,
            at: Option<DateTime<Utc>>,
        }
        let p: P = params(p)?;
        let now = Utc::now();
        let running: Vec<Entry> = self
            .view
            .entries
            .values()
            .filter(|entry| entry.is_running())
            .cloned()
            .collect();
        let targets: Vec<Entry> = match (&p.target, p.all) {
            (_, true) => running.clone(),
            (Some(key), false) => {
                if let Ok(entry) = self.view.resolve_entry(key) {
                    if !entry.is_running() {
                        return Err(RpcError::new(
                            CONFLICT,
                            format!("entry {key} is not running"),
                        ));
                    }
                    vec![entry.clone()]
                } else {
                    let task = self.task(key)?;
                    let of_task: Vec<Entry> = running
                        .iter()
                        .filter(|e| e.task == task.id)
                        .cloned()
                        .collect();
                    if of_task.is_empty() {
                        return Err(RpcError::new(
                            CONFLICT,
                            format!("task #{} has no running entry", task.seq),
                        ));
                    }
                    of_task
                }
            }
            (None, false) => {
                if running.len() > 1 {
                    let views: Vec<_> = running
                        .iter()
                        .map(|e| self.view.entry_view(e, now))
                        .collect();
                    return Err(RpcError::new(
                        CONFLICT,
                        format!(
                            "{} entries are running; pass an entry or task id, or --all",
                            running.len()
                        ),
                    )
                    .with_data(json!({"running": views})));
                }
                running.clone()
            }
        };
        if targets.is_empty() {
            return Err(RpcError::new(CONFLICT, "nothing is running"));
        }
        let at = p.at.unwrap_or(now);
        let mut stopped = Vec::new();
        for entry in &targets {
            let next = ops::patch_entry(
                entry,
                &ops::EntryPatch {
                    end: Some(Some(at)),
                    ..ops::EntryPatch::default()
                },
                now,
            )?;
            stopped.push(next);
        }
        for entry in &stopped {
            self.put_entry(entry.clone()).await?;
        }
        self.refresh(Origin::Local).await?;
        Ok(json!(
            stopped
                .iter()
                .map(|e| self.view.entry_view(e, now))
                .collect::<Vec<_>>()
        ))
    }

    async fn entry_list(&mut self, p: Value) -> RpcResult<Value> {
        let range: Range = params(p)?;
        self.ensure_range(Some(range)).await?;
        let now = Utc::now();
        Ok(json!(
            self.entries_in(&range, now)
                .iter()
                .map(|entry| {
                    let mut value = json!(self.view.entry_view(entry, now));
                    value["short_id"] = json!(self.view.short_entry_id(entry.id));
                    value
                })
                .collect::<Vec<_>>()
        ))
    }

    async fn entry_modify(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            entry: String,
            #[serde(default)]
            start: Option<DateTime<Utc>>,
            /// RFC 3339 time, or `"running"` to reopen.
            #[serde(default)]
            end: Option<String>,
            #[serde(default)]
            task: Option<String>,
            /// Empty string clears the note.
            #[serde(default)]
            note: Option<String>,
        }
        let p: P = params(p)?;
        self.ensure_range(None).await?;
        let entry = self.entry(&p.entry)?;
        let end = match p.end.as_deref() {
            None => None,
            Some("running") => Some(None),
            Some(text) => Some(Some(
                DateTime::parse_from_rfc3339(text)
                    .map_err(|e| RpcError::params(format!("end: {e}")))?
                    .with_timezone(&Utc),
            )),
        };
        let task = p.task.as_deref().map(|key| self.task(key)).transpose()?;
        let patch = ops::EntryPatch {
            start: p.start,
            end,
            task: task.map(|t| t.id),
            note: p.note.map(|note| Some(note).filter(|n| !n.is_empty())),
        };
        let now = Utc::now();
        let next = ops::patch_entry(&entry, &patch, now)?;
        self.put_entry(next.clone()).await?;
        self.refresh(Origin::Local).await?;
        Ok(json!(self.view.entry_view(&next, now)))
    }

    async fn entry_split(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            entry: String,
            at: DateTime<Utc>,
        }
        let p: P = params(p)?;
        self.ensure_range(None).await?;
        let entry = self.entry(&p.entry)?;
        let now = Utc::now();
        let (first, second) = ops::split_entry(&entry, p.at, now)?;
        self.put_entry(first.clone()).await?;
        self.put_entry(second.clone()).await?;
        self.refresh(Origin::Local).await?;
        Ok(json!([
            self.view.entry_view(&first, now),
            self.view.entry_view(&second, now)
        ]))
    }

    async fn entry_remove(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            entry: String,
        }
        let p: P = params(p)?;
        self.ensure_range(None).await?;
        let entry = self.entry(&p.entry)?;
        let now = Utc::now();
        let value = json!(self.view.entry_view(&entry, now));
        self.delete_entry(&entry).await?;
        self.refresh(Origin::Local).await?;
        Ok(value)
    }

    async fn entry_table(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            #[serde(flatten)]
            range: Range,
            #[serde(default)]
            tz: Option<String>,
        }
        let p: P = params(p)?;
        self.ensure_range(Some(p.range)).await?;
        let entries = self.entries_in(&p.range, Utc::now());
        let refs: Vec<&Entry> = entries.iter().collect();
        let text = text::render_table(&self.view, &refs, tz_of(p.tz.as_deref()));
        Ok(json!({"text": text, "ids": entries.iter().map(|e| e.id).collect::<Vec<_>>()}))
    }

    async fn entry_apply_table(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            text: String,
            ids: Vec<Uuid>,
            #[serde(default)]
            tz: Option<String>,
        }
        let p: P = params(p)?;
        self.ensure_range(None).await?;
        let originals: Vec<Entry> = p
            .ids
            .iter()
            .filter_map(|id| self.view.entries.get(id).cloned())
            .collect();
        let refs: Vec<&Entry> = originals.iter().collect();
        let rows = text::parse_table(&p.text, tz_of(p.tz.as_deref()))?;
        let changes = text::diff_table(&self.view, &refs, &rows)?;
        let now = Utc::now();
        let mut planned = Vec::new();
        for (id, patch) in &changes.update {
            let entry = &self.view.entries[id];
            planned.push(ops::patch_entry(entry, patch, now)?);
        }
        for create in &changes.create {
            let entry = ops::new_entry(
                create.task,
                create.start,
                create.end,
                create.note.clone(),
                now,
            );
            ops::validate_entry(&entry)?;
            planned.push(entry);
        }
        for entry in planned {
            self.put_entry(entry).await?;
        }
        for id in &changes.delete {
            let entry = self.view.entries[id].clone();
            self.delete_entry(&entry).await?;
        }
        self.refresh(Origin::Local).await?;
        Ok(json!({
            "created": changes.create.len(),
            "updated": changes.update.len(),
            "deleted": changes.delete.len(),
        }))
    }

    async fn report(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            #[serde(flatten)]
            range: Range,
            #[serde(default)]
            by: Option<String>,
            #[serde(default)]
            tz: Option<String>,
        }
        let p: P = params(p)?;
        let by =
            p.by.as_deref()
                .unwrap_or("task")
                .parse::<report::GroupBy>()?;
        self.ensure_range(Some(p.range)).await?;
        Ok(json!(report::report(
            &self.view,
            p.range,
            by,
            tz_of(p.tz.as_deref()),
            Utc::now()
        )))
    }

    async fn export(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct Rounding {
            grid_seconds: i64,
            #[serde(default)]
            mode: Option<String>,
            #[serde(default)]
            group: Option<String>,
        }
        #[derive(Deserialize)]
        struct P {
            #[serde(default)]
            range: Option<Range>,
            #[serde(default)]
            format: Option<String>,
            #[serde(default)]
            round: Option<Rounding>,
            #[serde(default)]
            tz: Option<String>,
        }
        let p: P = params(p)?;
        let tz = tz_of(p.tz.as_deref());
        let format = p.format.unwrap_or_else(|| "json".into());
        self.ensure_range(p.range).await?;
        let now = Utc::now();
        let range = p.range.unwrap_or(Range {
            from: DateTime::<Utc>::MIN_UTC,
            to: DateTime::<Utc>::MAX_UTC,
        });
        if let Some(rounding) = p.round {
            if rounding.grid_seconds <= 0 {
                return Err(RpcError::params("rounding grid must be positive"));
            }
            let mode = rounding
                .mode
                .as_deref()
                .unwrap_or("up")
                .parse::<round::RoundMode>()?;
            let group = rounding
                .group
                .as_deref()
                .unwrap_or("entry")
                .parse::<round::RoundGroup>()?;
            let items: Vec<round::Interval> = self
                .entries_in(&range, now)
                .iter()
                .filter_map(|entry| {
                    report::clip(entry, &range, now).map(|(start, end)| round::Interval {
                        task: entry.task,
                        start,
                        end,
                        entries: vec![entry.id],
                    })
                })
                .collect();
            let rounded = round::round_and_flatten(
                &items,
                chrono::Duration::seconds(rounding.grid_seconds),
                mode,
                group,
                tz,
            );
            return Ok(match format.as_str() {
                "csv" => {
                    json!({"format": "csv", "text": export::rounded_csv(&self.view, &rounded, tz)?})
                }
                "json" => json!({"format": "json", "data": {
                    "format": "tt-rounded",
                    "version": 1,
                    "grid_seconds": rounding.grid_seconds,
                    "mode": mode,
                    "group": group,
                    "range": p.range,
                    "items": rounded.iter().map(|item| json!({
                        "task": self.view.task_view_by_id(item.task),
                        "task_id": item.task,
                        "start": item.start,
                        "end": item.end,
                        "duration": item.duration,
                        "original": item.original,
                        "entries": item.entries,
                        "day": item.day,
                    })).collect::<Vec<_>>(),
                    "total": rounded.iter().map(|item| item.duration).sum::<i64>(),
                }}),
                other => {
                    return Err(RpcError::params(format!(
                        "unknown format {other:?} (json, csv)"
                    )));
                }
            });
        }
        match format.as_str() {
            "json" => {
                Ok(json!({"format": "json", "data": export::export(&self.view, p.range, now)}))
            }
            "csv" => {
                let entries = self.entries_in(&range, now);
                Ok(
                    json!({"format": "csv", "text": export::entries_csv(&self.view, &entries, now)?}),
                )
            }
            other => Err(RpcError::params(format!(
                "unknown format {other:?} (json, csv)"
            ))),
        }
    }

    async fn import(&mut self, p: Value) -> RpcResult<Value> {
        #[derive(Deserialize)]
        struct P {
            data: Value,
        }
        let p: P = params(p)?;
        let data = export::parse_export(&p.data.to_string())?;
        self.ensure_range(None).await?;
        let plan = export::plan_import(&self.view, &data);
        let workspace_plan = plan.clone();
        change(&self.workspace, move |tx| {
            for project in &workspace_plan.projects {
                schema::write_project(tx, project)?;
            }
            for tag in &workspace_plan.tags {
                schema::write_tag(tx, tag)?;
            }
            let mut max = schema::read_workspace(tx).task_seq;
            for task in &workspace_plan.tasks {
                schema::write_task(tx, task)?;
                max = max.max(task.seq);
            }
            schema::set_task_seq_counter(tx, max)?;
            Ok(())
        })
        .await?;
        for entry in &plan.entries {
            self.put_entry(entry.clone()).await?;
        }
        self.refresh(Origin::Local).await?;
        Ok(json!({
            "projects": plan.projects.len(),
            "tags": plan.tags.len(),
            "tasks": plan.tasks.len(),
            "entries": plan.entries.len(),
            "unchanged": plan.unchanged,
            "renumbered": plan.renumbered,
        }))
    }

    /// Snapshot of running entries for a watcher.
    #[must_use]
    pub fn snapshot(&self, filter: &EventFilter) -> Value {
        let running = self.view.running(Utc::now());
        bus::snapshot(self.bus.last_seq(), &filter.running(&running))
    }
}
