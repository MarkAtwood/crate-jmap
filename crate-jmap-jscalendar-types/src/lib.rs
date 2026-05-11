//! JSCalendar (RFC 8984) typed sub-types for the jmap-* crate family.
//!
//! Normative reference: RFC 8984 (JSCalendar).
//!
//! These are sub-object types that have no JMAP identity of their own.
//! They are embedded within `CalendarEvent` (from `jmap-calendars-types`),
//! `Task` (from `jmap-tasks-types`), and other JMAP objects.
//!
//! ## Crate family position
//!
//! ```text
//! jmap-types (RFC 8620 wire primitives)
//!     └── jmap-jscalendar-types  ← this crate (RFC 8984 typed sub-types)
//!             ├── jmap-calendars-types (consumes via path-dep + re-export)
//!             └── jmap-tasks-types     (consumes via path-dep + re-export)
//! ```
//!
//! ## Design: newtype wrappers for scalar temporal values
//!
//! RFC 8984 §1.4.5 defines `LocalDateTime` as a string without a timezone
//! offset (e.g. `"2024-06-15T09:00:00"`).  RFC 8984 §1.4.6 defines `Duration`
//! as an ISO 8601-subset string (e.g. `"PT1H"`).  RFC 8984 §1.4.7 defines
//! `SignedDuration` as an optional-sign prefix on Duration.
//!
//! These are modelled as newtype wrappers around `String` to document intent
//! at the type level without pulling in a heavy parser dependency.  Validation
//! of internal format is left to the backend.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

// ── Scalar wrappers ───────────────────────────────────────────────────────────

/// A date-time string without a timezone offset (RFC 8984 §1.4.5).
///
/// Format: `YYYY-MM-DDTHH:MM:SS` (no `Z`, no `±offset`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalDateTime(String);

impl From<String> for LocalDateTime {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for LocalDateTime {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for LocalDateTime {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// An ISO 8601 duration string (RFC 8984 §1.4.6).
///
/// Example: `"PT1H"`, `"P1DT2H"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Duration(String);

impl From<String> for Duration {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Duration {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for Duration {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A signed ISO 8601 duration string (RFC 8984 §1.4.7).
///
/// Like `Duration` but may be prefixed with `+` or `-`.
/// Example: `"-PT15M"`, `"+PT30M"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignedDuration(String);

impl From<String> for SignedDuration {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SignedDuration {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for SignedDuration {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ── RecurrenceRule ────────────────────────────────────────────────────────────

/// The `nthOfPeriod` field of an [`NDay`] entry (RFC 8984 §4.3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NDay {
    /// Object type discriminator; always `"NDay"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Day of the week: `"mo"`, `"tu"`, `"we"`, `"th"`, `"fr"`, `"sa"`, `"su"`.
    pub day: String,

    /// Which occurrence within the period (non-zero integer), or `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nth_of_period: Option<i32>,
}

/// A recurrence rule as defined in RFC 8984 §4.3.3.
///
/// Used in `recurrenceRules` and `excludedRecurrenceRules` of a
/// `CalendarEvent` (from `jmap-calendars-types`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRule {
    /// Object type discriminator; always `"RecurrenceRule"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Recurrence frequency: `"yearly"`, `"monthly"`, `"weekly"`, `"daily"`,
    /// `"hourly"`, `"minutely"`, or `"secondly"`.
    pub frequency: String,

    /// Interval between recurrences (≥ 1; default 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,

    /// Calendar system (default `"gregorian"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rscale: Option<String>,

    /// How to handle skipped dates: `"omit"`, `"backward"`, `"forward"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<String>,

    /// First day of week (default `"mo"`): `"mo"`–`"su"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_day_of_week: Option<String>,

    /// Specific days within the frequency period.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_day: Option<Vec<NDay>>,

    /// Specific days of the month (±1–±31).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_month_day: Option<Vec<i32>>,

    /// Specific months (e.g. `"1"`–`"12"`, optionally suffixed with `"L"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_month: Option<Vec<String>>,

    /// Specific days of the year (±1–±366).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_year_day: Option<Vec<i32>>,

    /// Specific weeks of the year (±1–±53).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_week_no: Option<Vec<i32>>,

    /// Specific hours (0–23).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_hour: Option<Vec<u8>>,

