//! BusyPeriod and related types for `Principal/getAvailability`.
//!
//! Normative reference: draft-ietf-jmap-calendars-26 §2.2.

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
    /// UTC start of the busy period (inclusive).  UTCDateTime string.
    pub utc_start: String,

    /// UTC end of the busy period (exclusive).  UTCDateTime string.
    pub utc_end: String,

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
