//! Domain operations. Workspace operations run inside one Automerge
//! transaction on the workspace document; entry operations are pure functions
//! whose results the caller writes into the right per-year document.

use std::collections::{BTreeMap, BTreeSet};

use automerge::{ReadDoc, transaction::Transactable};
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    model::{Entry, Project, Tag, Task, TaskState, Timestamp, Workspace},
    schema,
};

/// The calendar year (UTC) whose entries document holds an entry starting at `start`.
#[must_use]
pub fn entry_year(start: Timestamp) -> i32 {
    start.year()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewTask {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Tag names; missing tags are created.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Project name; created when missing.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub state: Option<TaskState>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaskPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Replaces the whole tag set (names) when present.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub add_tags: Vec<String>,
    #[serde(default)]
    pub remove_tags: Vec<String>,
    /// `Some(None)` clears the project; `Some(Some(name))` sets it.
    #[serde(default)]
    pub project: Option<Option<String>>,
    /// Replaces all metadata when present.
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub set_metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub remove_metadata: Vec<String>,
    #[serde(default)]
    pub state: Option<TaskState>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NamedPatch {
    #[serde(default)]
    pub name: Option<String>,
    /// `Some(None)` clears the color.
    #[serde(default)]
    pub color: Option<Option<String>>,
    #[serde(default)]
    pub archived: Option<bool>,
}

fn next_seq(workspace: &Workspace) -> u64 {
    workspace
        .tasks
        .values()
        .map(|task| task.seq)
        .max()
        .unwrap_or(0)
        .max(workspace.task_seq)
        + 1
}

fn clean_name(name: &str, what: &str) -> CoreResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::Invalid(format!("{what} name must not be empty")));
    }
    Ok(name.to_owned())
}

fn find_tag<'a>(workspace: &'a Workspace, name: &str) -> Option<&'a Tag> {
    workspace
        .tags
        .values()
        .find(|tag| tag.name == name)
        .or_else(|| {
            workspace
                .tags
                .values()
                .find(|tag| tag.name.eq_ignore_ascii_case(name))
        })
}

fn find_project<'a>(workspace: &'a Workspace, name: &str) -> Option<&'a Project> {
    workspace
        .projects
        .values()
        .find(|project| project.name == name)
        .or_else(|| {
            workspace
                .projects
                .values()
                .find(|project| project.name.eq_ignore_ascii_case(name))
        })
}

/// Returns the tag named `name`, creating it when missing.
pub fn ensure_tag<T: Transactable + ReadDoc>(
    tx: &mut T,
    workspace: &mut Workspace,
    name: &str,
    now: Timestamp,
) -> CoreResult<Uuid> {
    let name = clean_name(name.trim_start_matches('+'), "tag")?;
    if let Some(tag) = find_tag(workspace, &name) {
        return Ok(tag.id);
    }
    let tag = Tag {
        id: Uuid::now_v7(),
        name,
        color: None,
        created: now,
        updated: now,
    };
    schema::write_tag(tx, &tag)?;
    let id = tag.id;
    workspace.tags.insert(id, tag);
    Ok(id)
}

/// Returns the project named `name`, creating it when missing.
pub fn ensure_project<T: Transactable + ReadDoc>(
    tx: &mut T,
    workspace: &mut Workspace,
    name: &str,
    now: Timestamp,
) -> CoreResult<Uuid> {
    let name = clean_name(name.trim_start_matches('@'), "project")?;
    if let Some(project) = find_project(workspace, &name) {
        return Ok(project.id);
    }
    let project = Project {
        id: Uuid::now_v7(),
        name,
        color: None,
        archived: false,
        created: now,
        updated: now,
    };
    schema::write_project(tx, &project)?;
    let id = project.id;
    workspace.projects.insert(id, project);
    Ok(id)
}

