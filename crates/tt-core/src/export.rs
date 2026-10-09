//! Versioned JSON export/import and CSV writer.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    model::{Entry, Project, Tag, Task, View},
    round::Rounded,
    time::Range,
};

pub const EXPORT_FORMAT: &str = "tt-export";
pub const EXPORT_VERSION: u32 = 1;

/// The documented export schema (see `docs/export.md`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    pub format: String,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
    pub projects: Vec<Project>,
    pub tags: Vec<Tag>,
    pub tasks: Vec<Task>,
    pub entries: Vec<Entry>,
}

/// Everything (or the entries overlapping `range`, plus all workspace records).
#[must_use]
pub fn export(view: &View, range: Option<Range>, now: DateTime<Utc>) -> Export {
    let mut entries: Vec<Entry> = view
        .entries
        .values()
        .filter(|entry| {
            range.is_none_or(|range| range.overlaps(entry.start, entry.end.unwrap_or(now)))
        })
        .cloned()
        .collect();
    entries.sort_by_key(|entry| (entry.start, entry.id));
    let mut tasks: Vec<Task> = view.workspace.tasks.values().cloned().collect();
    tasks.sort_by_key(|task| task.seq);
    Export {
        format: EXPORT_FORMAT.into(),
        version: EXPORT_VERSION,
        exported_at: now,
        range,
        projects: view.workspace.projects.values().cloned().collect(),
        tags: view.workspace.tags.values().cloned().collect(),
        tasks,
        entries,
    }
}

/// Parses and validates an export file.
pub fn parse_export(text: &str) -> CoreResult<Export> {
    let export: Export = serde_json::from_str(text)
        .map_err(|error| CoreError::Invalid(format!("not a tt export: {error}")))?;
    if export.format != EXPORT_FORMAT {
        return Err(CoreError::Invalid(format!(
            "unknown export format {:?}",
            export.format
        )));
    }
    if export.version > EXPORT_VERSION {
        return Err(CoreError::Invalid(format!(
            "export version {} is newer than supported {EXPORT_VERSION}",
            export.version
        )));
    }
    for entry in &export.entries {
        crate::ops::validate_entry(entry)?;
    }
    Ok(export)
}

/// Normalized form for equality "modulo ordering" (and export time).
#[must_use]
pub fn normalized(export: &Export) -> Export {
    let mut copy = export.clone();
    copy.exported_at = DateTime::<Utc>::UNIX_EPOCH;
    copy.projects.sort_by_key(|p| p.id);
    copy.tags.sort_by_key(|t| t.id);
    copy.tasks.sort_by_key(|t| t.id);
    copy.entries.sort_by_key(|e| e.id);
    copy
}

/// What an import will write, matching existing records by uuid.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPlan {
    pub projects: Vec<Project>,
    pub tags: Vec<Tag>,
    pub tasks: Vec<Task>,
    pub entries: Vec<Entry>,
    /// Imported tasks whose seq was taken by a different task locally: uuid → new seq.
    pub renumbered: BTreeMap<Uuid, u64>,
    pub unchanged: usize,
}

/// Plans an import: records whose uuid exists with identical content are
/// skipped, others are upserted; seqs taken by a different local task are
/// renumbered after the current maximum.
#[must_use]
pub fn plan_import(view: &View, export: &Export) -> ImportPlan {
    let mut plan = ImportPlan::default();
    let ws = &view.workspace;
    for project in &export.projects {
        if ws.projects.get(&project.id) == Some(project) {
            plan.unchanged += 1;
        } else {
            plan.projects.push(project.clone());
        }
    }
    for tag in &export.tags {
        if ws.tags.get(&tag.id) == Some(tag) {
            plan.unchanged += 1;
        } else {
            plan.tags.push(tag.clone());
        }
    }
    let mut taken: BTreeMap<u64, Uuid> = ws.tasks.values().map(|t| (t.seq, t.id)).collect();
    let mut next = ws
        .tasks
        .values()
        .map(|t| t.seq)
        .chain(export.tasks.iter().map(|t| t.seq))
        .max()
        .unwrap_or(0)
        .max(ws.task_seq)
        + 1;
    for task in &export.tasks {
        if ws.tasks.get(&task.id) == Some(task) {
            plan.unchanged += 1;
            continue;
        }
        let mut task = task.clone();
        if let Some(owner) = taken.get(&task.seq)
            && *owner != task.id
        {
            plan.renumbered.insert(task.id, next);
            task.seq = next;
            next += 1;
        }
        taken.insert(task.seq, task.id);
        plan.tasks.push(task);
    }
    for entry in &export.entries {
        if view.entries.get(&entry.id) == Some(entry) {
            plan.unchanged += 1;
        } else {
            plan.entries.push(entry.clone());
        }
    }
    plan
}

const CSV_HEADER: [&str; 10] = [
    "id",
    "task_seq",
    "task_title",
    "project",
    "tags",
    "start",
    "end",
    "duration_seconds",
    "note",
    "task_id",
];

