//! Calendar object, CalendarRights, and IncludeInAvailability.
//!
//! Normative reference: draft-ietf-jmap-calendars-26 §4.

use std::collections::HashMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Controls which events in a Calendar contribute to availability calculations
/// (draft-ietf-jmap-calendars-26 §4).
///
/// The spec defines exactly three values with no extension mechanism; modelled
/// as a closed enum (no `Other` variant).  Wire values are lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub enum IncludeInAvailability {
    /// All events in this calendar are considered for availability.
    #[serde(rename = "all")]
    All,
    /// Only events where the user is a confirmed or tentative participant are
    /// considered.
    #[serde(rename = "attending")]
    Attending,
    /// No events in this calendar are used for availability (but may be
    /// included via another calendar).
    #[serde(rename = "none")]
    None,
}

/// Access control rights the authenticated user holds for a Calendar
/// (draft-ietf-jmap-calendars-26 §4, after the `myRights` field definition).
///
/// `Default` produces all-false (no access), which is the most restrictive
/// valid value and a safe starting point when constructing rights in tests
/// or server code.
///
/// ## Invariant (spec §4)
///
/// If `may_write_all` is `true`, then `may_write_own`, `may_update_private`,
/// and `may_rsvp` MUST also be `true`.  This invariant is enforced by the
/// handler/backend layer, not this type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarRights {
    /// User may read free/busy information for this calendar as part of a
    /// `Principal/getAvailability` call.
    pub may_read_free_busy: bool,

    /// User may fetch the events in this calendar.
    pub may_read_items: bool,

    /// User may create, modify, or destroy all events in this calendar, or
    /// move events to/from this calendar.  If true, `may_write_own`,
    /// `may_update_private`, and `may_rsvp` MUST also be true.
    pub may_write_all: bool,

    /// User may create, modify, or destroy an event if they are the owner or
    /// the event has no owner.
    pub may_write_own: bool,

    /// User may modify per-user properties on all events in this calendar,
    /// even without general write access.
    pub may_update_private: bool,

    /// User may modify participant status and related scheduling fields for
    /// their own ParticipantIdentity objects.
    #[serde(rename = "mayRSVP")]
    pub may_rsvp: bool,

    /// User may modify the `shareWith` property of this calendar.
    pub may_share: bool,

    /// User may delete the calendar itself.
    pub may_delete: bool,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP Calendar object (draft-ietf-jmap-calendars-26 §4).
///
/// A Calendar is a named collection of CalendarEvents.  All events are
/// associated with at least one calendar.  The `id` is immutable and
/// server-set.
///
/// ## Nullable vs. absent fields
///
/// Per RFC 8620 §5.1, a field may be absent from a response when the
/// `properties` argument was used to request a subset.  Fields that the
/// spec defines as nullable (e.g. `description`, `color`, `share_with`)
/// use `Option<T>` **without** `skip_serializing_if` so that `null` round-
/// trips correctly.  Fields that are simply optional in the request context
/// use `Option<T>` **with** `skip_serializing_if = "Option::is_none"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    /// Server-assigned immutable identifier.
    pub id: Id,

    /// User-visible name for this calendar (1–255 UTF-8 octets).
    pub name: String,

    /// Optional longer-form description; `null` if not set.
    pub description: Option<String>,

    /// CSS color name or `#rrggbb`; `null` if not set.
    pub color: Option<String>,

    /// Client UI sort position; lower values sort first (0 ≤ n < 2^31).
    pub sort_order: u32,

    /// Whether the user has subscribed to this calendar.
    pub is_subscribed: bool,

    /// Whether to display this calendar's events (ignored if not subscribed).
    pub is_visible: bool,

    /// Server-set; true for at most one calendar per account.
    pub is_default: bool,

    /// Which events in this calendar contribute to availability calculations.
    pub include_in_availability: IncludeInAvailability,

    /// Default alerts for timed events when `useDefaultAlerts` is true.
    /// Keys are UUIDs; values are Alert objects (RFC 8984 §4.5.2).
    /// Complex sub-objects passed through as opaque JSON.
    pub default_alerts_with_time: Option<HashMap<String, serde_json::Value>>,

    /// Default alerts for all-day events when `useDefaultAlerts` is true.
    /// Keys are UUIDs; values are Alert objects (RFC 8984 §4.5.2).
    pub default_alerts_without_time: Option<HashMap<String, serde_json::Value>>,

    /// IANA Time Zone Database id; `null` to inherit from the account Principal.
    pub time_zone: Option<String>,

    /// Map of Principal id → rights; `null` if not shared.
    /// May only be modified by a user with `may_share` right.
    pub share_with: Option<HashMap<Id, CalendarRights>>,

    /// Access rights the authenticated user holds for this calendar (server-set).
    pub my_rights: CalendarRights,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Filter condition for `Calendar/query` (draft-ietf-jmap-calendars-26 §4.3).
///
/// All fields are optional; a condition with no fields set matches every Calendar.
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field. Filter clauses the
/// server does not understand are a query-correctness hazard — silently
/// preserving an unrecognised clause and round-tripping it back to the
/// client can return the wrong set of records with no error signal.
///
/// ## What to do instead
///
/// **IETF-track path.** Vendors who need both capability-level declaration
/// and filterability for custom fields should use
/// `draft-ietf-jmap-metadata` (capability URI
/// `urn:ietf:params:jmap:metadata`), which defines a filterable
/// `Metadata` / `Annotation` companion object. Workspace implementation
/// tracker: bd JMAP-06zp.
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// `FilterCondition` type. See `crate-jmap-calendars-types/PLAN.md` for
/// the hybrid sloppy-value pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarFilterCondition {
    /// Calendar name must contain this string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// If `true`, only return calendars the user is subscribed to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subscribed: Option<bool>,
}
