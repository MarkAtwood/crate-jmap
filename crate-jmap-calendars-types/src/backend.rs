//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP Calendars types.
//!
//! These are defined here so that `jmap-calendars-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the calendars types
//! are local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Calendar`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarProperty {
    Id,
    Name,
    Description,
    Color,
    SortOrder,
    IsSubscribed,
    IsVisible,
    IsDefault,
    IncludeInAvailability,
    DefaultAlertsWithTime,
    DefaultAlertsWithoutTime,
    TimeZone,
    ShareWith,
    MyRights,
}

/// Property selector for [`crate::CalendarEvent`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarEventProperty {
    Id,
    BaseEventId,
    CalendarIds,
    IsDraft,
    IsOrigin,
    UtcStart,
    UtcEnd,
    UseDefaultAlerts,
    MayInviteSelf,
    MayInviteOthers,
    HideAttendees,
    BlobId,
    Uid,
    Title,
    Description,
    Start,
    Duration,
    Status,
}

/// Names of [`CalendarEvent`](crate::CalendarEvent) properties that the JMAP
/// Calendars draft (draft-ietf-jmap-calendars-26 §5.4) classifies as
/// **per-user**.
///
/// Per-user properties belong to the authenticated user's view of the event;
/// patching them MUST NOT change the shared `updated` timestamp on the
/// underlying object. Backends serving multiple users SHOULD store these
/// separately from the shared event body.
///
/// This list mirrors the IANA-registered set in §10.8.2 of the draft and is
/// intentionally not exposed as a typed enum because several of these
/// properties — `keywords`, `color`, `freeBusyStatus`, `alerts` — are
/// reserved as future additions to [`CalendarEventProperty`] but not yet
/// enumerated there.
pub const PER_USER_CALENDAR_EVENT_PROPERTIES: &[&str] = &[
    "keywords",
    "color",
    "freeBusyStatus",
    "useDefaultAlerts",
    "alerts",
];

/// Returns `true` if `name` is a per-user [`CalendarEvent`](crate::CalendarEvent)
/// property name per draft-ietf-jmap-calendars-26 §5.4.
///
/// See [`PER_USER_CALENDAR_EVENT_PROPERTIES`] for the full set. This is a
/// wire-protocol property classification: the spec list is fixed by IANA
/// registration and backends MUST NOT redefine it.
#[must_use]
pub fn is_per_user_calendar_event_property(name: &str) -> bool {
    PER_USER_CALENDAR_EVENT_PROPERTIES.contains(&name)
}

/// Property selector for [`crate::CalendarEventNotification`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarEventNotificationProperty {
    Id,
    Created,
    ChangedBy,
    Comment,
    Type,
    CalendarEventId,
    IsDraft,
    Event,
    EventPatch,
}

/// Property selector for [`crate::ParticipantIdentity`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParticipantIdentityProperty {
    Id,
    Name,
    CalendarAddress,
    IsDefault,
}

// ---------------------------------------------------------------------------
// JmapObject impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::Calendar {
    const TYPE_NAME: &'static str = "Calendar";
    type Property = CalendarProperty;
}

impl GetObject for crate::Calendar {}

impl SetObject for crate::Calendar {
    type Patch = PatchObject;
}

impl QueryObject for crate::Calendar {
    type Filter = crate::CalendarFilterCondition;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::CalendarEvent {
    const TYPE_NAME: &'static str = "CalendarEvent";
    type Property = CalendarEventProperty;
}

impl GetObject for crate::CalendarEvent {}

impl SetObject for crate::CalendarEvent {
    type Patch = PatchObject;
}

impl QueryObject for crate::CalendarEvent {
    type Filter = crate::CalendarEventFilterCondition;
    type Comparator = crate::CalendarEventComparator;
}

impl JmapObject for crate::CalendarEventNotification {
    const TYPE_NAME: &'static str = "CalendarEventNotification";
    type Property = CalendarEventNotificationProperty;
}

impl GetObject for crate::CalendarEventNotification {}

/// `SetObject` for `CalendarEventNotification` is destroy-only.
/// The `Patch` type is never used in practice; [`PatchObject`] is a
/// safe placeholder that satisfies the trait bound while keeping the
/// type-system contract aligned with sibling types (RFC 8620 §5.3).
impl SetObject for crate::CalendarEventNotification {
    type Patch = PatchObject;
}

impl QueryObject for crate::CalendarEventNotification {
    type Filter = crate::NotificationFilterCondition;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::ParticipantIdentity {
    const TYPE_NAME: &'static str = "ParticipantIdentity";
    type Property = ParticipantIdentityProperty;
}

impl GetObject for crate::ParticipantIdentity {}

impl SetObject for crate::ParticipantIdentity {
    type Patch = PatchObject;
}

impl QueryObject for crate::ParticipantIdentity {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinning test: the per-user property list MUST match the IANA-registered
    /// set in draft-ietf-jmap-calendars-26 §5.4 / §10.8.2 exactly. Update this
    /// table only when the spec list itself changes.
    #[test]
    fn per_user_calendar_event_properties_match_spec() {
        assert_eq!(
            PER_USER_CALENDAR_EVENT_PROPERTIES,
            &[
                "keywords",
                "color",
                "freeBusyStatus",
                "useDefaultAlerts",
                "alerts"
            ]
        );
    }

    #[test]
    fn is_per_user_classifies_spec_properties_as_true() {
        for name in PER_USER_CALENDAR_EVENT_PROPERTIES {
            assert!(
                is_per_user_calendar_event_property(name),
                "expected {name} to be classified per-user"
            );
        }
    }

    #[test]
    fn is_per_user_classifies_shared_properties_as_false() {
        // Spot-check a few shared (non-per-user) properties from the draft.
        for shared in &["id", "title", "start", "duration", "calendarIds", "uid"] {
            assert!(
                !is_per_user_calendar_event_property(shared),
                "expected {shared} to be classified shared"
            );
        }
    }

    #[test]
    fn is_per_user_rejects_unknown_property() {
        assert!(!is_per_user_calendar_event_property(""));
        assert!(!is_per_user_calendar_event_property("notARealProperty"));
        // Property-path forms like "alerts/abc" are NOT classified per-user;
        // the routing logic must look at the top-level patch key after
        // expanding any nested path.
        assert!(!is_per_user_calendar_event_property("alerts/abc"));
    }
}
