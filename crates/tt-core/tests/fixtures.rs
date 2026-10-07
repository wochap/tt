//! Shared fixtures under `fixtures/` at the repository root. The TypeScript
//! domain package (`packages/domain/test/fixtures.test.ts`) runs the same
//! cases, so the two implementations cannot drift.
//!
//! `TT_FIXTURES_BLESS=1 cargo test -p tt-core --test fixtures` rewrites every
//! `expected` field from this (reference) implementation.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Duration, Utc, Weekday};
use chrono_tz::Tz;
use serde_json::{Value, json};
use tt_core::{
    Entry, Project, Tag, Task, View, Workspace,
    report::{GroupBy, report, union_seconds},
    round::{Interval, RoundGroup, RoundMode, round_and_flatten},
    text::parse_quick,
    time::{Range, parse_duration, parse_range, parse_time},
};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn bless() -> bool {
    std::env::var_os("TT_FIXTURES_BLESS").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Runs `compute` over every case of `file`, comparing (or blessing) `expected`.
fn run(file: &str, compute: impl Fn(&Value) -> Value) {
    let path = fixtures_dir().join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
    let cases = doc["cases"].as_array_mut().expect("cases array");
    let mut failures = Vec::new();
    for case in cases.iter_mut() {
        let actual = compute(case);
        if bless() {
            case["expected"] = actual;
        } else if case["expected"] != actual {
            failures.push(format!(
                "{file} / {}:\n  expected {}\n  actual   {}",
                case["name"], case["expected"], actual
            ));
        }
    }
    if bless() {
        let mut out = serde_json::to_string_pretty(&doc).expect("serialize");
        out.push('\n');
        std::fs::write(&path, out).expect("write fixture");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn time(value: &Value) -> DateTime<Utc> {
    value
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| panic!("not a timestamp: {value}"))
}

fn tz(value: &Value) -> Tz {
    value.as_str().unwrap_or("UTC").parse().expect("time zone")
}

fn from<T: serde::de::DeserializeOwned>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
}

#[test]
fn union() {
    run("union.json", |case| {
        let intervals: Vec<_> = case["intervals"]
            .as_array()
            .expect("intervals")
            .iter()
            .map(|pair| (time(&pair[0]), time(&pair[1])))
            .collect();
        json!(union_seconds(&intervals))
    });
}

#[test]
fn round() {
    run("round.json", |case| {
        let items: Vec<Interval> = from(&case["items"]);
        let mode: RoundMode = case["mode"].as_str().expect("mode").parse().expect("mode");
        let group: RoundGroup = case["group"]
            .as_str()
            .expect("group")
            .parse()
            .expect("group");
        let grid = Duration::minutes(case["grid_minutes"].as_i64().expect("grid_minutes"));
        json!(round_and_flatten(
            &items,
            grid,
            mode,
            group,
            tz(&case["tz"])
        ))
    });
}

fn view(value: &Value) -> View {
    let projects: Vec<Project> = from(&value["projects"]);
    let tags: Vec<Tag> = from(&value["tags"]);
    let tasks: Vec<Task> = from(&value["tasks"]);
    let entries: Vec<Entry> = from(&value["entries"]);
    View {
        workspace: Workspace {
            projects: projects.into_iter().map(|p| (p.id, p)).collect(),
            tags: tags.into_iter().map(|t| (t.id, t)).collect(),
            tasks: tasks.into_iter().map(|t| (t.id, t)).collect(),
            ..Workspace::default()
        },
        entries: entries
            .into_iter()
            .map(|e| (e.id, e))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn reports() {
    let path = fixtures_dir().join("report.json");
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json");
    let view = view(&doc["view"]);
    run("report.json", |case| {
        let range = Range {
            from: time(&case["range"]["from"]),
            to: time(&case["range"]["to"]),
        };
        let group_by: GroupBy = case["group_by"]
            .as_str()
            .expect("group_by")
            .parse()
            .expect("group_by");
        json!(report(
            &view,
            range,
            group_by,
            tz(&case["tz"]),
            time(&case["now"])
        ))
    });
}

#[test]
fn quick() {
    run("quick.json", |case| {
        let args: Vec<String> = from(&case["args"]);
        let task = parse_quick(&args);
        json!({
            "title": task.title,
            "tags": task.tags,
            "project": task.project,
            "metadata": task.metadata,
        })
    });
}

fn weekday(value: &Value) -> Weekday {
    value.as_str().unwrap_or("mon").parse().expect("weekday")
}

#[test]
fn times() {
    run("time.json", |case| {
        let now = time(&case["now"]);
        let tz = tz(&case["tz"]);
        let input = case["input"].as_str().expect("input");
        // Error messages differ between the implementations; only failure is shared.
        let fail = |_: tt_core::CoreError| json!("error");
        match case["kind"].as_str().expect("kind") {
            "duration" => parse_duration(input).map_or_else(fail, |d| json!(d.num_seconds())),
            "time" => parse_time(input, now, tz).map_or_else(fail, |t| json!(t)),
            "range" => parse_range(input, now, tz, weekday(&case["week_start"]))
                .map_or_else(fail, |r| json!({ "from": r.from, "to": r.to })),
            other => panic!("unknown kind {other}"),
        }
    });
}
