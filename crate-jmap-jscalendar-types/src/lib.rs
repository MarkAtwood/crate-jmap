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
//!
//! ## Spec-driven divergences (deliberate, do not "fix")
//!
//! Three design choices in this crate look like inconsistency at a glance but
//! are deliberate spec-compliance decisions; preserve them against future
//! "consistency" or "simplification" PRs.
//!
//! 1. **Bare `String` `at_type` (not `Option<String>`).**  Diverges from the
//!    sibling `jmap-jscontact-types` which uses `Option<String>`.  Spec
//!    authority: RFC 8984 marks every `@type` discriminator as
//!    `(mandatory)` with zero `defaultType` annotations; RFC 9553 §1.3.4
//!    introduces `defaultType` and permits omitting `@type` in
//!    implied-type positions.  Workspace canonical-templates rule
//!    explicitly permits "differences mandated by the relevant RFC or
//!    draft".  See `PLAN.md` and `bd:JMAP-sgrr.3`.
//!
//! 2. **`AlertTrigger::Unknown(serde_json::Value)`.**  Holds an opaque
//!    `Value` rather than a typed struct or a `String`.  Spec authority:
//!    RFC 8984 §4.5.2 "Implementations MUST NOT trigger for trigger types
//!    they do not understand but MUST preserve them."  A typed
//!    `Unknown(String)` would discard the inner fields; a typed
//!    `Unknown { at_type, fields }` would force a schema on what is
//!    explicitly unschema'd.  The manual `Deserialize` impl is required
//!    because serde does not support `#[serde(tag = "@type", other)]`
//!    with non-unit tuple variants.  See [`AlertTrigger`] rustdoc.
//!
//! 3. **`serde_json::{Map, Value}` in the public API surface.**  The
//!    workspace extras-preservation policy mandates a
//!    `pub extra: serde_json::Map<String, serde_json::Value>` field on
//!    every wire-format struct, and `AlertTrigger::Unknown` carries a
//!    raw `Value`.  This locks the crate's major version to
//!    `serde_json`'s, which is the explicit trade-off: round-trip
//!    fidelity for vendor / site / private fields outweighs the
//!    coupling cost.  See `PLAN.md` "Extras-preservation policy" and
//!    workspace `AGENTS.md`.
//!
//! ## Test-oracle discipline
//!
//! Test fixtures are constructed from `serde_json::json!({...})` literals
//! whose shape comes directly from RFC 8984 example text.  Per workspace
//! test-integrity rules, the oracle MUST be the spec example, NOT the code
//! under test.  Do NOT replace fixture construction with "build a typed
//! struct, serialize, deserialize, compare" — that pattern uses the code
//! under test as its own oracle.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// Re-export the `jmap-types` symbols that appear in this crate's public
// API surface so consumers can name them without taking a separate
// `jmap-types` dependency.  `Id` appears on `Link.blob_id`, `UTCDate` on
// `Participant.schedule_updated` / `progress_updated` and
// `AbsoluteTrigger.when` / `Alert.acknowledged` / `TimeZone.updated` /
// `TimeZone.valid_until`, and `PatchObject` on
// `TimeZoneRule.recurrence_overrides`.
pub use jmap_types::{Id, PatchObject, UTCDate};

// ── Type-tag discriminator ────────────────────────────────────────────────────

/// Mismatch between an object's `at_type` wire string and the
/// RFC 8984-mandated discriminator literal for its Rust type.
///
/// Returned by [`TypeDiscriminator::validate_at_type`].  RFC 8984 marks
/// every `@type` discriminator as `(mandatory)` and assigns a specific
/// string literal per type (e.g. `"NDay"`, `"Participant"`,
/// `"OffsetTrigger"`).  Deserialize itself does NOT enforce the match
/// so that round-trip preservation of unfamiliar payloads still works;
/// consumers that need strict input validation call
/// `validate_at_type()` after deserializing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeTagMismatch {
    /// The literal the Rust type expects (e.g. `"NDay"`).
    pub expected: &'static str,
    /// The literal carried in the deserialized value's `at_type` field.
    pub actual: String,
}

impl std::fmt::Display for TypeTagMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JSCalendar @type mismatch: expected {:?}, got {:?}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for TypeTagMismatch {}

/// Wire-format type-tag discriminator for JSCalendar sub-objects
/// (RFC 8984).
///
/// Each implementing struct names its mandatory `@type` wire literal in
/// the [`Self::TYPE_TAG`] associated const.  The default
/// [`Self::validate_at_type`] method compares the carried `at_type`
/// field against that literal.
///
/// Deserialize is deliberately permissive — it does NOT enforce the
/// match, so an object carrying a vendor-extended or future-spec
/// `@type` value can still be deserialized for round-trip preservation
/// per RFC 8984's preserve-mandate.  Consumers needing strict input
/// validation MUST call `validate_at_type()` explicitly after
/// deserializing.
pub trait TypeDiscriminator {
    /// The mandatory `@type` wire literal for this Rust type per
    /// RFC 8984.  Example: `"NDay"` for [`NDay`], `"Participant"` for
    /// [`Participant`].
    const TYPE_TAG: &'static str;

    /// The `at_type` field value carried by this instance.  Implementors
    /// just return `&self.at_type`.
    fn at_type(&self) -> &str;

    /// Validate that the carried `at_type` matches the RFC 8984
    /// mandatory discriminator literal for this Rust type.
    ///
    /// Default implementation compares against [`Self::TYPE_TAG`].
    /// Returns `Err(TypeTagMismatch)` on mismatch; `Ok(())` otherwise.
    fn validate_at_type(&self) -> Result<(), TypeTagMismatch> {
        if self.at_type() == Self::TYPE_TAG {
            Ok(())
        } else {
            Err(TypeTagMismatch {
                expected: Self::TYPE_TAG,
                actual: self.at_type().to_owned(),
            })
        }
    }
}

// ── Scalar wrappers ───────────────────────────────────────────────────────────

/// A date-time string without a timezone offset (RFC 8984 §1.4.5).
///
/// Format: `YYYY-MM-DDTHH:MM:SS` (no `Z`, no `±offset`).
///
/// # Validation
///
/// The `From<String>` and `From<&str>` constructors accept **any** string
/// without validating against the RFC 8984 §1.4.5 ABNF.  This is
/// deliberate: parsing the format is left to the backend (per `PLAN.md`)
/// to avoid pulling in a heavy date-time parser dependency.  Callers
/// MUST treat the inner string as opaque-but-presumed-well-formed and
/// validate at the system boundary.
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

impl std::fmt::Display for LocalDateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// An ISO 8601 duration string (RFC 8984 §1.4.6).
///
/// Example: `"PT1H"`, `"P1DT2H"`.
///
/// # Validation
///
/// The `From<String>` and `From<&str>` constructors accept **any** string
/// without validating against the RFC 8984 §1.4.6 ABNF.  This is
/// deliberate: parsing the format is left to the backend (per `PLAN.md`)
/// to avoid pulling in a heavy duration parser dependency.  Callers MUST
/// treat the inner string as opaque-but-presumed-well-formed and
/// validate at the system boundary.
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

impl std::fmt::Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A signed ISO 8601 duration string (RFC 8984 §1.4.7).
///
/// Like `Duration` but may be prefixed with `+` or `-`.
/// Example: `"-PT15M"`, `"+PT30M"`.
///
/// # Validation
///
/// The `From<String>` and `From<&str>` constructors accept **any** string
/// without validating against the RFC 8984 §1.4.7 ABNF.  This is
/// deliberate: parsing the format is left to the backend (per `PLAN.md`)
/// to avoid pulling in a heavy duration parser dependency.  Callers MUST
/// treat the inner string as opaque-but-presumed-well-formed and
/// validate at the system boundary.
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