    /// Specific minutes (0–59).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_minute: Option<Vec<u8>>,

    /// Specific seconds (0–60).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_second: Option<Vec<u8>>,

    /// Filter by position within the set (positive = from start, negative = from end).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_set_position: Option<Vec<i32>>,

    /// Maximum number of occurrences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,

    /// The recurrence ends on or before this `LocalDateTime`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

// ── Location and VirtualLocation ─────────────────────────────────────────────

/// A physical or virtual location associated with an event (RFC 8984 §4.2.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// Object type discriminator; always `"Location"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Human-readable name for this location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Additional description of the location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Map of location type URIs → `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_types: Option<HashMap<String, bool>>,

    /// Relation of this location to the event: `"start"` or `"end"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_to: Option<String>,

    /// IANA time zone id for this location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,

    /// Geographic coordinates as a `geo:` URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<String>,

    /// Attachments and images associated with this location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<HashMap<String, Link>>,
}

/// An online meeting or virtual location (RFC 8984 §4.2.6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualLocation {
    /// Object type discriminator; always `"VirtualLocation"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Human-readable name for this virtual location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Additional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// URI to join the virtual location (e.g. a conference call or meeting URL).
    ///
    /// Mandatory per RFC 8984 §4.2.6 — a `VirtualLocation` without a `uri` is
    /// malformed.  Unlike top-level JMAP object fields, sub-object fields are NOT
    /// subject to RFC 8620 §5.1 partial-response suppression, so this cannot be
    /// absent in a well-formed server response.
    pub uri: String,

    /// Map of feature type URIs → `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,
}

// ── Link ─────────────────────────────────────────────────────────────────────

/// An attachment, image, or URL associated with an event (RFC 8984 §1.4.11).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// Object type discriminator; always `"Link"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// URI of the linked resource; may be absent when `blob_id` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,

    /// Content type (MIME type) of the linked resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    /// Size of the linked resource in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Relationship of this link to the event (e.g. `"enclosure"`, `"describedby"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rel: Option<String>,

    /// Display/file name for the link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,

    /// Content-id value for inline images embedded in a `text/html` description
    /// via `cid:` URLs (RFC 8984 §1.4.11).
    ///
    /// Only meaningful when `CalendarEvent.descriptionContentType` is `text/html`
    /// and the HTML body references this link as `<img src="cid:…">`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,

    /// Human-readable, plain-text description of the linked resource
    /// (RFC 8984 §1.4.11).
    ///
    /// Distinct from `display` (which is a file name); `title` is a longer
    /// description suitable for accessibility text or tooltips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// JMAP blob id; may be set instead of `href` for server-stored attachments
    /// (draft-ietf-jmap-calendars-26 §5.3 / §10.9.14).
    ///
    /// When present, `href` may be absent.  The server MUST translate this to
    /// an embedded data: URL when sending to systems that cannot access blobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<Id>,
}

// ── Relation ─────────────────────────────────────────────────────────────────

/// A relationship between this object and another, identified by UID
/// (RFC 8984 §1.4.10).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// Object type discriminator; always `"Relation"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Map of relationship type URIs → `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<HashMap<String, bool>>,
}

// ── Participant ───────────────────────────────────────────────────────────────

/// A participant in an event (RFC 8984 §4.4.6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    /// Object type discriminator; always `"Participant"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Display name of the participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Email address (addr-spec) of the participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Additional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Map of scheduling method → URI for sending scheduling messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_to: Option<HashMap<String, String>>,

    /// Kind of participant: `"individual"`, `"group"`, `"location"`, `"resource"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Map of role URIs → `true` (e.g. `"owner"`, `"attendee"`, `"chair"`).
    pub roles: HashMap<String, bool>,

    /// Id of the location this participant is associated with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_id: Option<String>,

    /// BCP 47 language tag for this participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Participation status (default `"needs-action"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participation_status: Option<String>,

    /// Free-form comment on participation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participation_comment: Option<String>,

    /// Whether the participant is expected to send a reply (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect_reply: Option<bool>,

    /// Scheduling agent: `"server"`, `"client"`, or `"none"` (default `"server"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_agent: Option<String>,

    /// iTIP scheduling address URI for this participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_address: Option<String>,

