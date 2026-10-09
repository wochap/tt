//! Document schemas and typed read/write helpers.
//!
//! ```text
//! index      { schema, kind:"index", workspace:"<bs58>", entries:{ "2026":"<bs58>" } }
//! workspace  { schema, kind:"workspace", user:{id,name},
//!              projects:{ <uuid>:{name,color?,archived,created,updated} },
//!              tags:{ <uuid>:{name,color?,created,updated} },
//!              tasks:{ <uuid>:{seq,title,description,tags:{<tagId>:true},project?,
//!                              metadata:{k:v},state,previous_seqs?:[n],created,updated} },
//!              counters:{taskSeq} }
//! entries    { schema, kind:"entries", year, entries:{ <uuid>:{task,start,end|null,note?,created,updated} } }
//! ```
//! Strings are Automerge Text, timestamps integer milliseconds UTC. Tags are
//! a map used as a set so concurrent tag edits merge without duplicates.

use std::collections::{BTreeMap, BTreeSet};

use automerge::{ObjId, ROOT, ReadDoc, transaction::Transactable};
use uuid::Uuid;

use crate::{
    am::{self, AmResult},
    error::{CoreError, CoreResult},
    model::{Entry, Index, Project, Tag, Task, TaskState, Timestamp, User, Workspace},
};

/// Current schema version written to every document.
pub const SCHEMA_VERSION: i64 = 1;

pub const KIND_INDEX: &str = "index";
pub const KIND_WORKSPACE: &str = "workspace";
pub const KIND_ENTRIES: &str = "entries";

/// Upgrades a document written with an older schema. Version 1 is the first
/// schema, so this only stamps missing versions; later versions add steps.
pub fn migrate<T: Transactable + ReadDoc>(tx: &mut T) -> CoreResult<()> {
    let version = am::get_i64(tx, &ROOT, "schema").unwrap_or(0);
    if version > SCHEMA_VERSION {
        return Err(CoreError::Document(format!(
            "document schema {version} is newer than supported {SCHEMA_VERSION}; upgrade tt"
        )));
    }
    if version < SCHEMA_VERSION {
        tx.put(ROOT, "schema", SCHEMA_VERSION)?;
    }
    Ok(())
}

pub fn kind<D: ReadDoc>(doc: &D) -> Option<String> {
    am::get_string(doc, &ROOT, "kind")
}

// ---------- index ----------

pub fn init_index<T: Transactable + ReadDoc>(tx: &mut T, workspace: &str) -> AmResult<()> {
    tx.put(ROOT, "schema", SCHEMA_VERSION)?;
    am::put_text(tx, &ROOT, "kind", KIND_INDEX)?;
    am::put_text(tx, &ROOT, "workspace", workspace)?;
    am::ensure_map(tx, &ROOT, "entries")?;
    Ok(())
}

pub fn read_index<D: ReadDoc>(doc: &D) -> Index {
    let mut entries = BTreeMap::new();
    if let Some(map) = am::get_map(doc, &ROOT, "entries") {
        for key in am::keys(doc, &map) {
            if let (Ok(year), Some(id)) = (key.parse::<i32>(), am::get_string(doc, &map, &key)) {
                entries.insert(year, id);
            }
        }
    }
    Index {
        schema: am::get_i64(doc, &ROOT, "schema").unwrap_or(0),
        workspace: am::get_string(doc, &ROOT, "workspace"),
        entries,
    }
}

pub fn index_set_year<T: Transactable + ReadDoc>(tx: &mut T, year: i32, doc: &str) -> AmResult<()> {
    let map = am::ensure_map(tx, &ROOT, "entries")?;
    am::put_text(tx, &map, &year.to_string(), doc)
}

// ---------- workspace ----------

