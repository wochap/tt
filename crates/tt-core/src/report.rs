//! Summaries: per-group durations plus summed and wall-clock totals.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::{
    error::CoreError,
    model::{Entry, View},
    time::{Range, local_date, start_of_day},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupBy {
    #[default]
    Task,
    Tag,
    Project,
    Day,
}

impl std::str::FromStr for GroupBy {
    type Err = CoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "task" => Ok(Self::Task),
            "tag" => Ok(Self::Tag),
            "project" => Ok(Self::Project),
            "day" => Ok(Self::Day),
            other => Err(CoreError::Invalid(format!(
                "unknown grouping {other:?} (task, tag, project, day)"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportGroup {
    /// Stable key: task/tag/project uuid, `YYYY-MM-DD`, or `none`.
    pub key: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// Σ durations in seconds.
    pub summed: i64,
    /// Union of intervals in seconds.
    pub wall: i64,
    pub entries: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub group_by: GroupBy,
    pub groups: Vec<ReportGroup>,
    /// Σ of every clipped entry duration, seconds.
    pub summed: i64,
    /// Length of the union of every clipped interval, seconds.
    pub wall: i64,
}

/// Total length in seconds of the union of `[start, end)` intervals.
#[must_use]
pub fn union_seconds(intervals: &[(DateTime<Utc>, DateTime<Utc>)]) -> i64 {
    let mut sorted: Vec<_> = intervals.iter().filter(|(s, e)| e > s).copied().collect();
    sorted.sort();
    let mut total = 0;
    let mut current: Option<(DateTime<Utc>, DateTime<Utc>)> = None;
    for (start, end) in sorted {
        match &mut current {
            Some((_, current_end)) if start <= *current_end => {
                if end > *current_end {
                    *current_end = end;
                }
            }
            _ => {
                if let Some((s, e)) = current {
                    total += (e - s).num_seconds();
                }
                current = Some((start, end));
            }
        }
    }
    if let Some((s, e)) = current {
        total += (e - s).num_seconds();
    }
    total
}

/// Entry clipped to `range`, with running entries ending at `now`.
#[must_use]
pub fn clip(
    entry: &Entry,
    range: &Range,
    now: DateTime<Utc>,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let end = entry.end.unwrap_or(now);
    let start = entry.start.max(range.from);
    let end = end.min(range.to);
    (end > start).then_some((start, end))
}

/// Splits an interval at local midnights.
fn split_days(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    tz: Tz,
) -> Vec<(NaiveDate, DateTime<Utc>, DateTime<Utc>)> {
    let mut parts = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let day = local_date(tz, cursor);
        let next = start_of_day(tz, day + Duration::days(1)).min(end);
        parts.push((day, cursor, next));
        cursor = next;
    }
    parts
}

/// Builds a report over every entry overlapping `range`.
#[must_use]
pub fn report(view: &View, range: Range, group_by: GroupBy, tz: Tz, now: DateTime<Utc>) -> Report {
    type Bucket = (String, Option<u64>, Vec<(DateTime<Utc>, DateTime<Utc>)>);
    let mut groups: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut all = Vec::new();
    let mut add = |key: String, label: String, seq: Option<u64>, interval| {
        groups
            .entry(key)
            .or_insert_with(|| (label, seq, Vec::new()))
            .2
            .push(interval);
    };
    for entry in view.entries.values() {
        let Some(interval) = clip(entry, &range, now) else {
            continue;
        };
        all.push(interval);
        let task = view.workspace.tasks.get(&entry.task);
        match group_by {
            GroupBy::Task => match task {
                Some(task) => add(
                    task.id.to_string(),
                    task.title.clone(),
                    Some(task.seq),
                    interval,
                ),
                None => add("none".into(), "(deleted task)".into(), None, interval),
            },
            GroupBy::Tag => {
                let tags: Vec<_> = task
                    .map(|task| {
                        task.tags
                            .iter()
                            .filter_map(|id| view.workspace.tags.get(id))
                            .collect()
                    })
                    .unwrap_or_default();
                if tags.is_empty() {
                    add("none".into(), "(no tag)".into(), None, interval);
                }
                for tag in tags {
                    add(tag.id.to_string(), tag.name.clone(), None, interval);
                }
            }
            GroupBy::Project => match task
                .and_then(|task| task.project)
                .and_then(|id| view.workspace.projects.get(&id))
            {
                Some(project) => add(project.id.to_string(), project.name.clone(), None, interval),
                None => add("none".into(), "(no project)".into(), None, interval),
            },
            GroupBy::Day => {
                for (day, start, end) in split_days(interval.0, interval.1, tz) {
                    let key = day.format("%Y-%m-%d").to_string();
                    add(key.clone(), key, None, (start, end));
                }
            }
        }
    }
    let mut groups: Vec<ReportGroup> = groups
        .into_iter()
        .map(|(key, (label, seq, intervals))| ReportGroup {
            summed: intervals.iter().map(|(s, e)| (*e - *s).num_seconds()).sum(),
            wall: union_seconds(&intervals),
            entries: intervals.len(),
            key,
            label,
            seq,
        })
        .collect();
    if group_by == GroupBy::Day {
        groups.sort_by(|a, b| a.key.cmp(&b.key));
    } else {
        groups.sort_by(|a, b| b.summed.cmp(&a.summed).then_with(|| a.label.cmp(&b.label)));
    }
    Report {
        from: range.from,
        to: range.to,
        group_by,
        groups,
        summed: all.iter().map(|(s, e)| (*e - *s).num_seconds()).sum(),
        wall: union_seconds(&all),
    }
}

/// `1h05m`, `25m`, `0m`.
#[must_use]
pub fn format_seconds(seconds: i64) -> String {
    let minutes = seconds / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Tag, Task, TaskState},
        ops,
    };
    use chrono::TimeZone;
    use uuid::Uuid;

    fn t(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, h, m, 0).unwrap()
    }

