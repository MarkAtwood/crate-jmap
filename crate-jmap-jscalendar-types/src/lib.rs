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

use jmap_types::{impl_string_enum, Id, PatchObject, UTCDate};
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

// ── Enumerated-string types ──────────────────────────────────────────────────
//
// RFC 8984 enumerates a closed (or vendor-extensible) set of string values
// for several fields. These are modelled as `#[non_exhaustive] enum` types
// with an `Other(String)` catch-all that preserves any unrecognised wire
// value for round-trip fidelity, per the workspace extras-preservation
// policy (see workspace `AGENTS.md`).

/// A day of the week (RFC 8984 §4.3.3).
///
/// The seven canonical lowercase two-letter abbreviations from the
/// iCalendar BYDAY part. Used for both [`NDay::day`] and
/// [`RecurrenceRule::first_day_of_week`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Weekday {
    /// Monday (`"mo"`).
    Monday,
    /// Tuesday (`"tu"`).
    Tuesday,
    /// Wednesday (`"we"`).
    Wednesday,
    /// Thursday (`"th"`).
    Thursday,
    /// Friday (`"fr"`).
    Friday,
    /// Saturday (`"sa"`).
    Saturday,
    /// Sunday (`"su"`).
    Sunday,
    /// Any day string not recognised by this implementation. RFC 8984 §4.3.3
    /// defines a closed set, but `Other` is kept for forward-compatibility
    /// and lossless round-trip of unexpected wire values.
    Other(String),
}

impl_string_enum!(Weekday, "a JSCalendar weekday string",
    "mo" => Monday,
    "tu" => Tuesday,
    "we" => Wednesday,
    "th" => Thursday,
    "fr" => Friday,
    "sa" => Saturday,
    "su" => Sunday,
);

/// Recurrence frequency (RFC 8984 §4.3.3).
///
/// The seven canonical lowercase frequency values from the iCalendar FREQ
/// part. Used for [`RecurrenceRule::frequency`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Frequency {
    /// `"yearly"`.
    Yearly,
    /// `"monthly"`.
    Monthly,
    /// `"weekly"`.
    Weekly,
    /// `"daily"`.
    Daily,
    /// `"hourly"`.
    Hourly,
    /// `"minutely"`.
    Minutely,
    /// `"secondly"`.
    Secondly,
    /// Any frequency string not recognised by this implementation. RFC 8984
    /// §4.3.3 defines a closed set, but `Other` is kept for
    /// forward-compatibility and lossless round-trip of unexpected wire values.
    Other(String),
}

impl_string_enum!(Frequency, "a JSCalendar RecurrenceRule frequency string",
    "yearly"   => Yearly,
    "monthly"  => Monthly,
    "weekly"   => Weekly,
    "daily"    => Daily,
    "hourly"   => Hourly,
    "minutely" => Minutely,
    "secondly" => Secondly,
);

/// Behaviour when a recurrence rule produces an invalid date (RFC 8984 §4.3.3).
///
/// Maps to the iCalendar RSCALE SKIP part. Only meaningful when the rule's
/// frequency is `"yearly"` or `"monthly"`. Used for [`RecurrenceRule::skip`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecurrenceSkip {
    /// Omit invalid dates from the expansion (`"omit"`).
    Omit,
    /// Move invalid dates backward to the previous valid date (`"backward"`).
    Backward,
    /// Move invalid dates forward to the next valid date (`"forward"`).
    Forward,
    /// Any skip mode not recognised by this implementation. RFC 8984 §4.3.3
    /// defines a closed set, but `Other` is kept for forward-compatibility
    /// and lossless round-trip of unexpected wire values.
    Other(String),
}

impl_string_enum!(RecurrenceSkip, "a JSCalendar RecurrenceRule skip mode string",
    "omit"     => Omit,
    "backward" => Backward,
    "forward"  => Forward,
);

/// Relation of a sub-object to the event/task time (RFC 8984 §4.2.5, §4.5.2).
///
/// Used by [`Location::relative_to`] (to associate a physical location with
/// the start or end of an event) and [`OffsetTrigger::relative_to`] (to anchor
/// an alert offset to the start or end of the calendar object).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RelativeTo {
    /// Relative to the start of the calendar object (`"start"`).
    Start,
    /// Relative to the end/due time of the calendar object (`"end"`).
    End,
    /// Any relativeTo value not recognised by this implementation. RFC 8984
    /// §4.2.5 permits IANA-registered or vendor-specific values; `Other`
    /// preserves those for round-trip fidelity. RFC 8984 §4.5.2 defines a
    /// closed set for OffsetTrigger but `Other` is kept for
    /// forward-compatibility.
    Other(String),
}

