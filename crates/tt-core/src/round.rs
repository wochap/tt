//! Rounding and flattening for billing-style exports.
//!
//! Each item's duration is rounded to the duration grid and its start snapped
//! to the start grid on the local wall clock, then items are laid out in
//! order of original start so none overlap: `start = max(snapped start,
//! previous end)`, `end = start + rounded`. With `task-day` grouping, durations are
//! first summed per (task, local day) and each group starts at its first
//! entry's start, snapped.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    time::local_date,
};

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
    /// First raw start of the item.
    pub raw_start: DateTime<Utc>,
    /// Start snapped to the start grid, before the push.
    pub grid_start: DateTime<Utc>,
    /// Local day of `raw_start`.
    pub day: NaiveDate,
    /// Rounded to 0: left out of downloads, does not move the cursor.
    pub dropped: bool,
}

/// Grids and modes for [`round_and_flatten`]; a zero `start_grid` is off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundOptions {
    pub duration_grid: Duration,
    pub mode: RoundMode,
    pub start_grid: Duration,
    pub start_mode: RoundMode,
    pub group: RoundGroup,
}

/// Start grids offered besides off, minutes.
pub const START_GRID_MINUTES: [i64; 5] = [5, 10, 15, 30, 60];

/// Start grids (seconds, 0 = off) allowed with `duration`.
#[must_use]
pub fn valid_start_grids(duration: Duration) -> Vec<i64> {
    let duration = duration.num_seconds();
    std::iter::once(0)
        .chain(
            START_GRID_MINUTES
                .iter()
                .map(|m| m * 60)
                .filter(|g| duration > 0 && duration % g == 0),
        )
        .collect()
}

fn grid_label(seconds: i64) -> String {
    if seconds == 0 {
        "off".to_owned()
    } else {
        format!("{}m", seconds / 60)
    }
}