fn validate_metadata(metadata: &BTreeMap<String, String>) -> CoreResult<()> {
    for key in metadata.keys() {
        if key.trim().is_empty() {
            return Err(CoreError::Invalid("metadata keys must not be empty".into()));
        }
    }
    Ok(())
}

pub fn add_task<T: Transactable + ReadDoc>(
    tx: &mut T,
    input: &NewTask,
    now: Timestamp,
) -> CoreResult<Task> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(CoreError::Invalid("task title must not be empty".into()));
    }
    validate_metadata(&input.metadata)?;
    let mut workspace = schema::read_workspace(tx);
    let mut tags = BTreeSet::new();
    for name in &input.tags {
        tags.insert(ensure_tag(tx, &mut workspace, name, now)?);
    }
    let project = match &input.project {
        Some(name) => Some(ensure_project(tx, &mut workspace, name, now)?),
        None => None,
    };
    let seq = next_seq(&workspace);
    let task = Task {
        id: Uuid::now_v7(),
        seq,
        title: title.to_owned(),
        description: input.description.clone().unwrap_or_default(),
        tags,
        project,
        metadata: input.metadata.clone(),
        state: input.state.unwrap_or_default(),
        previous_seqs: Vec::new(),
        created: now,
        updated: now,
    };
    schema::write_task(tx, &task)?;
    schema::set_task_seq_counter(tx, seq)?;
    Ok(task)
}

pub fn modify_task<T: Transactable + ReadDoc>(
    tx: &mut T,
    id: Uuid,
    patch: &TaskPatch,
    now: Timestamp,
) -> CoreResult<Task> {
    let mut workspace = schema::read_workspace(tx);
    let mut task = workspace
        .tasks
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::NotFound(format!("task {id}")))?;
    let before = task.clone();
    if let Some(title) = &patch.title {
        let title = title.trim();
        if title.is_empty() {
            return Err(CoreError::Invalid("task title must not be empty".into()));
        }
        title.clone_into(&mut task.title);
    }
    if let Some(description) = &patch.description {
        task.description.clone_from(description);
    }
    if let Some(names) = &patch.tags {
        task.tags.clear();
        for name in names {
            task.tags.insert(ensure_tag(tx, &mut workspace, name, now)?);
        }
    }
    for name in &patch.add_tags {
        task.tags.insert(ensure_tag(tx, &mut workspace, name, now)?);
    }
    for name in &patch.remove_tags {
        if let Some(tag) = find_tag(&workspace, name.trim().trim_start_matches('+')) {
            task.tags.remove(&tag.id);
        }
    }
    match &patch.project {
        Some(Some(name)) => task.project = Some(ensure_project(tx, &mut workspace, name, now)?),
        Some(None) => task.project = None,
        None => {}
    }
    if let Some(metadata) = &patch.metadata {
        task.metadata.clone_from(metadata);
    }
    for (key, value) in &patch.set_metadata {
        task.metadata.insert(key.trim().to_owned(), value.clone());
    }
    for key in &patch.remove_metadata {
        task.metadata.remove(key.trim());
    }
    validate_metadata(&task.metadata)?;
    if let Some(state) = patch.state {
        task.state = state;
    }
    if task != before {
        task.updated = now;
        schema::write_task(tx, &task)?;
    }
    Ok(task)
}

pub fn delete_task<T: Transactable + ReadDoc>(tx: &mut T, id: Uuid) -> CoreResult<Task> {
    let workspace = schema::read_workspace(tx);
    let task = workspace
        .tasks
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::NotFound(format!("task {id}")))?;
    schema::delete_item(tx, "tasks", id)?;
    Ok(task)
}

