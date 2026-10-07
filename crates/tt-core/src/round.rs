//! Rounding and flattening for billing-style exports.
//!
//! Each item's duration is rounded to the grid, then items are laid out in
//! order of original start so none overlap: `start = max(own start, previous
//! end)`, `end = start + rounded`. With `task-day` grouping, durations are
//! first summed per (task, local day) and each group starts at its first
//! entry's start.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::CoreError, time::local_date};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoundMode {
    #[default]
    Up,
    Nearest,
}

impl std::str::FromStr for RoundMode {
    type Err = CoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "up" => Ok(Self::Up),
            "nearest" => Ok(Self::Nearest),
            other => Err(CoreError::Invalid(format!(
                "unknown rounding mode {other:?} (up, nearest)"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoundGroup {
    #[default]
    Entry,
    TaskDay,
}

impl std::str::FromStr for RoundGroup {
    type Err = CoreError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "entry" => Ok(Self::Entry),
            "task-day" | "taskday" | "task_day" => Ok(Self::TaskDay),
            other => Err(CoreError::Invalid(format!(
                "unknown rounding group {other:?} (entry, task-day)"
            ))),
        }
    }
}

/// One input interval.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Interval {
    pub task: Uuid,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Source entry ids (one for `entry` grouping).
    pub entries: Vec<Uuid>,
}

/// One output interval.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Rounded {
    pub task: Uuid,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Rounded duration, seconds.
    pub duration: i64,
    /// Original (unrounded) duration, seconds.
    pub original: i64,
    pub entries: Vec<Uuid>,
    /// Local day for `task-day` groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<NaiveDate>,
}

/// Rounds `seconds` to `grid` seconds.
#[must_use]
pub fn round_seconds(seconds: i64, grid: i64, mode: RoundMode) -> i64 {
    if grid <= 0 {
        return seconds;
    }
    match mode {
        RoundMode::Up => (seconds + grid - 1).div_euclid(grid) * grid,
        RoundMode::Nearest => (seconds + grid / 2).div_euclid(grid) * grid,
    }
}

/// (first start, task, seconds, source entries, local day)
type Prepared = (DateTime<Utc>, Uuid, i64, Vec<Uuid>, Option<NaiveDate>);
/// (task, local day) → (first start, seconds, source entries)
type DayGroups = BTreeMap<(Uuid, NaiveDate), (DateTime<Utc>, i64, Vec<Uuid>)>;

