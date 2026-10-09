//! Domain events derived by diffing two views.
//!
//! Local writes and changes arriving through sync go through the same diff,
//! so both produce identical event shapes; only `origin` differs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{EntryView, Project, Tag, Task, TaskView, Timestamp, View};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Local,
    Remote,
}

impl Origin {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

/// Event type names, in the order a diff emits them.
pub const EVENT_TYPES: &[&str] = &[
    "project.created",
    "project.updated",
    "project.deleted",
    "tag.created",
    "tag.updated",
    "tag.deleted",
    "task.created",
    "task.updated",
    "task.renumbered",
    "task.deleted",
    "entry.started",
    "entry.created",
    "entry.stopped",
    "entry.moved",
    "entry.updated",
    "entry.deleted",
];

/// One domain event without its bus sequence number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub origin: Origin,
    pub at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryView>,
    /// The task the record belongs to (the entry's task, or the task itself).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskView>,
    /// For `entry.moved`: the task the entry belonged to before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_task: Option<TaskView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<Project>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Tag>,
    /// For `task.renumbered`: the old short id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<u64>,
    /// For `task.renumbered`: the new short id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<u64>,
    /// Every running entry after the change.
    pub running: Vec<EntryView>,
}

impl DomainEvent {
    /// Serializes with a bus sequence number as the first field.
    #[must_use]
    pub fn to_json(&self, seq: u64) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Value::Object(map) = &mut value {
            let mut ordered = serde_json::Map::new();
            ordered.insert("seq".into(), seq.into());
            ordered.extend(std::mem::take(map));
            *map = ordered;
        }
        value
    }

    /// Task id the event concerns, for filtering.
    #[must_use]
    pub fn task_view(&self) -> Option<&TaskView> {
        self.task.as_ref()
    }
}

/// Typed events for everything that differs between `prev` and `next`.
#[must_use]
pub fn diff(prev: &View, next: &View, origin: Origin, now: Timestamp) -> Vec<DomainEvent> {
    let running = next.running(now);
    let base = |kind: &str| DomainEvent {
        kind: kind.to_owned(),
        origin,
        at: now,
        entry: None,
        task: None,
        previous_task: None,
        project: None,
        tag: None,
        from: None,
        to: None,
        running: running.clone(),
    };
    let mut events = Vec::new();
    let (pw, nw) = (&prev.workspace, &next.workspace);

    for (id, project) in &nw.projects {
        match pw.projects.get(id) {
            None => events.push(DomainEvent {
                project: Some(project.clone()),
                ..base("project.created")
            }),
            Some(old) if old != project => events.push(DomainEvent {
                project: Some(project.clone()),
                ..base("project.updated")
            }),
            _ => {}
        }
    }
    for (id, project) in &pw.projects {
        if !nw.projects.contains_key(id) {
            events.push(DomainEvent {
                project: Some(project.clone()),
                ..base("project.deleted")
            });
        }
    }
    for (id, tag) in &nw.tags {
        match pw.tags.get(id) {
            None => events.push(DomainEvent {
                tag: Some(tag.clone()),
                ..base("tag.created")
            }),
            Some(old) if old != tag => events.push(DomainEvent {
                tag: Some(tag.clone()),
                ..base("tag.updated")
            }),
            _ => {}
        }
    }
    for (id, tag) in &pw.tags {
        if !nw.tags.contains_key(id) {
            events.push(DomainEvent {
                tag: Some(tag.clone()),
                ..base("tag.deleted")
            });
        }
    }
    for (id, task) in &nw.tasks {
        match pw.tasks.get(id) {
            None => events.push(DomainEvent {
                task: Some(next.task_view(task)),
                ..base("task.created")
            }),
            Some(old) if old != task => {
                if renumbered(old, task) {
                    events.push(DomainEvent {
                        task: Some(next.task_view(task)),
                        from: Some(old.seq),
                        to: Some(task.seq),
                        ..base("task.renumbered")
                    });
                    let unchanged = Task {
                        seq: old.seq,
                        previous_seqs: old.previous_seqs.clone(),
                        updated: old.updated,
                        ..task.clone()
                    };
                    if unchanged == *old {
                        continue;
                    }
                }
                events.push(DomainEvent {
                    task: Some(next.task_view(task)),
                    ..base("task.updated")
                });
            }
            _ => {}
        }
    }
    for (id, task) in &pw.tasks {
        if !nw.tasks.contains_key(id) {
            events.push(DomainEvent {
                task: Some(prev.task_view(task)),
                ..base("task.deleted")
            });
        }
    }
    for (id, entry) in &next.entries {
        let view = next.entry_view(entry, now);
        let task = view.task.clone();
        let kind = match prev.entries.get(id) {
            None if entry.is_running() => "entry.started",
            None => "entry.created",
            Some(old) if old == entry => continue,
            Some(old) if old.task != entry.task => {
                events.push(DomainEvent {
                    entry: Some(view),
                    task,
                    previous_task: prev
                        .task_view_by_id(old.task)
                        .or_else(|| next.task_view_by_id(old.task)),
                    ..base("entry.moved")
                });
                continue;
            }
            Some(old) if old.end.is_none() && entry.end.is_some() => "entry.stopped",
            Some(old) if old.end.is_some() && entry.end.is_none() => "entry.started",
            Some(_) => "entry.updated",
        };
        events.push(DomainEvent {
            entry: Some(view),
            task,
            ..base(kind)
        });
    }
    for (id, entry) in &prev.entries {
        if !next.entries.contains_key(id) {
            let view = prev.entry_view(entry, now);
            events.push(DomainEvent {
                task: view.task.clone(),
                entry: Some(view),
                ..base("entry.deleted")
            });
        }
    }
    events
}