/// Rejects a start grid that does not divide the duration grid.
pub fn validate_grids(options: &RoundOptions) -> CoreResult<()> {
    let start = options.start_grid.num_seconds();
    let valid = valid_start_grids(options.duration_grid);
    if valid.contains(&start) {
        return Ok(());
    }
    Err(CoreError::Invalid(format!(
        "start grid {} does not divide the duration grid {}; valid start grids: {}",
        if start % 60 == 0 {
            grid_label(start)
        } else {
            format!("{start}s")
        },
        grid_label(options.duration_grid.num_seconds()),
        valid
            .iter()
            .map(|g| grid_label(*g))
            .collect::<Vec<_>>()
            .join(", ")
    )))
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

/// Snaps `at` to a `grid`-second mark of the local wall clock, using the
/// zone offset at `at` (never converting a local time back).
#[must_use]
pub fn snap_start(at: DateTime<Utc>, grid: i64, mode: RoundMode, tz: Tz) -> DateTime<Utc> {
    if grid <= 0 {
        return at;
    }
    let offset = i64::from(
        tz.offset_from_utc_datetime(&at.naive_utc())
            .fix()
            .local_minus_utc(),
    );
    let r = (at.timestamp() + offset).rem_euclid(grid);
    let delta = match mode {
        _ if r == 0 => 0,
        RoundMode::Nearest if 2 * r < grid => -r,
        _ => grid - r,
    };
    at + Duration::seconds(delta)
}

/// (first start, task, seconds, source entries, local day)
type Prepared = (DateTime<Utc>, Uuid, i64, Vec<Uuid>, NaiveDate);
/// (task, local day) → (first start, seconds, source entries)
type DayGroups = BTreeMap<(Uuid, NaiveDate), (DateTime<Utc>, i64, Vec<Uuid>)>;

/// Rounds and flattens `items`; output order equals input order by start.
/// Dropped (0-length) items stay in the output, flagged.
pub fn round_and_flatten(
    items: &[Interval],
    options: &RoundOptions,
    tz: Tz,
) -> CoreResult<Vec<Rounded>> {
    validate_grids(options)?;
    let grid = options.duration_grid.num_seconds();
    let start_grid = options.start_grid.num_seconds();
    let mut prepared: Vec<Prepared> = match options.group {
        RoundGroup::Entry => items
            .iter()
            .map(|item| {
                (
                    item.start,
                    item.task,
                    (item.end - item.start).num_seconds().max(0),
                    item.entries.clone(),
                    local_date(tz, item.start),
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
                    (start, task, seconds, entries, day)
                })
                .collect()
        }
    };
    prepared.sort_by_key(|(start, task, ..)| (*start, *task));
    let mut cursor: Option<DateTime<Utc>> = None;
    Ok(prepared
        .into_iter()
        .map(|(raw_start, task, original, entries, day)| {
            let duration = round_seconds(original, grid, options.mode);
            let grid_start = snap_start(raw_start, start_grid, options.start_mode, tz);
            let dropped = duration == 0;
            let start = if dropped {
                grid_start
            } else {
                cursor.map_or(grid_start, |cursor| cursor.max(grid_start))
            };
            let end = start + Duration::seconds(duration);
            if !dropped {
                cursor = Some(end);
            }
            Rounded {
                task,
                start,
                end,
                duration,
                original,
                entries,
                raw_start,
                grid_start,
                day,
                dropped,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn opts(duration: i64, mode: RoundMode, start: i64, start_mode: RoundMode) -> RoundOptions {
        RoundOptions {
            duration_grid: Duration::minutes(duration),
            mode,
            start_grid: Duration::minutes(start),
            start_mode,
            group: RoundGroup::Entry,
        }
    }

    fn spans(
        items: &[Interval],
        options: &RoundOptions,
        tz: Tz,
    ) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        round_and_flatten(items, options, tz)
            .unwrap()
            .iter()
            .filter(|r| !r.dropped)
            .map(|r| (r.start, r.end))
            .collect()
    }

    use RoundMode::{Nearest, Up};

    #[test]
    fn grid_pairs() {
        assert!(validate_grids(&opts(15, Up, 10, Up)).is_err());
        let error = validate_grids(&opts(15, Up, 10, Up))
            .unwrap_err()
            .to_string();
        assert!(error.contains("off, 5m, 15m"), "{error}");
        assert!(validate_grids(&opts(60, Up, 15, Up)).is_ok());
        assert!(validate_grids(&opts(1, Up, 0, Up)).is_ok());
        assert_eq!(
            valid_start_grids(Duration::minutes(30)),
            vec![0, 300, 600, 900, 1800]
        );
        assert!(round_and_flatten(&[], &opts(15, Up, 10, Up), Tz::UTC).is_err());
    }

    #[test]
    fn example_from_the_brief() {
        let task = Uuid::now_v7();
        let items = [
            item(task, t(13, 0), t(13, 5)),
            item(task, t(13, 10), t(13, 45)),
        ];
        assert_eq!(
            spans(&items, &opts(15, Up, 0, Up), Tz::UTC),
            vec![(t(13, 0), t(13, 15)), (t(13, 15), t(14, 0))]
        );
    }

    #[test]
    fn start_snapped_nearest_and_up() {
        let task = Uuid::now_v7();
        let items = [item(task, t(21, 22), t(21, 37))];
        assert_eq!(
            spans(&items, &opts(15, Nearest, 15, Nearest), Tz::UTC),
            vec![(t(21, 15), t(21, 30))]
        );
        assert_eq!(
            spans(&items, &opts(15, Up, 15, Up), Tz::UTC),
            vec![(t(21, 30), t(21, 45))]
        );
        let half = [item(task, t(21, 22) + Duration::seconds(30), t(21, 37))];
        let out = round_and_flatten(&half, &opts(15, Nearest, 15, Nearest), Tz::UTC).unwrap();
        assert_eq!(out[0].grid_start, t(21, 30));
    }

    #[test]
    fn large_duration_small_start() {
        let task = Uuid::now_v7();
        let items = [item(task, t(9, 5), t(9, 20))];
        assert_eq!(
            spans(&items, &opts(60, Up, 15, Up), Tz::UTC),
            vec![(t(9, 15), t(10, 15))]
        );
    }

    #[test]
    fn snapped_start_pushed() {
        let task = Uuid::now_v7();
        let items = [
            item(task, t(9, 0), t(9, 55)),
            item(task, t(9, 50), t(10, 5)),
        ];
        let out = round_and_flatten(&items, &opts(15, Nearest, 15, Nearest), Tz::UTC).unwrap();
        assert_eq!(
            out.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>(),
            vec![(t(9, 0), t(10, 0)), (t(10, 0), t(10, 15))]
        );
        assert_eq!(out[1].grid_start, t(9, 45));
    }

    #[test]
    fn push_carries_across_midnight() {
        let task = Uuid::now_v7();
        let wed = |h, m| t(h, m) + Duration::days(1);
        let items = [
            item(task, t(23, 55), wed(0, 20)),
            item(task, wed(0, 10), wed(0, 40)),
        ];
        let out = round_and_flatten(&items, &opts(15, Nearest, 15, Nearest), Tz::UTC).unwrap();
        assert_eq!((out[0].start, out[0].end), (wed(0, 0), wed(0, 30)));
        assert_eq!(out[0].day, NaiveDate::from_ymd_opt(2026, 10, 7).unwrap());
        assert_eq!((out[1].start, out[1].end), (wed(0, 30), wed(1, 0)));
        assert_eq!(out[1].day, NaiveDate::from_ymd_opt(2026, 10, 8).unwrap());
    }

    #[test]
    fn zero_minute_item_dropped() {
        let task = Uuid::now_v7();
        let items = [
            item(task, t(21, 7), t(21, 11)),
            item(task, t(21, 12), t(21, 30)),
        ];
        let out = round_and_flatten(&items, &opts(15, Nearest, 0, Nearest), Tz::UTC).unwrap();
        assert!(out[0].dropped);
        assert_eq!(out[0].start, out[0].end);
        assert!(!out[1].dropped);
        assert_eq!(out[1].start, t(21, 12));
    }

    #[test]
    fn half_hour_offset_zone() {
        let tz: Tz = "Asia/Kolkata".parse().unwrap();
        let task = Uuid::now_v7();
        // 09:20 IST = 03:50 UTC.
        let items = [item(task, t(3, 50), t(4, 50))];
        let out = round_and_flatten(&items, &opts(60, Nearest, 60, Nearest), tz).unwrap();
        assert_eq!(out[0].start, t(3, 30));
    }

    #[test]
    fn per_task_per_day_merges_before_rounding() {
        let task = Uuid::now_v7();
        let items = [
            item(task, t(9, 5), t(9, 10)),
            item(task, t(13, 10), t(13, 15)),
            item(task, t(17, 20), t(17, 25)),
        ];
        let options = RoundOptions {
            group: RoundGroup::TaskDay,
            ..opts(15, Up, 15, Up)
        };
        let out = round_and_flatten(&items, &options, Tz::UTC).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].duration, 15 * 60);
        assert_eq!((out[0].start, out[0].end), (t(9, 15), t(9, 30)));
        assert_eq!(out[0].raw_start, t(9, 5));
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

    fn on_local_grid(at: DateTime<Utc>, grid: i64, tz: Tz) -> bool {
        snap_start(at, grid, RoundMode::Up, tz) == at
    }

    proptest! {
        #[test]
        fn flatten_properties(
            raw in prop::collection::vec((0_i64..3000, 0_i64..240, 0_usize..3, 0_i64..60), 0..40),
            duration in prop::sample::select(vec![1_i64, 5, 10, 15, 30, 45, 60, 90, 120]),
            start_pick in 0_usize..6,
            nearest in any::<bool>(),
            start_nearest in any::<bool>(),
            per_day in any::<bool>(),
            kolkata in any::<bool>(),
        ) {
            let tasks = [Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3)];
            let tz: Tz = if kolkata { "Asia/Kolkata".parse().unwrap() } else { Tz::UTC };
            let base = t(0, 0);
            let items: Vec<_> = raw
                .iter()
                .map(|(offset, length, task, secs)| {
                    let start = base + Duration::minutes(*offset) + Duration::seconds(*secs);
                    item(tasks[*task], start, start + Duration::minutes(*length))
                })
                .collect();
            let valid = valid_start_grids(Duration::minutes(duration));
            let start_grid = valid[start_pick % valid.len()];
            let options = RoundOptions {
                duration_grid: Duration::minutes(duration),
                mode: if nearest { Nearest } else { Up },
                start_grid: Duration::seconds(start_grid),
                start_mode: if start_nearest { Nearest } else { Up },
                group: if per_day { RoundGroup::TaskDay } else { RoundGroup::Entry },
            };
            let out = round_and_flatten(&items, &options, tz).unwrap();
            let kept: Vec<_> = out.iter().filter(|r| !r.dropped).collect();
            for pair in kept.windows(2) {
                prop_assert!(pair[0].end <= pair[1].start);
            }
            // Order by start is the input order by start.
            for pair in out.windows(2) {
                prop_assert!(pair[0].raw_start <= pair[1].raw_start);
            }
            for rounded in &out {
                prop_assert_eq!(rounded.duration, round_seconds(rounded.original, duration * 60, options.mode));
                prop_assert_eq!((rounded.end - rounded.start).num_seconds(), rounded.duration);
                prop_assert_eq!(rounded.dropped, rounded.duration == 0);
                prop_assert_eq!(rounded.day, local_date(tz, rounded.raw_start));
                if start_grid > 0 {
                    prop_assert!(on_local_grid(rounded.start, start_grid, tz));
                    prop_assert!(on_local_grid(rounded.end, start_grid, tz));
                }
            }
            if !per_day {
                prop_assert_eq!(out.len(), items.len());
            }
        }
    }
}
