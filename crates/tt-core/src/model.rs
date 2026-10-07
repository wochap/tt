//! Domain records and the combined read view.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::CoreError;

pub type Timestamp = DateTime<Utc>;

#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    #[default]
    Open,
    Done,
    Archived,
}

impl TaskState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
            Self::Archived => "archived",
        }
    }
}

impl std::str::FromStr for TaskState {
    type Err = CoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "done" => Ok(Self::Done),
            "archived" => Ok(Self::Archived),
            other => Err(CoreError::Invalid(format!(
                "unknown task state {other:?} (open, done, archived)"
            ))),
        }
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub archived: bool,
    pub created: Timestamp,
    pub updated: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub created: Timestamp,
    pub updated: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub seq: u64,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: BTreeSet<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<Uuid>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub state: TaskState,
    pub created: Timestamp,
    pub updated: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: Uuid,
    pub task: Uuid,
    pub start: Timestamp,
    #[serde(default)]
    pub end: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created: Timestamp,
    pub updated: Timestamp,
}

impl Entry {
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.end.is_none()
    }
    /// Duration in seconds; running entries count up to `now`.
    #[must_use]
    pub fn duration(&self, now: Timestamp) -> i64 {
        (self.end.unwrap_or(now) - self.start).num_seconds().max(0)
    }
}

/// Everything in one user's workspace document.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub schema: i64,
    pub user: Option<User>,
    pub projects: BTreeMap<Uuid, Project>,
    pub tags: BTreeMap<Uuid, Tag>,
    pub tasks: BTreeMap<Uuid, Task>,
    pub task_seq: u64,
}

/// The index document: where the workspace and per-year entry documents live.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub schema: i64,
    /// bs58check document id of the workspace document.
    pub workspace: Option<String>,
    /// UTC year → bs58check document id of `entries-YYYY`.
    pub entries: BTreeMap<i32, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectRef {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TagRef {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A task with its project and tags resolved, as shown to users and hooks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskView {
    pub id: Uuid,
    pub seq: u64,
    pub title: String,
    pub description: String,
    pub state: TaskState,
    pub project: Option<ProjectRef>,
    pub tags: Vec<TagRef>,
    pub metadata: BTreeMap<String, String>,
    pub created: Timestamp,
    pub updated: Timestamp,
}

/// An entry with its task resolved and its duration in seconds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryView {
    pub id: Uuid,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub duration: i64,
    pub running: bool,
    pub note: Option<String>,
    pub task: Option<TaskView>,
    pub task_id: Uuid,
    pub created: Timestamp,
    pub updated: Timestamp,
}

/// Workspace plus every loaded entry, merged across year documents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub workspace: Workspace,
    pub entries: BTreeMap<Uuid, Entry>,
}

impl View {
    #[must_use]
    pub fn project_ref(&self, id: Uuid) -> Option<ProjectRef> {
        self.workspace.projects.get(&id).map(|project| ProjectRef {
            id,
            name: project.name.clone(),
            color: project.color.clone(),
        })
    }

    #[must_use]
    pub fn task_view(&self, task: &Task) -> TaskView {
        let mut tags: Vec<TagRef> = task
            .tags
            .iter()
            .filter_map(|id| self.workspace.tags.get(id))
            .map(|tag| TagRef {
                id: tag.id,
                name: tag.name.clone(),
                color: tag.color.clone(),
            })
            .collect();
        tags.sort_by(|a, b| a.name.cmp(&b.name));
        TaskView {
            id: task.id,
            seq: task.seq,
            title: task.title.clone(),
            description: task.description.clone(),
            state: task.state,
            project: task.project.and_then(|id| self.project_ref(id)),
            tags,
            metadata: task.metadata.clone(),
            created: task.created,
            updated: task.updated,
        }
    }

    #[must_use]
    pub fn task_view_by_id(&self, id: Uuid) -> Option<TaskView> {
        self.workspace
            .tasks
            .get(&id)
            .map(|task| self.task_view(task))
    }

    #[must_use]
    pub fn entry_view(&self, entry: &Entry, now: Timestamp) -> EntryView {
        EntryView {
            id: entry.id,
            start: entry.start,
            end: entry.end,
            duration: entry.duration(now),
            running: entry.is_running(),
            note: entry.note.clone(),
            task: self.task_view_by_id(entry.task),
            task_id: entry.task,
            created: entry.created,
            updated: entry.updated,
        }
    }