/// A seq repair moved `task` off `old.seq` (recorded in `previous_seqs`).
fn renumbered(old: &Task, task: &Task) -> bool {
    task.seq != old.seq
        && task.previous_seqs.len() > old.previous_seqs.len()
        && task.previous_seqs.last() == Some(&old.seq)
}

/// Filters for watchers. Empty filters match everything.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventFilter {
    /// Event types; `entry.*` style prefixes allowed.
    #[serde(default)]
    pub events: Vec<String>,
    /// Task seq (`12`, `#12`) or uuid.
    #[serde(default)]
    pub tasks: Vec<String>,
    /// Tag names.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Project names.
    #[serde(default)]
    pub projects: Vec<String>,
}

impl EventFilter {
    #[must_use]
    pub fn matches_kind(&self, kind: &str) -> bool {
        self.events.is_empty()
            || self
                .events
                .iter()
                .any(|pattern| match pattern.strip_suffix('*') {
                    Some(prefix) => kind.starts_with(prefix),
                    None => pattern == kind,
                })
    }

    fn matches_task(&self, task: Option<&TaskView>) -> bool {
        if !self.tasks.is_empty() {
            let Some(task) = task else { return false };
            let ok = self.tasks.iter().any(|key| {
                let key = key.trim().trim_start_matches('#');
                key.parse::<u64>().is_ok_and(|seq| seq == task.seq)
                    || task.id.to_string() == key
                    || (key.len() >= 4 && task.id.simple().to_string().starts_with(key))
            });
            if !ok {
                return false;
            }
        }
        if !self.tags.is_empty() {
            let Some(task) = task else { return false };
            if !self.tags.iter().any(|name| {
                task.tags.iter().any(|tag| {
                    tag.name
                        .eq_ignore_ascii_case(name.trim().trim_start_matches('+'))
                })
            }) {
                return false;
            }
        }
        if !self.projects.is_empty() {
            let Some(project) = task.and_then(|task| task.project.as_ref()) else {
                return false;
            };
            if !self.projects.iter().any(|name| {
                project
                    .name
                    .eq_ignore_ascii_case(name.trim().trim_start_matches('@'))
            }) {
                return false;
            }
        }
        true
    }

    #[must_use]
    pub fn matches(&self, event: &DomainEvent) -> bool {
        if !self.matches_kind(&event.kind) {
            return false;
        }
        // Project and tag events have no task; task-scoped filters exclude them.
        if event.task.is_none()
            && (!self.tasks.is_empty() || !self.tags.is_empty() || !self.projects.is_empty())
        {
            return false;
        }
        self.matches_task(event.task.as_ref()) || self.matches_task(event.previous_task.as_ref())
    }

