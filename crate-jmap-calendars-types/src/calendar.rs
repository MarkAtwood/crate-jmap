//! Calendar object, CalendarRights, and IncludeInAvailability.
//!
//! Normative reference: draft-ietf-jmap-calendars-33 §4.

use std::collections::BTreeMap;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Controls which events in a Calendar contribute to availability calculations
/// (draft-ietf-jmap-calendars-33 §4).
///
/// The spec defines exactly three values with no extension mechanism; modelled
/// as a closed enum (no `Other` variant).  Wire values are lowercase.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
/// (draft-ietf-jmap-calendars-33 §4, after the `myRights` field definition).
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

    /// User may modify the shared metadata of this calendar and of the
    /// events in it (draft-ietf-jmap-metadata-04 §4.1).
    /// `None` when the server omits it: the right is present only when the
    /// metadata capability lists this type or a type it contains (§4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub may_set_metadata: Option<bool>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP Calendar object (draft-ietf-jmap-calendars-33 §4).
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
    ///
    /// `Option<Id>` with `skip_serializing_if` so /set create payloads
    /// constructed by a typed caller do NOT emit a client-supplied `id`
    /// on the wire. Per RFC 8620 §5.3, the `id` property MUST NOT be set
    /// in the create object — the server assigns it. Matches the
    /// in-crate sibling `CalendarEvent.id` (event.rs:43-44).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// User-visible name for this calendar (1–255 UTF-8 octets).
    pub name: String,

    /// Optional longer-form description; `null` if not set.
    pub description: Option<String>,

    /// CSS color name or `#rrggbb`; `null` if not set.
    pub color: Option<String>,

    /// Client UI sort position; lower values sort first (0 ≤ n < 2^31).
    #[serde(default)]
    pub sort_order: u32,

    /// Whether the user has subscribed to this calendar.
    pub is_subscribed: bool,

    /// Whether to display this calendar's events (ignored if not subscribed).
    #[serde(default = "default_true")]
    pub is_visible: bool,

    /// Server-set; true for at most one calendar per account.
    pub is_default: bool,

    /// Which events in this calendar contribute to availability calculations.
    pub include_in_availability: IncludeInAvailability,

    /// Default alerts for timed events when `useDefaultAlerts` is true.
    /// `Id[Alert]` (draft-ietf-jmap-calendars-33 §4): keys are alert ids;
    /// values are Alert objects (RFC 8984 §4.5.2).
    /// Complex sub-objects passed through as opaque JSON.
    pub default_alerts_with_time: Option<BTreeMap<Id, serde_json::Value>>,

    /// Default alerts for all-day events when `useDefaultAlerts` is true.
    /// `Id[Alert]` (draft-ietf-jmap-calendars-33 §4): keys are alert ids;
    /// values are Alert objects (RFC 8984 §4.5.2).
    pub default_alerts_without_time: Option<BTreeMap<Id, serde_json::Value>>,

    /// IANA Time Zone Database id; `null` to inherit from the account Principal.
    ///
    /// This crate does not check the value against the IANA Time Zone
    /// Database; validating the zone name is the backend's job.
    pub time_zone: Option<String>,

    /// Map of Principal id → rights; `null` if not shared.
    /// May only be modified by a user with `may_share` right.
    pub share_with: Option<BTreeMap<Id, CalendarRights>>,

    /// Access rights the authenticated user holds for this calendar (server-set).
    pub my_rights: CalendarRights,

    /// Shared metadata, keyed by namespace identifier
    /// (draft-ietf-jmap-metadata-04 §2). Each value is a JSON object (§2.1).
    /// `None` when absent, which -04 §2 treats as `{}`; in an update a
    /// `null` value "is equivalent to {}" (-04 §3.2) and also decodes as
    /// `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,

    /// The authenticated user's own metadata, keyed by namespace
    /// identifier (draft-ietf-jmap-metadata-04 §2). Same shape and `None`
    /// handling as `metadata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_metadata: Option<serde_json::Map<String, serde_json::Value>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Serde default for `Calendar::is_visible` ("default: true").
fn default_true() -> bool {
    true
}