    /// Id of the participant who invited this participant, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_by: Option<String>,

    /// Map of participant ids → `true` for participants this one delegated to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_to: Option<HashMap<String, bool>>,

    /// Map of participant ids → `true` for participants who delegated to this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_from: Option<HashMap<String, bool>>,

    /// Map of group participant ids → `true` that this participant is a member of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_of: Option<HashMap<String, bool>>,

    /// Links associated with this participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<HashMap<String, Link>>,

    /// iTIP scheduling sequence number for this participant (RFC 8984 §5.2.1).
    ///
    /// Context: Participant — this is a per-participant iTIP tracking field,
    /// not an event-level field.  The server updates it when an iTIP message
    /// is processed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_sequence: Option<u64>,

    /// UTC date-time of the last iTIP scheduling message processed for this
    /// participant (RFC 8984 §5.2.2).
    ///
    /// Context: Participant — per-participant iTIP tracking, not event-level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_updated: Option<String>,

    /// iTIP status codes from the most recent scheduling message sent to this
    /// participant (RFC 8984 §4.4.6).
    ///
    /// An array of iTIP status code strings (e.g. `"1.0"`, `"2.0"`, `"5.0"`).
    /// Server-set and persisted; absent when no scheduling has occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_status: Option<Vec<String>>,
}

// ── Alert ─────────────────────────────────────────────────────────────────────

/// A trigger time given as an offset from the event start or end
/// (RFC 8984 §4.5.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetTrigger {
    /// Object type discriminator; always `"OffsetTrigger"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Duration offset from `relative_to` (SignedDuration string).
    pub offset: String,

    /// Whether to measure from `"start"` or `"end"` of the event.
    /// Default is `"start"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_to: Option<String>,
}

/// A trigger time given as an absolute UTC date-time (RFC 8984 §4.5.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbsoluteTrigger {
    /// Object type discriminator; always `"AbsoluteTrigger"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// The absolute UTC date-time at which to trigger the alert.
    pub when: String,
}

/// Alert trigger — either offset-based, absolute, or an unknown future type
/// (RFC 8984 §4.5.2).
///
/// The `@type` field on the wire selects the variant.  The `Unknown` variant
/// preserves any unrecognised trigger type for round-trip fidelity, as
/// required by the spec: "Implementations MUST NOT trigger for trigger types
/// they do not understand but MUST preserve them."
///
/// Serde is implemented manually because `#[serde(tag = "@type", other)]`
/// with a tuple variant is not supported by serde's derive macros; `other`
/// only works with unit variants in internally-tagged enums.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum AlertTrigger {
    /// Offset-based trigger: fires at `offset` relative to event start/end.
    OffsetTrigger(OffsetTrigger),
    /// Absolute trigger: fires at a specific UTC date-time.
    AbsoluteTrigger(AbsoluteTrigger),
    /// Any other trigger type; preserved opaquely as raw JSON.
    Unknown(serde_json::Value),
}

impl Serialize for AlertTrigger {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            AlertTrigger::OffsetTrigger(t) => t.serialize(s),
            AlertTrigger::AbsoluteTrigger(t) => t.serialize(s),
            AlertTrigger::Unknown(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for AlertTrigger {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Deserialize into an intermediate Value, then dispatch on @type.
        let v = serde_json::Value::deserialize(d)?;
        let tag = v
            .get("@type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_owned();
        match tag.as_str() {
            "OffsetTrigger" => {
                let t: OffsetTrigger =
                    serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(AlertTrigger::OffsetTrigger(t))
            }
            "AbsoluteTrigger" => {
                let t: AbsoluteTrigger =
                    serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(AlertTrigger::AbsoluteTrigger(t))
            }
            _ => Ok(AlertTrigger::Unknown(v)),
        }
    }
}

/// An alert to be shown or emailed before or after an event (RFC 8984 §4.5.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// Object type discriminator; always `"Alert"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// When to trigger the alert.
    pub trigger: AlertTrigger,

    /// UTC date-time when the user acknowledged this alert, or `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged: Option<String>,

    /// Related alerts (e.g. for snooze chains); keys are alert ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_to: Option<HashMap<String, Relation>>,

    /// How to present the alert: `"display"` or `"email"` (default `"display"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}
