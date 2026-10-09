//! Human-readable rendering of daemon results.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use tt_core::report::format_seconds;

fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn local(tz: Tz, value: &Value) -> Option<DateTime<Tz>> {
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&Utc).with_timezone(&tz))
}

#[must_use]
pub fn time(tz: Tz, value: &Value) -> String {
    local(tz, value).map_or_else(|| "-".into(), |t| t.format("%Y-%m-%d %H:%M").to_string())
}

/// `#12 Fix login @web +backend`
#[must_use]
pub fn task_label(task: &Value) -> String {
    let mut out = format!("#{} {}", task["seq"], s(task, "title"));
    if let Some(project) = task.get("project").filter(|p| !p.is_null()) {
        out.push_str(&format!(" @{}", s(project, "name")));
    }
    for tag in task["tags"].as_array().into_iter().flatten() {
        out.push_str(&format!(" +{}", s(tag, "name")));
    }
    out
}

#[must_use]
pub fn task_line(task: &Value) -> String {
    let mut out = format!(
        "{:<5} {:<8} {}",
        format!("#{}", task["seq"]),
        s(task, "state"),
        s(task, "title")
    );
    if let Some(project) = task.get("project").filter(|p| !p.is_null()) {
        out.push_str(&format!("  @{}", s(project, "name")));
    }
    for tag in task["tags"].as_array().into_iter().flatten() {
        out.push_str(&format!(" +{}", s(tag, "name")));
    }
    if let Some(metadata) = task["metadata"].as_object() {
        for (key, value) in metadata {
            out.push_str(&format!("  {key}={}", value.as_str().unwrap_or_default()));
        }
    }
    out
}

#[must_use]
pub fn task_detail(task: &Value, tz: Tz) -> String {
    let mut out = format!("{}\n", task_line(task));
    out.push_str(&format!("id:      {}\n", s(task, "id")));
    out.push_str(&format!("created: {}\n", time(tz, &task["created"])));
    out.push_str(&format!("updated: {}\n", time(tz, &task["updated"])));
    if let Some(seqs) = task["previous_seqs"].as_array().filter(|s| !s.is_empty()) {
        let seqs: Vec<String> = seqs.iter().map(|seq| format!("#{seq}")).collect();
        out.push_str(&format!("previously: {}\n", seqs.join(", ")));
    }
    for hint in task["renumbered_from"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "note: {:?} was renumbered from #{} to #{}\n",
            s(hint, "title"),
            hint["from"],
            hint["to"]
        ));
    }
    let description = s(task, "description");
    if !description.is_empty() {
        out.push_str(&format!("\n{description}\n"));
    }
    if let Some(entries) = task["entries"].as_array().filter(|e| !e.is_empty()) {
        out.push('\n');
        for entry in entries {
            out.push_str(&entry_line(entry, tz));
            out.push('\n');
        }
        out.push_str(&format!(
            "total: {}\n",
            format_seconds(task["total"].as_i64().unwrap_or(0))
        ));
    }
    out
}

#[must_use]
pub fn entry_line(entry: &Value, tz: Tz) -> String {
    let id = entry
        .get("short_id")
        .and_then(Value::as_str)
        .map_or_else(|| s(entry, "id").chars().take(8).collect(), str::to_owned);
    let start = local(tz, &entry["start"]);
    let end = local(tz, &entry["end"]);
    let span = match (start, end) {
        (Some(start), Some(end)) if start.date_naive() == end.date_naive() => {
            format!(
                "{} – {}",
                start.format("%Y-%m-%d %H:%M"),
                end.format("%H:%M")
            )
        }
        (Some(start), Some(end)) => format!(
            "{} – {}",
            start.format("%Y-%m-%d %H:%M"),
            end.format("%Y-%m-%d %H:%M")
        ),
        (Some(start), None) => format!("{} – running", start.format("%Y-%m-%d %H:%M")),
        _ => "-".into(),
    };
    let task = entry
        .get("task")
        .filter(|t| !t.is_null())
        .map_or_else(|| format!("(task {})", s(entry, "task_id")), task_label);
    let mut out = format!(
        "{id}  {span}  {:>7}  {task}",
        format_seconds(entry["duration"].as_i64().unwrap_or(0))
    );
    if let Some(note) = entry.get("note").and_then(Value::as_str) {
        out.push_str(&format!("  — {note}"));
    }
    out
}

#[must_use]
pub fn report(report: &Value, tz: Tz) -> String {
    let mut out = format!(
        "{} → {}  (by {})\n",
        time(tz, &report["from"]),
        time(tz, &report["to"]),
        s(report, "group_by")
    );
    for group in report["groups"].as_array().into_iter().flatten() {
        let label = match group.get("seq").and_then(Value::as_u64) {
            Some(seq) => format!("#{seq} {}", s(group, "label")),
            None => s(group, "label").to_owned(),
        };
        out.push_str(&format!(
            "{:>8}  {label}\n",
            format_seconds(group["summed"].as_i64().unwrap_or(0))
        ));
    }
    out.push_str(&format!(
        "summed {}  wall {}\n",
        format_seconds(report["summed"].as_i64().unwrap_or(0)),
        format_seconds(report["wall"].as_i64().unwrap_or(0))
    ));
    out
}

#[must_use]
pub fn named_line(value: &Value) -> String {
    let mut out = s(value, "name").to_owned();
    if let Some(color) = value.get("color").and_then(Value::as_str) {
        out.push_str(&format!("  {color}"));
    }
    if value.get("archived").and_then(Value::as_bool) == Some(true) {
        out.push_str("  (archived)");
    }
    out
}

#[must_use]
pub fn status(status: &Value, tz: Tz) -> String {
    let daemon = &status["daemon"];
    let sync = &status["sync"];
    let mut out = format!(
        "daemon:  running (pid {}, {}, up {}s)\n",
        daemon["pid"],
        match &daemon["state"] {
            Value::String(state) => state.clone(),
            other => other.to_string(),
        },
        daemon["uptime_seconds"]
    );
    out.push_str(&format!("socket:  {}\n", s(daemon, "socket")));
    out.push_str(&format!("log:     {}\n", s(daemon, "log")));
    if sync["state"] == "login_required" {
        out.push_str(&format!(
            "sync:    login required ({}): run `tt login {}`\n",
            s(sync, "url"),
            s(sync, "url")
        ));
    } else if sync["state"] == "ca_cert_invalid" {
        out.push_str(&format!(
            "sync:    stopped, server.ca_cert invalid: {}\n",
            s(&sync["detail"], "error")
        ));
    } else if sync["configured"].as_bool() == Some(true) {
        out.push_str(&format!(
            "sync:    {} ({})",
            s(sync, "state"),
            s(sync, "url")
        ));
        if !sync["detail"].is_null() {
            out.push_str(&format!(" {}", sync["detail"]));
        }
        out.push('\n');
    } else {
        out.push_str("sync:    offline (no server configured)\n");
    }
    let workspace = &status["workspace"];
    if !workspace.is_null() {
        out.push_str(&format!("index:   {}\n", s(workspace, "index_doc")));
        let running = workspace["running"].as_array().cloned().unwrap_or_default();
        if running.is_empty() {
            out.push_str("running: nothing\n");
        } else {
            out.push_str("running:\n");
            for entry in &running {
                out.push_str(&format!("  {}\n", entry_line(entry, tz)));
            }
        }
    }
    out
}