    fn task(view: &mut View, seq: u64, tags: &[Uuid]) -> Uuid {
        let task = Task {
            id: Uuid::now_v7(),
            seq,
            title: format!("t{seq}"),
            description: String::new(),
            tags: tags.iter().copied().collect(),
            project: None,
            metadata: Default::default(),
            state: TaskState::Open,
            previous_seqs: Vec::new(),
            created: t(0, 0),
            updated: t(0, 0),
        };
        let id = task.id;
        view.workspace.tasks.insert(id, task);
        id
    }

    fn entry(view: &mut View, task: Uuid, start: DateTime<Utc>, end: Option<DateTime<Utc>>) {
        let entry = ops::new_entry(task, start, end, None, start);
        view.entries.insert(entry.id, entry);
    }

    fn day() -> Range {
        Range {
            from: t(0, 0),
            to: t(23, 59),
        }
    }

    #[test]
    fn overlap_makes_summed_exceed_wall_by_overlap() {
        let mut view = View::default();
        let a = task(&mut view, 1, &[]);
        let b = task(&mut view, 2, &[]);
        entry(&mut view, a, t(13, 0), Some(t(13, 30)));
        entry(&mut view, b, t(13, 20), Some(t(13, 50)));
        let report = report(&view, day(), GroupBy::Task, Tz::UTC, t(20, 0));
        assert_eq!(report.summed - report.wall, 600);
        assert_eq!(report.groups.len(), 2);
    }

    #[test]
    fn concurrent_timers_stop_all_durations() {
        let mut view = View::default();
        let a = task(&mut view, 20, &[]);
        let b = task(&mut view, 30, &[]);
        entry(&mut view, a, t(9, 0), Some(t(9, 25)));
        entry(&mut view, b, t(9, 20), Some(t(9, 25)));
        let report = report(&view, day(), GroupBy::Task, Tz::UTC, t(20, 0));
        let by_seq: BTreeMap<_, _> = report
            .groups
            .iter()
            .map(|group| (group.seq.unwrap(), group.summed))
            .collect();
        assert_eq!(by_seq[&20], 25 * 60);
        assert_eq!(by_seq[&30], 5 * 60);
    }

    #[test]
    fn tag_grouping_and_day_splitting_and_clipping() {
        let mut view = View::default();
        let work = Uuid::now_v7();
        view.workspace.tags.insert(
            work,
            Tag {
                id: work,
                name: "work".into(),
                color: None,
                created: t(0, 0),
                updated: t(0, 0),
            },
        );
        let a = task(&mut view, 1, &[work]);
        let b = task(&mut view, 2, &[]);
        entry(&mut view, a, t(10, 0), Some(t(11, 0)));
        entry(&mut view, b, t(12, 0), None);
        let by_tag = report(&view, day(), GroupBy::Tag, Tz::UTC, t(12, 30));
        let labels: Vec<_> = by_tag.groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["work", "(no tag)"]);
        assert_eq!(by_tag.summed, 3600 + 1800, "running entry counts up to now");

        let mut view = View::default();
        let a = task(&mut view, 1, &[]);
        entry(
            &mut view,
            a,
            Utc.with_ymd_and_hms(2026, 10, 6, 23, 0, 0).unwrap(),
            Some(t(1, 0)),
        );
        let range = Range {
            from: Utc.with_ymd_and_hms(2026, 10, 6, 0, 0, 0).unwrap(),
            to: t(23, 0),
        };
        let by_day = report(&view, range, GroupBy::Day, Tz::UTC, t(20, 0));
        let days: Vec<_> = by_day
            .groups
            .iter()
            .map(|g| (g.key.as_str(), g.summed))
            .collect();
        assert_eq!(days, vec![("2026-10-06", 3600), ("2026-10-07", 3600)]);
        let clipped = report(&view, day(), GroupBy::Task, Tz::UTC, t(20, 0));
        assert_eq!(clipped.summed, 3600);
    }

    #[test]
    fn union_handles_nesting_and_gaps() {
        assert_eq!(
            union_seconds(&[
                (t(1, 0), t(2, 0)),
                (t(1, 10), t(1, 20)),
                (t(3, 0), t(3, 30))
            ]),
            5400
        );
        assert_eq!(union_seconds(&[]), 0);
        assert_eq!(format_seconds(3900), "1h05m");
        assert_eq!(format_seconds(1500), "25m");
    }
}
