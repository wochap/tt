//! Time, duration, and range parsing in the configured time zone.

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
    Weekday,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// Half-open interval `[from, to)` in UTC.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

impl Range {
    #[must_use]
    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.from <= at && at < self.to
    }
    /// Whether `[start, end)` overlaps this range.
    #[must_use]
    pub fn overlaps(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
        start < self.to && end > self.from
    }
}

fn invalid(input: &str, what: &str) -> CoreError {
    CoreError::Invalid(format!("cannot parse {what} {input:?}"))
}

/// Resolves a local wall-clock time, taking the earlier instant on DST
/// overlap and skipping forward over a DST gap.
#[must_use]
pub fn local_to_utc(tz: Tz, local: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => value.with_timezone(&Utc),
        LocalResult::None => {
            let shifted = local + Duration::hours(1);
            tz.from_local_datetime(&shifted)
                .earliest()
                .map_or_else(|| Utc.from_utc_datetime(&local), |v| v.with_timezone(&Utc))
        }
    }
}

/// Local midnight at the start of `date`.
#[must_use]
pub fn start_of_day(tz: Tz, date: NaiveDate) -> DateTime<Utc> {
    local_to_utc(tz, date.and_time(NaiveTime::MIN))
}

#[must_use]
pub fn local_date(tz: Tz, at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&tz).date_naive()
}

/// Parses a duration: `90` (minutes), `15m`, `1h30m`, `2h`, `45s`, `1d`.
pub fn parse_duration(input: &str) -> CoreResult<Duration> {
    let text = input.trim().to_ascii_lowercase();
    if text.is_empty() {
        return Err(invalid(input, "duration"));
    }
    if let Ok(minutes) = text.parse::<i64>() {
        return Ok(Duration::minutes(minutes));
    }
    let mut total = Duration::zero();
    let mut number = String::new();
    let mut any = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        let value: i64 = number.parse().map_err(|_| invalid(input, "duration"))?;
        number.clear();
        total += match ch {
            'd' => Duration::days(value),
            'h' => Duration::hours(value),
            'm' => Duration::minutes(value),
            's' => Duration::seconds(value),
            _ => return Err(invalid(input, "duration")),
        };
        any = true;
    }
    if !number.is_empty() || !any {
        return Err(invalid(input, "duration"));
    }
    Ok(total)
}

fn parse_clock(input: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(input, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(input, "%H:%M"))
        .ok()
}

fn parse_date(input: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d").ok()
}

/// Parses a point in time:
/// `now`, `-30m`/`+1h` (relative to now), `13:00` (today), `yesterday 13:00`,
/// `today 9:15`, `tomorrow 8:00`, `2026-10-07`, `2026-10-07 13:00`,
/// `2026-10-07T13:00[:SS]`, or RFC 3339 with an offset.
pub fn parse_time(input: &str, now: DateTime<Utc>, tz: Tz) -> CoreResult<DateTime<Utc>> {
    let text = input.trim();
    let lower = text.to_ascii_lowercase();
    if lower == "now" {
        return Ok(now);
    }
    if let Some(rest) = lower.strip_prefix('-') {
        return Ok(now - parse_duration(rest)?);
    }
    if let Some(rest) = lower.strip_prefix('+') {
        return Ok(now + parse_duration(rest)?);
    }
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Ok(value.with_timezone(&Utc));
    }
    let today = local_date(tz, now);
    let mut parts = text.split_whitespace();
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if parts.next().is_some() {
        return Err(invalid(input, "time"));
    }
    let day = match first.to_ascii_lowercase().as_str() {
        "today" => Some(today),
        "yesterday" => today.pred_opt(),
        "tomorrow" => today.succ_opt(),
        _ => parse_date(first),
    };
    if let Some(day) = day {
        let clock = match second {
            Some(clock) => parse_clock(clock).ok_or_else(|| invalid(input, "time"))?,
            None => NaiveTime::MIN,
        };
        return Ok(local_to_utc(tz, day.and_time(clock)));
    }
    if second.is_none() {
        if let Some(clock) = parse_clock(first) {
            return Ok(local_to_utc(tz, today.and_time(clock)));
        }
        for format in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
            if let Ok(local) = NaiveDateTime::parse_from_str(&first.replace('t', "T"), format) {
                return Ok(local_to_utc(tz, local));
            }
        }
    }
    Err(invalid(input, "time"))
}

