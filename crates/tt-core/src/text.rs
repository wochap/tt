//! Text formats: quick-create arguments, the task editor file, and the
//! editable time table.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    model::{Entry, TaskState, TaskView, View},
    ops::{EntryPatch, NewTask, TaskPatch},
    time::local_to_utc,
};

/// Parses `"Fix login PROJ-123" +backend +urgent @web ticket:PROJ-123`.
///
/// Arguments containing whitespace are title text verbatim; otherwise `+tag`,
/// `@project` and `key:value` (key starts with a letter, value non-empty and
/// not `//…`) are extracted and the rest joins into the title.
#[must_use]
pub fn parse_quick(args: &[String]) -> NewTask {
    let mut task = NewTask::default();
    let mut title = Vec::new();
    for arg in args {
        if arg.chars().any(char::is_whitespace) {
            title.push(arg.clone());
        } else if let Some(tag) = arg.strip_prefix('+').filter(|t| !t.is_empty()) {
            task.tags.push(tag.to_owned());
        } else if let Some(project) = arg.strip_prefix('@').filter(|p| !p.is_empty()) {
            task.project = Some(project.to_owned());
        } else if let Some((key, value)) = metadata_pair(arg) {
            task.metadata.insert(key, value);
        } else {
            title.push(arg.clone());
        }
    }
    task.title = title.join(" ");
    task
}