pub fn add_project<T: Transactable + ReadDoc>(
    tx: &mut T,
    name: &str,
    color: Option<String>,
    now: Timestamp,
) -> CoreResult<Project> {
    let name = clean_name(name, "project")?;
    let mut workspace = schema::read_workspace(tx);
    if find_project(&workspace, &name).is_some_and(|project| project.name == name) {
        return Err(CoreError::Conflict(format!(
            "project {name} already exists"
        )));
    }
    let id = ensure_project(tx, &mut workspace, &name, now)?;
    let mut project = workspace.projects[&id].clone();
    if color.is_some() {
        project.color = color;
        schema::write_project(tx, &project)?;
    }
    Ok(project)
}

pub fn modify_project<T: Transactable + ReadDoc>(
    tx: &mut T,
    id: Uuid,
    patch: &NamedPatch,
    now: Timestamp,
) -> CoreResult<Project> {
    let workspace = schema::read_workspace(tx);
    let mut project = workspace
        .projects
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::NotFound(format!("project {id}")))?;
    if let Some(name) = &patch.name {
        let name = clean_name(name, "project")?;
        if workspace
            .projects
            .values()
            .any(|other| other.id != id && other.name == name)
        {
            return Err(CoreError::Conflict(format!(
                "project {name} already exists"
            )));
        }
        project.name = name;
    }
    if let Some(color) = &patch.color {
        project.color.clone_from(color);
    }
    if let Some(archived) = patch.archived {
        project.archived = archived;
    }
    project.updated = now;
    schema::write_project(tx, &project)?;
    Ok(project)
}

/// Deletes a project and detaches it from its tasks.
pub fn delete_project<T: Transactable + ReadDoc>(
    tx: &mut T,
    id: Uuid,
    now: Timestamp,
) -> CoreResult<Project> {
    let workspace = schema::read_workspace(tx);
    let project = workspace
        .projects
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::NotFound(format!("project {id}")))?;
    for task in workspace.tasks.values() {
        if task.project == Some(id) {
            let mut task = task.clone();
            task.project = None;
            task.updated = now;
            schema::write_task(tx, &task)?;
        }
    }
    schema::delete_item(tx, "projects", id)?;
    Ok(project)
}

pub fn add_tag<T: Transactable + ReadDoc>(
    tx: &mut T,
    name: &str,
    color: Option<String>,
    now: Timestamp,
) -> CoreResult<Tag> {
    let name = clean_name(name.trim_start_matches('+'), "tag")?;
    let mut workspace = schema::read_workspace(tx);
    if find_tag(&workspace, &name).is_some_and(|tag| tag.name == name) {
        return Err(CoreError::Conflict(format!("tag {name} already exists")));
    }
    let id = ensure_tag(tx, &mut workspace, &name, now)?;
    let mut tag = workspace.tags[&id].clone();
    if color.is_some() {
        tag.color = color;
        schema::write_tag(tx, &tag)?;
    }
    Ok(tag)
}

pub fn modify_tag<T: Transactable + ReadDoc>(
    tx: &mut T,
    id: Uuid,
    patch: &NamedPatch,
    now: Timestamp,
) -> CoreResult<Tag> {
    let workspace = schema::read_workspace(tx);
    let mut tag = workspace
        .tags
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::NotFound(format!("tag {id}")))?;
    if let Some(name) = &patch.name {
        let name = clean_name(name.trim_start_matches('+'), "tag")?;
        if workspace
            .tags
            .values()
            .any(|other| other.id != id && other.name == name)
        {
            return Err(CoreError::Conflict(format!("tag {name} already exists")));
        }
        tag.name = name;
    }
    if let Some(color) = &patch.color {
        tag.color.clone_from(color);
    }
    tag.updated = now;
    schema::write_tag(tx, &tag)?;
    Ok(tag)
}

/// Deletes a tag and removes it from every task.
pub fn delete_tag<T: Transactable + ReadDoc>(
    tx: &mut T,
    id: Uuid,
    now: Timestamp,
) -> CoreResult<Tag> {
    let workspace = schema::read_workspace(tx);
    let tag = workspace
        .tags
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::NotFound(format!("tag {id}")))?;
    for task in workspace.tasks.values() {
        if task.tags.contains(&id) {
            let mut task = task.clone();
            task.tags.remove(&id);
            task.updated = now;
            schema::write_task(tx, &task)?;
        }
    }
    schema::delete_item(tx, "tags", id)?;
    Ok(tag)
}

