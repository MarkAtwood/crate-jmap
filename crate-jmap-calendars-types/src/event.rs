//! CalendarEvent object and CalendarEventFilterCondition.
//!
//! Normative references:
//!   - draft-ietf-jmap-calendars-26 §5 — JMAP-specific additions
//!   - RFC 8984 §2.1, §4, §5.1 — JSCalendar Event properties

use std::collections::HashMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// A JMAP CalendarEvent object (draft-ietf-jmap-calendars-26 §5; RFC 8984 §5.1).
///
/// `CalendarEvent` is a JSCalendar Event with additional JMAP-specific properties.
/// Every field is `Option<T>` because RFC 8620 §5.1 allows partial responses
/// (clients may request only specific fields via the `properties` argument).
/// A field absent from the server response MUST NOT fail deserialization.
///
/// ## Complex sub-objects
///
/// JSCalendar sub-objects with their own rich structure (`locations`,
/// `participants`, `alerts`, `recurrenceRules`, etc.) are represented as
/// `Option<serde_json::Value>`.  This avoids an exhaustive typed representation
/// while preserving full round-trip fidelity.  Consumers that need to
/// manipulate these fields should parse the `Value` using the concrete types in
/// [`crate::jscalendar`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    // ── JMAP-specific properties (draft §5) ──────────────────────────────────
    /// Server-assigned immutable identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// `Id` of the base recurring event when `id` is a synthetic expanded
    /// instance id.  `null` for non-synthetic events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_event_id: Option<Id>,

    /// Set of Calendar ids this event belongs to (values always `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_ids: Option<HashMap<Id, bool>>,

    /// If `true`, this is a draft; the server will not send scheduling messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,

    /// Server-set; `true` if this account is the authoritative source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_origin: Option<bool>,

    /// Computed UTC start time (not returned by default; must be requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc_start: Option<String>,

    /// Computed UTC end time (not returned by default; must be requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc_end: Option<String>,

    /// If `true`, use per-calendar default alerts instead of `alerts` (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_default_alerts: Option<bool>,

    /// If `true`, any user may add themselves as an attendee (draft §5.1.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub may_invite_self: Option<bool>,

    /// If `true`, existing attendees may add new attendees (draft §5.1.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub may_invite_others: Option<bool>,

    /// If `true`, non-owners see only owners and themselves in participants
    /// (draft §5.1.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_attendees: Option<bool>,

    /// JMAP Blob id for the iCalendar representation of this event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<Id>,

    /// Raw iCalendar (RFC 5545) data for this event, base64-encoded.
    ///
    /// Only returned when explicitly requested via `properties:["iCalComponent"]`
    /// (draft-ietf-jmap-calendars-26 §5.7).  Never included by default.
    ///
    /// The value is the base64url-encoded VEVENT iCalendar component, suitable
    /// for interoperability with systems that do not support the native
    /// CalendarEvent representation.
    ///
    /// Wire name is `"iCalComponent"` — not `"icalComponent"` — because "iCal"
    /// is a brand abbreviation with mixed case.  Manual rename required since
    /// `rename_all = "camelCase"` would produce `"icalComponent"`.
    #[serde(rename = "iCalComponent", skip_serializing_if = "Option::is_none")]
    pub ical_component: Option<String>,

    // ── JSCalendar @type ─────────────────────────────────────────────────────
    /// JSCalendar object type; always `"Event"` when present.
    /// Included for round-trip fidelity; omitted on create.
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    // ── JSCalendar metadata properties (RFC 8984 §4.1) ───────────────────────
    /// Globally unique identifier for this event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Map of related event UIDs → `Relation` objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_to: Option<serde_json::Value>,

    /// Product identifier (who created the event data).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prod_id: Option<String>,

    /// UTC date-time when this event was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    /// UTC date-time of last modification (mandatory in full responses).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,

    /// iCalendar sequence number (default `0`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,

    // ── JSCalendar what/where properties (RFC 8984 §4.2) ─────────────────────
    /// Human-readable summary/title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Longer-form description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Content type of `description` (default `"text/plain"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_content_type: Option<String>,

    /// If `true`, this is an all-day event (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_without_time: Option<bool>,

    /// Map of location id → `Location` object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<serde_json::Value>,

    /// Map of virtual location id → `VirtualLocation` object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_locations: Option<serde_json::Value>,

    /// Map of link id → `Link` object (attachments, images, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<serde_json::Value>,

    /// BCP 47 language tag for this event's locale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,

    /// Map of keyword strings → `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<serde_json::Value>,

    /// Map of category URI strings → `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<serde_json::Value>,

    /// CSS color name or `#rrggbb` for this event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    // ── JSCalendar recurrence properties (RFC 8984 §4.3) ─────────────────────
    /// `LocalDateTime` string identifying which recurrence instance this
    /// object overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_id: Option<String>,

    /// Time zone for `recurrence_id` (required when `recurrence_id` is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_id_time_zone: Option<String>,

    /// Array of `RecurrenceRule` objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_rules: Option<serde_json::Value>,

    /// Array of `RecurrenceRule` objects to exclude from the series.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_recurrence_rules: Option<serde_json::Value>,

    /// Map of `LocalDateTime` string → PatchObject for per-occurrence overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_overrides: Option<serde_json::Value>,

    /// If `true`, this occurrence is excluded from the series.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded: Option<bool>,

    // ── JSCalendar scheduling/sharing properties (RFC 8984 §4.4) ─────────────
    /// Priority of this event (0–9; 0 = undefined).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,

    /// Free/busy status: `"free"` or `"busy"` (default `"busy"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_busy_status: Option<String>,

    /// Privacy: `"public"`, `"private"`, or `"secret"` (default `"public"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy: Option<String>,

    /// Map of scheduling method → reply-to URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<serde_json::Value>,

    /// Addr-spec of the person who sent this event data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_by: Option<String>,

    /// The organizer's calendarAddress (wire: `organizerCalendarAddress`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizer_calendar_address: Option<String>,

    /// Map of participant id → `Participant` object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<serde_json::Value>,

    /// iTIP request status string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_status: Option<String>,

    // ── JSCalendar alert properties (RFC 8984 §4.5) ───────────────────────────
    /// Map of alert id → `Alert` object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alerts: Option<serde_json::Value>,

    // ── JSCalendar multilingual properties (RFC 8984 §4.6) ───────────────────
    /// Map of language tag → PatchObject for per-language overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localizations: Option<serde_json::Value>,

    // ── JSCalendar time zone properties (RFC 8984 §4.7) ───────────────────────
    /// IANA time zone id for the event's start/end times.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,

    /// Custom time zone definitions; opaque passthrough (RFC 8984 §4.7.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zones: Option<serde_json::Value>,

    // ── JSCalendar Event-specific properties (RFC 8984 §5.1) ─────────────────
    /// Start date-time as a `LocalDateTime` string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,

    /// Duration as an ISO 8601 duration string (e.g. `"PT1H"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,

    /// Event status: `"confirmed"`, `"cancelled"`, or `"tentative"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Filter condition for `CalendarEvent/query`
