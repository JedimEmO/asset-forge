//! Dates and timestamps, in UTC, without a calendar crate.
//!
//! The library needs two date-shaped things and nothing else: the day a
//! record was written, and a sortable instant for a temporary file name.
//! Pulling a timezone database into a metadata crate to get that would be the
//! tail wagging the dog, so the civil-date conversion is here in twenty lines.
//!
//! Everything is UTC on purpose. These strings end up in git-tracked records
//! that several machines and several agents write, and a local-time stamp
//! would make two of them disagree about which day an asset shipped.
//!
//! # `FORGE_TODAY`
//!
//! A test that promotes an asset and compares the record it wrote against a
//! golden would fail at every midnight. Setting `FORGE_TODAY=YYYY-MM-DD` pins
//! [`today`]; anything that does not parse as a date is ignored rather than
//! honoured, because a pinned clock that reads `"tomorrow"` as 1970 would
//! write a record nobody asked for.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The environment variable that pins [`today`].
pub const TODAY_ENV: &str = "FORGE_TODAY";

/// A civil date, which is all a record needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    /// Year, e.g. 2026.
    pub year: i64,
    /// Month, 1..=12.
    pub month: u32,
    /// Day of month, 1..=31.
    pub day: u32,
}

impl Date {
    /// `YYYY-MM-DD`, the form the sidecars use.
    #[must_use]
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Read a `YYYY-MM-DD` date. Anything else — including a trailing time —
    /// is `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('-');
        let year: i64 = parts.next()?.parse().ok()?;
        let month: u32 = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Self { year, month, day })
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.iso())
    }
}

/// Seconds since the Unix epoch, or 0 if the clock is before it.
#[must_use]
pub fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Today's date in UTC, unless [`TODAY_ENV`] pins it.
#[must_use]
pub fn today() -> Date {
    std::env::var(TODAY_ENV)
        .ok()
        .and_then(|text| Date::parse(&text))
        .unwrap_or_else(|| date_of(unix_seconds()))
}

/// Today's date as `YYYY-MM-DD`.
#[must_use]
pub fn today_iso() -> String {
    today().iso()
}

/// Now as `YYYY-MM-DDTHH:MM:SSZ`. Sorts lexicographically, which is the only
/// ordering anything here needs.
#[must_use]
pub fn now_iso() -> String {
    stamp_of(unix_seconds())
}

/// The `YYYY-MM-DDTHH:MM:SSZ` form of a Unix timestamp.
#[must_use]
pub fn stamp_of(seconds: u64) -> String {
    let date = date_of(seconds);
    let day_seconds = seconds % 86_400;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year,
        date.month,
        date.day,
        day_seconds / 3600,
        (day_seconds % 3600) / 60,
        day_seconds % 60,
    )
}

/// Read a timestamp this module wrote back into Unix seconds.
///
/// Deliberately forgiving about the shape: it accepts a bare `YYYY-MM-DD` as
/// midnight, because sidecars record dates that way and a record that cannot
/// be aged is better than a record that cannot be read.
#[must_use]
pub fn parse_stamp(text: &str) -> Option<u64> {
    let text = text.trim();
    let (date, time) = text.split_once('T').unwrap_or((text, "00:00:00"));
    let date = Date::parse(date)?;
    let mut clock = time.trim_end_matches('Z').split(':');
    let hour: u64 = clock.next().unwrap_or("0").parse().ok()?;
    let minute: u64 = clock.next().unwrap_or("0").parse().ok()?;
    let second: u64 = clock.next().unwrap_or("0").parse().ok()?;
    let days = days_from_civil(date.year, date.month, date.day);
    u64::try_from(days)
        .ok()
        .map(|d| d * 86_400 + hour.min(23) * 3600 + minute.min(59) * 60 + second.min(60))
}

/// A short, always-different token, for temporary file names.
///
/// Time alone is not enough — two writes in the same microsecond happen — so a
/// process-wide counter is mixed in.
#[must_use]
pub fn monotonic_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}{count:x}", nanos as u64)
}

/// Civil date from a Unix timestamp.
///
/// Howard Hinnant's `civil_from_days`, which is exact for the whole range of
/// the proleptic Gregorian calendar and has no table, no leap-second fudge and
/// no timezone.
#[must_use]
pub fn date_of(seconds: u64) -> Date {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * march_month + 2) / 5 + 1) as u32;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    } as u32;
    Date {
        year: if month <= 2 { year + 1 } else { year },
        month,
        day,
    }
}

/// Inverse of [`date_of`]'s day count: `days_from_civil`.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let march_month = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let day_of_year = (153 * march_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants_convert_both_ways() {
        // Transcribed from `date -u -d @<n> +%Y-%m-%dT%H:%M:%SZ`.
        for (seconds, iso) in [
            (0_u64, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"), // leap day of a leap century
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (1_767_225_599, "2025-12-31T23:59:59Z"),
            (1_785_614_160, "2026-08-01T19:56:00Z"),
        ] {
            assert_eq!(stamp_of(seconds), iso, "formatting {seconds}");
            assert_eq!(parse_stamp(iso), Some(seconds), "parsing {iso}");
        }
    }

    #[test]
    fn a_bare_date_reads_as_midnight() {
        assert_eq!(
            parse_stamp("2026-08-01"),
            parse_stamp("2026-08-01T00:00:00Z")
        );
    }

    #[test]
    fn a_date_parses_and_refuses_what_is_not_one() {
        assert_eq!(
            Date::parse("2026-08-23"),
            Some(Date {
                year: 2026,
                month: 8,
                day: 23
            })
        );
        for bad in [
            "tomorrow",
            "2026-13-01",
            "2026-08",
            "2026-08-23T00:00:00Z",
            "",
        ] {
            assert_eq!(Date::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn tokens_differ_within_the_same_instant() {
        let a = monotonic_token();
        let b = monotonic_token();
        assert_ne!(a, b);
    }

    #[test]
    fn today_is_a_date_whether_or_not_it_is_pinned() {
        // The environment is process-wide, so this does not set it; the
        // integration tests pin it and check the record. Here: the real clock
        // produces something `Date::parse` reads back.
        assert!(Date::parse(&today_iso()).is_some());
    }
}
