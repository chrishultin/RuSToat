//! Message timestamps. Stoat IDs are ULIDs, whose first 10 characters encode
//! the creation time in milliseconds, so a message's time comes from its ID.

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

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
    ulid_local_time(id)
        .map(|when| format_message_time(when.naive_local(), Local::now().naive_local()))
        .unwrap_or_default()
}

/// Minutes between two ULIDs (b after a), if both are ULIDs.
pub fn minutes_between(a: &str, b: &str) -> Option<u64> {
    Some(ulid_millis(b)?.saturating_sub(ulid_millis(a)?) / 60_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

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
}
