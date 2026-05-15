//! Display-formatting helpers for JMAP Chat clients.

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};

use jmap_types::UTCDate;

/// Format a receipt timestamp without exposing sub-minute precision.
///
/// Returns a human-readable relative string:
/// - Same calendar day as now (or future): `"Today"`
/// - Previous calendar day: `"Yesterday"`
/// - 2–6 days ago: `"Mon 14:00"` (weekday abbreviation + HH:MM)
/// - Same year, older than 6 days: `"Apr 12"` (month abbreviation + day number)
/// - Different year: `"Apr 12 2023"` (month + day + year)
/// - Unparsable input: raw string returned unchanged
///
/// **UTC dates only.** Both `dt` and the implicit current time are treated as
/// UTC wall-clock dates. Callers in non-UTC time zones that need local-day
/// semantics should use [`format_receipt_timestamp_at`] with a local-adjusted
/// reference time.
///
/// **Stability note**: The exact output strings (`"Today"`, `"Jan 15"`, etc.)
/// are informational display strings, not a stable API contract.
pub fn format_receipt_timestamp(dt: &UTCDate) -> String {
    let now_str = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let now = UTCDate::from(now_str.as_str());
    format_receipt_timestamp_at(dt, &now)
}

/// Like [`format_receipt_timestamp`] but accepts an explicit reference time,
/// allowing deterministic unit tests and caller-managed timezone offset.
///
/// Both `dt` and `now` are treated as UTC. To display in local time, adjust
/// `now` to the desired reference point before calling.
///
/// `now` is typed as [`UTCDate`] (a JMAP-native String newtype, RFC 3339)
/// rather than `chrono::DateTime<chrono::Utc>` so the public signature
/// of this crate does not leak the `chrono` major-version into its
/// SemVer surface. The implementation parses `now` internally and
/// silently falls back to "Today" if the reference time is itself
/// unparsable (matching the existing behaviour for `dt`).
pub fn format_receipt_timestamp_at(dt: &UTCDate, now: &UTCDate) -> String {
    let parsed = match chrono::DateTime::parse_from_rfc3339(dt.as_ref()) {
        Ok(d) => d.with_timezone(&Utc),
        Err(_) => return dt.as_ref().to_string(),
    };

    // Parse the reference time. If it itself fails to parse (callers
    // typically supply a fresh Utc::now() via the public free function
    // above, so this is rare), fall back to treating `dt` as "Today".
    let now: DateTime<Utc> = match chrono::DateTime::parse_from_rfc3339(now.as_ref()) {
        Ok(d) => d.with_timezone(&Utc),
        Err(_) => return "Today".to_string(),
    };

    let dt_date = parsed.date_naive();
    let now_date = now.date_naive();
    let days_diff = (now_date - dt_date).num_days();

    // Negative days_diff means dt is in the future (clock skew); treat as today.
    match days_diff {
        ..=0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        2..=6 => {
            let weekday = match parsed.weekday() {
                Weekday::Mon => "Mon",
                Weekday::Tue => "Tue",
                Weekday::Wed => "Wed",
                Weekday::Thu => "Thu",
                Weekday::Fri => "Fri",
                Weekday::Sat => "Sat",
                Weekday::Sun => "Sun",
            };
            format!("{} {:02}:{:02}", weekday, parsed.hour(), parsed.minute())
        }
        _ => {
            let month = match parsed.month() {
                1 => "Jan",
                2 => "Feb",
                3 => "Mar",
                4 => "Apr",
                5 => "May",
                6 => "Jun",
                7 => "Jul",
                8 => "Aug",
                9 => "Sep",
                10 => "Oct",
                11 => "Nov",
                12 => "Dec",
                // chrono::Datelike::month() is contractually 1..=12. A
                // value outside that range is a chrono-API regression,
                // not a normal-flow case. Refuse to silently mis-label
                // (the old '_ => "Dec"' arm silently re-mapped every
                // out-of-range value to December).
                _ => unreachable!("chrono::Datelike::month() returns 1..=12"),
            };
            if parsed.year() != now.year() {
                format!("{} {} {}", month, parsed.day(), parsed.year())
            } else {
                format!("{} {}", month, parsed.day())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed reference time: 2024-03-20T15:00:00Z (Wednesday). Returned
    /// as `UTCDate` rather than `chrono::DateTime<Utc>` so the test
    /// suite does not depend on the `chrono` constructor API.
    fn now() -> UTCDate {
        UTCDate::from("2024-03-20T15:00:00Z")
    }

    /// Oracle: timestamp 2h before now (2024-03-20T13:00:00Z) — same calendar day → "Today".
    #[test]
    fn format_today() {
        let dt = UTCDate::from("2024-03-20T13:00:00Z");
        assert_eq!(format_receipt_timestamp_at(&dt, &now()), "Today");
    }

    /// Oracle: timestamp 25h before now (2024-03-19T14:00:00Z) — previous calendar day → "Yesterday".
    #[test]
    fn format_yesterday() {
        let dt = UTCDate::from("2024-03-19T14:00:00Z");
        assert_eq!(format_receipt_timestamp_at(&dt, &now()), "Yesterday");
    }

    /// Oracle: timestamp 3 days ago (2024-03-17T08:30:00Z — Sunday) → "Sun 08:30".
    #[test]
    fn format_this_week() {
        let dt = UTCDate::from("2024-03-17T08:30:00Z");
        assert_eq!(format_receipt_timestamp_at(&dt, &now()), "Sun 08:30");
    }

    /// Oracle: timestamp 8 days ago (2024-03-12T09:00:00Z) — same year → "Mar 12".
    ///
    /// Note: the bead spec says "DD/MM/YY" but the reference implementation uses
    /// "Mon DD" (month abbreviation + day), which is more readable and consistent
    /// with the rest of the formatting logic. The reference is authoritative.
    #[test]
    fn format_old() {
        let dt = UTCDate::from("2024-03-12T09:00:00Z");
        assert_eq!(format_receipt_timestamp_at(&dt, &now()), "Mar 12");
    }

    /// Oracle: unparsable string → returned unchanged (never panics).
    #[test]
    fn format_parse_error() {
        let dt = UTCDate::from("not-a-date");
        assert_eq!(format_receipt_timestamp_at(&dt, &now()), "not-a-date");
    }

    /// Oracle: prior year (2023-01-15) includes the year → "Jan 15 2023".
    #[test]
    fn format_prior_year() {
        let dt = UTCDate::from("2023-01-15T09:00:00Z");
        assert_eq!(format_receipt_timestamp_at(&dt, &now()), "Jan 15 2023");
    }

    /// Oracle: future timestamp (clock skew, dt > now) formats as "Today".
    #[test]
    fn format_future_clock_skew() {
        let dt = UTCDate::from("2024-03-21T10:00:00Z");
        assert_eq!(
            format_receipt_timestamp_at(&dt, &now()),
            "Today",
            "future timestamp must display as Today, not as a past date"
        );
    }

    /// Oracle: unparsable `now` reference time → fallback to "Today".
    /// This codifies the silent-degradation behaviour documented on
    /// the public function and locks it against accidental regression
    /// to a panic-on-malformed-now path.
    #[test]
    fn format_parse_error_on_now() {
        let dt = UTCDate::from("2024-03-20T13:00:00Z");
        let bad_now = UTCDate::from("not-a-date");
        assert_eq!(format_receipt_timestamp_at(&dt, &bad_now), "Today");
    }
}