impl_string_enum!(RelativeTo, "a JSCalendar relativeTo string",
    "start" => Start,
    "end"   => End,
);

/// Kind of participant (RFC 8984 §4.4.6).
///
/// Used for [`Participant::kind`]. The spec permits IANA-registered or
/// vendor-specific values; unrecognised values land in `Other` and are
/// preserved for round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParticipantKind {
    /// A single person (`"individual"`).
    Individual,
    /// A collection of people invited as a whole (`"group"`).
    Group,
    /// A physical location that needs to be scheduled (`"location"`).
    Location,
    /// A non-human resource other than a location, e.g. a projector
    /// (`"resource"`).
    Resource,
    /// Any participant kind not recognised by this implementation.
    Other(String),
}

impl_string_enum!(ParticipantKind, "a JSCalendar Participant kind string",
    "individual" => Individual,
    "group"      => Group,
    "location"   => Location,
    "resource"   => Resource,
);

/// Participation status of a participant in an event/task (RFC 8984 §4.4.6).
///
/// Used for [`Participant::participation_status`]. The spec permits
/// IANA-registered or vendor-specific values; unrecognised values land in
/// `Other` and are preserved for round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParticipationStatus {
    /// No status has yet been set by the participant (`"needs-action"`).
    NeedsAction,
    /// The invited participant will participate (`"accepted"`).
    Accepted,
    /// The invited participant will not participate (`"declined"`).
    Declined,
    /// The invited participant may participate (`"tentative"`).
    Tentative,
    /// The invited participant has delegated their attendance to another
    /// participant (`"delegated"`).
    Delegated,
    /// Any participation status not recognised by this implementation.
    Other(String),
}

impl_string_enum!(ParticipationStatus, "a JSCalendar Participant participationStatus string",
    "needs-action" => NeedsAction,
    "accepted"     => Accepted,
    "declined"     => Declined,
    "tentative"    => Tentative,
    "delegated"    => Delegated,
);

/// Who is responsible for sending scheduling messages (RFC 8984 §4.4.6).
///
/// Used for [`Participant::schedule_agent`]. The spec permits
/// IANA-registered or vendor-specific values; unrecognised values land in
/// `Other` and are preserved for round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScheduleAgent {
    /// The calendar server will send the scheduling messages (`"server"`).
    Server,
    /// The calendar client will send the scheduling messages (`"client"`).
    Client,
    /// No scheduling messages are to be sent to this participant (`"none"`).
    None,
    /// Any scheduling agent value not recognised by this implementation.
    Other(String),
}

impl_string_enum!(ScheduleAgent, "a JSCalendar Participant scheduleAgent string",
    "server" => Server,
    "client" => Client,
    "none"   => None,
);

/// How to present an alert (RFC 8984 §4.5.2).
///
/// Used for [`Alert::action`]. The spec permits IANA-registered or
/// vendor-specific values; unrecognised values land in `Other` and are
/// preserved for round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AlertAction {
    /// Display the alert on the user's device (`"display"`).
    Display,
    /// Send an email notification (`"email"`).
    Email,
    /// Any alert action not recognised by this implementation.
    Other(String),
}

impl_string_enum!(AlertAction, "a JSCalendar Alert action string",
    "display" => Display,
    "email"   => Email,
);

// ── RecurrenceRule ────────────────────────────────────────────────────────────

