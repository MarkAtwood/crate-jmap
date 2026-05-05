//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP Calendars types.
//!
//! These are defined here so that `jmap-calendars-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the calendars types
//! are local to this crate).

use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Calendar`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// Property selector for [`crate::CalendarEventNotification`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    type Patch = serde_json::Value;
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
    type Patch = serde_json::Value;
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
/// The `Patch` type is never used in practice; `serde_json::Value` is a
/// safe placeholder that satisfies the trait bound.
impl SetObject for crate::CalendarEventNotification {
    type Patch = serde_json::Value;
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
    type Patch = serde_json::Value;
}

impl QueryObject for crate::ParticipantIdentity {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}