pub fn init_workspace<T: Transactable + ReadDoc>(tx: &mut T, user: &User) -> AmResult<()> {
    tx.put(ROOT, "schema", SCHEMA_VERSION)?;
    am::put_text(tx, &ROOT, "kind", KIND_WORKSPACE)?;
    let user_obj = am::ensure_map(tx, &ROOT, "user")?;
    am::put_text(tx, &user_obj, "id", &user.id)?;
    am::put_text(tx, &user_obj, "name", &user.name)?;
    am::ensure_map(tx, &ROOT, "projects")?;
    am::ensure_map(tx, &ROOT, "tags")?;
    am::ensure_map(tx, &ROOT, "tasks")?;
    let counters = am::ensure_map(tx, &ROOT, "counters")?;
    if am::get_i64(tx, &counters, "taskSeq").is_none() {
        tx.put(&counters, "taskSeq", 0_i64)?;
    }
    Ok(())
}

pub fn read_workspace<D: ReadDoc>(doc: &D) -> Workspace {
    let user = am::get_map(doc, &ROOT, "user").map(|obj| User {
        id: am::get_string(doc, &obj, "id").unwrap_or_default(),
        name: am::get_string(doc, &obj, "name").unwrap_or_default(),
    });
    let mut projects = BTreeMap::new();
    if let Some(map) = am::get_map(doc, &ROOT, "projects") {
        for key in am::keys(doc, &map) {
            if let (Ok(id), Some(obj)) = (Uuid::parse_str(&key), am::get_map(doc, &map, &key))
                && let Some(project) = read_project(doc, &obj, id)
            {
                projects.insert(id, project);
            }
        }
    }
    let mut tags = BTreeMap::new();
    if let Some(map) = am::get_map(doc, &ROOT, "tags") {
        for key in am::keys(doc, &map) {
            if let (Ok(id), Some(obj)) = (Uuid::parse_str(&key), am::get_map(doc, &map, &key))
                && let Some(tag) = read_tag(doc, &obj, id)
            {
                tags.insert(id, tag);
            }
        }
    }
    let mut tasks = BTreeMap::new();
    if let Some(map) = am::get_map(doc, &ROOT, "tasks") {
        for key in am::keys(doc, &map) {
            if let (Ok(id), Some(obj)) = (Uuid::parse_str(&key), am::get_map(doc, &map, &key))
                && let Some(task) = read_task(doc, &obj, id)
            {
                tasks.insert(id, task);
            }
        }
    }
    let task_seq = am::get_map(doc, &ROOT, "counters")
        .and_then(|obj| am::get_i64(doc, &obj, "taskSeq"))
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0);
    Workspace {
        schema: am::get_i64(doc, &ROOT, "schema").unwrap_or(0),
        user,
        projects,
        tags,
        tasks,
        task_seq,
    }
}

fn read_times<D: ReadDoc>(doc: &D, obj: &ObjId) -> Option<(Timestamp, Timestamp)> {
    let created = am::get_time(doc, obj, "created")?;
    let updated = am::get_time(doc, obj, "updated").unwrap_or(created);
    Some((created, updated))
}

fn read_project<D: ReadDoc>(doc: &D, obj: &ObjId, id: Uuid) -> Option<Project> {
    let (created, updated) = read_times(doc, obj)?;
    Some(Project {
        id,
        name: am::get_string(doc, obj, "name")?,
        color: am::get_string(doc, obj, "color"),
        archived: am::get_bool(doc, obj, "archived").unwrap_or(false),
        created,
        updated,
    })
}

fn read_tag<D: ReadDoc>(doc: &D, obj: &ObjId, id: Uuid) -> Option<Tag> {
    let (created, updated) = read_times(doc, obj)?;
    Some(Tag {
        id,
        name: am::get_string(doc, obj, "name")?,
        color: am::get_string(doc, obj, "color"),
        created,
        updated,
    })
}

