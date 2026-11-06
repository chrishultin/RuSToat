//! Message timestamps. Stoat IDs are ULIDs, whose first 10 characters encode
//! the creation time in milliseconds, so a message's time comes from its ID.

use chrono::{DateTime, Local, NaiveDateTime, Offset, TimeZone};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Whether `s` is shaped like a Stoat ID: 26 upper-case Crockford base32 characters.
pub fn is_ulid(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|b| CROCKFORD.contains(&b))
}

/// Milliseconds since the Unix epoch encoded in a ULID, if it is one.
pub fn ulid_millis(id: &str) -> Option<u64> {
    let prefix = id.get(..10)?;
    prefix.bytes().try_fold(0u64, |acc, byte| {
        let digit = CROCKFORD
            .iter()
            .position(|&c| c == byte.to_ascii_uppercase())?;
        Some(acc * 32 + digit as u64)
    })
}

/// When a message (or anything with a ULID) was created, in local time.
pub fn ulid_local_time(id: &str) -> Option<DateTime<Local>> {
    let millis = i64::try_from(ulid_millis(id)?).ok()?;
    Local.timestamp_millis_opt(millis).single()
}

/// "Today at 14:05", "Yesterday at 09:12", or "3 Nov 2026 at 14:05".
pub fn format_message_time(when: NaiveDateTime, now: NaiveDateTime) -> String {
    let time = when.format("%H:%M");
    let days = (now.date() - when.date()).num_days();
    match days {
        0 => format!("Today at {time}"),
        1 => format!("Yesterday at {time}"),
        _ => format!("{} at {time}", when.format("%-d %b %Y")),
    }
}

/// [`format_message_time`] for a ULID, in local time. Empty if it isn't one.
pub fn message_time_label(id: &str) -> String {
    ulid_millis(id).map(millis_time_label).unwrap_or_default()
}

/// [`format_message_time`] for milliseconds since the Unix epoch, in local time.
pub fn millis_time_label(millis: u64) -> String {
    i64::try_from(millis)
        .ok()
        .and_then(|millis| Local.timestamp_millis_opt(millis).single())
        .map(|when| format_message_time(when.naive_local(), Local::now().naive_local()))
        .unwrap_or_default()
}

/// Minutes between two ULIDs (b after a), if both are ULIDs.
pub fn minutes_between(a: &str, b: &str) -> Option<u64> {
    Some(ulid_millis(b)?.saturating_sub(ulid_millis(a)?) / 60_000)
}

/// `<t:SECONDS:STYLE>` in message text, as Stoat's web client shows it:
/// `t` a time, `T` with seconds, `D` a date, `F` the day, date and time,
/// `R` relative to `now` (see [`relative_time`]). Any other style, or none,
/// is the date and time.
pub fn format_timestamp<Tz: TimeZone>(
    when: &DateTime<Tz>,
    style: Option<char>,
    now: &DateTime<Tz>,
) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let format = match style {
        Some('t') => "%H:%M",
        Some('T') => "%H:%M:%S",
        Some('D') => "%-d %B %Y",
        Some('F') => "%A, %-d %B %Y %H:%M",
        Some('R') => return relative_time(when, now),
        _ => "%-d %B %Y %H:%M",
    };
    when.format(format).to_string()
}

/// "in 3 hours", "5 minutes ago", "just now": how far `when` is from `now`,
/// rounded as dayjs's `fromNow` (which Stoat's web client uses) rounds it.
pub fn relative_time<Tz: TimeZone>(when: &DateTime<Tz>, now: &DateTime<Tz>) -> String {
    const MINUTE: f64 = 60.;
    const HOUR: f64 = 60. * MINUTE;
    const DAY: f64 = 24. * HOUR;
    // dayjs counts calendar months; an average month is close enough.
    const MONTH: f64 = 365.2425 / 12. * DAY;
    const YEAR: f64 = 12. * MONTH;
    let diff = when
        .clone()
        .signed_duration_since(now.clone())
        .num_milliseconds() as f64
        / 1000.;
    let secs = diff.abs();
    let units = |unit: f64| (secs / unit).round() as u64;
    // Each unit is used up to a threshold, after which the next one takes
    // over. A count that rounds to 1 reads as "a minute", "an hour", ….
    let phrase = if units(1.) <= 44 {
        return "just now".into();
    } else if units(1.) <= 89 || units(MINUTE) <= 1 {
        "a minute".to_string()
    } else if units(MINUTE) <= 44 {
        format!("{} minutes", units(MINUTE))
    } else if units(MINUTE) <= 89 || units(HOUR) <= 1 {
        "an hour".to_string()
    } else if units(HOUR) <= 21 {
        format!("{} hours", units(HOUR))
    } else if units(HOUR) <= 35 || units(DAY) <= 1 {
        "a day".to_string()
    } else if units(DAY) <= 25 {
        format!("{} days", units(DAY))
    } else if units(DAY) <= 45 || units(MONTH) <= 1 {
        "a month".to_string()
    } else if units(MONTH) <= 10 {
        format!("{} months", units(MONTH))
    } else if units(MONTH) <= 17 || units(YEAR) <= 1 {
        "a year".to_string()
    } else {
        format!("{} years", units(YEAR))
    };
    if diff > 0. {
        format!("in {phrase}")
    } else {
        format!("{phrase} ago")
    }
}

