//! Capability types for the JMAP Calendars extension.
//!
//! Normative reference: draft-ietf-jmap-calendars-26 §1.5.
//!
//! The Calendars extension defines three capability URIs.  Each has both a
//! session-level capability (value in the JMAP Session `capabilities` map)
//! and an account-level capability (value in the account's
//! `accountCapabilities` map).  Session-level values are empty objects for
//! all Calendars capabilities.

use jmap_types::UTCDate;
use serde::{Deserialize, Serialize};

/// Capability URI for core JMAP Calendars support
/// (draft-ietf-jmap-calendars-26 §1.5.1).
pub const JMAP_CALENDARS_URI: &str = "urn:ietf:params:jmap:calendars";

/// Capability URI for Principal availability queries
/// (draft-ietf-jmap-calendars-26 §1.5.2).
pub const JMAP_PRINCIPALS_AVAILABILITY_URI: &str = "urn:ietf:params:jmap:principals:availability";

/// Capability URI for the CalendarEvent/parse method
/// (draft-ietf-jmap-calendars-26 §1.5.3).
pub const JMAP_CALENDARS_PARSE_URI: &str = "urn:ietf:params:jmap:calendars:parse";

/// Session-level Calendars capability (draft-ietf-jmap-calendars-26 §1.5.1).
///
/// The value of `capabilities["urn:ietf:params:jmap:calendars"]` in the JMAP
/// Session object.  The spec mandates that this is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CalendarsCapability {}

/// Account-level Calendars capability (draft-ietf-jmap-calendars-26 §1.5.1).
///
/// The value of `accountCapabilities["urn:ietf:params:jmap:calendars"]` for a
/// given account.  Describes server capabilities and account-level permissions
/// for the Calendars extension.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarsAccountCapability {
    /// Maximum number of calendars a single event may belong to, or `null`
    /// for no limit.
    pub max_calendars_per_event: Option<u64>,

    /// Earliest UTC date-time value the server accepts for any CalendarEvent
    /// date property.
    pub min_date_time: UTCDate,

    /// Latest UTC date-time value the server accepts for any CalendarEvent
    /// date property.
    pub max_date_time: UTCDate,

    /// Maximum duration the user may query over when asking the server to
    /// expand recurrences (ISO 8601 Duration string).
    pub max_expanded_query_duration: String,

    /// Maximum number of participants a single event may have, or `null`
    /// for no limit.
    pub max_participants_per_event: Option<u64>,

    /// If `true`, the user may create a calendar in this account.
    pub may_create_calendar: bool,
}

/// Session-level capability for the Principal availability extension
/// (draft-ietf-jmap-calendars-26 §1.5.2).
///
/// Value is an empty object `{}` at the session level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PrincipalsAvailabilityCapability {}

/// Account-level capability for the Principal availability extension
/// (draft-ietf-jmap-calendars-26 §1.5.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalsAvailabilityAccountCapability {
    /// Maximum duration over which the server will calculate availability in
    /// a single `Principal/getAvailability` call (ISO 8601 Duration string).
    pub max_availability_duration: String,
}

/// Session-level capability for the CalendarEvent/parse method
/// (draft-ietf-jmap-calendars-26 §1.5.3).
///
/// Value is an empty object `{}` at both session and account level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CalendarsParseCapability {}

/// The value of `Principal.capabilities["urn:ietf:params:jmap:calendars"]`
/// on a JMAP Principals object (draft-ietf-jmap-calendars-26 §2.1).
///
/// This is distinct from the session-level [`CalendarsCapability`].  Where
/// [`CalendarsCapability`] appears in the JMAP Session `capabilities` map,
/// `PrincipalCalendarsCapability` appears inside a *Principal* object's own
/// `capabilities` map and describes that principal's calendar presence.
///
/// ## Field semantics
///
/// - `account_id` — the JMAP account id that contains this principal's
///   calendar data, or `null` if the principal has no calendar account
///   accessible to the requesting user.
/// - `may_get_availability` — the requesting user may call
///   `Principal/getAvailability` for this principal.
/// - `may_share_with` — the requesting user may add this principal to the
///   `shareWith` of their own Calendar objects.
/// - `calendar_address` — the iTIP scheduling address for this principal
///   (e.g. `"mailto:alice@example.com"`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalCalendarsCapability {
    /// Id of the account containing this principal's calendar data,
    /// or `null` if no accessible calendar account exists.
    ///
    /// Required-and-nullable: always present on the wire, as `null` or an Id.
    pub account_id: Option<jmap_types::Id>,

    /// The requesting user may call `Principal/getAvailability` for this
    /// principal.
    pub may_get_availability: bool,

    /// The requesting user may add this principal to the `shareWith` property
    /// of their own Calendar objects.
    pub may_share_with: bool,

    /// iTIP scheduling address for this principal.
    pub calendar_address: String,
}
