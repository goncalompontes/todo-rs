//! Calendar helpers, backed by `chrono`.
//!
//! The public surface is deliberately small and stable so plugins don't each
//! re-implement date maths.

use chrono::{Datelike, Duration, Local, NaiveDate};

pub use chrono::NaiveDate as Date;

/// The Unix epoch as a `NaiveDate`.
fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch is valid")
}

/// Parse a `YYYY-MM-DD` date.
pub fn parse(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// Format a date as `YYYY-MM-DD`.
pub fn format(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Today's date, in local time (matching `todo.sh`'s use of `date(1)`).
pub fn today() -> String {
    format(Local::now().date_naive())
}

/// Today as a `NaiveDate`.
pub fn now() -> NaiveDate {
    Local::now().date_naive()
}

/// Convert a count of days since the Unix epoch into `YYYY-MM-DD`.
pub fn from_unix_days(days: i64) -> String {
    format(epoch() + Duration::days(days))
}

/// Days since the Unix epoch for a `YYYY-MM-DD` date.
pub fn to_unix_days(date: &str) -> Option<i64> {
    let d = parse(date)?;
    Some((d - epoch()).num_days())
}

/// Parse a date at the start of `s`, returning it and the rest of the string.
pub fn parse_prefix(s: &str) -> Option<(String, &str)> {
    if s.len() < 10 {
        return None;
    }
    let (head, tail) = s.split_at(10);
    let date = parse(head)?;
    if !tail.is_empty() && !tail.starts_with(' ') {
        return None;
    }
    Some((format(date), tail))
}

/// Number of days in `YYYY-MM`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(ny, nm, 1).expect("valid month");
    (first_next - Duration::days(1)).day()
}

/// Add `months` calendar months, clamping to the end of the target month.
pub fn add_months(date: NaiveDate, months: i64) -> NaiveDate {
    let total = date.year() as i64 * 12 + date.month0() as i64 + months;
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12));
    let month = m0 as u32 + 1;
    let day = date.day().min(days_in_month(y as i32, month));
    NaiveDate::from_ymd_opt(y as i32, month, day).unwrap_or(date)
}

/// Add `years` calendar years, clamping to the end of the target month.
pub fn add_years(date: NaiveDate, years: i64) -> NaiveDate {
    add_months(date, years * 12)
}

/// Move forward to the next weekday if `date` falls on a weekend.
pub fn next_weekday(date: NaiveDate) -> NaiveDate {
    match date.weekday() {
        chrono::Weekday::Sat => date + Duration::days(2),
        chrono::Weekday::Sun => date + Duration::days(1),
        _ => date,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_days() {
        for days in [-1, 0, 1, 20_000, -20_000] {
            let s = from_unix_days(days);
            assert_eq!(to_unix_days(&s), Some(days));
        }
    }

    #[test]
    fn prefix() {
        let (d, rest) = parse_prefix("2026-09-29 buy milk").unwrap();
        assert_eq!(d, "2026-09-29");
        assert_eq!(rest, " buy milk");
        assert!(parse_prefix("2026-13-01 nope").is_none());
    }

    #[test]
    fn month_math() {
        let jan31 = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        assert_eq!(format(add_months(jan31, 1)), "2026-02-28");
    }
}