/// The full date and time of a `<t:…>` timestamp, for its tooltip:
/// "Friday, 6 November 2026 at 14:05:09 GMT+1".
pub fn timestamp_tooltip<Tz: TimeZone>(when: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let offset = when.offset().fix().local_minus_utc();
    let zone = match offset {
        0 => "GMT".to_string(),
        _ => {
            let sign = if offset < 0 { '-' } else { '+' };
            let (hours, minutes) = (offset.abs() / 3600, offset.abs() % 3600 / 60);
            match minutes {
                0 => format!("GMT{sign}{hours}"),
                _ => format!("GMT{sign}{hours}:{minutes:02}"),
            }
        }
    };
    format!("{} {zone}", when.format("%A, %-d %B %Y at %H:%M:%S"))
}

/// Seconds since the Unix epoch in local time, if chrono can represent them.
pub fn local_from_seconds(seconds: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(seconds, 0).single()
}

/// A length of time in its largest one or two units, for a slowmode: "5s",
/// "10m", "1m 30s", "1h 30m", "6h". Hours leave out the seconds.
pub fn format_duration_short(secs: u64) -> String {
    let (hours, minutes, seconds) = (secs / 3600, secs % 3600 / 60, secs % 60);
    match (hours, minutes, seconds) {
        (0, 0, s) => format!("{s}s"),
        (0, m, 0) => format!("{m}m"),
        (0, m, s) => format!("{m}m {s}s"),
        (h, 0, _) => format!("{h}h"),
        (h, m, _) => format!("{h}h {m}m"),
    }
}