fn read_task<D: ReadDoc>(doc: &D, obj: &ObjId, id: Uuid) -> Option<Task> {
    let (created, updated) = read_times(doc, obj)?;
    let mut tags = BTreeSet::new();
    if let Some(map) = am::get_map(doc, obj, "tags") {
        for key in am::keys(doc, &map) {
            if am::get_bool(doc, &map, &key).unwrap_or(true)
                && let Ok(tag) = Uuid::parse_str(&key)
            {
                tags.insert(tag);
            }
        }
    }
    let mut metadata = BTreeMap::new();
    if let Some(map) = am::get_map(doc, obj, "metadata") {
        for key in am::keys(doc, &map) {
            if let Some(value) = am::get_string(doc, &map, &key) {
                metadata.insert(key, value);
            }
        }
    }
    Some(Task {
        id,
        seq: am::get_i64(doc, obj, "seq").and_then(|seq| u64::try_from(seq).ok())?,
        title: am::get_string(doc, obj, "title").unwrap_or_default(),
        description: am::get_string(doc, obj, "description").unwrap_or_default(),
        tags,
        project: am::get_string(doc, obj, "project").and_then(|p| Uuid::parse_str(&p).ok()),
        metadata,
        state: am::get_string(doc, obj, "state")
            .and_then(|state| state.parse().ok())
            .unwrap_or(TaskState::Open),
        previous_seqs: read_previous_seqs(doc, obj),
        created,
        updated,
    })
}

/// Old seqs in order, without the duplicates two devices repairing the same
/// collision concurrently append.
fn read_previous_seqs<D: ReadDoc>(doc: &D, obj: &ObjId) -> Vec<u64> {
    let mut seqs: Vec<u64> = Vec::new();
    for seq in am::get_i64_list(doc, obj, "previous_seqs") {
        if let Ok(seq) = u64::try_from(seq)
            && !seqs.contains(&seq)
        {
            seqs.push(seq);
        }
    }
    seqs
}

fn collection<T: Transactable + ReadDoc>(tx: &mut T, name: &str) -> AmResult<ObjId> {
    am::ensure_map(tx, &ROOT, name)
}

fn item<T: Transactable + ReadDoc>(tx: &mut T, name: &str, id: Uuid) -> AmResult<ObjId> {
    let map = collection(tx, name)?;
    am::ensure_map(tx, &map, &id.to_string())
}

/// Writes every field of a project (create or full replace).
pub fn write_project<T: Transactable + ReadDoc>(tx: &mut T, project: &Project) -> AmResult<()> {
    let obj = item(tx, "projects", project.id)?;
    am::put_text(tx, &obj, "name", &project.name)?;
    am::put_opt_text(tx, &obj, "color", project.color.as_deref())?;
    if am::get_bool(tx, &obj, "archived") != Some(project.archived) {
        tx.put(&obj, "archived", project.archived)?;
    }
    put_time_if_changed(tx, &obj, "created", project.created)?;
    put_time_if_changed(tx, &obj, "updated", project.updated)
}

pub fn write_tag<T: Transactable + ReadDoc>(tx: &mut T, tag: &Tag) -> AmResult<()> {
    let obj = item(tx, "tags", tag.id)?;
    am::put_text(tx, &obj, "name", &tag.name)?;
    am::put_opt_text(tx, &obj, "color", tag.color.as_deref())?;
    put_time_if_changed(tx, &obj, "created", tag.created)?;
    put_time_if_changed(tx, &obj, "updated", tag.updated)
}

/// Writes only the fields that differ, so concurrent edits of other fields merge.
pub fn write_task<T: Transactable + ReadDoc>(tx: &mut T, task: &Task) -> AmResult<()> {
    let obj = item(tx, "tasks", task.id)?;
    let seq = i64::try_from(task.seq).unwrap_or(i64::MAX);
    if am::get_i64(tx, &obj, "seq") != Some(seq) {
        tx.put(&obj, "seq", seq)?;
    }
    am::put_text(tx, &obj, "title", &task.title)?;
    am::put_text(tx, &obj, "description", &task.description)?;
    let tags = am::ensure_map(tx, &obj, "tags")?;
    for key in am::keys(tx, &tags) {
        if Uuid::parse_str(&key).map_or(true, |id| !task.tags.contains(&id)) {
            tx.delete(&tags, key.as_str())?;
        }
    }
    for tag in &task.tags {
        if am::get_bool(tx, &tags, &tag.to_string()) != Some(true) {
            tx.put(&tags, tag.to_string(), true)?;
        }
    }
    am::put_opt_text(
        tx,
        &obj,
        "project",
        task.project.map(|id| id.to_string()).as_deref(),
    )?;
    let metadata = am::ensure_map(tx, &obj, "metadata")?;
    for key in am::keys(tx, &metadata) {
        if !task.metadata.contains_key(&key) {
            tx.delete(&metadata, key.as_str())?;
        }
    }
    for (key, value) in &task.metadata {
        am::put_text(tx, &metadata, key, value)?;
    }
    am::put_text(tx, &obj, "state", task.state.as_str())?;
    if read_previous_seqs(tx, &obj) != task.previous_seqs {
        let previous: Vec<i64> = task
            .previous_seqs
            .iter()
            .map(|seq| i64::try_from(*seq).unwrap_or(i64::MAX))
            .collect();
        am::put_i64_list(tx, &obj, "previous_seqs", &previous)?;
    }
    put_time_if_changed(tx, &obj, "created", task.created)?;
    put_time_if_changed(tx, &obj, "updated", task.updated)
}