/// Tasks whose `seq` collides with an earlier-created task, with the new
/// number each should get. The earliest (by `created`, then id) keeps its seq.
#[must_use]
pub fn seq_collisions(workspace: &Workspace) -> Vec<(Uuid, u64)> {
    let mut by_seq: BTreeMap<u64, Vec<&Task>> = BTreeMap::new();
    for task in workspace.tasks.values() {
        by_seq.entry(task.seq).or_default().push(task);
    }
    let mut next = next_seq(workspace);
    let mut losers: Vec<&Task> = Vec::new();
    for tasks in by_seq.values_mut() {
        if tasks.len() > 1 {
            tasks.sort_by_key(|task| (task.created, task.id));
            losers.extend(tasks.iter().skip(1));
        }
    }
    losers.sort_by_key(|task| (task.created, task.id));
    losers
        .into_iter()
        .map(|task| {
            let seq = next;
            next += 1;
            (task.id, seq)
        })
        .collect()
}

/// Reassigns colliding `seq` values (see [`seq_collisions`]), records each old
/// number in the task's `previous_seqs`, and bumps the counter. Returns the
/// renumbered tasks as `(id, from, to)`.
pub fn repair_seqs<T: Transactable + ReadDoc>(
    tx: &mut T,
    now: Timestamp,
) -> CoreResult<Vec<(Uuid, u64, u64)>> {
    let workspace = schema::read_workspace(tx);
    let collisions = seq_collisions(&workspace);
    let mut max = workspace.task_seq;
    let mut renumbered = Vec::with_capacity(collisions.len());
    for (id, seq) in collisions {
        let mut task = workspace.tasks[&id].clone();
        renumbered.push((id, task.seq, seq));
        task.previous_seqs.push(task.seq);
        task.seq = seq;
        task.updated = now;
        schema::write_task(tx, &task)?;
        max = max.max(seq);
    }
    if !renumbered.is_empty() {
        schema::set_task_seq_counter(tx, max)?;
    }
    Ok(renumbered)
}

// ---------- entries ----------

#[must_use]
pub fn new_entry(
    task: Uuid,
    start: Timestamp,
    end: Option<Timestamp>,
    note: Option<String>,
    now: Timestamp,
) -> Entry {
    Entry {
        // Random ids keep short (8 hex) entry prefixes unique; v7 shares time prefixes.
        id: Uuid::new_v4(),
        task,
        start,
        end,
        note: note.filter(|note| !note.trim().is_empty()),
        created: now,
        updated: now,
    }
}