/// Time left as a clock: "0:24", "12:05", or with hours "1:00:00".
pub fn format_countdown(secs: u64) -> String {
    let (hours, minutes, seconds) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate};

    #[test]
    fn short_durations() {
        assert_eq!(format_duration_short(0), "0s");
        assert_eq!(format_duration_short(5), "5s");
        assert_eq!(format_duration_short(59), "59s");
        assert_eq!(format_duration_short(60), "1m");
        assert_eq!(format_duration_short(90), "1m 30s");
        assert_eq!(format_duration_short(600), "10m");
        assert_eq!(format_duration_short(3600), "1h");
        assert_eq!(format_duration_short(5400), "1h 30m");
        assert_eq!(format_duration_short(5401), "1h 30m");
        assert_eq!(format_duration_short(21600), "6h");
    }

    #[test]
    fn countdowns() {
        assert_eq!(format_countdown(0), "0:00");
        assert_eq!(format_countdown(24), "0:24");
        assert_eq!(format_countdown(59), "0:59");
        assert_eq!(format_countdown(60), "1:00");
        assert_eq!(format_countdown(725), "12:05");
        assert_eq!(format_countdown(3599), "59:59");
        assert_eq!(format_countdown(3600), "1:00:00");
        assert_eq!(format_countdown(21600), "6:00:00");
    }

    #[test]
    fn decodes_the_ulid_readme_example() {
        // The ULID README's `decodeTime("01ARYZ6S41TSV4RRFFQ69G5FAV")` example.
        assert_eq!(
            ulid_millis("01ARYZ6S41TSV4RRFFQ69G5FAV"),
            Some(1_469_918_176_385)
        );
        // Crockford base32 is case-insensitive.
        assert_eq!(
            ulid_millis("01aryz6s41tsv4rrffq69g5fav"),
            Some(1_469_918_176_385)
        );
    }

    #[test]
    fn recognises_ulid_shapes() {
        assert!(is_ulid("01ARYZ6S41TSV4RRFFQ69G5FAV"));
        assert!(!is_ulid("01ARYZ6S41TSV4RRFFQ69G5FA")); // 25 chars
        assert!(!is_ulid("01ARYZ6S41TSV4RRFFQ69G5FAI")); // I isn't Crockford
        assert!(!is_ulid("01aryz6s41tsv4rrffq69g5fav")); // Stoat IDs are upper case
    }

    #[test]
    fn rejects_non_ulids() {
        assert_eq!(ulid_millis("short"), None);
        assert_eq!(ulid_millis("not-a-ulid-at-all!"), None);
    }

    #[test]
    fn formats_relative_days() {
        let at = |d: u32, h: u32, m: u32| {
            NaiveDate::from_ymd_opt(2026, 11, d)
                .unwrap()
                .and_hms_opt(h, m, 0)
                .unwrap()
        };
        let now = at(4, 18, 0);
        assert_eq!(format_message_time(at(4, 14, 5), now), "Today at 14:05");
        assert_eq!(format_message_time(at(3, 9, 12), now), "Yesterday at 09:12");
        assert_eq!(
            format_message_time(at(1, 23, 59), now),
            "1 Nov 2026 at 23:59"
        );
    }

    /// Build a ULID whose time prefix encodes `millis`.
    fn ulid_at(millis: u64) -> String {
        let mut prefix = [b'0'; 10];
        let mut n = millis;
        for slot in prefix.iter_mut().rev() {
            *slot = CROCKFORD[(n % 32) as usize];
            n /= 32;
        }
        format!("{}ZZZZZZZZZZZZZZZZ", std::str::from_utf8(&prefix).unwrap())
    }

    #[test]
    fn minutes_between_ulids() {
        let a = ulid_at(1_793_700_000_000);
        let b = ulid_at(1_793_700_000_000 + 6 * 60_000 + 59_000);
        assert_eq!(ulid_millis(&a), Some(1_793_700_000_000));
        assert_eq!(minutes_between(&a, &b), Some(6));
        assert_eq!(minutes_between(&b, &a), Some(0));
    }
    /// 6 November 2026, 14:05:09, an hour ahead of GMT.
    fn friday() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(3600)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 6, 14, 5, 9)
            .unwrap()
    }

    #[test]
    fn timestamp_styles() {
        let when = friday();
        let now = when + chrono::Duration::hours(1);
        let format = |style| format_timestamp(&when, style, &now);
        assert_eq!(format(Some('t')), "14:05");
        assert_eq!(format(Some('T')), "14:05:09");
        assert_eq!(format(Some('D')), "6 November 2026");
        assert_eq!(format(Some('F')), "Friday, 6 November 2026 14:05");
        assert_eq!(format(Some('R')), "an hour ago");
        assert_eq!(format(None), "6 November 2026 14:05");
        assert_eq!(format(Some('x')), "6 November 2026 14:05");
    }

    #[test]
    fn relative_times_either_side_of_now() {
        let now = friday();
        let at = |secs: i64| relative_time(&(now + chrono::Duration::seconds(secs)), &now);
        const MIN: i64 = 60;
        const HOUR: i64 = 60 * MIN;
        const DAY: i64 = 24 * HOUR;
        assert_eq!(at(0), "just now");
        assert_eq!(at(44), "just now");
        assert_eq!(at(-44), "just now");
        assert_eq!(at(45), "in a minute");
        assert_eq!(at(-89), "a minute ago");
        assert_eq!(at(90), "in 2 minutes");
        assert_eq!(at(5 * MIN), "in 5 minutes");
        assert_eq!(at(-5 * MIN), "5 minutes ago");
        assert_eq!(at(44 * MIN), "in 44 minutes");
        assert_eq!(at(45 * MIN), "in an hour");
        assert_eq!(at(89 * MIN), "in an hour");
        assert_eq!(at(90 * MIN), "in 2 hours");
        assert_eq!(at(3 * HOUR), "in 3 hours");
        // Just under 3 hours (a minute after sending "in 3 hours") still
        // rounds to 3; it reads "in 2 hours" from 2.5 hours on.
        assert_eq!(at(3 * HOUR - MIN), "in 3 hours");
        assert_eq!(at(150 * MIN - 1), "in 2 hours");
        assert_eq!(at(-3 * HOUR), "3 hours ago");
        assert_eq!(at(21 * HOUR), "in 21 hours");
        assert_eq!(at(22 * HOUR), "in a day");
        assert_eq!(at(-35 * HOUR), "a day ago");
        assert_eq!(at(36 * HOUR), "in 2 days");
        assert_eq!(at(25 * DAY), "in 25 days");
        assert_eq!(at(-26 * DAY), "a month ago");
        assert_eq!(at(46 * DAY), "in 2 months");
        assert_eq!(at(-300 * DAY), "10 months ago");
        assert_eq!(at(320 * DAY), "in a year");
        assert_eq!(at(-547 * DAY), "a year ago");
        assert_eq!(at(-560 * DAY), "2 years ago");
        assert_eq!(at(3 * 365 * DAY), "in 3 years");
    }

    #[test]
    fn tooltips_name_the_offset_from_gmt() {
        assert_eq!(
            timestamp_tooltip(&friday()),
            "Friday, 6 November 2026 at 14:05:09 GMT+1"
        );
        let utc = friday().with_timezone(&FixedOffset::east_opt(0).unwrap());
        assert_eq!(
            timestamp_tooltip(&utc),
            "Friday, 6 November 2026 at 13:05:09 GMT"
        );
        let india = friday().with_timezone(&FixedOffset::east_opt(5 * 3600 + 1800).unwrap());
        assert_eq!(
            timestamp_tooltip(&india),
            "Friday, 6 November 2026 at 18:35:09 GMT+5:30"
        );
        let west = friday().with_timezone(&FixedOffset::west_opt(5 * 3600).unwrap());
        assert!(timestamp_tooltip(&west).ends_with("08:05:09 GMT-5"));
    }

    #[test]
    fn seconds_out_of_range_have_no_time() {
        assert!(local_from_seconds(1_762_430_000).is_some());
        assert!(local_from_seconds(i64::MAX).is_none());
    }
}
