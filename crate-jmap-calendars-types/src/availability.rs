//! BusyPeriod and related types for `Principal/getAvailability`.
//!
//! Normative reference: draft-ietf-jmap-calendars-26 §2.2.

use jmap_types::UTCDate;
use serde::{Deserialize, Serialize};

/// A single busy period returned by `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-26 §2.2).
///
/// The server returns an array of `BusyPeriod` objects representing time ranges
/// when the principal is occupied.  If `showDetails` was `false` in the request
/// (or the calendar has `privacy:"secret"`, or the requesting user lacks
/// `mayReadItems` rights), the `event` and `account_id` fields are `null`.
///
/// ## Field semantics
///
/// - `utc_start` / `utc_end` — inclusive/exclusive UTC date-time bounds of the
///   busy period; both are always present.
/// - `busy_status` — hint about the kind of busy time: `"confirmed"`,
///   `"tentative"`, or `"unavailable"` (default).  May be `null`.
/// - `event` — the underlying [`crate::CalendarEvent`] if the requesting user
///   has the right to see it, otherwise `null`.
/// - `account_id` — the JMAP account id of the calendar containing this event;
///   `null` when `event` is `null`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusyPeriod {
    /// UTC start of the busy period (inclusive).
    ///
    /// Uses the [`UTCDate`] newtype (RFC 8620 §1.4) rather than a bare
    /// `String` to make the wire-format constraint
    /// (20-character `YYYY-MM-DDTHH:MM:SSZ`) explicit at the type level.
    /// JSON serialization is unchanged because `UTCDate` is a transparent
    /// newtype.
    pub utc_start: UTCDate,

    /// UTC end of the busy period (exclusive).
    ///
    /// See [`utc_start`](Self::utc_start) for the typing rationale.
    pub utc_end: UTCDate,

    /// Characterisation of the busy time.
    ///
    /// One of `"confirmed"`, `"tentative"`, or `"unavailable"` (the default
    /// when absent or null).  Clients SHOULD handle unknown values gracefully.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub busy_status: Option<String>,

    /// The underlying event, if the requester has read access.
    ///
    /// Represented as a raw JSON value to avoid a circular type dependency:
    /// `CalendarEvent` is defined in a sibling module, and callers that need
    /// the typed form can deserialize `event` into [`crate::CalendarEvent`]
    /// themselves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,

    /// JMAP account id of the calendar containing this event.
    ///
    /// `null` (i.e., `None`) when `event` is `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<jmap_types::Id>,
}