fn task_columns(view: &View, task: Uuid) -> (String, String, String, String) {
    view.task_view_by_id(task).map_or_else(
        || (String::new(), String::new(), String::new(), String::new()),
        |task| {
            (
                task.seq.to_string(),
                task.title,
                task.project.map(|p| p.name).unwrap_or_default(),
                task.tags
                    .into_iter()
                    .map(|t| t.name)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        },
    )
}

fn csv_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::Invalid(format!("csv: {error}"))
}

/// Entries as CSV with task seq, title, project, tags, start, end, duration, note.
pub fn entries_csv(view: &View, entries: &[Entry], now: DateTime<Utc>) -> CoreResult<String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(CSV_HEADER).map_err(csv_error)?;
    for entry in entries {
        let (seq, title, project, tags) = task_columns(view, entry.task);
        writer
            .write_record([
                entry.id.to_string(),
                seq,
                title,
                project,
                tags,
                entry.start.to_rfc3339(),
                entry.end.map(|end| end.to_rfc3339()).unwrap_or_default(),
                entry.duration(now).to_string(),
                entry.note.clone().unwrap_or_default(),
                entry.task.to_string(),
            ])
            .map_err(csv_error)?;
    }
    String::from_utf8(writer.into_inner().map_err(csv_error)?).map_err(csv_error)
}

/// Rounded intervals as CSV; the id column lists source entry ids.
pub fn rounded_csv(view: &View, rounded: &[Rounded], tz: Tz) -> CoreResult<String> {
    let _ = tz;
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(CSV_HEADER).map_err(csv_error)?;
    for item in rounded {
        let (seq, title, project, tags) = task_columns(view, item.task);
        writer
            .write_record([
                item.entries
                    .iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
                seq,
                title,
                project,
                tags,
                item.start.to_rfc3339(),
                item.end.to_rfc3339(),
                item.duration.to_string(),
                String::new(),
                item.task.to_string(),
            ])
            .map_err(csv_error)?;
    }
    String::from_utf8(writer.into_inner().map_err(csv_error)?).map_err(csv_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::TaskState, ops};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn sample() -> View {
        let mut view = View::default();
        let tag = Tag {
            id: Uuid::now_v7(),
            name: "be".into(),
            color: None,
            created: now(),
            updated: now(),
        };
        let task = Task {
            id: Uuid::now_v7(),
            seq: 1,
            title: "Fix, \"quoted\"".into(),
            description: "d".into(),
            tags: [tag.id].into(),
            project: None,
            metadata: [("ticket".into(), "P-1".into())].into(),
            state: TaskState::Open,
            previous_seqs: Vec::new(),
            created: now(),
            updated: now(),
        };
        let entry = ops::new_entry(
            task.id,
            now(),
            Some(now() + chrono::Duration::minutes(5)),
            Some("n".into()),
            now(),
        );
        view.workspace.tags.insert(tag.id, tag);
        view.workspace.tasks.insert(task.id, task);
        view.entries.insert(entry.id, entry);
        view
    }

    #[test]
    fn json_round_trip_into_empty_view_is_identical() {
        let view = sample();
        let exported = export(&view, None, now());
        let text = serde_json::to_string_pretty(&exported).unwrap();
        let parsed = parse_export(&text).unwrap();
        let plan = plan_import(&View::default(), &parsed);
        assert!(plan.renumbered.is_empty());
        let mut imported = View::default();
        for tag in plan.tags {
            imported.workspace.tags.insert(tag.id, tag);
        }
        for task in plan.tasks {
            imported.workspace.tasks.insert(task.id, task);
        }
        for entry in plan.entries {
            imported.entries.insert(entry.id, entry);
        }
        let again = export(&imported, None, now() + chrono::Duration::hours(1));
        assert_eq!(normalized(&again), normalized(&exported));
        assert_eq!(plan_import(&imported, &parsed).unchanged, 3);
    }

    #[test]
    fn seq_conflicts_are_renumbered() {
        let view = sample();
        let mut other = sample();
        let exported = export(&other, None, now());
        other.workspace.tasks.clear();
        let plan = plan_import(&view, &exported);
        assert_eq!(
            plan.renumbered.values().copied().collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn csv_quotes_and_columns() {
        let view = sample();
        let entries: Vec<_> = view.entries.values().cloned().collect();
        let csv = entries_csv(&view, &entries, now()).unwrap();
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "id,task_seq,task_title,project,tags,start,end,duration_seconds,note,task_id"
        );
        let row = lines.next().unwrap();
        assert!(row.contains("\"Fix, \"\"quoted\"\"\""));
        assert!(row.contains(",300,n,"));
    }

    #[test]
    fn rejects_foreign_or_invalid_files() {
        assert!(parse_export("{}").is_err());
        let mut exported = export(&sample(), None, now());
        exported.format = "other".into();
        assert!(parse_export(&serde_json::to_string(&exported).unwrap()).is_err());
    }
}
