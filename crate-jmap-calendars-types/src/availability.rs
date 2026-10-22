//! BusyPeriod and related types for `Principal/getAvailability`.
//!
//! Normative reference: draft-ietf-jmap-calendars-33 §2.2.

use jmap_types::UTCDate;
use serde::{Deserialize, Serialize};

/// The `busyStatus` of a [`BusyPeriod`] (draft-ietf-jmap-calendars-33
/// §2.2): `String (default: "unavailable")` that "MUST be one of" the
/// three values below, so any other string fails to deserialize.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BusyStatus {
    /// `"confirmed"` — "The event status is "confirmed" and the
    /// Principal's "participationStatus" is "accepted"."
    Confirmed,
    /// `"tentative"` — "The event status is "tentative" or the
    /// Principal's "participationStatus" is "tentative"."
    Tentative,
    /// `"unavailable"` — "The Principal is not available for scheduling
    /// at this time for any other reason." The default.
    #[default]
    Unavailable,
}

/// A single busy period returned by `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-33 §2.2).
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
/// - `busy_status` — the kind of busy time; [`BusyStatus::Unavailable`]
///   when absent on the wire.
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

    /// Characterisation of the busy time; an absent `busyStatus`
    /// deserializes as [`BusyStatus::Unavailable`], the draft's default.
    #[serde(default)]
    pub busy_status: BusyStatus,

    /// The underlying event, if the requester has read access.
    ///
    /// Represented as a raw JSON value because the server emits a
    /// server-side projection of the [`crate::CalendarEvent`] rather
    /// than the full object:
    ///
    /// - draft-ietf-jmap-calendars-33 §2.2 (the `Principal/getAvailability`
    ///   `eventProperties` argument) restricts the returned representation
    ///   to a caller-requested property subset.
    /// - The server also enforces privacy and `mayReadItems` permission
    ///   filtering on a per-event basis (the spec lists the conditions
    ///   under which `event` is `null`).
    ///
    /// Modelling that projection at the type level would require either
    /// a typed partial-`CalendarEvent` shape (large, tracks the spec) or
    /// `Option<crate::CalendarEvent>` where every field is `Option` by
    /// workspace convention. Using `serde_json::Value` keeps the
    /// projection nature explicit on the wire, avoids forcing
    /// availability consumers to walk the full `CalendarEvent` deserialize
    /// path, and lets callers that need typed access deserialize the
    /// value into [`crate::CalendarEvent`] on demand.
    ///
    /// (The previous version of this docstring cited a "circular type
    /// dependency" between `BusyPeriod` and `CalendarEvent`. That was
    /// inaccurate — both live in the same crate as sibling modules and
    /// neither depends on the other.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,

    /// JMAP account id of the calendar containing this event.
    ///
    /// `null` (i.e., `None`) when `event` is `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<jmap_types::Id>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