fn week_start_date(date: NaiveDate, week_start: Weekday) -> NaiveDate {
    let offset =
        (7 + date.weekday().num_days_from_monday() - week_start.num_days_from_monday()) % 7;
    date - Duration::days(i64::from(offset))
}

fn month_start(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("first of month")
}

fn next_month(date: NaiveDate) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1).expect("first of month")
}

fn day_range(tz: Tz, from: NaiveDate, to_exclusive: NaiveDate) -> Range {
    Range {
        from: start_of_day(tz, from),
        to: start_of_day(tz, to_exclusive),
    }
}

/// Parses `from` of a `from..to` range bound: a date means its midnight; an
/// end-side date means the end of that day.
fn parse_bound(input: &str, now: DateTime<Utc>, tz: Tz, end: bool) -> CoreResult<DateTime<Utc>> {
    let text = input.trim();
    if let Some(date) = parse_date(text) {
        let date = if end {
            date.succ_opt().ok_or_else(|| invalid(input, "date"))?
        } else {
            date
        };
        return Ok(start_of_day(tz, date));
    }
    match text.to_ascii_lowercase().as_str() {
        "today" if end => return Ok(start_of_day(tz, local_date(tz, now) + Duration::days(1))),
        "yesterday" if end => return Ok(start_of_day(tz, local_date(tz, now))),
        _ => {}
    }
    parse_time(text, now, tz)
}

/// Parses a range: `today`/`day`, `yesterday`, `week`, `lastweek`, `month`,
/// `lastmonth`, `year`, `all`, a single `YYYY-MM-DD`, or `<from>..<to>`
/// where either side is a date or a time (`2026-10-01..2026-10-07T12:00`).
/// An empty side means unbounded start or now.
pub fn parse_range(
    input: &str,
    now: DateTime<Utc>,
    tz: Tz,
    week_start: Weekday,
) -> CoreResult<Range> {
    let text = input.trim();
    let today = local_date(tz, now);
    let one_day = Duration::days(1);
    let range = match text.to_ascii_lowercase().as_str() {
        "today" | "day" => day_range(tz, today, today + one_day),
        "yesterday" => day_range(tz, today - one_day, today),
        "week" => {
            let start = week_start_date(today, week_start);
            day_range(tz, start, start + Duration::days(7))
        }
        "lastweek" | "last-week" => {
            let start = week_start_date(today, week_start) - Duration::days(7);
            day_range(tz, start, start + Duration::days(7))
        }
        "month" => {
            let start = month_start(today);
            day_range(tz, start, next_month(start))
        }
        "lastmonth" | "last-month" => {
            let this = month_start(today);
            let start = month_start(this - one_day);
            day_range(tz, start, this)
        }
        "year" => {
            let start = NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("jan 1");
            let end = NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).expect("jan 1");
            day_range(tz, start, end)
        }
        "all" => Range {
            from: DateTime::<Utc>::MIN_UTC,
            to: DateTime::<Utc>::MAX_UTC,
        },
        _ => {
            if let Some((from, to)) = text.split_once("..") {
                let from = if from.trim().is_empty() {
                    DateTime::<Utc>::MIN_UTC
                } else {
                    parse_bound(from, now, tz, false)?
                };
                let to = if to.trim().is_empty() {
                    now
                } else {
                    parse_bound(to, now, tz, true)?
                };
                Range { from, to }
            } else if let Some(date) = parse_date(text) {
                day_range(tz, date, date + one_day)
            } else {
                return Err(invalid(input, "range"));
            }
        }
    };
    if range.from >= range.to {
        return Err(CoreError::Invalid(format!("range {input:?} is empty")));
    }
    Ok(range)
}

/// Parses `mon`..`sun` (or full names).
pub fn parse_weekday(input: &str) -> CoreResult<Weekday> {
    input
        .trim()
        .parse::<Weekday>()
        .map_err(|_| invalid(input, "weekday"))
}

