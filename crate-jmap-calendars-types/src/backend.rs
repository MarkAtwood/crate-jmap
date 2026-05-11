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
    /// The `id` property (draft-ietf-jmap-calendars-26 §4).
    Id,
    /// The `name` property (draft-ietf-jmap-calendars-26 §4).
    Name,
    /// The `description` property (draft-ietf-jmap-calendars-26 §4).
    Description,
    /// The `color` property (draft-ietf-jmap-calendars-26 §4).
    Color,
    /// The `sortOrder` property (draft-ietf-jmap-calendars-26 §4).
    SortOrder,
    /// The `isSubscribed` property (draft-ietf-jmap-calendars-26 §4).
    IsSubscribed,
    /// The `isVisible` property (draft-ietf-jmap-calendars-26 §4).
    IsVisible,
    /// The `isDefault` property (draft-ietf-jmap-calendars-26 §4).
    IsDefault,
    /// The `includeInAvailability` property (draft-ietf-jmap-calendars-26 §4).
    IncludeInAvailability,
    /// The `defaultAlertsWithTime` property (draft-ietf-jmap-calendars-26 §4).
    DefaultAlertsWithTime,
    /// The `defaultAlertsWithoutTime` property (draft-ietf-jmap-calendars-26 §4).
    DefaultAlertsWithoutTime,
    /// The `timeZone` property (draft-ietf-jmap-calendars-26 §4).
    TimeZone,
    /// The `shareWith` property (draft-ietf-jmap-calendars-26 §4).
    ShareWith,
    /// The `myRights` property (draft-ietf-jmap-calendars-26 §4).
    MyRights,
}

/// Property selector for [`crate::CalendarEvent`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarEventProperty {
    /// The `id` property (draft-ietf-jmap-calendars-26 §5).
    Id,
    /// The `baseEventId` property (draft-ietf-jmap-calendars-26 §5).
    BaseEventId,
    /// The `calendarIds` property (draft-ietf-jmap-calendars-26 §5).
    CalendarIds,
    /// The `isDraft` property (draft-ietf-jmap-calendars-26 §5).
    IsDraft,
    /// The `isOrigin` property (draft-ietf-jmap-calendars-26 §5).
    IsOrigin,
    /// The `utcStart` property (draft-ietf-jmap-calendars-26 §5).
    UtcStart,
    /// The `utcEnd` property (draft-ietf-jmap-calendars-26 §5).
    UtcEnd,
    /// The `useDefaultAlerts` property (draft-ietf-jmap-calendars-26 §5).
    UseDefaultAlerts,
    /// The `mayInviteSelf` property (draft-ietf-jmap-calendars-26 §5.1.1).
    MayInviteSelf,
    /// The `mayInviteOthers` property (draft-ietf-jmap-calendars-26 §5.1.2).
    MayInviteOthers,
    /// The `hideAttendees` property (draft-ietf-jmap-calendars-26 §5.1.3).
    HideAttendees,
    /// The `blobId` property (draft-ietf-jmap-calendars-26 §10.9.14).
    BlobId,
    /// The `uid` property, inherited from the JSCalendar Event object (RFC 8984 §4.1.2).
    Uid,
    /// The `title` property, inherited from the JSCalendar Event object (RFC 8984 §4.2.1).
    Title,
    /// The `description` property, inherited from the JSCalendar Event object (RFC 8984 §4.2.2).
    Description,
    /// The `start` property, inherited from the JSCalendar Event object (RFC 8984 §5.1.1).
    Start,
    /// The `duration` property, inherited from the JSCalendar Event object (RFC 8984 §5.1.2).
    Duration,
    /// The `status` property, inherited from the JSCalendar Event object (RFC 8984 §5.1.3).
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
    /// The `id` property (draft-ietf-jmap-calendars-26 §7).
    Id,
    /// The `created` property (draft-ietf-jmap-calendars-26 §7).
    Created,
    /// The `changedBy` property (draft-ietf-jmap-calendars-26 §7).
    ChangedBy,
    /// The `comment` property (draft-ietf-jmap-calendars-26 §7).
    Comment,
    /// The `type` property (draft-ietf-jmap-calendars-26 §7).
    Type,
    /// The `calendarEventId` property (draft-ietf-jmap-calendars-26 §7).
    CalendarEventId,
    /// The `isDraft` property (draft-ietf-jmap-calendars-26 §7).
    IsDraft,
    /// The `event` property (draft-ietf-jmap-calendars-26 §7).
    Event,
    /// The `eventPatch` property (draft-ietf-jmap-calendars-26 §7).
    EventPatch,
}

/// Property selector for [`crate::ParticipantIdentity`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParticipantIdentityProperty {
    /// The `id` property (draft-ietf-jmap-calendars-26 §3).
    Id,
    /// The `name` property (draft-ietf-jmap-calendars-26 §3).
    Name,
    /// The `calendarAddress` property (draft-ietf-jmap-calendars-26 §3).
    CalendarAddress,
    /// The `isDefault` property (draft-ietf-jmap-calendars-26 §3).
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