pub fn delete_item<T: Transactable + ReadDoc>(tx: &mut T, name: &str, id: Uuid) -> AmResult<()> {
    let map = collection(tx, name)?;
    am::delete_if_present(tx, &map, &id.to_string())
}

pub fn set_task_seq_counter<T: Transactable + ReadDoc>(tx: &mut T, value: u64) -> AmResult<()> {
    let counters = am::ensure_map(tx, &ROOT, "counters")?;
    tx.put(
        &counters,
        "taskSeq",
        i64::try_from(value).unwrap_or(i64::MAX),
    )
}

fn put_time_if_changed<T: Transactable + ReadDoc>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
    value: Timestamp,
) -> AmResult<()> {
    if am::get_i64(tx, obj, key) != Some(am::millis(value)) {
        am::put_time(tx, obj, key, value)?;
    }
    Ok(())
}

// ---------- entries ----------

pub fn init_entries<T: Transactable + ReadDoc>(tx: &mut T, year: i32) -> AmResult<()> {
    tx.put(ROOT, "schema", SCHEMA_VERSION)?;
    am::put_text(tx, &ROOT, "kind", KIND_ENTRIES)?;
    tx.put(ROOT, "year", i64::from(year))?;
    am::ensure_map(tx, &ROOT, "entries")?;
    Ok(())
}

pub fn entries_year<D: ReadDoc>(doc: &D) -> Option<i32> {
    am::get_i64(doc, &ROOT, "year").and_then(|year| i32::try_from(year).ok())
}

pub fn read_entries<D: ReadDoc>(doc: &D) -> BTreeMap<Uuid, Entry> {
    let mut entries = BTreeMap::new();
    if let Some(map) = am::get_map(doc, &ROOT, "entries") {
        for key in am::keys(doc, &map) {
            let (Ok(id), Some(obj)) = (Uuid::parse_str(&key), am::get_map(doc, &map, &key)) else {
                continue;
            };
            let Some(task) =
                am::get_string(doc, &obj, "task").and_then(|t| Uuid::parse_str(&t).ok())
            else {
                continue;
            };
            let Some(start) = am::get_time(doc, &obj, "start") else {
                continue;
            };
            let created = am::get_time(doc, &obj, "created").unwrap_or(start);
            entries.insert(
                id,
                Entry {
                    id,
                    task,
                    start,
                    end: am::get_time(doc, &obj, "end"),
                    note: am::get_string(doc, &obj, "note").filter(|note| !note.is_empty()),
                    created,
                    updated: am::get_time(doc, &obj, "updated").unwrap_or(created),
                },
            );
        }
    }
    entries
}