/// The `nthOfPeriod` field of an [`NDay`] entry (RFC 8984 §4.3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NDay {
    /// Object type discriminator; always `"NDay"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Day of the week. See [`Weekday`] for the seven canonical values.
    pub day: Weekday,

    /// Which occurrence within the period (non-zero integer), or `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nth_of_period: Option<i32>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Recurrence frequency. See [`Frequency`] for the seven canonical values.
    pub frequency: Frequency,

    /// Interval between recurrences (≥ 1; default 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,

    /// Calendar system (default `"gregorian"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rscale: Option<String>,

    /// How to handle skipped dates. See [`RecurrenceSkip`] for the canonical
    /// values; defaults to [`RecurrenceSkip::Omit`] when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<RecurrenceSkip>,

    /// First day of week (default [`Weekday::Monday`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_day_of_week: Option<Weekday>,

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

    /// The recurrence ends on or before this `LocalDateTime`
    /// (RFC 8984 §4.3.3 — `until` is a LocalDateTime, NOT a UTC date-time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<LocalDateTime>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Relation of this location to the event. See [`RelativeTo`] for the
    /// canonical values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_to: Option<RelativeTo>,

    /// IANA time zone id for this location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,

    /// Geographic coordinates as a `geo:` URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<String>,

    /// Attachments and images associated with this location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<HashMap<String, Link>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Kind of participant. See [`ParticipantKind`] for the canonical values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ParticipantKind>,

    /// Map of role URIs → `true` (e.g. `"owner"`, `"attendee"`, `"chair"`).
    pub roles: HashMap<String, bool>,

    /// Id of the location this participant is associated with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_id: Option<String>,

    /// BCP 47 language tag for this participant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Participation status. See [`ParticipationStatus`] for the canonical
    /// values; defaults to [`ParticipationStatus::NeedsAction`] when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participation_status: Option<ParticipationStatus>,

    /// Free-form comment on participation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participation_comment: Option<String>,

    /// Whether the participant is expected to send a reply (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect_reply: Option<bool>,

    /// Scheduling agent. See [`ScheduleAgent`] for the canonical values;
    /// defaults to [`ScheduleAgent::Server`] when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_agent: Option<ScheduleAgent>,

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
    pub schedule_updated: Option<UTCDate>,

    /// iTIP status codes from the most recent scheduling message sent to this
    /// participant (RFC 8984 §4.4.6).
    ///
    /// An array of iTIP status code strings (e.g. `"1.0"`, `"2.0"`, `"5.0"`).
    /// Server-set and persisted; absent when no scheduling has occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_status: Option<Vec<String>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Duration offset from `relative_to`.
    pub offset: SignedDuration,

    /// Whether to measure from start or end of the event. See [`RelativeTo`]
    /// for the canonical values; defaults to [`RelativeTo::Start`] when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_to: Option<RelativeTo>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    pub when: UTCDate,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    pub acknowledged: Option<UTCDate>,

    /// Related alerts (e.g. for snooze chains); keys are alert ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_to: Option<HashMap<String, Relation>>,

    /// How to present the alert. See [`AlertAction`] for the canonical values;
    /// defaults to [`AlertAction::Display`] when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<AlertAction>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── TimeZone / TimeZoneRule ───────────────────────────────────────────────────

/// A STANDARD or DAYLIGHT sub-component of a [`TimeZone`] (RFC 8984 §4.7.2).
///
/// Maps to a VTIMEZONE STANDARD or DAYLIGHT sub-component from iCalendar.
/// At most one recurrence rule is allowed per rule.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeZoneRule {
    /// Object type discriminator; always `"TimeZoneRule"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// DTSTART from iCalendar — the local date-time the rule first applies.
    pub start: LocalDateTime,

    /// TZOFFSETFROM from iCalendar — the UTC offset in effect before the
    /// transition (format `±HHMM` or `±HHMMSS`).
    pub offset_from: String,

    /// TZOFFSETTO from iCalendar — the UTC offset in effect after the
    /// transition (format `±HHMM` or `±HHMMSS`).
    pub offset_to: String,

    /// RRULE from iCalendar — recurrence rules for the transition.
    /// Per RFC 8984 §4.7.2 the `until` value MUST be interpreted as a
    /// local time in the UTC time zone during evaluation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_rules: Option<Vec<RecurrenceRule>>,

    /// RDATE properties from iCalendar — additional explicit transition
    /// dates. Keys are LocalDateTime strings; the PatchObject value MUST
    /// be the empty JSON object (`{}`) per RFC 8984 §4.7.2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_overrides: Option<HashMap<LocalDateTime, PatchObject>>,

    /// TZNAME properties from iCalendar — set of human-readable names
    /// for this rule. The map value MUST be `true` for each key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<HashMap<String, bool>>,

    /// COMMENT properties from iCalendar — order MUST be preserved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<Vec<String>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A time-zone definition embedded in `CalendarEvent.timeZones` or