/// (draft-ietf-jmap-calendars-26 §5.11.1).
///
/// All fields are optional; a condition with no fields set matches every
/// CalendarEvent.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventFilterCondition {
    /// Event must be in this Calendar (Calendar id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_calendar: Option<Id>,

    /// Event end (or any recurrence end) must be on or after this
    /// `LocalDateTime`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,

    /// Event start (or any recurrence start) must be before this
    /// `LocalDateTime`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,

    /// Full-text search across title, description, locations, and participants.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Event `title` must contain this text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Event `description` must contain this text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Matches against name/description of a participant-associated location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Matches against name or email of an `"owner"` participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,

    /// Matches against name or email of an `"attendee"` participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendee: Option<String>,

    /// Event `uid` must be exactly this string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
}

/// Comparator for `CalendarEvent/query`
/// (draft-ietf-jmap-calendars-26 §5.11.2).
///
/// The spec mandates `"start"`, `"uid"`, and `"recurrenceId"` MUST be
/// supported for sorting.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventComparator {
    /// Property name to sort by.  `"start"` MUST be supported.
    pub property: String,

    /// If `true`, sort ascending; if `false`, sort descending (default `true`).
    #[serde(default = "default_ascending")]
    pub is_ascending: bool,

    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

fn default_ascending() -> bool {
    true
}