/// Start must be strictly before end when both are set.
pub fn validate_entry(entry: &Entry) -> CoreResult<()> {
    if let Some(end) = entry.end
        && end <= entry.start
    {
        return Err(CoreError::Invalid(format!(
            "entry end {} must be after start {}",
            end.to_rfc3339(),
            entry.start.to_rfc3339()
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EntryPatch {
    #[serde(default)]
    pub start: Option<Timestamp>,
    /// `Some(None)` makes the entry running again.
    #[serde(default)]
    pub end: Option<Option<Timestamp>>,
    #[serde(default)]
    pub task: Option<Uuid>,
    /// `Some(None)` clears the note.
    #[serde(default)]
    pub note: Option<Option<String>>,
}

/// Applies a patch, validating the resulting range.
pub fn patch_entry(entry: &Entry, patch: &EntryPatch, now: Timestamp) -> CoreResult<Entry> {
    let mut next = entry.clone();
    if let Some(start) = patch.start {
        next.start = start;
    }
    if let Some(end) = patch.end {
        next.end = end;
    }
    if let Some(task) = patch.task {
        next.task = task;
    }
    if let Some(note) = &patch.note {
        next.note = note.clone().filter(|note| !note.trim().is_empty());
    }
    validate_entry(&next)?;
    if next != *entry {
        next.updated = now;
    }
    Ok(next)
}

/// Splits an entry at `at` into two adjacent entries on the same task. A
/// running entry's second half keeps running.
pub fn split_entry(entry: &Entry, at: Timestamp, now: Timestamp) -> CoreResult<(Entry, Entry)> {
    let upper = entry.end.unwrap_or(now);
    if at <= entry.start || at >= upper {
        return Err(CoreError::Invalid(format!(
            "split time {} must be strictly inside the entry",
            at.to_rfc3339()
        )));
    }
    let mut first = entry.clone();
    first.end = Some(at);
    first.updated = now;
    let second = Entry {
        id: Uuid::new_v4(),
        task: entry.task,
        start: at,
        end: entry.end,
        note: entry.note.clone(),
        created: now,
        updated: now,
    };
    Ok((first, second))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::User;
    use automerge::AutoCommit;
    use chrono::{Duration, TimeZone, Utc};

    fn now() -> Timestamp {
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn workspace_doc() -> AutoCommit {
        let mut doc = AutoCommit::new();
        schema::init_workspace(
            &mut doc,
            &User {
                id: "u".into(),
                name: "me".into(),
            },
        )
        .unwrap();
        doc
    }

    #[test]
    fn quick_create_creates_tags_project_and_seq() {
        let mut doc = workspace_doc();
        let task = add_task(
            &mut doc,
            &NewTask {
                title: "Fix login PROJ-123".into(),
                tags: vec!["backend".into(), "urgent".into()],
                project: Some("web".into()),
                metadata: [("ticket".into(), "PROJ-123".into())].into(),
                ..NewTask::default()
            },
            now(),
        )
        .unwrap();
        assert_eq!(task.seq, 1);
        let workspace = schema::read_workspace(&doc);
        assert_eq!(workspace.tags.len(), 2);
        assert_eq!(workspace.projects.len(), 1);
        let again = add_task(
            &mut doc,
            &NewTask {
                title: "Second".into(),
                tags: vec!["Backend".into()],
                ..NewTask::default()
            },
            now(),
        )
        .unwrap();
        assert_eq!(again.seq, 2);
        assert_eq!(schema::read_workspace(&doc).tags.len(), 2);
    }

    #[test]
    fn done_task_keeps_its_number() {
        let mut doc = workspace_doc();
        let first = add_task(
            &mut doc,
            &NewTask {
                title: "a".into(),
                ..NewTask::default()
            },
            now(),
        )
        .unwrap();
        modify_task(
            &mut doc,
            first.id,
            &TaskPatch {
                state: Some(TaskState::Done),
                ..TaskPatch::default()
            },
            now(),
        )
        .unwrap();
        delete_task(&mut doc, first.id).unwrap();
        // Even after deletion the counter never reuses #1.
        let next = add_task(
            &mut doc,
            &NewTask {
                title: "b".into(),
                ..NewTask::default()
            },
            now(),
        )
        .unwrap();
        assert_eq!(next.seq, 2);
    }

    #[test]
    fn empty_title_and_duplicate_names_are_rejected() {
        let mut doc = workspace_doc();
        assert!(matches!(
            add_task(&mut doc, &NewTask::default(), now()),
            Err(CoreError::Invalid(_))
        ));
        add_tag(&mut doc, "x", None, now()).unwrap();
        assert!(matches!(
            add_tag(&mut doc, "x", None, now()),
            Err(CoreError::Conflict(_))
        ));
    }

    #[test]
    fn rename_and_delete_tag_propagate_to_tasks() {
        let mut doc = workspace_doc();
        let task = add_task(
            &mut doc,
            &NewTask {
                title: "t".into(),
                tags: vec!["backend".into()],
                ..NewTask::default()
            },
            now(),
        )
        .unwrap();
        let tag = *task.tags.iter().next().unwrap();
        modify_tag(
            &mut doc,
            tag,
            &NamedPatch {
                name: Some("be".into()),
                ..NamedPatch::default()
            },
            now(),
        )
        .unwrap();
        let workspace = schema::read_workspace(&doc);
        assert_eq!(workspace.tags[&tag].name, "be");
        assert!(workspace.tasks[&task.id].tags.contains(&tag));
        delete_tag(&mut doc, tag, now()).unwrap();
        assert!(schema::read_workspace(&doc).tasks[&task.id].tags.is_empty());
    }

    #[test]
    fn concurrent_seq_allocation_is_repaired_after_merge() {
        let mut base = workspace_doc();
        add_task(
            &mut base,
            &NewTask {
                title: "shared".into(),
                ..NewTask::default()
            },
            now(),
        )
        .unwrap();
        let mut a = base.fork();
        let mut b = base.fork();
        let earlier = add_task(
            &mut a,
            &NewTask {
                title: "from a".into(),
                ..NewTask::default()
            },
            now(),
        )
        .unwrap();
        let later = add_task(
            &mut b,
            &NewTask {
                title: "from b".into(),
                ..NewTask::default()
            },
            now() + Duration::seconds(5),
        )
        .unwrap();
        assert_eq!(earlier.seq, later.seq);
        a.merge(&mut b).unwrap();
        let mut other = a.fork();
        let changed = repair_seqs(&mut a, now() + Duration::seconds(10)).unwrap();
        assert_eq!(changed, vec![(later.id, 2, 3)]);
        let workspace = schema::read_workspace(&a);
        assert_eq!(workspace.tasks[&earlier.id].seq, 2);
        assert_eq!(workspace.tasks[&later.id].seq, 3);
        assert_eq!(workspace.tasks[&later.id].previous_seqs, vec![2]);
        assert!(workspace.tasks[&earlier.id].previous_seqs.is_empty());
        let view = crate::model::View {
            workspace: workspace.clone(),
            ..crate::model::View::default()
        };
        let holder = view.resolve_task("#2").unwrap();
        assert_eq!(holder.id, earlier.id);
        let shown = view.task_view_for_key(holder, "2");
        assert_eq!(shown.renumbered_from.len(), 1);
        assert_eq!(
            (shown.renumbered_from[0].id, shown.renumbered_from[0].to),
            (later.id, 3)
        );
        assert!(
            view.task_view_for_key(holder, &earlier.id.to_string())
                .renumbered_from
                .is_empty()
        );
        assert_eq!(workspace.task_seq, 3);
        assert!(repair_seqs(&mut a, now()).unwrap().is_empty());
        // Both devices repairing the same collision keep one old number.
        assert_eq!(repair_seqs(&mut other, now()).unwrap(), changed);
        a.merge(&mut other).unwrap();
        let task = &schema::read_workspace(&a).tasks[&later.id];
        assert_eq!((task.seq, task.previous_seqs.clone()), (3, vec![2]));
    }

    #[test]
    fn entry_validation_split_and_patch() {
        let start = now();
        let entry = new_entry(Uuid::now_v7(), start, None, None, start);
        let (first, second) = split_entry(
            &entry,
            start + Duration::minutes(30),
            start + Duration::hours(1),
        )
        .unwrap();
        assert_eq!(first.end, Some(start + Duration::minutes(30)));
        assert_eq!(second.start, start + Duration::minutes(30));
        assert!(second.end.is_none());
        assert!(split_entry(&entry, start, start + Duration::hours(1)).is_err());
        let bad = patch_entry(
            &first,
            &EntryPatch {
                end: Some(Some(start - Duration::minutes(1))),
                ..EntryPatch::default()
            },
            start,
        );
        assert!(matches!(bad, Err(CoreError::Invalid(_))));
    }
}