/// Rounds and flattens `items`; output order equals input order by start.
#[must_use]
pub fn round_and_flatten(
    items: &[Interval],
    grid: Duration,
    mode: RoundMode,
    group: RoundGroup,
    tz: Tz,
) -> Vec<Rounded> {
    let grid = grid.num_seconds();
    let mut prepared: Vec<Prepared> = match group {
        RoundGroup::Entry => items
            .iter()
            .map(|item| {
                (
                    item.start,
                    item.task,
                    (item.end - item.start).num_seconds().max(0),
                    item.entries.clone(),
                    None,
                )
            })
            .collect(),
        RoundGroup::TaskDay => {
            let mut groups: DayGroups = BTreeMap::new();
            for item in items {
                let key = (item.task, local_date(tz, item.start));
                let slot = groups.entry(key).or_insert((item.start, 0, Vec::new()));
                slot.0 = slot.0.min(item.start);
                slot.1 += (item.end - item.start).num_seconds().max(0);
                slot.2.extend(item.entries.iter().copied());
            }
            groups
                .into_iter()
                .map(|((task, day), (start, seconds, entries))| {
                    (start, task, seconds, entries, Some(day))
                })
                .collect()
        }
    };
    prepared.sort_by_key(|(start, task, ..)| (*start, *task));
    let mut cursor: Option<DateTime<Utc>> = None;
    prepared
        .into_iter()
        .map(|(start, task, original, entries, day)| {
            let duration = round_seconds(original, grid, mode);
            let start = cursor.map_or(start, |cursor| cursor.max(start));
            let end = start + Duration::seconds(duration);
            cursor = Some(end);
            Rounded {
                task,
                start,
                end,
                duration,
                original,
                entries,
                day,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use proptest::prelude::*;

    fn t(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, h, m, 0).unwrap()
    }

    fn item(task: Uuid, start: DateTime<Utc>, end: DateTime<Utc>) -> Interval {
        Interval {
            task,
            start,
            end,
            entries: vec![Uuid::now_v7()],
        }
    }

    #[test]
    fn example_from_the_brief() {
        let task = Uuid::now_v7();
        let out = round_and_flatten(
            &[
                item(task, t(13, 0), t(13, 5)),
                item(task, t(13, 10), t(13, 45)),
            ],
            Duration::minutes(15),
            RoundMode::Up,
            RoundGroup::Entry,
            Tz::UTC,
        );
        let spans: Vec<_> = out.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(spans, vec![(t(13, 0), t(13, 15)), (t(13, 15), t(14, 0))]);
    }

    #[test]
    fn per_task_per_day_merges_before_rounding() {
        let task = Uuid::now_v7();
        let out = round_and_flatten(
            &[
                item(task, t(9, 0), t(9, 5)),
                item(task, t(11, 0), t(11, 5)),
                item(task, t(15, 0), t(15, 5)),
            ],
            Duration::minutes(15),
            RoundMode::Up,
            RoundGroup::TaskDay,
            Tz::UTC,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].duration, 15 * 60);
        assert_eq!(out[0].start, t(9, 0));
        assert_eq!(out[0].entries.len(), 3);
    }

    #[test]
    fn nearest_mode() {
        assert_eq!(round_seconds(7 * 60, 900, RoundMode::Nearest), 0);
        assert_eq!(round_seconds(8 * 60, 900, RoundMode::Nearest), 900);
        assert_eq!(round_seconds(23 * 60, 900, RoundMode::Nearest), 1800);
        assert_eq!(round_seconds(900, 900, RoundMode::Up), 900);
        assert_eq!(round_seconds(901, 900, RoundMode::Up), 1800);
    }

    proptest! {
        #[test]
        fn flatten_properties(
            raw in prop::collection::vec((0_i64..600, 0_i64..240, 0_usize..3), 0..40),
            grid in 1_i64..60,
            nearest in any::<bool>(),
            per_day in any::<bool>(),
        ) {
            let tasks = [Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3)];
            let base = t(0, 0);
            let items: Vec<_> = raw
                .iter()
                .map(|(offset, length, task)| {
                    let start = base + Duration::minutes(*offset);
                    item(tasks[*task], start, start + Duration::minutes(*length))
                })
                .collect();
            let mode = if nearest { RoundMode::Nearest } else { RoundMode::Up };
            let group = if per_day { RoundGroup::TaskDay } else { RoundGroup::Entry };
            let grid = Duration::minutes(grid);
            let out = round_and_flatten(&items, grid, mode, group, Tz::UTC);
            // Never overlap, and order by start is the input order by start.
            for pair in out.windows(2) {
                prop_assert!(pair[0].end <= pair[1].start);
            }
            let mut input_starts: Vec<_> = out.iter().map(|r| r.entries.clone()).collect();
            input_starts.sort_by_key(|entries| {
                items.iter().filter(|i| entries.contains(&i.entries[0])).map(|i| i.start).min()
            });
            prop_assert_eq!(input_starts, out.iter().map(|r| r.entries.clone()).collect::<Vec<_>>());
            for rounded in &out {
                prop_assert!(rounded.duration >= round_seconds(rounded.original, grid.num_seconds(), mode));
                prop_assert_eq!((rounded.end - rounded.start).num_seconds(), rounded.duration);
                let earliest = items
                    .iter()
                    .filter(|i| rounded.entries.contains(&i.entries[0]))
                    .map(|i| i.start)
                    .min()
                    .unwrap();
                prop_assert!(rounded.start >= earliest);
            }
            if !per_day {
                prop_assert_eq!(out.len(), items.len());
            }
        }
    }
}
