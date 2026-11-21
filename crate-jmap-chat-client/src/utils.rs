//! Display-formatting and composing helpers for JMAP Chat clients.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use jmap_chat_types::{BroadcastMention, Mention, RichBody, SpanType};
use jmap_types::{Id, UTCDate};

/// The `mentions` a client composing `body` must send: one [`Mention`] per
/// distinct `userId` of a `"mention"` span, in first-seen order
/// (draft-atwood-jmap-chat-00 §Rich Body Format: "A client composing a rich
/// body MUST list each user it marks with a `"mention"` span in
/// `mentions`").
///
/// A `"mention"` span without a `userId` marks no user and is skipped.
///
/// ```rust
/// use jmap_chat_client::mentions_from_spans;
/// use jmap_chat_types::RichBody;
///
/// let body: RichBody = serde_json::from_str(
///     r#"{"spans":[{"type":"mention","text":"@alice","userId":"contact-alice"}]}"#,
/// )
/// .expect("valid rich body");
/// assert_eq!(mentions_from_spans(&body)[0].id.as_ref(), "contact-alice");
/// ```
pub fn mentions_from_spans(body: &RichBody) -> Vec<Mention> {
    // ponytail: linear dedup scan, O(n^2) in mention spans; a message has few.
    let mut out: Vec<Mention> = Vec::new();
    for user_id in body
        .spans
        .iter()
        .filter(|s| s.kind == SpanType::Mention)
        .filter_map(|s| s.user_id.as_deref())
    {
        if !out.iter().any(|m| m.id.as_ref() == user_id) {
            out.push(Mention::new(Id::from(user_id)));
        }
    }
    out
}

/// The `broadcastMentions` a client composing `body` must send: one
/// [`BroadcastMention`] per distinct `scope` of a `"broadcast"` span, in
/// first-seen order (draft-atwood-jmap-chat-00 §Rich Body Format: "A client
/// composing a rich body MUST list ... each scope it marks with a
/// `"broadcast"` span in `broadcastMentions`").
///
/// A `"broadcast"` span without a `scope` marks no scope and is skipped. A
/// scope outside [`BROADCAST_MENTION_SCOPES`](jmap_chat_types::BROADCAST_MENTION_SCOPES)
/// is passed through unchanged; servers reject it.
pub fn broadcast_mentions_from_spans(body: &RichBody) -> Vec<BroadcastMention> {
    // ponytail: linear dedup scan; at most a handful of distinct scopes.
    let mut out: Vec<BroadcastMention> = Vec::new();
    for scope in body
        .spans
        .iter()
        .filter(|s| s.kind == SpanType::Broadcast)
        .filter_map(|s| s.scope.as_ref())
    {
        if !out.iter().any(|b| b.scope == *scope) {
            out.push(BroadcastMention::new(scope.clone()));
        }
    }
    out
}

/// How long a typing indicator stays visible without a refreshing typing
/// event (draft-atwood-jmap-chat-00 §Chat/typing: "if no typing event is
/// received for a given `(chatId, senderId)` pair within approximately 10
/// seconds, the client SHOULD hide the typing indicator for that pair").
pub const TYPING_DECAY: Duration = Duration::from_secs(10);

/// Client-side typing-indicator state with the [`TYPING_DECAY`] timer.
///
/// Feed every received typing event to [`record`](Self::record); ask
/// [`is_typing`](Self::is_typing) or [`typing_in`](Self::typing_in) when
/// rendering. An indicator is hidden on a `typing: false` event or once
/// [`TYPING_DECAY`] has elapsed since the last `typing: true` event for the
/// pair. The caller supplies `now`, normally [`Instant::now`].
///
/// ```rust
/// use std::time::{Duration, Instant};
/// use jmap_chat_client::TypingIndicators;
/// use jmap_types::Id;
///
/// let (chat, alice) = (Id::from("chat-1"), Id::from("alice"));
/// let start = Instant::now();
/// let mut typing = TypingIndicators::new();
/// typing.record(&chat, &alice, true, start);
/// assert!(typing.is_typing(&chat, &alice, start + Duration::from_secs(5)));
/// assert!(!typing.is_typing(&chat, &alice, start + Duration::from_secs(11)));
/// ```
#[derive(Debug, Clone, Default)]
pub struct TypingIndicators {
    last_seen: HashMap<(Id, Id), Instant>,
}

impl TypingIndicators {
    /// An empty tracker: no one is typing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a typing event for `(chat_id, sender_id)` received at `now`.
    ///
    /// `typing: true` shows (or refreshes) the indicator; `typing: false`
    /// hides it. Expired entries are dropped so the tracker does not grow
    /// without bound.
    pub fn record(&mut self, chat_id: &Id, sender_id: &Id, typing: bool, now: Instant) {
        // ponytail: O(n) expiry sweep per event; n is the number of senders
        // seen typing in the last 10 s, which is small.
        self.last_seen
            .retain(|_, seen| now.saturating_duration_since(*seen) < TYPING_DECAY);
        let key = (chat_id.clone(), sender_id.clone());
        if typing {
            self.last_seen.insert(key, now);
        } else {
            self.last_seen.remove(&key);
        }
    }

    /// Whether the indicator for `(chat_id, sender_id)` is visible at `now`.
    pub fn is_typing(&self, chat_id: &Id, sender_id: &Id, now: Instant) -> bool {
        self.last_seen
            .get(&(chat_id.clone(), sender_id.clone()))
            .is_some_and(|seen| now.saturating_duration_since(*seen) < TYPING_DECAY)
    }