    /// Running entries a filtered snapshot should list.
    #[must_use]
    pub fn running<'a>(&self, running: &'a [EntryView]) -> Vec<&'a EntryView> {
        running
            .iter()
            .filter(|entry| self.matches_task(entry.task.as_ref()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Entry, Task, TaskState},
        ops,
    };
    use chrono::{Duration, TimeZone, Utc};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn now() -> Timestamp {
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn task(seq: u64, tags: &[Uuid]) -> Task {
        Task {
            id: Uuid::now_v7(),
            seq,
            title: format!("task {seq}"),
            description: String::new(),
            tags: tags.iter().copied().collect(),
            project: None,
            metadata: BTreeMap::new(),
            state: TaskState::Open,
            previous_seqs: Vec::new(),
            created: now(),
            updated: now(),
        }
    }

    fn view_with(tasks: &[&Task], entries: &[&Entry]) -> View {
        let mut view = View::default();
        for task in tasks {
            view.workspace.tasks.insert(task.id, (*task).clone());
        }
        for entry in entries {
            view.entries.insert(entry.id, (*entry).clone());
        }
        view
    }

    #[test]
    fn start_stop_move_delete_and_rename() {
        let a = task(1, &[]);
        let b = task(2, &[]);
        let empty = view_with(&[&a, &b], &[]);
        let running = ops::new_entry(a.id, now(), None, None, now());
        let started = view_with(&[&a, &b], &[&running]);
        let events = diff(&empty, &started, Origin::Local, now());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "entry.started");
        assert_eq!(events[0].running.len(), 1);
        assert_eq!(events[0].task.as_ref().unwrap().seq, 1);

        let mut stopped_entry = running.clone();
        stopped_entry.end = Some(now() + Duration::minutes(5));
        let stopped = view_with(&[&a, &b], &[&stopped_entry]);
        let events = diff(&started, &stopped, Origin::Remote, now());
        assert_eq!(events[0].kind, "entry.stopped");
        assert_eq!(events[0].origin, Origin::Remote);
        assert!(events[0].running.is_empty());

        let mut moved_entry = stopped_entry.clone();
        moved_entry.task = b.id;
        let moved = view_with(&[&a, &b], &[&moved_entry]);
        let events = diff(&stopped, &moved, Origin::Local, now());
        assert_eq!(events[0].kind, "entry.moved");
        assert_eq!(events[0].previous_task.as_ref().unwrap().id, a.id);

        let mut renamed = b.clone();
        renamed.title = "renamed".into();
        let after = view_with(&[&a, &renamed], &[&moved_entry]);
        let events = diff(&moved, &after, Origin::Local, now());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "task.updated");

        let gone = view_with(&[&a, &renamed], &[]);
        let events = diff(&after, &gone, Origin::Local, now());
        assert_eq!(events[0].kind, "entry.deleted");
        assert_eq!(events[0].entry.as_ref().unwrap().id, moved_entry.id);
        let json = events[0].to_json(42);
        assert_eq!(json["seq"], 42);
        assert_eq!(json["type"], "entry.deleted");
        assert_eq!(json["origin"], "local");
    }

    #[test]
    fn seq_repair_emits_renumbered_instead_of_updated() {
        let a = task(20, &[]);
        let before = view_with(&[&a], &[]);
        let mut moved = a.clone();
        moved.seq = 31;
        moved.previous_seqs.push(20);
        moved.updated = now() + Duration::minutes(1);
        let after = view_with(&[&moved], &[]);
        let events = diff(&before, &after, Origin::Remote, now());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "task.renumbered");
        let json = events[0].to_json(1);
        assert_eq!(
            (json["from"].clone(), json["to"].clone()),
            (20.into(), 31.into())
        );
        assert_eq!(json["task"]["seq"], 31);

        let mut renamed = moved.clone();
        renamed.title = "renamed".into();
        let after = view_with(&[&renamed], &[]);
        let kinds: Vec<_> = diff(&before, &after, Origin::Local, now())
            .into_iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(kinds, ["task.renumbered", "task.updated"]);
    }

    #[test]
    fn remote_and_local_payloads_have_the_same_shape() {
        let a = task(1, &[]);
        let empty = view_with(&[&a], &[]);
        let entry = ops::new_entry(a.id, now(), None, None, now());
        let next = view_with(&[&a], &[&entry]);
        let local = diff(&empty, &next, Origin::Local, now()).remove(0);
        let remote = diff(&empty, &next, Origin::Remote, now()).remove(0);
        let mut local_json = local.to_json(1);
        let mut remote_json = remote.to_json(1);
        local_json["origin"] = Value::Null;
        remote_json["origin"] = Value::Null;
        assert_eq!(local_json, remote_json);
    }

    #[test]
    fn filters() {
        let work = Uuid::now_v7();
        let mut view = View::default();
        view.workspace.tags.insert(
            work,
            Tag {
                id: work,
                name: "work".into(),
                color: None,
                created: now(),
                updated: now(),
            },
        );
        let tagged = task(1, &[work]);
        let plain = task(2, &[]);
        view.workspace.tasks.insert(tagged.id, tagged.clone());
        view.workspace.tasks.insert(plain.id, plain.clone());
        let mut next = view.clone();
        let entry = ops::new_entry(plain.id, now(), None, None, now());
        next.entries.insert(entry.id, entry);
        let event = diff(&view, &next, Origin::Local, now()).remove(0);
        let filter = EventFilter {
            tags: vec!["work".into()],
            ..EventFilter::default()
        };
        assert!(!filter.matches(&event));
        let filter = EventFilter {
            tasks: vec!["#2".into()],
            events: vec!["entry.*".into()],
            ..EventFilter::default()
        };
        assert!(filter.matches(&event));
        let filter = EventFilter {
            events: vec!["task.*".into()],
            ..EventFilter::default()
        };
        assert!(!filter.matches(&event));
    }
}