/// `Task.timeZones` (RFC 8984 §4.7.2).
///
/// Maps to a VTIMEZONE component from iCalendar. A valid TimeZone MUST
/// define at least one transition rule in `standard` or `daylight`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeZone {
    /// Object type discriminator; always `"TimeZone"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// TZID from iCalendar — the time-zone identifier.
    ///
    /// MUST be a valid `paramtext` value per RFC 5545 §3.1.
    pub tz_id: String,

    /// LAST-MODIFIED from iCalendar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<UTCDate>,

    /// TZURL from iCalendar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// TZUNTIL from iCalendar (RFC 7808).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<UTCDate>,

    /// TZID-ALIAS-OF properties from iCalendar (RFC 7808). Map keys are
    /// the alias identifiers; the value MUST be `true` for each key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aliases: Option<HashMap<String, bool>>,

    /// STANDARD sub-components from iCalendar. Order MUST be preserved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standard: Option<Vec<TimeZoneRule>>,

    /// DAYLIGHT sub-components from iCalendar. Order MUST be preserved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daylight: Option<Vec<TimeZoneRule>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    //! Wire-format regression tests for the newtype-typed temporal fields
    //! introduced by bd:JMAP-sc1b.74.
    //!
    //! These tests deserialize hand-built JSON whose shape matches
    //! RFC 8984 examples, then re-serialize and compare. The oracle is the
    //! input JSON — never the code under test. They exist to catch a
    //! regression where the newtype loses its transparent serde behaviour
    //! (e.g. by adding a second field) and wraps the value in `[…]` or
    //! `{"0": …}` on the wire.
    use super::*;
    use serde_json::json;

    /// Oracle: `RecurrenceRule.until` serializes as a bare LocalDateTime
    /// string (RFC 8984 §4.3.3 example shape), not a wrapped array or
    /// object.
    #[test]
    fn recurrence_rule_until_is_bare_string_on_the_wire() {
        let raw = json!({
            "@type": "RecurrenceRule",
            "frequency": "monthly",
            "until": "2024-12-31T23:59:59"
        });
        let rule: RecurrenceRule =
            serde_json::from_value(raw.clone()).expect("RecurrenceRule must deserialize");
        // Sanity-check that the canary value really did land in the field.
        assert_eq!(
            rule.until.as_ref().map(AsRef::as_ref),
            Some("2024-12-31T23:59:59"),
            "until must deserialize into a LocalDateTime carrying the wire string"
        );

        let round_tripped = serde_json::to_value(&rule).expect("serialize must succeed");
        assert_eq!(
            round_tripped["until"],
            json!("2024-12-31T23:59:59"),
            "until must serialize as a bare string; got {round_tripped:?}"
        );
    }

    /// Oracle: `OffsetTrigger.offset` serializes as a bare SignedDuration
    /// string (RFC 8984 §4.5.2 example: `"-PT15M"`).
    #[test]
    fn offset_trigger_offset_is_bare_string_on_the_wire() {
        let raw = json!({
            "@type": "OffsetTrigger",
            "offset": "-PT15M"
        });
        let trigger: OffsetTrigger =
            serde_json::from_value(raw).expect("OffsetTrigger must deserialize");
        assert_eq!(
            trigger.offset.as_ref(),
            "-PT15M",
            "offset must deserialize into a SignedDuration"
        );

        let round_tripped = serde_json::to_value(&trigger).expect("serialize must succeed");
        assert_eq!(
            round_tripped["offset"],
            json!("-PT15M"),
            "offset must serialize as a bare string; got {round_tripped:?}"
        );
    }

    /// Oracle: `AbsoluteTrigger.when` serializes as a bare UTC date-time
    /// string (RFC 8984 §4.5.2 example: `"2024-06-15T08:45:00Z"`).
    #[test]
    fn absolute_trigger_when_is_bare_string_on_the_wire() {
        let raw = json!({
            "@type": "AbsoluteTrigger",
            "when": "2024-06-15T08:45:00Z"
        });
        let trigger: AbsoluteTrigger =
            serde_json::from_value(raw).expect("AbsoluteTrigger must deserialize");
        assert_eq!(
            trigger.when.as_ref(),
            "2024-06-15T08:45:00Z",
            "when must deserialize into a UTCDate"
        );

        let round_tripped = serde_json::to_value(&trigger).expect("serialize must succeed");
        assert_eq!(
            round_tripped["when"],
            json!("2024-06-15T08:45:00Z"),
            "when must serialize as a bare string; got {round_tripped:?}"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.4) ───────────────────
    //
    // One round-trip preservation test per migrated type. Each asserts
    // that an unknown vendor / site / private-extension field survives
    // deserialize/serialize unchanged. Per workspace AGENTS.md
    // "Extras-preservation policy for vendor/site fields".

    /// `NDay.extra` captures vendor fields and preserves them.
    #[test]
    fn nday_preserves_vendor_extras() {
        let raw = json!({
            "@type": "NDay",
            "day": "mo",
            "acmeCorpDayLabel": "first-mon"
        });
        let n: NDay = serde_json::from_value(raw).unwrap();
        assert_eq!(
            n.extra.get("acmeCorpDayLabel").and_then(|v| v.as_str()),
            Some("first-mon")
        );
        let back = serde_json::to_value(&n).unwrap();
        assert_eq!(back["acmeCorpDayLabel"], "first-mon");
    }

    /// `RecurrenceRule.extra` captures vendor fields and preserves them.
    #[test]
    fn recurrence_rule_preserves_vendor_extras() {
        let raw = json!({
            "@type": "RecurrenceRule",
            "frequency": "monthly",
            "acmeCorpRuleNote": "billing-cycle"
        });
        let r: RecurrenceRule = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpRuleNote").and_then(|v| v.as_str()),
            Some("billing-cycle")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpRuleNote"], "billing-cycle");
    }

    /// `Location.extra` captures vendor fields and preserves them.
    #[test]
    fn location_preserves_vendor_extras() {
        let raw = json!({
            "@type": "Location",
            "name": "HQ",
            "acmeCorpInternalCode": "bldg-7"
        });
        let l: Location = serde_json::from_value(raw).unwrap();
        assert_eq!(
            l.extra.get("acmeCorpInternalCode").and_then(|v| v.as_str()),
            Some("bldg-7")
        );
        let back = serde_json::to_value(&l).unwrap();
        assert_eq!(back["acmeCorpInternalCode"], "bldg-7");
    }

    /// `VirtualLocation.extra` captures vendor fields and preserves them.
    #[test]
    fn virtual_location_preserves_vendor_extras() {
        let raw = json!({
            "@type": "VirtualLocation",
            "uri": "https://example.com/meet/42",
            "acmeCorpMeetingId": "meet-42"
        });
        let v: VirtualLocation = serde_json::from_value(raw).unwrap();
        assert_eq!(
            v.extra.get("acmeCorpMeetingId").and_then(|x| x.as_str()),
            Some("meet-42")
        );
        let back = serde_json::to_value(&v).unwrap();
        assert_eq!(back["acmeCorpMeetingId"], "meet-42");
    }

    /// `Link.extra` captures vendor fields and preserves them.
    #[test]
    fn link_preserves_vendor_extras() {
        let raw = json!({
            "@type": "Link",
            "href": "https://example.com/x",
            "acmeCorpClassification": "internal"
        });
        let l: Link = serde_json::from_value(raw).unwrap();
        assert_eq!(
            l.extra
                .get("acmeCorpClassification")
                .and_then(|v| v.as_str()),
            Some("internal")
        );
        let back = serde_json::to_value(&l).unwrap();
        assert_eq!(back["acmeCorpClassification"], "internal");
    }

    /// `Relation.extra` captures vendor fields and preserves them.
    #[test]
    fn relation_preserves_vendor_extras() {
        let raw = json!({
            "@type": "Relation",
            "acmeCorpDirection": "outbound"
        });
        let r: Relation = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpDirection").and_then(|v| v.as_str()),
            Some("outbound")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpDirection"], "outbound");
    }

    /// `Participant.extra` captures vendor fields and preserves them.
    #[test]
    fn participant_preserves_vendor_extras() {
        let raw = json!({
            "@type": "Participant",
            "roles": {"attendee": true},
            "acmeCorpEmployeeId": "emp-42"
        });
        let p: Participant = serde_json::from_value(raw).unwrap();
        assert_eq!(
            p.extra.get("acmeCorpEmployeeId").and_then(|v| v.as_str()),
            Some("emp-42")
        );
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["acmeCorpEmployeeId"], "emp-42");
    }

    /// `OffsetTrigger.extra` captures vendor fields and preserves them.
    #[test]
    fn offset_trigger_preserves_vendor_extras() {
        let raw = json!({
            "@type": "OffsetTrigger",
            "offset": "-PT15M",
            "acmeCorpClientTag": "mobile"
        });
        let t: OffsetTrigger = serde_json::from_value(raw).unwrap();
        assert_eq!(
            t.extra.get("acmeCorpClientTag").and_then(|v| v.as_str()),
            Some("mobile")
        );
        let back = serde_json::to_value(&t).unwrap();
        assert_eq!(back["acmeCorpClientTag"], "mobile");
    }

    /// `AbsoluteTrigger.extra` captures vendor fields and preserves them.
    #[test]
    fn absolute_trigger_preserves_vendor_extras() {
        let raw = json!({
            "@type": "AbsoluteTrigger",
            "when": "2024-06-15T08:45:00Z",
            "acmeCorpTriggerSource": "iCal"
        });
        let t: AbsoluteTrigger = serde_json::from_value(raw).unwrap();
        assert_eq!(
            t.extra
                .get("acmeCorpTriggerSource")
                .and_then(|v| v.as_str()),
            Some("iCal")
        );
        let back = serde_json::to_value(&t).unwrap();
        assert_eq!(back["acmeCorpTriggerSource"], "iCal");
    }

    /// `Alert.extra` captures vendor fields and preserves them.
    #[test]
    fn alert_preserves_vendor_extras() {
        let raw = json!({
            "@type": "Alert",
            "trigger": {
                "@type": "OffsetTrigger",
                "offset": "-PT15M"
            },
            "acmeCorpAlertChannel": "mobile-push"
        });
        let a: Alert = serde_json::from_value(raw).unwrap();
        assert_eq!(
            a.extra.get("acmeCorpAlertChannel").and_then(|v| v.as_str()),
            Some("mobile-push")
        );
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["acmeCorpAlertChannel"], "mobile-push");
    }

    /// `TimeZoneRule.extra` captures vendor fields and preserves them.
    #[test]
    fn time_zone_rule_preserves_vendor_extras() {
        let raw = json!({
            "@type": "TimeZoneRule",
            "start": "1970-01-01T00:00:00",
            "offsetFrom": "+0000",
            "offsetTo": "+0000",
            "acmeCorpRuleOrigin": "iana-tzdata-2024a"
        });
        let r: TimeZoneRule = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpRuleOrigin").and_then(|v| v.as_str()),
            Some("iana-tzdata-2024a")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpRuleOrigin"], "iana-tzdata-2024a");
    }

    /// `TimeZone.extra` captures vendor fields and preserves them.
    #[test]
    fn time_zone_preserves_vendor_extras() {
        let raw = json!({
            "@type": "TimeZone",
            "tzId": "Etc/UTC",
            "acmeCorpDataSource": "iana"
        });
        let t: TimeZone = serde_json::from_value(raw).unwrap();
        assert_eq!(
            t.extra.get("acmeCorpDataSource").and_then(|v| v.as_str()),
            Some("iana")
        );
        let back = serde_json::to_value(&t).unwrap();
        assert_eq!(back["acmeCorpDataSource"], "iana");
    }

    /// Oracle: a minimal `TimeZone` with a STANDARD rule round-trips per
    /// RFC 8984 §4.7.2. The wire shape — `tzId`, `@type` discriminators on
    /// both TimeZone and TimeZoneRule, and `offsetFrom` / `offsetTo` as
    /// signed offset strings — comes directly from the spec text.
    #[test]
    fn time_zone_with_standard_rule_round_trips() {
        let raw = json!({
            "@type": "TimeZone",
            "tzId": "Europe/Berlin",
            "standard": [{
                "@type": "TimeZoneRule",
                "start": "1996-10-27T03:00:00",
                "offsetFrom": "+0200",
                "offsetTo": "+0100",
                "recurrenceRules": [{
                    "@type": "RecurrenceRule",
                    "frequency": "yearly",
                    "byMonth": ["10"],
                    "byDay": [{
                        "@type": "NDay",
                        "day": "su",
                        "nthOfPeriod": -1
                    }]
                }],
                "names": {"CET": true}
            }],
            "daylight": [{
                "@type": "TimeZoneRule",
                "start": "1996-03-31T02:00:00",
                "offsetFrom": "+0100",
                "offsetTo": "+0200",
                "recurrenceRules": [{
                    "@type": "RecurrenceRule",
                    "frequency": "yearly",
                    "byMonth": ["3"],
                    "byDay": [{
                        "@type": "NDay",
                        "day": "su",
                        "nthOfPeriod": -1
                    }]
                }],
                "names": {"CEST": true}
            }]
        });
        let tz: TimeZone = serde_json::from_value(raw.clone()).expect("TimeZone must deserialize");
        assert_eq!(tz.tz_id, "Europe/Berlin");
        assert_eq!(tz.standard.as_ref().map(Vec::len), Some(1));
        assert_eq!(tz.daylight.as_ref().map(Vec::len), Some(1));
        let standard = &tz.standard.as_ref().unwrap()[0];
        assert_eq!(standard.offset_from, "+0200");
        assert_eq!(standard.offset_to, "+0100");
        assert_eq!(
            standard.recurrence_rules.as_ref().map(Vec::len),
            Some(1),
            "STANDARD rule must carry exactly one RRULE per RFC 8984 §4.7.2"
        );

        let back = serde_json::to_value(&tz).expect("serialize must succeed");
        assert_eq!(back, raw, "round-trip must preserve wire shape");
    }

    /// Oracle: `TimeZoneRule.recurrenceOverrides` is a `LocalDateTime[PatchObject]`
    /// map; per RFC 8984 §4.7.2 the patch object MUST be the empty `{}`.
    /// This test verifies the typed map deserializes and the empty-patch
    /// constraint survives round-trip.
    #[test]
    fn time_zone_rule_recurrence_overrides_round_trips() {
        let raw = json!({
            "@type": "TimeZoneRule",
            "start": "1970-01-01T00:00:00",
            "offsetFrom": "+0000",
            "offsetTo": "+0000",
            "recurrenceOverrides": {
                "1990-04-01T02:00:00": {},
                "1991-04-07T02:00:00": {}
            }
        });
        let r: TimeZoneRule = serde_json::from_value(raw).expect("TimeZoneRule must deserialize");
        let overrides = r
            .recurrence_overrides
            .as_ref()
            .expect("recurrenceOverrides must deserialize as Some");
        assert_eq!(overrides.len(), 2);
        for v in overrides.values() {
            assert!(
                v.as_map().is_empty(),
                "PatchObject value MUST be empty per RFC 8984 §4.7.2"
            );
        }
    }

    // ── Enumerated-string round-trip tests (JMAP-sc1b.77) ────────────────
    //
    // Each enum gets two oracles: (1) every known variant deserializes from
    // its canonical wire string and re-serializes to the same string, and
    // (2) an unrecognised wire value round-trips losslessly through the
    // `Other(String)` catch-all. The oracles are the spec-mandated literal
    // wire strings — never the code under test.

    /// Oracle: every `Weekday` variant maps to the RFC 8984 §4.3.3
    /// lowercase two-letter day code, and an unknown day round-trips as
    /// `Other`.
    #[test]
    fn weekday_known_variants_round_trip() {
        let cases = [
            (r#""mo""#, Weekday::Monday),
            (r#""tu""#, Weekday::Tuesday),
            (r#""we""#, Weekday::Wednesday),
            (r#""th""#, Weekday::Thursday),
            (r#""fr""#, Weekday::Friday),
            (r#""sa""#, Weekday::Saturday),
            (r#""su""#, Weekday::Sunday),
        ];
        for (wire, expected) in cases {
            let got: Weekday = serde_json::from_str(wire).expect("weekday deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn weekday_unknown_round_trips_via_other() {
        let raw = r#""xx""#;
        let got: Weekday = serde_json::from_str(raw).expect("unknown weekday deserialize");
        assert_eq!(got, Weekday::Other("xx".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: every `Frequency` variant maps to the RFC 8984 §4.3.3
    /// canonical lowercase token; unknown frequencies round-trip via `Other`.
    #[test]
    fn frequency_known_variants_round_trip() {
        let cases = [
            (r#""yearly""#, Frequency::Yearly),
            (r#""monthly""#, Frequency::Monthly),
            (r#""weekly""#, Frequency::Weekly),
            (r#""daily""#, Frequency::Daily),
            (r#""hourly""#, Frequency::Hourly),
            (r#""minutely""#, Frequency::Minutely),
            (r#""secondly""#, Frequency::Secondly),
        ];
        for (wire, expected) in cases {
            let got: Frequency = serde_json::from_str(wire).expect("frequency deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn frequency_unknown_round_trips_via_other() {
        let raw = r#""quarterly""#;
        let got: Frequency = serde_json::from_str(raw).expect("unknown frequency deserialize");
        assert_eq!(got, Frequency::Other("quarterly".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: every `RecurrenceSkip` variant maps to the RFC 8984 §4.3.3
    /// canonical token; unknown skip modes round-trip via `Other`.
    #[test]
    fn recurrence_skip_known_variants_round_trip() {
        let cases = [
            (r#""omit""#, RecurrenceSkip::Omit),
            (r#""backward""#, RecurrenceSkip::Backward),
            (r#""forward""#, RecurrenceSkip::Forward),
        ];
        for (wire, expected) in cases {
            let got: RecurrenceSkip = serde_json::from_str(wire).expect("skip deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn recurrence_skip_unknown_round_trips_via_other() {
        let raw = r#""sideways""#;
        let got: RecurrenceSkip = serde_json::from_str(raw).expect("unknown skip deserialize");
        assert_eq!(got, RecurrenceSkip::Other("sideways".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: `RelativeTo::{Start, End}` map to `"start"` / `"end"` per
    /// RFC 8984 §4.2.5 and §4.5.2; unknown values round-trip via `Other`.
    #[test]
    fn relative_to_known_variants_round_trip() {
        let cases = [
            (r#""start""#, RelativeTo::Start),
            (r#""end""#, RelativeTo::End),
        ];
        for (wire, expected) in cases {
            let got: RelativeTo = serde_json::from_str(wire).expect("relativeTo deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn relative_to_unknown_round_trips_via_other() {
        let raw = r#""halfway""#;
        let got: RelativeTo = serde_json::from_str(raw).expect("unknown relativeTo deserialize");
        assert_eq!(got, RelativeTo::Other("halfway".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: `ParticipantKind` known variants per RFC 8984 §4.4.6;
    /// unknown values round-trip via `Other`.
    #[test]
    fn participant_kind_known_variants_round_trip() {
        let cases = [
            (r#""individual""#, ParticipantKind::Individual),
            (r#""group""#, ParticipantKind::Group),
            (r#""location""#, ParticipantKind::Location),
            (r#""resource""#, ParticipantKind::Resource),
        ];
        for (wire, expected) in cases {
            let got: ParticipantKind =
                serde_json::from_str(wire).expect("participant kind deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn participant_kind_unknown_round_trips_via_other() {
        let raw = r#""robot""#;
        let got: ParticipantKind =
            serde_json::from_str(raw).expect("unknown participant kind deserialize");
        assert_eq!(got, ParticipantKind::Other("robot".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: `ParticipationStatus` known variants per RFC 8984 §4.4.6.
    /// The hyphenated `"needs-action"` exercises the macro's literal-string
    /// matching. Unknown values round-trip via `Other`.
    #[test]
    fn participation_status_known_variants_round_trip() {
        let cases = [
            (r#""needs-action""#, ParticipationStatus::NeedsAction),
            (r#""accepted""#, ParticipationStatus::Accepted),
            (r#""declined""#, ParticipationStatus::Declined),
            (r#""tentative""#, ParticipationStatus::Tentative),
            (r#""delegated""#, ParticipationStatus::Delegated),
        ];
        for (wire, expected) in cases {
            let got: ParticipationStatus =
                serde_json::from_str(wire).expect("participation status deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn participation_status_unknown_round_trips_via_other() {
        let raw = r#""maybe""#;
        let got: ParticipationStatus =
            serde_json::from_str(raw).expect("unknown participation status deserialize");
        assert_eq!(got, ParticipationStatus::Other("maybe".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: `ScheduleAgent` known variants per RFC 8984 §4.4.6;
    /// unknown values round-trip via `Other`.
    #[test]
    fn schedule_agent_known_variants_round_trip() {
        let cases = [
            (r#""server""#, ScheduleAgent::Server),
            (r#""client""#, ScheduleAgent::Client),
            (r#""none""#, ScheduleAgent::None),
        ];
        for (wire, expected) in cases {
            let got: ScheduleAgent =
                serde_json::from_str(wire).expect("schedule agent deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn schedule_agent_unknown_round_trips_via_other() {
        let raw = r#""peer""#;
        let got: ScheduleAgent =
            serde_json::from_str(raw).expect("unknown schedule agent deserialize");
        assert_eq!(got, ScheduleAgent::Other("peer".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: `AlertAction` known variants per RFC 8984 §4.5.2;
    /// unknown values round-trip via `Other`.
    #[test]
    fn alert_action_known_variants_round_trip() {
        let cases = [
            (r#""display""#, AlertAction::Display),
            (r#""email""#, AlertAction::Email),
        ];
        for (wire, expected) in cases {
            let got: AlertAction = serde_json::from_str(wire).expect("alert action deserialize");
            assert_eq!(got, expected);
            assert_eq!(serde_json::to_string(&got).unwrap(), wire);
        }
    }

    #[test]
    fn alert_action_unknown_round_trips_via_other() {
        let raw = r#""sms""#;
        let got: AlertAction = serde_json::from_str(raw).expect("unknown alert action deserialize");
        assert_eq!(got, AlertAction::Other("sms".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), raw);
    }

    /// Oracle: an NDay carrying a typed `Weekday` round-trips through the
    /// enclosing RecurrenceRule unchanged. Catches a regression where
    /// the field's serde behaviour deviates from the raw-string form.
    #[test]
    fn recurrence_rule_with_typed_weekday_round_trips() {
        let raw = json!({
            "@type": "RecurrenceRule",
            "frequency": "weekly",
            "byDay": [
                {"@type": "NDay", "day": "mo"},
                {"@type": "NDay", "day": "we"},
                {"@type": "NDay", "day": "fr"}
            ],
            "firstDayOfWeek": "su"
        });
        let rule: RecurrenceRule = serde_json::from_value(raw.clone()).expect("deserialize");
        assert_eq!(rule.frequency, Frequency::Weekly);
        assert_eq!(rule.first_day_of_week, Some(Weekday::Sunday));
        let days = rule.by_day.as_ref().expect("byDay present");
        assert_eq!(days[0].day, Weekday::Monday);
        assert_eq!(days[1].day, Weekday::Wednesday);
        assert_eq!(days[2].day, Weekday::Friday);
        let back = serde_json::to_value(&rule).expect("serialize");
        assert_eq!(back, raw, "wire shape must round-trip unchanged");
    }

    /// Oracle: a Participant carrying all three typed enums round-trips
    /// through the enclosing JSON unchanged.
    #[test]
    fn participant_with_typed_enums_round_trips() {
        let raw = json!({
            "@type": "Participant",
            "kind": "individual",
            "roles": {"attendee": true},
            "participationStatus": "accepted",
            "scheduleAgent": "server"
        });
        let p: Participant = serde_json::from_value(raw.clone()).expect("deserialize");
        assert_eq!(p.kind, Some(ParticipantKind::Individual));
        assert_eq!(p.participation_status, Some(ParticipationStatus::Accepted));
        assert_eq!(p.schedule_agent, Some(ScheduleAgent::Server));
        let back = serde_json::to_value(&p).expect("serialize");
        assert_eq!(back, raw, "wire shape must round-trip unchanged");
    }
}