impl std::fmt::Display for SignedDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A UTC offset string (RFC 5545 / RFC 8984 §4.7.2 — the TZOFFSETFROM /
/// TZOFFSETTO format).
///
/// Format: `±HHMM` or `±HHMMSS`.  Examples: `"+0100"`, `"-0500"`,
/// `"+053000"`.  Used by `TimeZoneRule.offset_from` / `offset_to`.
///
/// # Validation
///
/// The `From<String>` and `From<&str>` constructors accept **any** string
/// without validating against the format.  This is deliberate: parsing
/// is left to the backend (per `PLAN.md`).  Callers MUST treat the inner
/// string as opaque-but-presumed-well-formed and validate at the system
/// boundary.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UTCOffset(String);

impl From<String> for UTCOffset {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for UTCOffset {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for UTCOffset {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UTCOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── @type serde-default functions ─────────────────────────────────────────────
//
// RFC 8984 marks every `@type` discriminator as `(mandatory)`, but a
// spec-violating vendor server, partial fixture, or sub-object built via
// `serde_json::to_value` may omit the field. Without a serde default,
// such input fails the whole parent object's deserialize (e.g. a Task
// carrying a CheckItem whose @type was dropped).
//
// Each per-type function supplies the RFC 8984-mandated literal so
// deserialize is liberal in what it accepts while serialize still always
// emits the field. The shape stays bare `String` per this crate's
// documented design decision (see crate-level rustdoc item 1 and
// AGENTS.md). Mirrors the same pattern in the sibling
// `jmap-tasks-types` (bd:JMAP-ky8g.1) for Person / CheckItem /
// Checklist / Comment. See bd:JMAP-ky8g.10.

fn n_day_at_type_default() -> String {
    "NDay".to_owned()
}

fn recurrence_rule_at_type_default() -> String {
    "RecurrenceRule".to_owned()
}

fn location_at_type_default() -> String {
    "Location".to_owned()
}

fn virtual_location_at_type_default() -> String {
    "VirtualLocation".to_owned()
}

fn link_at_type_default() -> String {
    "Link".to_owned()
}

fn relation_at_type_default() -> String {
    "Relation".to_owned()
}

fn participant_at_type_default() -> String {
    "Participant".to_owned()
}

fn offset_trigger_at_type_default() -> String {
    "OffsetTrigger".to_owned()
}

fn absolute_trigger_at_type_default() -> String {
    "AbsoluteTrigger".to_owned()
}

fn alert_at_type_default() -> String {
    "Alert".to_owned()
}

fn time_zone_rule_at_type_default() -> String {
    "TimeZoneRule".to_owned()
}

fn time_zone_at_type_default() -> String {
    "TimeZone".to_owned()
}

// ── RecurrenceRule ────────────────────────────────────────────────────────────

/// The `nthOfPeriod` field of an [`NDay`] entry (RFC 8984 §4.3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NDay {
    /// Object type discriminator; always `"NDay"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"NDay"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "n_day_at_type_default")]
    pub at_type: String,

    /// Day of the week: `"mo"`, `"tu"`, `"we"`, `"th"`, `"fr"`, `"sa"`, `"su"`.
    pub day: String,

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

impl NDay {
    /// Construct a new `NDay` with the mandatory `day` value
    /// (RFC 8984 §4.3.3).  `at_type` is set to `"NDay"`; all optional
    /// fields default to `None` / empty.
    pub fn new(day: impl Into<String>) -> Self {
        Self {
            at_type: "NDay".to_owned(),
            day: day.into(),
            nth_of_period: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl TypeDiscriminator for NDay {
    const TYPE_TAG: &'static str = "NDay";
    fn at_type(&self) -> &str {
        &self.at_type
    }
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
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"RecurrenceRule"` rather
    /// than failing the whole parent object's deserialize. Serialize
    /// always emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "recurrence_rule_at_type_default")]
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

impl RecurrenceRule {
    /// Construct a new `RecurrenceRule` with the mandatory `frequency`
    /// value (RFC 8984 §4.3.3).  `at_type` is set to `"RecurrenceRule"`;
    /// all optional fields default to `None`.
    ///
    /// `frequency` MUST be one of `"yearly"`, `"monthly"`, `"weekly"`,
    /// `"daily"`, `"hourly"`, `"minutely"`, `"secondly"` per the spec —
    /// not enforced at construction time.
    pub fn new(frequency: impl Into<String>) -> Self {
        Self {
            at_type: "RecurrenceRule".to_owned(),
            frequency: frequency.into(),
            interval: None,
            rscale: None,
            skip: None,
            first_day_of_week: None,
            by_day: None,
            by_month_day: None,
            by_month: None,
            by_year_day: None,
            by_week_no: None,
            by_hour: None,
            by_minute: None,
            by_second: None,
            by_set_position: None,
            count: None,
            until: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl TypeDiscriminator for RecurrenceRule {
    const TYPE_TAG: &'static str = "RecurrenceRule";
    fn at_type(&self) -> &str {
        &self.at_type
    }
}

// ── Location and VirtualLocation ─────────────────────────────────────────────

/// A physical or virtual location associated with an event (RFC 8984 §4.2.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// Object type discriminator; always `"Location"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Location"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "location_at_type_default")]
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Location {
    /// Construct a new `Location` (RFC 8984 §4.2.5) with `at_type` set to
    /// `"Location"` and all optional fields defaulted to `None`.
    pub fn new() -> Self {
        Self {
            at_type: "Location".to_owned(),
            name: None,
            description: None,
            location_types: None,
            relative_to: None,
            time_zone: None,
            coordinates: None,
            links: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl Default for Location {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeDiscriminator for Location {
    const TYPE_TAG: &'static str = "Location";
    fn at_type(&self) -> &str {
        &self.at_type
    }
}

/// An online meeting or virtual location (RFC 8984 §4.2.6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualLocation {
    /// Object type discriminator; always `"VirtualLocation"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"VirtualLocation"` rather
    /// than failing the whole parent object's deserialize. Serialize
    /// always emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "virtual_location_at_type_default")]
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

impl VirtualLocation {
    /// Construct a new `VirtualLocation` with the mandatory `uri` value
    /// (RFC 8984 §4.2.6).  `at_type` is set to `"VirtualLocation"`; all
    /// optional fields default to `None`.
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            at_type: "VirtualLocation".to_owned(),
            name: None,
            description: None,
            uri: uri.into(),
            features: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl TypeDiscriminator for VirtualLocation {
    const TYPE_TAG: &'static str = "VirtualLocation";
    fn at_type(&self) -> &str {
        &self.at_type
    }
}

// ── Link ─────────────────────────────────────────────────────────────────────

/// An attachment, image, or URL associated with an event (RFC 8984 §1.4.11).
///
/// # Source invariant: at least one of `href` or `blob_id` MUST be set
///
/// RFC 8984 §1.4.11 marks `href` as `"String" (mandatory)`.  The JMAP
/// Calendars draft (draft-ietf-jmap-calendars-26 §5.3) relaxes that
/// mandate: "Instead of mandating an 'href' property, clients may set a
/// 'blobId' property instead to reference a blob of binary data in the
/// account".  The combined contract is therefore **exactly one of
/// `href` or `blob_id` MUST be present**, and both MAY be set
/// simultaneously (a server-stored blob with a public-fetch URL).
///
/// This invariant is **not** encoded in the Rust type — both fields are
/// `Option` so that partial deserialization (e.g. of an in-flight update
/// where only one half has been populated) succeeds.  Encoding the
/// invariant via `enum LinkSource { Href, BlobId, Both }` would be a
/// breaking API change blocking partial constructors and is deliberately
/// deferred until consumer evidence warrants it.
///
/// Consumers that need to validate the invariant on inbound data SHOULD
/// call [`Link::validate_source`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// Object type discriminator; always `"Link"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Link"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "link_at_type_default")]
    pub at_type: String,

    /// URI from which the linked resource may be fetched (RFC 8984
    /// §1.4.11 — `"String" (mandatory)`).
    ///
    /// The pure RFC 8984 mandate is relaxed by the JMAP Calendars draft
    /// (draft-ietf-jmap-calendars-26 §5.3) when `blob_id` is set: the
    /// client may omit `href` and reference the resource by JMAP blob
    /// id instead.  At least one of `href` or `blob_id` MUST be
    /// present on a well-formed Link; see the struct-level docs.
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

    /// JMAP blob id (draft-ietf-jmap-calendars-26 §5.3 / §10.9.14).
    ///
    /// When present, `href` may be absent — the JMAP Calendars draft
    /// §5.3 explicitly permits substituting a `blob_id` for the
    /// otherwise-mandatory `href`.  At least one of `href` or
    /// `blob_id` MUST be present on a well-formed Link; see the
    /// struct-level docs.
    ///
    /// Per draft §5.3: the server MUST translate this to an embedded
    /// `data:` URL when sending to systems that cannot access blobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<Id>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Error returned by [`Link::validate_source`] when the source invariant
/// (RFC 8984 §1.4.11 + JMAP Calendars draft §5.3) is violated.
///
/// Currently the only failure mode is "neither `href` nor `blob_id` is
/// set"; the type is `#[non_exhaustive]` so additional variants can be
/// added without breaking matches.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSourceError {
    /// Neither `href` nor `blob_id` is set.  Per RFC 8984 §1.4.11
    /// `href` is mandatory; per JMAP Calendars draft §5.3 a
    /// `blob_id` may substitute.  At least one of the two MUST be
    /// present.
    Missing,
}

impl std::fmt::Display for LinkSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkSourceError::Missing => f.write_str(
                "Link has neither href nor blobId set; RFC 8984 §1.4.11 and \
                 JMAP Calendars draft §5.3 require at least one",
            ),
        }
    }
}

impl std::error::Error for LinkSourceError {}

impl Link {
    /// Construct an empty `Link` (RFC 8984 §1.4.11) with `at_type` set
    /// to `"Link"` and all optional fields defaulted to `None`.
    ///
    /// Note: the returned `Link` does NOT satisfy the source invariant
    /// (at least one of `href`/`blob_id` MUST be set); callers are
    /// expected to set one of them before serializing.  Use
    /// [`Link::validate_source`] to check.
    pub fn new() -> Self {
        Self {
            at_type: "Link".to_owned(),
            href: None,
            content_type: None,
            size: None,
            rel: None,
            display: None,
            cid: None,
            title: None,
            blob_id: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Construct a `Link` from an `href` URI string (the RFC 8984 §1.4.11
    /// happy path).
    pub fn with_href(href: impl Into<String>) -> Self {
        Self {
            href: Some(href.into()),
            ..Self::new()
        }
    }

    /// Construct a `Link` referencing a JMAP blob by id (JMAP Calendars
    /// draft §5.3).
    pub fn with_blob_id(blob_id: Id) -> Self {
        Self {
            blob_id: Some(blob_id),
            ..Self::new()
        }
    }

    /// Validate the source invariant: at least one of `href` or
    /// `blob_id` MUST be present.
    ///
    /// Combined contract from RFC 8984 §1.4.11 (`href` mandatory) and
    /// the JMAP Calendars draft (draft-ietf-jmap-calendars-26 §5.3,
    /// `blob_id` may substitute for `href`).  Both fields MAY be set
    /// simultaneously — that is permitted, only the "neither set"
    /// case fails.
    ///
    /// This is an opt-in check.  Deserialization itself does NOT enforce
    /// the invariant so that partial Links (e.g. in-flight updates)
    /// round-trip cleanly; consumers that need a wire-validity check on
    /// inbound data call this method.
    pub fn validate_source(&self) -> Result<(), LinkSourceError> {
        if self.href.is_none() && self.blob_id.is_none() {
            Err(LinkSourceError::Missing)
        } else {
            Ok(())
        }
    }
}

impl Default for Link {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeDiscriminator for Link {
    const TYPE_TAG: &'static str = "Link";
    fn at_type(&self) -> &str {
        &self.at_type
    }
}

// ── Relation ─────────────────────────────────────────────────────────────────

/// A relationship between this object and another, identified by UID
/// (RFC 8984 §1.4.10).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// Object type discriminator; always `"Relation"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Relation"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "relation_at_type_default")]
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

impl Relation {
    /// Construct an empty `Relation` (RFC 8984 §1.4.10) with `at_type`
    /// set to `"Relation"` and all optional fields defaulted to `None`.
    pub fn new() -> Self {
        Self {
            at_type: "Relation".to_owned(),
            relation: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl Default for Relation {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeDiscriminator for Relation {
    const TYPE_TAG: &'static str = "Relation";
    fn at_type(&self) -> &str {
        &self.at_type
    }
}

// ── Participant ───────────────────────────────────────────────────────────────

/// A participant in an event (RFC 8984 §4.4.6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    /// Object type discriminator; always `"Participant"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Participant"` rather
    /// than failing the whole parent object's deserialize. Serialize
    /// always emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "participant_at_type_default")]
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
    ///
    /// RFC 8984 §4.4.6: "At least one role MUST be specified for the
    /// participant".  The non-empty mandate is NOT enforced by the type
    /// system or by deserialize; use [`Participant::validate_roles`]
    /// for an opt-in check.
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
    pub schedule_updated: Option<UTCDate>,

    /// iTIP status codes from the most recent scheduling message sent to this
    /// participant (RFC 8984 §4.4.6).
    ///
    /// An array of iTIP status code strings (e.g. `"1.0"`, `"2.0"`, `"5.0"`).
    /// Server-set and persisted; absent when no scheduling has occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_status: Option<Vec<String>>,

    /// Client request to force a scheduling message (RFC 8984 §4.4.6,
    /// default `false`).
    ///
    /// A client sets this to `true` to ask the server to send a scheduling
    /// message to the participant even when it would not normally do so
    /// (e.g. no significant change was made, or `scheduleAgent` is
    /// `"client"`).  Per the spec this property MUST NOT be stored on the
    /// server or appear in a scheduling message — it is request-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_force_send: Option<bool>,

    /// Email address of the iMIP sender, if different from the
    /// participant's `imip` send-to URI (RFC 8984 §4.4.6).
    ///
    /// SHOULD only be set when the From-header address of the email that
    /// last updated this participant differs from the `mailto:` URI in
    /// `sendTo["imip"]`.  If set, MUST be a valid `addr-spec` per
    /// RFC 5322 §3.4.1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_by: Option<String>,

    /// Task-only: progress of the participant for this task
    /// (RFC 8984 §4.4.6; allowed values in §5.2.5).
    ///
    /// MUST NOT be set when `participationStatus` is anything other than
    /// `"accepted"`.  Only meaningful on a `Task`; ignored on an `Event`.
    /// Type-level forward-compatibility: this field is kept as
    /// `Option<String>` rather than a typed enum because RFC 8984 §5.2.5
    /// defines an open value set extensible via the IANA "JSCalendar
    /// Enum Values" registry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,

    /// Task-only: timestamp the `progress` property was last set
    /// (RFC 8984 §4.4.6; semantics in §5.2.6).
    ///
    /// Only meaningful on a `Task`; ignored on an `Event`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_updated: Option<UTCDate>,

    /// Task-only: percent completion of the participant for this task
    /// (RFC 8984 §4.4.6).
    ///
    /// MUST be a value in the range `0..=100` per the spec.  Only
    /// meaningful on a `Task`; ignored on an `Event`.  The type permits
    /// the full `u8` range; values outside `0..=100` are wire-invalid
    /// and the consumer is responsible for range-checking on input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<u8>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Error returned by [`Participant::validate_roles`] when the
/// RFC 8984 §4.4.6 non-empty-roles invariant is violated.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParticipantRolesError {
    /// The `roles` map is empty.  RFC 8984 §4.4.6 says
    /// "At least one role MUST be specified for the participant".
    Empty,
}

impl std::fmt::Display for ParticipantRolesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParticipantRolesError::Empty => f.write_str(
                "Participant.roles is empty; RFC 8984 §4.4.6 requires at \
                 least one role",
            ),
        }
    }
}

impl std::error::Error for ParticipantRolesError {}

impl Participant {
    /// Construct a new `Participant` with the mandatory `roles` map
    /// (RFC 8984 §4.4.6).  `at_type` is set to `"Participant"`; all
    /// optional fields default to `None`.
    ///
    /// Per RFC 8984 §4.4.6 the `roles` map MUST be non-empty: "At least
    /// one role MUST be specified for the participant".  This
    /// constructor accepts any `HashMap`, including an empty one — the
    /// non-empty mandate is not enforced at construction time so that
    /// partial in-flight values round-trip cleanly.  Callers SHOULD
    /// populate at least one role entry before serializing.  Use
    /// [`Participant::validate_roles`] to check.
    pub fn new(roles: HashMap<String, bool>) -> Self {
        Self {
            at_type: "Participant".to_owned(),
            name: None,
            email: None,
            description: None,
            send_to: None,
            kind: None,
            roles,
            location_id: None,
            language: None,
            participation_status: None,
            participation_comment: None,
            expect_reply: None,
            schedule_agent: None,
            calendar_address: None,
            invited_by: None,
            delegated_to: None,
            delegated_from: None,
            member_of: None,
            links: None,
            schedule_sequence: None,
            schedule_updated: None,
            schedule_status: None,
            schedule_force_send: None,
            sent_by: None,
            progress: None,
            progress_updated: None,
            percent_complete: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl Participant {
    /// Validate the RFC 8984 §4.4.6 non-empty-roles invariant.
    ///
    /// Returns `Err(ParticipantRolesError::Empty)` when `roles` is
    /// empty; `Ok(())` otherwise.  Opt-in check — deserialize does NOT
    /// enforce the mandate so that partial in-flight Participant values
    /// round-trip cleanly.
    pub fn validate_roles(&self) -> Result<(), ParticipantRolesError> {
        if self.roles.is_empty() {
            Err(ParticipantRolesError::Empty)
        } else {
            Ok(())
        }
    }
}

impl TypeDiscriminator for Participant {
    const TYPE_TAG: &'static str = "Participant";
    fn at_type(&self) -> &str {
        &self.at_type
    }
}

// ── Alert ─────────────────────────────────────────────────────────────────────

/// A trigger time given as an offset from the event start or end
/// (RFC 8984 §4.5.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetTrigger {
    /// Object type discriminator; always `"OffsetTrigger"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"OffsetTrigger"` rather
    /// than failing the whole parent object's deserialize. Serialize
    /// always emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "offset_trigger_at_type_default")]
    pub at_type: String,

    /// Duration offset from `relative_to`.
    pub offset: SignedDuration,

    /// Whether to measure from `"start"` or `"end"` of the event.
    /// Default is `"start"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_to: Option<String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl OffsetTrigger {
    /// Construct a new `OffsetTrigger` with the mandatory `offset` value
    /// (RFC 8984 §4.5.2).  `at_type` is set to `"OffsetTrigger"`;
    /// `relative_to` defaults to `None` (the spec default is `"start"`).
    pub fn new(offset: SignedDuration) -> Self {
        Self {
            at_type: "OffsetTrigger".to_owned(),
            offset,
            relative_to: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl TypeDiscriminator for OffsetTrigger {
    const TYPE_TAG: &'static str = "OffsetTrigger";
    fn at_type(&self) -> &str {
        &self.at_type
    }
}

/// A trigger time given as an absolute UTC date-time (RFC 8984 §4.5.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbsoluteTrigger {
    /// Object type discriminator; always `"AbsoluteTrigger"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"AbsoluteTrigger"` rather
    /// than failing the whole parent object's deserialize. Serialize
    /// always emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "absolute_trigger_at_type_default")]
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

impl AbsoluteTrigger {
    /// Construct a new `AbsoluteTrigger` with the mandatory `when` value
    /// (RFC 8984 §4.5.2).  `at_type` is set to `"AbsoluteTrigger"`.
    pub fn new(when: UTCDate) -> Self {
        Self {
            at_type: "AbsoluteTrigger".to_owned(),
            when,
            extra: serde_json::Map::new(),
        }
    }
}

impl TypeDiscriminator for AbsoluteTrigger {
    const TYPE_TAG: &'static str = "AbsoluteTrigger";
    fn at_type(&self) -> &str {
        &self.at_type
    }
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
///
/// # Deserialization behaviour on malformed input
///
/// The custom `Deserialize` impl is deliberately permissive about the JSON
/// *shape* of the input — only the `@type` tag drives the dispatch.  Any
/// input whose `@type` is missing, is not a string, or names a tag other
/// than `"OffsetTrigger"` / `"AbsoluteTrigger"` is captured as
/// [`AlertTrigger::Unknown`] carrying the original `serde_json::Value`
/// unchanged.  This includes:
///
/// - top-level non-objects: `null`, arrays, numbers, strings, booleans
/// - objects without an `@type` key
/// - objects whose `@type` is a non-string value (`null`, `42`, `[]`)
/// - objects whose `@type` is a string outside the known set
///
/// The rationale is round-trip fidelity per the spec's preserve-mandate:
/// rejecting at deserialize time would force consumers to drop data they
/// are supposed to preserve.  Consumers that need a stricter check should
/// pattern-match on the variant and inspect the carried `Value`.  A future
/// well-typed `AlertTrigger` variant only ever displaces input that
/// previously matched on `@type` exactly, so this permissive behaviour is
/// forward-compatible with the spec's evolution.
///
/// # Maintainer note: do NOT "clean up" this enum
///
/// The `Unknown(serde_json::Value)` variant exists because RFC 8984 §4.5.2
/// requires preservation of unrecognised trigger types. The following
/// three "cleanups" all violate that MUST and break the regression tests
/// `alert_trigger_unknown_dispatch_on_hostile_input` and
/// `alert_trigger_unknown_round_trips_through_serialize` (both in this
/// file), as well as `alert_unknown_trigger_roundtrip` in
/// `crate-jmap-calendars-types/tests/types_test.rs`:
///
/// 1. **Remove the `Unknown` variant.** An exhaustive enum forces
///    deserialize to either fail on an unknown `@type` (data loss + error)
///    or silently drop the input (data loss). Both violate
///    `MUST preserve them`.
///
/// 2. **Replace `Unknown(Value)` with `#[serde(other)]` on a unit variant.**
///    A unit variant discards the carried JSON payload. The spec preserve-
///    mandate requires the *bytes* round-trip, not just the variant
///    discriminator. `Unknown(String)` and `Unknown { at_type: String }`
///    have the same defect — they drop the inner fields.
///
/// 3. **Add `#[serde(deny_unknown_fields)]` to `AlertTrigger` or to
///    `OffsetTrigger` / `AbsoluteTrigger`.** A peer that emits an extension
///    field on a known trigger type (e.g. `OffsetTrigger` with a vendor
///    field) would fail to deserialize entirely. The catch-all `extra`
///    map on the inner structs is the workspace's
///    extras-preservation mechanism for that case; `deny_unknown_fields`
///    fights it.
///
/// If you are reading this comment because you have a fourth "cleanup"
/// in mind: confirm against the regression test and the spec passage
/// (RFC 8984 §4.5.2) before proposing it. The design defense is
/// bd:JMAP-1rwf.8.
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
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Alert"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "alert_at_type_default")]
    pub at_type: String,

    /// When to trigger the alert.
    pub trigger: AlertTrigger,

    /// UTC date-time when the user acknowledged this alert, or `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged: Option<UTCDate>,

    /// Related alerts (e.g. for snooze chains); keys are alert ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_to: Option<HashMap<String, Relation>>,

    /// How to present the alert: `"display"` or `"email"` (default `"display"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Alert {
    /// Construct a new `Alert` with the mandatory `trigger` value
    /// (RFC 8984 §4.5.2).  `at_type` is set to `"Alert"`; all optional
    /// fields default to `None`.
    pub fn new(trigger: AlertTrigger) -> Self {
        Self {
            at_type: "Alert".to_owned(),
            trigger,
            acknowledged: None,
            related_to: None,
            action: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl TypeDiscriminator for Alert {
    const TYPE_TAG: &'static str = "Alert";
    fn at_type(&self) -> &str {
        &self.at_type
    }
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
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"TimeZoneRule"` rather
    /// than failing the whole parent object's deserialize. Serialize
    /// always emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "time_zone_rule_at_type_default")]
    pub at_type: String,

    /// DTSTART from iCalendar — the local date-time the rule first applies.
    pub start: LocalDateTime,

    /// TZOFFSETFROM from iCalendar — the UTC offset in effect before the
    /// transition (format `±HHMM` or `±HHMMSS`).  Typed as
    /// [`UTCOffset`] for consistency with the other temporal newtypes;
    /// the inner string is opaque to this crate (validation deferred to
    /// the backend per `PLAN.md`).
    pub offset_from: UTCOffset,

    /// TZOFFSETTO from iCalendar — the UTC offset in effect after the
    /// transition (format `±HHMM` or `±HHMMSS`).  Typed as
    /// [`UTCOffset`] for consistency with the other temporal newtypes.
    pub offset_to: UTCOffset,

    /// RRULE from iCalendar — recurrence rules for the transition.
    /// Per RFC 8984 §4.7.2 the `until` value MUST be interpreted as a
    /// local time in the UTC time zone during evaluation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_rules: Option<Vec<RecurrenceRule>>,

    /// RDATE properties from iCalendar — additional explicit transition
    /// dates. Keys are LocalDateTime strings; the PatchObject value MUST
    /// be the empty JSON object (`{}`) per RFC 8984 §4.7.2.
    ///
    /// The type permits non-empty PatchObject values that the wire spec
    /// forbids — this is deliberate for round-trip preservation of
    /// in-flight data.  Use
    /// [`TimeZoneRule::validate_recurrence_overrides_empty`] for an
    /// opt-in check that every value is the empty patch.
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

impl TimeZoneRule {
    /// Construct a new `TimeZoneRule` with the three mandatory fields
    /// (RFC 8984 §4.7.2).  `at_type` is set to `"TimeZoneRule"`; all
    /// optional fields default to `None`.
    ///
    /// `offset_from` / `offset_to` MUST be a valid signed offset string
    /// (`±HHMM` or `±HHMMSS`) per the spec — not enforced at
    /// construction time.  Accepts anything `Into<UTCOffset>`, which
    /// includes `&str` and `String` via the newtype's `From` impls.
    pub fn new(
        start: LocalDateTime,
        offset_from: impl Into<UTCOffset>,
        offset_to: impl Into<UTCOffset>,
    ) -> Self {
        Self {
            at_type: "TimeZoneRule".to_owned(),
            start,
            offset_from: offset_from.into(),
            offset_to: offset_to.into(),
            recurrence_rules: None,
            recurrence_overrides: None,
            names: None,
            comments: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Error returned by [`TimeZoneRule::validate_recurrence_overrides_empty`]
/// when the RFC 8984 §4.7.2 "PatchObject value MUST be the empty patch"
/// constraint is violated.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecurrenceOverridesError {
    /// At least one entry in `recurrence_overrides` carries a
    /// non-empty PatchObject value.  RFC 8984 §4.7.2 requires every
    /// value to be the empty patch (`{}`).
    NonEmptyPatch {
        /// The wire-format key (LocalDateTime string) of the offending
        /// entry.
        key: String,
    },
}

impl std::fmt::Display for RecurrenceOverridesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecurrenceOverridesError::NonEmptyPatch { key } => write!(
                f,
                "TimeZoneRule.recurrenceOverrides[{key:?}] carries a non-empty \
                 PatchObject; RFC 8984 §4.7.2 requires the empty patch"
            ),
        }
    }
}

impl std::error::Error for RecurrenceOverridesError {}

impl TimeZoneRule {
    /// Validate the RFC 8984 §4.7.2 constraint on
    /// `recurrence_overrides`: every PatchObject value MUST be the
    /// empty patch (`{}`).
    ///
    /// Returns `Ok(())` if `recurrence_overrides` is `None`, an empty
    /// map, or contains only empty-patch values.  Returns
    /// `Err(RecurrenceOverridesError::NonEmptyPatch { key })` naming
    /// the first offending entry's wire key.  Opt-in check —
    /// deserialize itself does NOT enforce the constraint.
    pub fn validate_recurrence_overrides_empty(&self) -> Result<(), RecurrenceOverridesError> {
        if let Some(map) = &self.recurrence_overrides {
            for (k, v) in map {
                if !v.as_map().is_empty() {
                    return Err(RecurrenceOverridesError::NonEmptyPatch {
                        key: k.as_ref().to_owned(),
                    });
                }
            }
        }
        Ok(())
    }
}

impl TypeDiscriminator for TimeZoneRule {
    const TYPE_TAG: &'static str = "TimeZoneRule";
    fn at_type(&self) -> &str {
        &self.at_type
    }
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
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"TimeZone"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "time_zone_at_type_default")]
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

impl TimeZone {
    /// Construct a new `TimeZone` with the mandatory `tz_id` value
    /// (RFC 8984 §4.7.2).  `at_type` is set to `"TimeZone"`; all
    /// optional fields default to `None`.
    ///
    /// Per RFC 8984 §4.7.2 a valid TimeZone MUST define at least one
    /// transition rule in `standard` or `daylight`; this constructor
    /// does not enforce that — callers are expected to populate at
    /// least one of them before serializing.
    pub fn new(tz_id: impl Into<String>) -> Self {
        Self {
            at_type: "TimeZone".to_owned(),
            tz_id: tz_id.into(),
            updated: None,
            url: None,
            valid_until: None,
            aliases: None,
            standard: None,
            daylight: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl TypeDiscriminator for TimeZone {
    const TYPE_TAG: &'static str = "TimeZone";
    fn at_type(&self) -> &str {
        &self.at_type
    }
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

    /// Oracle: `TypeDiscriminator::validate_at_type` enforces the
    /// RFC 8984-mandated `@type` literal.  Hostile input carrying a
    /// wrong `@type` (e.g. `{"@type": "NotNDay", "day": "mo"}`)
    /// deserializes successfully for round-trip preservation but
    /// `validate_at_type` rejects with `TypeTagMismatch`.
    /// (bd:JMAP-mno4.15)
    #[test]
    fn validate_at_type_rejects_wrong_discriminator() {
        // NDay with hostile @type — deserialize succeeds, validate
        // rejects.
        let raw = json!({"@type": "NotNDay", "day": "mo"});
        let bad: NDay = serde_json::from_value(raw).unwrap();
        assert_eq!(bad.day, "mo"); // payload survived for round-trip
        let err = bad.validate_at_type().expect_err("expected mismatch");
        assert_eq!(err.expected, "NDay");
        assert_eq!(err.actual, "NotNDay");

        // Constructor-built value passes validate_at_type.
        let good = NDay::new("mo");
        assert!(good.validate_at_type().is_ok());

        // Spot-check a few other types: constructor-built passes,
        // wrong-tag input fails.
        let bad_loc: Location =
            serde_json::from_value(json!({"@type": "Place", "name": "HQ"})).unwrap();
        assert_eq!(bad_loc.validate_at_type().unwrap_err().expected, "Location");
        assert!(Location::new().validate_at_type().is_ok());

        let bad_alert: Alert = serde_json::from_value(json!({
            "@type": "Notification",
            "trigger": {"@type": "OffsetTrigger", "offset": "-PT5M"}
        }))
        .unwrap();
        assert_eq!(bad_alert.validate_at_type().unwrap_err().expected, "Alert");
    }

    /// Oracle: every `new` constructor sets `at_type` to the wire
    /// discriminator literal mandated by RFC 8984.  Round-trips through
    /// serde_json::to_value reproduces the discriminator on the wire as
    /// `"@type": "<TypeName>"`, matching the spec text verbatim.
    /// (bd:JMAP-mno4.10)
    #[test]
    fn constructors_set_at_type_to_wire_discriminator() {
        // Cases (constructed_value, expected_wire_discriminator).
        let nday = NDay::new("mo");
        let rule = RecurrenceRule::new("weekly");
        let loc = Location::new();
        let vloc = VirtualLocation::new("https://example.com/m");
        let link = Link::with_href("https://example.com/x");
        let rel = Relation::new();
        let mut roles = HashMap::new();
        roles.insert("attendee".to_owned(), true);
        let part = Participant::new(roles);
        let off = OffsetTrigger::new(SignedDuration::from("-PT15M"));
        let abs = AbsoluteTrigger::new(UTCDate::from("2024-06-15T08:45:00Z"));
        let alert = Alert::new(AlertTrigger::OffsetTrigger(off.clone()));
        let rule_in_tz =
            TimeZoneRule::new(LocalDateTime::from("1970-01-01T00:00:00"), "+0000", "+0000");
        let tz = TimeZone::new("Etc/UTC");

        for (val, expected_tag) in [
            (serde_json::to_value(&nday).unwrap(), "NDay"),
            (serde_json::to_value(&rule).unwrap(), "RecurrenceRule"),
            (serde_json::to_value(&loc).unwrap(), "Location"),
            (serde_json::to_value(&vloc).unwrap(), "VirtualLocation"),
            (serde_json::to_value(&link).unwrap(), "Link"),
            (serde_json::to_value(&rel).unwrap(), "Relation"),
            (serde_json::to_value(&part).unwrap(), "Participant"),
            (serde_json::to_value(&off).unwrap(), "OffsetTrigger"),
            (serde_json::to_value(&abs).unwrap(), "AbsoluteTrigger"),
            (serde_json::to_value(&alert).unwrap(), "Alert"),
            (serde_json::to_value(&rule_in_tz).unwrap(), "TimeZoneRule"),
            (serde_json::to_value(&tz).unwrap(), "TimeZone"),
        ] {
            assert_eq!(
                val["@type"], expected_tag,
                "constructor must set @type to {expected_tag}; got {val:?}"
            );
        }
    }

    /// Oracle: `Link::with_blob_id` and `Link::with_href` both satisfy
    /// the source invariant.
    #[test]
    fn link_constructors_satisfy_source_invariant() {
        let with_href = Link::with_href("https://example.com");
        let with_blob = Link::with_blob_id(Id::from("Ge682d5d7aad50b3a4f"));
        let empty = Link::new();
        assert!(with_href.validate_source().is_ok());
        assert!(with_blob.validate_source().is_ok());
        assert_eq!(empty.validate_source(), Err(LinkSourceError::Missing));
    }

    /// Oracle: the three scalar newtypes (`LocalDateTime`, `Duration`,
    /// `SignedDuration`) format via `std::fmt::Display` as the bare wire
    /// string, matching the wire format defined in RFC 8984 §1.4.5/.6/.7.
    /// Without `Display`, `format!("at {dt}")` would not compile.
    #[test]
    fn scalar_newtypes_display_as_wire_string() {
        let dt = LocalDateTime::from("2024-06-15T09:00:00");
        let dur = Duration::from("PT1H");
        let sdur = SignedDuration::from("-PT15M");
        assert_eq!(format!("{dt}"), "2024-06-15T09:00:00");
        assert_eq!(format!("{dur}"), "PT1H");
        assert_eq!(format!("{sdur}"), "-PT15M");
    }

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

    /// Oracle: `AlertTrigger::deserialize` accepts all malformed/hostile
    /// JSON shapes and routes them to `Unknown(Value)` per the documented
    /// permissive policy (RFC 8984 §4.5.2 preserve-mandate).  Verifies
    /// the bd:JMAP-mno4.19 docstring claim with a probe — none of these
    /// inputs panic, error, or get reshaped; all land in `Unknown` with
    /// the input `Value` intact.
    #[test]
    fn alert_trigger_unknown_dispatch_on_hostile_input() {
        let hostile_values = [
            ("null", json!(null)),
            ("empty_array", json!([])),
            ("integer", json!(42)),
            ("bare_string", json!("hello")),
            ("boolean", json!(true)),
            ("object_without_at_type", json!({"offset": "-PT15M"})),
            ("object_with_int_at_type", json!({"@type": 42})),
            ("object_with_null_at_type", json!({"@type": null})),
            ("object_with_array_at_type", json!({"@type": []})),
            (
                "object_with_unknown_tag",
                json!({"@type": "FuturisticTrigger", "futuristicArg": 1}),
            ),
        ];
        for (label, v) in hostile_values {
            let parsed: AlertTrigger = serde_json::from_value(v.clone())
                .unwrap_or_else(|e| panic!("{label}: must not error, got {e}"));
            match parsed {
                AlertTrigger::Unknown(round) => assert_eq!(
                    round, v,
                    "{label}: Unknown must preserve the input Value bit-exactly"
                ),
                other => panic!("{label}: expected Unknown variant, got {other:?}"),
            }
        }
    }

    /// Oracle: `AlertTrigger::Unknown(Value)` round-trips through serialize
    /// → deserialize unchanged, including a non-object payload.  The
    /// Serialize impl just delegates to the inner Value, so a non-object
    /// stored in `Unknown` round-trips verbatim.
    #[test]
    fn alert_trigger_unknown_round_trips_through_serialize() {
        let original = AlertTrigger::Unknown(json!({"@type": "X", "k": 1}));
        let wire = serde_json::to_value(&original).unwrap();
        let back: AlertTrigger = serde_json::from_value(wire).unwrap();
        assert_eq!(original, back);
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

    /// Oracle: `Link::validate_source` accepts a Link with `href` only,
    /// `blob_id` only, or both; rejects a Link with neither.  Combined
    /// RFC 8984 §1.4.11 + JMAP Calendars draft §5.3 contract.
    #[test]
    fn link_validate_source_enforces_invariant() {
        // href only — accepted (pure RFC 8984 case).
        let href_only: Link = serde_json::from_value(json!({
            "@type": "Link",
            "href": "https://example.com/x"
        }))
        .unwrap();
        assert!(href_only.validate_source().is_ok());

        // blob_id only — accepted (JMAP Calendars §5.3 case).
        let blob_only: Link = serde_json::from_value(json!({
            "@type": "Link",
            "blobId": "Ge682d5d7aad50b3a4f7180a7ed9276476485ea52"
        }))
        .unwrap();
        assert!(blob_only.validate_source().is_ok());

        // Both — accepted (server-stored blob with public-fetch URL).
        let both: Link = serde_json::from_value(json!({
            "@type": "Link",
            "href": "https://example.com/x",
            "blobId": "Ge682d5d7aad50b3a4f7180a7ed9276476485ea52"
        }))
        .unwrap();
        assert!(both.validate_source().is_ok());

        // Neither — rejected.
        let neither: Link = serde_json::from_value(json!({"@type": "Link"})).unwrap();
        assert_eq!(neither.validate_source(), Err(LinkSourceError::Missing));
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

    /// Oracle: `Participant::validate_roles` enforces the RFC 8984
    /// §4.4.6 non-empty-roles mandate.  Empty `roles` deserializes
    /// cleanly (for round-trip preservation) but `validate_roles`
    /// rejects.  (bd:JMAP-mno4.16)
    #[test]
    fn participant_validate_roles_rejects_empty() {
        // Empty roles — deserialize succeeds, validate rejects.
        let raw_empty = json!({"@type": "Participant", "roles": {}});
        let p_empty: Participant = serde_json::from_value(raw_empty).unwrap();
        assert_eq!(p_empty.validate_roles(), Err(ParticipantRolesError::Empty));

        // Non-empty roles — validate accepts.
        let mut roles = HashMap::new();
        roles.insert("attendee".to_owned(), true);
        let p_good = Participant::new(roles);
        assert!(p_good.validate_roles().is_ok());
    }

    /// Oracle: the five RFC 8984 §4.4.6 fields that bd:JMAP-mno4.1 added
    /// (`scheduleForceSend`, `sentBy`, `progress`, `progressUpdated`,
    /// `percentComplete`) deserialize into their typed fields and round-trip
    /// to identical wire JSON.  Each field name and shape comes verbatim
    /// from the RFC 8984 §4.4.6 spec text — not from the code under test.
    #[test]
    fn participant_new_rfc8984_fields_round_trip() {
        let raw = json!({
            "@type": "Participant",
            "roles": {"attendee": true},
            "scheduleForceSend": true,
            "sentBy": "delegate@example.com",
            "progress": "in-process",
            "progressUpdated": "2024-06-15T08:45:00Z",
            "percentComplete": 42
        });
        let p: Participant =
            serde_json::from_value(raw.clone()).expect("Participant must deserialize");
        assert_eq!(p.schedule_force_send, Some(true));
        assert_eq!(p.sent_by.as_deref(), Some("delegate@example.com"));
        assert_eq!(p.progress.as_deref(), Some("in-process"));
        assert_eq!(
            p.progress_updated.as_ref().map(AsRef::as_ref),
            Some("2024-06-15T08:45:00Z")
        );
        assert_eq!(p.percent_complete, Some(42));

        let back = serde_json::to_value(&p).expect("serialize must succeed");
        assert_eq!(back, raw, "round-trip must preserve wire shape");
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
        assert_eq!(standard.offset_from.as_ref(), "+0200");
        assert_eq!(standard.offset_to.as_ref(), "+0100");
        assert_eq!(
            standard.recurrence_rules.as_ref().map(Vec::len),
            Some(1),
            "STANDARD rule must carry exactly one RRULE per RFC 8984 §4.7.2"
        );

        let back = serde_json::to_value(&tz).expect("serialize must succeed");
        assert_eq!(back, raw, "round-trip must preserve wire shape");
    }

    /// Oracle: `TimeZoneRule::validate_recurrence_overrides_empty`
    /// enforces the RFC 8984 §4.7.2 empty-patch constraint.  A
    /// non-empty PatchObject value deserializes cleanly (for
    /// round-trip preservation) but the validator names the offending
    /// key.  (bd:JMAP-mno4.18)
    #[test]
    fn time_zone_rule_validate_recurrence_overrides_rejects_non_empty() {
        // None: validate passes trivially.
        let none_rule =
            TimeZoneRule::new(LocalDateTime::from("1970-01-01T00:00:00"), "+0000", "+0000");
        assert!(none_rule.validate_recurrence_overrides_empty().is_ok());

        // All empty: validate passes.
        let raw_ok = json!({
            "@type": "TimeZoneRule",
            "start": "1970-01-01T00:00:00",
            "offsetFrom": "+0000",
            "offsetTo": "+0000",
            "recurrenceOverrides": {
                "1990-04-01T02:00:00": {},
                "1991-04-07T02:00:00": {}
            }
        });
        let ok_rule: TimeZoneRule = serde_json::from_value(raw_ok).unwrap();
        assert!(ok_rule.validate_recurrence_overrides_empty().is_ok());

        // One non-empty: validate names the offender.
        let raw_bad = json!({
            "@type": "TimeZoneRule",
            "start": "1970-01-01T00:00:00",
            "offsetFrom": "+0000",
            "offsetTo": "+0000",
            "recurrenceOverrides": {
                "1990-04-01T02:00:00": {},
                "1991-04-07T02:00:00": {"acmeCorp": "shouldnt-be-here"}
            }
        });
        let bad_rule: TimeZoneRule = serde_json::from_value(raw_bad).unwrap();
        let err = bad_rule.validate_recurrence_overrides_empty().unwrap_err();
        match err {
            RecurrenceOverridesError::NonEmptyPatch { key } => {
                assert_eq!(key, "1991-04-07T02:00:00");
            }
        }
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

    // ── @type-default regression tests (bd:JMAP-ky8g.10) ──────────────────
    //
    // Every JSCalendar sub-type declares `@type` as a bare `String` with a
    // serde-default function returning the RFC 8984-mandated literal.
    // Deserialize MUST succeed when `@type` is absent (spec-violating
    // producer input or partial fixture), populating the field with the
    // literal. Serialize MUST always emit the field.
    //
    // Independent oracle: hand-written JSON shaped against RFC 8984 §4.x
    // example text with `@type` omitted, plus the produced serialize-back
    // JSON checked against the same RFC's mandated literal.
    //
    // The bead's acceptance criterion is "one regression test per type
    // asserting (a) deserialize succeeds when `@type` is absent and the
    // field equals the literal; (b) explicit non-default values round-trip
    // verbatim". Per-type (a) tests follow. A representative (b) test on
    // Participant covers the contract uniformly (the serde-default mechanism
    // is the same across all 12 types; per-type duplication would not add
    // signal). A nested-parent regression test covers the concrete failure
    // mode the bead identifies.

    /// `NDay` deserialize succeeds when `@type` is absent and defaults to
    /// `"NDay"`. Re-serialize emits the field with the default value.
    #[test]
    fn n_day_at_type_defaults_when_absent() {
        let raw = json!({ "day": "mo" });
        let n: NDay = serde_json::from_value(raw).unwrap();
        assert_eq!(n.at_type, "NDay");
        let back = serde_json::to_value(&n).unwrap();
        assert_eq!(back["@type"], "NDay");
    }

    /// `RecurrenceRule` deserialize succeeds when `@type` is absent and
    /// defaults to `"RecurrenceRule"`. Re-serialize emits the field.
    #[test]
    fn recurrence_rule_at_type_defaults_when_absent() {
        let raw = json!({ "frequency": "weekly" });
        let r: RecurrenceRule = serde_json::from_value(raw).unwrap();
        assert_eq!(r.at_type, "RecurrenceRule");
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["@type"], "RecurrenceRule");
    }

    /// `Location` deserialize succeeds when `@type` is absent and defaults
    /// to `"Location"`. Re-serialize emits the field with the default.
    #[test]
    fn location_at_type_defaults_when_absent() {
        let raw = json!({ "name": "HQ" });
        let l: Location = serde_json::from_value(raw).unwrap();
        assert_eq!(l.at_type, "Location");
        let back = serde_json::to_value(&l).unwrap();
        assert_eq!(back["@type"], "Location");
    }

    /// `VirtualLocation` deserialize succeeds when `@type` is absent and
    /// defaults to `"VirtualLocation"`. Re-serialize emits the field.
    #[test]
    fn virtual_location_at_type_defaults_when_absent() {
        let raw = json!({ "uri": "https://example.com/meet/abc" });
        let v: VirtualLocation = serde_json::from_value(raw).unwrap();
        assert_eq!(v.at_type, "VirtualLocation");
        let back = serde_json::to_value(&v).unwrap();
        assert_eq!(back["@type"], "VirtualLocation");
    }

    /// `Link` deserialize succeeds when `@type` is absent and defaults to
    /// `"Link"`. Re-serialize emits the field with the default value.
    #[test]
    fn link_at_type_defaults_when_absent() {
        let raw = json!({ "href": "https://example.com/attach.pdf" });
        let l: Link = serde_json::from_value(raw).unwrap();
        assert_eq!(l.at_type, "Link");
        let back = serde_json::to_value(&l).unwrap();
        assert_eq!(back["@type"], "Link");
    }

    /// `Relation` deserialize succeeds when `@type` is absent and defaults
    /// to `"Relation"`. Re-serialize emits the field with the default.
    #[test]
    fn relation_at_type_defaults_when_absent() {
        let raw = json!({ "relation": { "parent": true } });
        let r: Relation = serde_json::from_value(raw).unwrap();
        assert_eq!(r.at_type, "Relation");
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["@type"], "Relation");
    }

    /// `Participant` deserialize succeeds when `@type` is absent and
    /// defaults to `"Participant"`. Re-serialize emits the field.
    #[test]
    fn participant_at_type_defaults_when_absent() {
        let raw = json!({ "name": "Alice", "roles": { "attendee": true } });
        let p: Participant = serde_json::from_value(raw).unwrap();
        assert_eq!(p.at_type, "Participant");
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["@type"], "Participant");
    }

    /// `OffsetTrigger` deserialize succeeds when `@type` is absent and
    /// defaults to `"OffsetTrigger"`. Re-serialize emits the field.
    #[test]
    fn offset_trigger_at_type_defaults_when_absent() {
        let raw = json!({ "offset": "-PT5M" });
        let t: OffsetTrigger = serde_json::from_value(raw).unwrap();
        assert_eq!(t.at_type, "OffsetTrigger");
        let back = serde_json::to_value(&t).unwrap();
        assert_eq!(back["@type"], "OffsetTrigger");
    }

    /// `AbsoluteTrigger` deserialize succeeds when `@type` is absent and
    /// defaults to `"AbsoluteTrigger"`. Re-serialize emits the field.
    #[test]
    fn absolute_trigger_at_type_defaults_when_absent() {
        let raw = json!({ "when": "2024-01-19T18:00:00Z" });
        let t: AbsoluteTrigger = serde_json::from_value(raw).unwrap();
        assert_eq!(t.at_type, "AbsoluteTrigger");
        let back = serde_json::to_value(&t).unwrap();
        assert_eq!(back["@type"], "AbsoluteTrigger");
    }

    /// `Alert` deserialize succeeds when `@type` is absent and defaults to
    /// `"Alert"`. Re-serialize emits the field with the default value.
    ///
    /// The nested `trigger` keeps its explicit `@type` here so the
    /// `AlertTrigger` manual deserializer dispatches; the bead's hazard
    /// is the wrapping object's `@type` being missing, not the nested
    /// trigger's tag (which is required by the dispatch logic).
    #[test]
    fn alert_at_type_defaults_when_absent() {
        let raw = json!({
            "trigger": { "@type": "OffsetTrigger", "offset": "-PT5M" }
        });
        let a: Alert = serde_json::from_value(raw).unwrap();
        assert_eq!(a.at_type, "Alert");
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["@type"], "Alert");
    }

    /// `TimeZoneRule` deserialize succeeds when `@type` is absent and
    /// defaults to `"TimeZoneRule"`. Re-serialize emits the field.
    #[test]
    fn time_zone_rule_at_type_defaults_when_absent() {
        let raw = json!({
            "start": "1970-01-01T00:00:00",
            "offsetFrom": "+0000",
            "offsetTo": "+0000"
        });
        let r: TimeZoneRule = serde_json::from_value(raw).unwrap();
        assert_eq!(r.at_type, "TimeZoneRule");
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["@type"], "TimeZoneRule");
    }

    /// `TimeZone` deserialize succeeds when `@type` is absent and defaults
    /// to `"TimeZone"`. Re-serialize emits the field with the default.
    #[test]
    fn time_zone_at_type_defaults_when_absent() {
        let raw = json!({ "tzId": "Etc/UTC" });
        let z: TimeZone = serde_json::from_value(raw).unwrap();
        assert_eq!(z.at_type, "TimeZone");
        let back = serde_json::to_value(&z).unwrap();
        assert_eq!(back["@type"], "TimeZone");
    }

    /// Explicit non-default `@type` values round-trip verbatim — the
    /// serde-default does NOT overwrite an explicit wire value. Locks in
    /// the contract that a vendor shipping a non-conformant string is
    /// preserved end-to-end rather than silently normalised. The serde-
    /// default mechanism is the same across all 12 sub-types; a single
    /// representative test (Participant) covers the contract uniformly.
    /// `validate_at_type()` is the strict-input path callers opt into.
    #[test]
    fn participant_at_type_explicit_value_round_trips_verbatim() {
        let raw = json!({
            "@type": "AcmeCorpParticipant",
            "name": "Alice",
            "roles": { "attendee": true }
        });
        let p: Participant = serde_json::from_value(raw).unwrap();
        assert_eq!(p.at_type, "AcmeCorpParticipant");
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["@type"], "AcmeCorpParticipant");
        // The strict-input path surfaces the mismatch when callers opt in.
        assert!(p.validate_at_type().is_err());
    }

    /// A parent object (Alert wrapping OffsetTrigger; representative of
    /// every JSCalendar parent-with-sub-objects case) deserializes
    /// successfully when nested sub-objects omit their `@type`. This is
    /// the concrete failure mode the bead identifies: a server response
    /// missing `@type` on a sub-object would previously fail the whole
    /// parent's deserialize.
    ///
    /// NOTE: `AlertTrigger` is the one exception — its manual
    /// `Deserialize` dispatches on `@type` and so the discriminator MUST
    /// be present on the trigger to select the variant. The bead's
    /// fix-scope explicitly does not touch the dispatch logic (a
    /// missing-`@type` trigger falls into `AlertTrigger::Unknown` by
    /// design per RFC 8984 §4.5.2 preserve-mandate). The outer `Alert`
    /// container's `@type` is what gains the default.
    #[test]
    fn alert_with_missing_outer_at_type_deserializes() {
        let raw = json!({
            "trigger": {
                "@type": "OffsetTrigger",
                "offset": "-PT15M"
            },
            "action": "display"
        });
        let a: Alert = serde_json::from_value(raw).unwrap();
        assert_eq!(a.at_type, "Alert");
        match a.trigger {
            AlertTrigger::OffsetTrigger(ref t) => {
                assert_eq!(t.at_type, "OffsetTrigger");
                assert_eq!(t.offset.as_ref(), "-PT15M");
            }
            _ => panic!("trigger MUST deserialize as OffsetTrigger variant"),
        }
    }
}