/// Writes only changed fields of an entry.
pub fn write_entry<T: Transactable + ReadDoc>(tx: &mut T, entry: &Entry) -> AmResult<()> {
    let obj = item(tx, "entries", entry.id)?;
    am::put_text(tx, &obj, "task", &entry.task.to_string())?;
    put_time_if_changed(tx, &obj, "start", entry.start)?;
    match entry.end {
        Some(end) => put_time_if_changed(tx, &obj, "end", end)?,
        None => am::put_opt_time(tx, &obj, "end", None)?,
    }
    am::put_opt_text(tx, &obj, "note", entry.note.as_deref())?;
    put_time_if_changed(tx, &obj, "created", entry.created)?;
    put_time_if_changed(tx, &obj, "updated", entry.updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::AutoCommit;
    use chrono::{TimeZone, Utc};

    fn ts(minute: u32) -> Timestamp {
        Utc.with_ymd_and_hms(2026, 10, 7, 13, minute, 0).unwrap()
    }

    #[test]
    fn workspace_round_trip_and_field_level_writes() {
        let mut doc = AutoCommit::new();
        init_workspace(
            &mut doc,
            &User {
                id: "u1".into(),
                name: "me".into(),
            },
        )
        .unwrap();
        let tag = Tag {
            id: Uuid::new_v4(),
            name: "backend".into(),
            color: None,
            created: ts(0),
            updated: ts(0),
        };
        write_tag(&mut doc, &tag).unwrap();
        let project = Project {
            id: Uuid::new_v4(),
            name: "web".into(),
            color: Some("#ff0000".into()),
            archived: false,
            created: ts(0),
            updated: ts(0),
        };
        write_project(&mut doc, &project).unwrap();
        let task = Task {
            id: Uuid::new_v4(),
            seq: 1,
            title: "Fix login".into(),
            description: "body".into(),
            tags: [tag.id].into(),
            project: Some(project.id),
            metadata: [("ticket".to_string(), "PROJ-1".to_string())].into(),
            state: TaskState::Open,
            previous_seqs: vec![4],
            created: ts(1),
            updated: ts(1),
        };
        write_task(&mut doc, &task).unwrap();
        set_task_seq_counter(&mut doc, 1).unwrap();
        let workspace = read_workspace(&doc);
        assert_eq!(workspace.tasks[&task.id], task);
        assert_eq!(workspace.tags[&tag.id], tag);
        assert_eq!(workspace.projects[&project.id], project);
        assert_eq!(workspace.task_seq, 1);
        assert_eq!(workspace.schema, SCHEMA_VERSION);

        let mut changed = task.clone();
        changed.tags.clear();
        changed.metadata.clear();
        changed.project = None;
        changed.title = "Fix login flow".into();
        changed.previous_seqs.push(7);
        write_task(&mut doc, &changed).unwrap();
        assert_eq!(read_workspace(&doc).tasks[&task.id], changed);
    }

    #[test]
    fn entries_round_trip_with_running_and_note() {
        let mut doc = AutoCommit::new();
        init_entries(&mut doc, 2026).unwrap();
        let mut entry = Entry {
            id: Uuid::new_v4(),
            task: Uuid::new_v4(),
            start: ts(0),
            end: None,
            note: Some("n".into()),
            created: ts(0),
            updated: ts(0),
        };
        write_entry(&mut doc, &entry).unwrap();
        assert_eq!(read_entries(&doc)[&entry.id], entry);
        entry.end = Some(ts(30));
        entry.note = None;
        write_entry(&mut doc, &entry).unwrap();
        assert_eq!(read_entries(&doc)[&entry.id], entry);
        assert_eq!(entries_year(&doc), Some(2026));
    }

    #[test]
    fn migrate_stamps_version_and_rejects_newer() {
        let mut doc = AutoCommit::new();
        migrate(&mut doc).unwrap();
        assert_eq!(am::get_i64(&doc, &ROOT, "schema"), Some(SCHEMA_VERSION));
        doc.put(ROOT, "schema", SCHEMA_VERSION + 1).unwrap();
        assert!(migrate(&mut doc).is_err());
    }

    #[test]
    fn index_lists_years() {
        let mut doc = AutoCommit::new();
        init_index(&mut doc, "ws").unwrap();
        index_set_year(&mut doc, 2026, "e26").unwrap();
        let index = read_index(&doc);
        assert_eq!(index.workspace.as_deref(), Some("ws"));
        assert_eq!(index.entries[&2026], "e26");
    }
}
