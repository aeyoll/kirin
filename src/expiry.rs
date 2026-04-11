//! Expiry calculation aligned with Jirafeau `jirafeau_datestr_to_int` semantics.

use chrono::{DateTime, Utc};

const MINUTE: i64 = 60;
const HOUR: i64 = 3600;
const DAY: i64 = 86400;
const WEEK: i64 = 604_800;
const FORTNIGHT: i64 = 1_209_600;
const MONTH: i64 = 2_592_000;
const QUARTER: i64 = 7_776_000;
const YEAR: i64 = 31_536_000;

/// Returns absolute Unix timestamp when the file expires, or `None` for unlimited (`none`).
pub fn expires_at_unix(now: i64, availability: &str) -> Option<i64> {
    let delta = match availability {
        "minute" => MINUTE,
        "hour" => HOUR,
        "day" => DAY,
        "week" => WEEK,
        "fortnight" => FORTNIGHT,
        "month" => MONTH,
        "quarter" => QUARTER,
        "year" => YEAR,
        "none" => return None,
        _ => return Some(now),
    };
    Some(now.saturating_add(delta))
}

pub fn expires_at_datetime(now: i64, availability: &str) -> Option<DateTime<Utc>> {
    expires_at_unix(now, availability)
        .map(|u| DateTime::from_timestamp(u, 0).unwrap_or_else(Utc::now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_open_ended() {
        assert!(expires_at_unix(1_000, "none").is_none());
    }

    #[test]
    fn hour_adds_3600() {
        assert_eq!(expires_at_unix(0, "hour"), Some(3600));
    }

    #[test]
    fn month_30_days() {
        assert_eq!(expires_at_unix(0, "month"), Some(MONTH));
    }
}
