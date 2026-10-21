//! Display-formatting and composing helpers for JMAP Chat clients.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Timelike, Utc};

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
            // chrono's %a is locale-independent: short English weekday
            // name (Mon, Tue, Wed, Thu, Fri, Sat, Sun) regardless of
            // the host OS locale — matching the prior hand-rolled
            // table byte-for-byte while delegating to chrono's tested
            // implementation.
            format!(
                "{} {:02}:{:02}",
                parsed.format("%a"),
                parsed.hour(),
                parsed.minute()
            )
        }
        _ => {
            // %b is the locale-independent short month name (Jan, Feb,
            // ..., Dec). Same byte-equivalent guarantee as %a above.
            if parsed.year() != now.year() {
                format!("{} {} {}", parsed.format("%b"), parsed.day(), parsed.year())
            } else {
                format!("{} {}", parsed.format("%b"), parsed.day())
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