/// `key:value` or `key=value` with a key that starts with a letter.
#[must_use]
pub fn metadata_pair(arg: &str) -> Option<(String, String)> {
    let (key, value) = arg.split_once(':').or_else(|| arg.split_once('='))?;
    let valid_key = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    (valid_key && !value.is_empty() && !value.starts_with("//"))
        .then(|| (key.to_owned(), value.to_owned()))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    title: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

/// Renders a task for `$EDITOR`: YAML frontmatter plus the description.
#[must_use]
pub fn render_task_file(task: &TaskView) -> String {
    let front = Frontmatter {
        title: task.title.clone(),
        state: Some(task.state.as_str().to_owned()),
        project: task.project.as_ref().map(|p| p.name.clone()),
        tags: task.tags.iter().map(|t| t.name.clone()).collect(),
        metadata: task.metadata.clone(),
    };
    let yaml = serde_yaml::to_string(&front).unwrap_or_default();
    let mut out = format!("---\n{yaml}---\n");
    if !task.description.is_empty() {
        out.push_str(&task.description);
        if !task.description.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Strictly parses an edited task file into a full-replacement patch.
pub fn parse_task_file(text: &str) -> CoreResult<TaskPatch> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(|| CoreError::Invalid("file must start with a --- frontmatter line".into()))?;
    let (yaml, body) = rest
        .split_once("\n---\n")
        .or_else(|| rest.split_once("\n---\r\n"))
        .or_else(|| rest.strip_suffix("\n---").map(|yaml| (yaml, "")))
        .ok_or_else(|| CoreError::Invalid("frontmatter is not closed with ---".into()))?;
    let front: Frontmatter = serde_yaml::from_str(yaml)
        .map_err(|error| CoreError::Invalid(format!("frontmatter: {error}")))?;
    if front.title.trim().is_empty() {
        return Err(CoreError::Invalid("title must not be empty".into()));
    }
    let state = front
        .state
        .as_deref()
        .map(str::parse::<TaskState>)
        .transpose()?;
    let tags: BTreeSet<String> = front
        .tags
        .iter()
        .map(|t| t.trim().trim_start_matches('+').to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    let description = body.trim_end_matches(['\n', '\r']).to_owned();
    Ok(TaskPatch {
        title: Some(front.title.trim().to_owned()),
        description: Some(description),
        tags: Some(tags.into_iter().collect()),
        project: Some(
            front
                .project
                .map(|p| p.trim().trim_start_matches('@').to_owned())
                .filter(|p| !p.is_empty()),
        ),
        metadata: Some(front.metadata),
        state,
        ..TaskPatch::default()
    })
}

const TABLE_TIME: &str = "%Y-%m-%d %H:%M";

/// One row of the editable time table, in local time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    /// Short entry id, or `None` for a new row (`new` / `-`).
    pub id: Option<String>,
    pub task: String,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

/// Renders entries as the `tt time edit` table.
#[must_use]
pub fn render_table(view: &View, entries: &[&Entry], tz: Tz) -> String {
    let mut out = format!(
        "# tt time edit — one entry per line: id  task  start  end  note\n\
         # Times are local ({tz}) as YYYY-MM-DD HH:MM; end \"-\" means running.\n\
         # Delete a line to delete that entry. Add a line with id \"new\" to create one.\n\
         # Lines starting with # are ignored.\n"
    );
    let ids: Vec<Uuid> = entries.iter().map(|entry| entry.id).collect();
    for entry in entries {
        let id = crate::model::short_id(entry.id, ids.iter().copied());
        let task = view
            .workspace
            .tasks
            .get(&entry.task)
            .map_or_else(|| entry.task.to_string(), |task| format!("#{}", task.seq));
        let start = entry
            .start
            .with_timezone(&tz)
            .format(TABLE_TIME)
            .to_string();
        let end = entry.end.map_or_else(
            || "-".to_owned(),
            |end| end.with_timezone(&tz).format(TABLE_TIME).to_string(),
        );
        let note = entry.note.clone().unwrap_or_default();
        out.push_str(format!("{id}  {task}  {start}  {end}  {note}").trim_end());
        out.push('\n');
    }
    out
}

fn parse_local(date: &str, time: &str, tz: Tz, line: usize) -> CoreResult<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(&format!("{date} {time}"), TABLE_TIME)
        .map(|local| local_to_utc(tz, local))
        .map_err(|_| CoreError::Invalid(format!("line {line}: bad time {date} {time}")))
}

/// Parses an edited table.
pub fn parse_table(text: &str, tz: Tz) -> CoreResult<Vec<TableRow>> {
    let mut rows = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() < 5 {
            return Err(CoreError::Invalid(format!(
                "line {line}: expected id, task, start date, start time, end"
            )));
        }
        let id = match tokens[0] {
            "new" | "-" | "+" => None,
            id => Some(id.to_owned()),
        };
        let start = parse_local(tokens[2], tokens[3], tz, line)?;
        let (end, rest) = if tokens[4] == "-" {
            (None, &tokens[5..])
        } else {
            let time = tokens
                .get(5)
                .ok_or_else(|| CoreError::Invalid(format!("line {line}: end time missing")))?;
            (Some(parse_local(tokens[4], time, tz, line)?), &tokens[6..])
        };
        if let Some(end) = end
            && end <= start
        {
            return Err(CoreError::Invalid(format!(
                "line {line}: end must be after start"
            )));
        }
        let note = rest.join(" ");
        rows.push(TableRow {
            id,
            task: tokens[1].to_owned(),
            start,
            end,
            note: (!note.is_empty()).then_some(note),
        });
    }
    Ok(rows)
}

/// Changes to apply after a table edit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableChanges {
    pub create: Vec<TableCreate>,
    pub update: Vec<(Uuid, EntryPatch)>,
    pub delete: Vec<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCreate {
    pub task: Uuid,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

impl PartialEq for EntryPatch {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
            && self.end == other.end
            && self.task == other.task
            && self.note == other.note
    }
}
impl Eq for EntryPatch {}

/// Diffs edited rows against the entries the table was rendered from.
pub fn diff_table(view: &View, original: &[&Entry], rows: &[TableRow]) -> CoreResult<TableChanges> {
    let ids: Vec<Uuid> = original.iter().map(|entry| entry.id).collect();
    let mut by_short: BTreeMap<String, &Entry> = BTreeMap::new();
    for entry in original {
        by_short.insert(crate::model::short_id(entry.id, ids.iter().copied()), entry);
    }
    let resolve = |prefix: &str| -> Option<&Entry> {
        by_short.get(prefix).copied().or_else(|| {
            let mut found = original
                .iter()
                .filter(|entry| entry.id.simple().to_string().starts_with(prefix));
            match (found.next(), found.next()) {
                (Some(entry), None) => Some(*entry),
                _ => None,
            }
        })
    };
    let mut changes = TableChanges::default();
    let mut seen = BTreeSet::new();
    for row in rows {
        let task = view.resolve_task(&row.task)?.id;
        match &row.id {
            None => changes.create.push(TableCreate {
                task,
                start: row.start,
                end: row.end,
                note: row.note.clone(),
            }),
            Some(prefix) => {
                let entry = resolve(prefix).ok_or_else(|| {
                    CoreError::Invalid(format!(
                        "unknown entry id {prefix} (use \"new\" to add an entry)"
                    ))
                })?;
                if !seen.insert(entry.id) {
                    return Err(CoreError::Invalid(format!("entry {prefix} appears twice")));
                }
                // Table times have minute precision; keep seconds when unchanged.
                let same_minute = |a: DateTime<Utc>, b: DateTime<Utc>| {
                    a.timestamp().div_euclid(60) == b.timestamp().div_euclid(60)
                };
                let patch = EntryPatch {
                    start: (!same_minute(entry.start, row.start)).then_some(row.start),
                    end: match (entry.end, row.end) {
                        (Some(a), Some(b)) if same_minute(a, b) => None,
                        (None, None) => None,
                        (_, end) => Some(end),
                    },
                    task: (entry.task != task).then_some(task),
                    note: (entry.note != row.note).then(|| row.note.clone()),
                };
                if patch != EntryPatch::default() {
                    changes.update.push((entry.id, patch));
                }
            }
        }
    }
    for entry in original {
        if !seen.contains(&entry.id) {
            changes.delete.push(entry.id);
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::Task, ops};
    use chrono::TimeZone;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn quick_syntax_from_the_brief() {
        let task = parse_quick(&args(&[
            "Fix login PROJ-123",
            "+backend",
            "+urgent",
            "@web",
            "ticket:PROJ-123",
        ]));
        assert_eq!(task.title, "Fix login PROJ-123");
        assert_eq!(task.tags, vec!["backend", "urgent"]);
        assert_eq!(task.project.as_deref(), Some("web"));
        assert_eq!(task.metadata["ticket"], "PROJ-123");
        let task = parse_quick(&args(&["see", "https://x.y", "12:30"]));
        assert_eq!(task.title, "see https://x.y 12:30");
        assert!(task.metadata.is_empty());
    }

    fn view_with_task() -> (View, Task) {
        let mut view = View::default();
        let now = Utc::now();
        let task = Task {
            id: Uuid::now_v7(),
            seq: 7,
            title: "t".into(),
            description: "line one\n\nline two".into(),
            tags: Default::default(),
            project: None,
            metadata: [("ticket".into(), "A-1".into())].into(),
            state: TaskState::Open,
            created: now,
            updated: now,
        };
        view.workspace.tasks.insert(task.id, task.clone());
        (view, task)
    }

    #[test]
    fn task_file_round_trip_and_strictness() {
        let (view, task) = view_with_task();
        let text = render_task_file(&view.task_view(&task));
        let patch = parse_task_file(&text).unwrap();
        assert_eq!(patch.title.as_deref(), Some("t"));
        assert_eq!(patch.description.as_deref(), Some("line one\n\nline two"));
        assert_eq!(patch.metadata.unwrap()["ticket"], "A-1");
        assert_eq!(patch.project, Some(None));
        assert!(parse_task_file("no frontmatter").is_err());
        assert!(parse_task_file("---\ntitle: [unclosed\n---\n").is_err());
        assert!(parse_task_file("---\ntitle: x\nbogus: 1\n---\n").is_err());
        assert!(parse_task_file("---\ntitle: x\nstate: sleeping\n---\n").is_err());
        assert!(parse_task_file("---\ntitle: ''\n---\n").is_err());
    }

    #[test]
    fn table_round_trip_edit_delete_and_add() {
        let (view, task) = view_with_task();
        let tz = Tz::UTC;
        let t = |h, m| Utc.with_ymd_and_hms(2026, 10, 7, h, m, 0).unwrap();
        let a = ops::new_entry(
            task.id,
            t(9, 0),
            Some(t(10, 0)),
            Some("a note".into()),
            t(9, 0),
        );
        let b = ops::new_entry(task.id, t(11, 0), None, None, t(11, 0));
        let original = vec![&a, &b];
        let text = render_table(&view, &original, tz);
        let rows = parse_table(&text, tz).unwrap();
        assert!(diff_table(&view, &original, &rows).unwrap() == TableChanges::default());

        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let a_line = lines.iter().position(|l| l.contains("a note")).unwrap();
        lines[a_line] = lines[a_line].replace("10:00", "10:30");
        lines.retain(|l| !l.contains("  -"));
        lines.push("new  #7  2026-10-07 12:00  2026-10-07 12:15  added".into());
        let rows = parse_table(&lines.join("\n"), tz).unwrap();
        let changes = diff_table(&view, &original, &rows).unwrap();
        assert_eq!(changes.delete, vec![b.id]);
        assert_eq!(changes.update.len(), 1);
        assert_eq!(changes.update[0].1.end, Some(Some(t(10, 30))));
        assert_eq!(changes.create.len(), 1);
        assert_eq!(changes.create[0].note.as_deref(), Some("added"));
        assert!(parse_table("x #7 2026-10-07 12:00 2026-10-07 11:00", tz).is_err());
    }
}