    /// Running entries ordered by start.
    #[must_use]
    pub fn running(&self, now: Timestamp) -> Vec<EntryView> {
        let mut running: Vec<_> = self
            .entries
            .values()
            .filter(|entry| entry.is_running())
            .map(|entry| self.entry_view(entry, now))
            .collect();
        running.sort_by_key(|entry| (entry.start, entry.id));
        running
    }

    /// Resolves `#12`, `12`, a full uuid, or a unique uuid prefix (≥ 4 hex).
    pub fn resolve_task(&self, key: &str) -> Result<&Task, CoreError> {
        let key = key.trim();
        let seq_text = key.strip_prefix('#').unwrap_or(key);
        if let Ok(seq) = seq_text.parse::<u64>()
            && let Some(task) = self.workspace.tasks.values().find(|task| task.seq == seq)
        {
            return Ok(task);
        }
        resolve_by_id(key, &self.workspace.tasks, "task")
    }

    /// Resolves a full entry uuid or a unique prefix (≥ 4 hex).
    pub fn resolve_entry(&self, key: &str) -> Result<&Entry, CoreError> {
        resolve_by_id(key.trim(), &self.entries, "entry")
    }

    #[must_use]
    pub fn tag_by_name(&self, name: &str) -> Option<&Tag> {
        let name = name.trim();
        self.workspace
            .tags
            .values()
            .find(|tag| tag.name == name)
            .or_else(|| {
                self.workspace
                    .tags
                    .values()
                    .find(|tag| tag.name.eq_ignore_ascii_case(name))
            })
    }

    #[must_use]
    pub fn project_by_name(&self, name: &str) -> Option<&Project> {
        let name = name.trim();
        self.workspace
            .projects
            .values()
            .find(|project| project.name == name)
            .or_else(|| {
                self.workspace
                    .projects
                    .values()
                    .find(|project| project.name.eq_ignore_ascii_case(name))
            })
    }

    pub fn resolve_tag(&self, key: &str) -> Result<&Tag, CoreError> {
        self.tag_by_name(key)
            .map_or_else(|| resolve_by_id(key, &self.workspace.tags, "tag"), Ok)
    }

    pub fn resolve_project(&self, key: &str) -> Result<&Project, CoreError> {
        self.project_by_name(key).map_or_else(
            || resolve_by_id(key, &self.workspace.projects, "project"),
            Ok,
        )
    }

    /// Shortest unique prefix (at least 8 chars) of an entry id among all entries.
    #[must_use]
    pub fn short_entry_id(&self, id: Uuid) -> String {
        short_id(id, self.entries.keys().copied())
    }
}

/// Shortest prefix of `id`'s simple hex form (at least 8) unique among `all`.
pub fn short_id(id: Uuid, all: impl IntoIterator<Item = Uuid>) -> String {
    let text = id.simple().to_string();
    let others: Vec<String> = all
        .into_iter()
        .filter(|other| *other != id)
        .map(|other| other.simple().to_string())
        .collect();
    for len in 8..=32 {
        let prefix = &text[..len];
        if !others.iter().any(|other| other.starts_with(prefix)) {
            return prefix.to_owned();
        }
    }
    text
}

fn resolve_by_id<'a, T>(
    key: &str,
    items: &'a BTreeMap<Uuid, T>,
    what: &str,
) -> Result<&'a T, CoreError> {
    if let Ok(id) = Uuid::parse_str(key) {
        return items
            .get(&id)
            .ok_or_else(|| CoreError::NotFound(format!("{what} {key}")));
    }
    let prefix: String = key
        .chars()
        .filter(|c| *c != '-')
        .collect::<String>()
        .to_ascii_lowercase();
    if prefix.len() < 4 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(CoreError::NotFound(format!("{what} {key}")));
    }
    let mut matches = items
        .iter()
        .filter(|(id, _)| id.simple().to_string().starts_with(&prefix));
    match (matches.next(), matches.next()) {
        (Some((_, item)), None) => Ok(item),
        (None, _) => Err(CoreError::NotFound(format!("{what} {key}"))),
        (Some(_), Some(_)) => Err(CoreError::Conflict(format!(
            "{what} id prefix {key} is ambiguous"
        ))),
    }
}