/// Resolves a time zone name, `local`/empty meaning the system zone (via
/// `TZ` or the platform), falling back to UTC.
#[must_use]
pub fn resolve_tz(name: Option<&str>, system: Option<&str>) -> Tz {
    let pick = |value: &str| value.trim().trim_start_matches(':').parse::<Tz>().ok();
    match name.map(str::trim) {
        Some(value) if !value.is_empty() && value != "local" => pick(value),
        _ => None,
    }
    .or_else(|| system.and_then(pick))
    .unwrap_or(Tz::UTC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Berlin;

    fn now() -> DateTime<Utc> {
        // Wednesday 2026-10-07 14:00 Berlin (CEST, UTC+2)
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("15m").unwrap(), Duration::minutes(15));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("90").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("45s").unwrap(), Duration::seconds(45));
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("15").is_ok());
        assert!(parse_duration("1h30").is_err());
    }

    #[test]
    fn points_in_time() {
        assert_eq!(parse_time("now", now(), Berlin).unwrap(), now());
        assert_eq!(
            parse_time("-30m", now(), Berlin).unwrap(),
            now() - Duration::minutes(30)
        );
        assert_eq!(
            parse_time("-15m", now(), Berlin).unwrap(),
            now() - Duration::minutes(15)
        );
        assert_eq!(
            parse_time("13:00", now(), Berlin).unwrap(),
            at(2026, 10, 7, 11, 0)
        );
        assert_eq!(
            parse_time("yesterday 13:00", now(), Berlin).unwrap(),
            at(2026, 10, 6, 11, 0)
        );
        assert_eq!(
            parse_time("2026-10-01T09:30", now(), Berlin).unwrap(),
            at(2026, 10, 1, 7, 30)
        );
        assert_eq!(
            parse_time("2026-10-01 09:30", now(), Berlin).unwrap(),
            at(2026, 10, 1, 7, 30)
        );
        assert_eq!(
            parse_time("2026-10-01T09:30:00Z", now(), Berlin).unwrap(),
            at(2026, 10, 1, 9, 30)
        );
        assert_eq!(
            parse_time("2026-01-15 09:30", now(), Berlin).unwrap(),
            at(2026, 1, 15, 8, 30),
            "winter time is UTC+1"
        );
        assert!(parse_time("lunch", now(), Berlin).is_err());
    }

    #[test]
    fn named_ranges() {
        let week = parse_range("week", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(week.from, at(2026, 10, 4, 22, 0));
        assert_eq!(week.to, at(2026, 10, 11, 22, 0));
        let sunday_week = parse_range("week", now(), Berlin, Weekday::Sun).unwrap();
        assert_eq!(sunday_week.from, at(2026, 10, 3, 22, 0));
        let last = parse_range("lastweek", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(last.to, week.from);
        let month = parse_range("month", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(month.from, at(2026, 9, 30, 22, 0));
        assert_eq!(month.to, at(2026, 10, 31, 23, 0), "DST ends inside October");
        let lastmonth = parse_range("lastmonth", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(lastmonth.from, at(2026, 8, 31, 22, 0));
        assert_eq!(lastmonth.to, month.from);
        let today = parse_range("today", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(today.from, at(2026, 10, 6, 22, 0));
        let yesterday = parse_range("yesterday", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(yesterday.to, today.from);
    }

    #[test]
    fn explicit_ranges() {
        let range = parse_range("2026-10-01..2026-10-07", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(range.from, at(2026, 9, 30, 22, 0));
        assert_eq!(range.to, at(2026, 10, 7, 22, 0), "end date is inclusive");
        let range =
            parse_range("2026-10-01..2026-10-07T12:00", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(range.to, at(2026, 10, 7, 10, 0));
        let open = parse_range("2026-10-01..", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(open.to, now());
        let single = parse_range("2026-10-02", now(), Berlin, Weekday::Mon).unwrap();
        assert_eq!(single.to - single.from, Duration::days(1));
        assert!(parse_range("2026-10-07..2026-10-01", now(), Berlin, Weekday::Mon).is_err());
        assert!(parse_range("someday", now(), Berlin, Weekday::Mon).is_err());
    }

    #[test]
    fn tz_resolution() {
        assert_eq!(resolve_tz(Some("Europe/Berlin"), None), Berlin);
        assert_eq!(resolve_tz(None, Some("Europe/Berlin")), Berlin);
        assert_eq!(resolve_tz(Some("local"), Some(":Europe/Berlin")), Berlin);
        assert_eq!(resolve_tz(Some("Nowhere/Land"), None), Tz::UTC);
    }
}