    /// The senders whose indicator in `chat_id` is visible at `now`, in no
    /// particular order.
    pub fn typing_in<'a>(&'a self, chat_id: &'a Id, now: Instant) -> impl Iterator<Item = &'a Id> {
        self.last_seen
            .iter()
            .filter(move |((chat, _), seen)| {
                chat == chat_id && now.saturating_duration_since(**seen) < TYPING_DECAY
            })
            .map(|((_, sender), _)| sender)
    }
}

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
    format_receipt_timestamp_at(dt, &now_utcdate())
}

/// Like [`format_receipt_timestamp`] but accepts an explicit reference time,
/// allowing deterministic unit tests and caller-managed timezone offset.
///
/// Both `dt` and `now` are treated as UTC. To display in local time, adjust
/// `now` to the desired reference point before calling.
///
/// `now` is typed as [`UTCDate`] (a JMAP-native String newtype, RFC 8620
/// §1.4) so the public signature leaks no date-library type into this
/// crate's semver surface. Both values must be RFC 8620 `UTCDate`s (`Z`
/// suffix). If the reference time is itself unparsable the result falls
/// back to "Today" (matching the behaviour for an unparsable `dt`, which
/// is returned unchanged).
pub fn format_receipt_timestamp_at(dt: &UTCDate, now: &UTCDate) -> String {
    let Ok(dt_secs) = dt.to_epoch_seconds() else {
        return dt.as_ref().to_string();
    };
    // If the reference time fails to parse (callers normally pass the
    // current time through the free function above, so this is rare),
    // treat `dt` as "Today".
    let Ok(now_secs) = now.to_epoch_seconds() else {
        return "Today".to_string();
    };
    let parsed = Civil::from_epoch(dt_secs);
    let now = Civil::from_epoch(now_secs);
    let days_diff = now.days - parsed.days;

    // Negative days_diff means dt is in the future (clock skew); treat as today.
    match days_diff {
        ..=0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        2..=6 => format!(
            "{} {:02}:{:02}",
            parsed.weekday_abbr(),
            parsed.hour,
            parsed.minute
        ),
        _ => {
            if parsed.year != now.year {
                format!("{} {} {}", parsed.month_abbr(), parsed.day, parsed.year)
            } else {
                format!("{} {}", parsed.month_abbr(), parsed.day)
            }
        }
    }
}

/// The current time as an RFC 8620 `UTCDate` with whole seconds.
pub(crate) fn now_utcdate() -> UTCDate {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let c = Civil::from_epoch(secs);
    UTCDate::from(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    ))
}

/// A UTC instant broken into calendar fields (proleptic Gregorian).
struct Civil {
    /// Whole days since 1970-01-01.
    days: i64,
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl Civil {
    /// Split `secs` since the Unix epoch, with Howard Hinnant's
    /// `civil_from_days` ("chrono-Compatible Low-Level Date Algorithms"),
    /// the inverse of the `days_from_civil` behind
    /// [`UTCDate::to_epoch_seconds`].
    fn from_epoch(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097; // [0, 146096]
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
        let mp = (5 * doy + 2) / 153; // [0, 11], March-based month
        let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
        let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
        let year = yoe + era * 400 + i64::from(month <= 2);
        // Each value is within the range commented above, so it fits.
        let small = |n: i64| u32::try_from(n).unwrap_or(0);
        Self {
            days,
            year,
            month: small(month),
            day: small(day),
            hour: small(rem / 3600),
            minute: small(rem / 60 % 60),
            second: small(rem % 60),
        }
    }

    /// The English three-letter weekday name; 1970-01-01 was a Thursday.
    fn weekday_abbr(&self) -> &'static str {
        const NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
        let i = usize::try_from(self.days.rem_euclid(7)).unwrap_or(0);
        NAMES[i]
    }

    /// The English three-letter month name.
    fn month_abbr(&self) -> &'static str {
        const NAMES: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let i = usize::try_from(self.month.saturating_sub(1)).unwrap_or(0);
        NAMES.get(i).copied().unwrap_or("???")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed reference time: 2024-03-20T15:00:00Z (Wednesday).
    fn now() -> UTCDate {
        UTCDate::from("2024-03-20T15:00:00Z")
    }

    /// Weekday names for the five days the "2 to 6 days ago" branch
    /// covers, checked against a 2024 calendar (2024-03-14 was a
    /// Thursday).
    #[test]
    fn weekday_names_follow_the_calendar() {
        for (date, name) in [
            ("2024-03-14", "Thu"),
            ("2024-03-15", "Fri"),
            ("2024-03-16", "Sat"),
            ("2024-03-17", "Sun"),
            ("2024-03-18", "Mon"),
        ] {
            let dt = UTCDate::from(format!("{date}T07:05:00Z"));
            assert_eq!(
                format_receipt_timestamp_at(&dt, &now()),
                format!("{name} 07:05")
            );
        }
    }

    /// The civil split inverts `UTCDate::to_epoch_seconds`, leap day and
    /// pre-1970 included, and the current time is a valid `UTCDate`.
    #[test]
    fn civil_split_round_trips() {
        for s in [
            "2000-02-29T12:00:00Z",
            "1969-12-31T23:59:59Z",
            "2024-12-31T00:00:01Z",
        ] {
            let c = Civil::from_epoch(UTCDate::from(s).to_epoch_seconds().unwrap());
            let back = format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                c.year, c.month, c.day, c.hour, c.minute, c.second
            );
            assert_eq!(back, s);
        }
        let now = now_utcdate();
        assert!(UTCDate::new_validated(now.as_ref()).is_ok(), "{now:?}");
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
