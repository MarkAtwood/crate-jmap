//! CalendarEventNotification, Person, and NotificationType.
//!
//! Normative reference: draft-ietf-jmap-calendars-26 §7.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Who made the change that triggered a [`CalendarEventNotification`]
/// (draft-ietf-jmap-calendars-26 §7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// Display name of the person who made the change.
    pub name: String,

    /// Email address of the person, or `null` if not available.
    pub email: Option<String>,

    /// Id of the Principal corresponding to this person, or `null` if
    /// the change came from an external entity (e.g. an iTIP invitation).
    pub principal_id: Option<Id>,

    /// CalendarAddress URI of the person, or `null`.
    pub calendar_address: Option<String>,
}

/// Type of change recorded by a [`CalendarEventNotification`]
/// (draft-ietf-jmap-calendars-26 §7).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotificationType {
    /// A calendar event was created.
    Created,
    /// A calendar event was updated.
    Updated,
    /// A calendar event was destroyed.
    Destroyed,
    /// Any notification type string not recognised by this implementation.
    Other(String),
}

impl_string_enum!(NotificationType, "a JMAP CalendarEventNotification type string",
    "created"   => Created,
    "updated"   => Updated,
    "destroyed" => Destroyed,
);

/// A JMAP CalendarEventNotification object
/// (draft-ietf-jmap-calendars-26 §7).
///
/// Records a change made to a calendar event by an external entity or
/// another client.  CalendarEventNotifications have no per-user data; a
/// single notification object is shared across all users who have access
/// to the changed event.
///
/// Only `destroyed` notifications may be explicitly managed via
/// `CalendarEventNotification/set`; `created` and `updated` notifications
/// are server-set.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventNotification {
    /// Server-assigned identifier for this notification.
    pub id: Id,

    /// UTC date-time when this notification was created.
    pub created: String,

    /// Who made the change.
    pub changed_by: Person,

    /// Comment sent along with the change (e.g. iTIP COMMENT property), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,

    /// Type of change recorded.
    ///
    /// Wire field name is `type`.
    #[serde(rename = "type")]
    pub notification_type: NotificationType,

    /// Id of the CalendarEvent this notification is about (always the base
    /// event id, even for single-instance changes).
    pub calendar_event_id: Id,

    /// Whether the event is a draft.  Present for `created` and `updated`
    /// notifications only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,

    /// The CalendarEvent data before the change (for `updated` or `destroyed`),
    /// or the data after creation (for `created`).
    pub event: serde_json::Value,

    /// A PatchObject encoding the change between `event` and the state after
    /// the update.  Present for `updated` notifications only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_patch: Option<serde_json::Value>,
}

/// Push-notification payload emitted when a calendar alert fires
/// (draft-ietf-jmap-calendars-26 §6.4).
///
/// Sent over the JMAP push channel when the server determines that an alert
/// for a CalendarEvent should fire.  Clients receiving this payload should
/// trigger the appropriate local notification (e.g. an OS alert).
///
/// `recurrenceId` is `null` for non-recurring events and MUST serialize as
/// `null` (not be omitted) — the receiver uses its presence to distinguish
/// recurring from non-recurring.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarAlert {
    /// Object type discriminator; always `"CalendarAlert"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// The account that owns the calendar event.
    pub account_id: Id,

    /// Id of the CalendarEvent for which this alert is firing.
    pub calendar_event_id: Id,

    /// The UID property of the iCalendar object underlying the calendar event.
    pub uid: String,

    /// The recurrenceId of the specific occurrence, or `null` for non-recurring
    /// events (draft-ietf-jmap-calendars-26 §6.4).
    ///
    /// Serializes as `null` when `None` — intentionally NOT marked
    /// `skip_serializing_if` so the receiver can distinguish recurring from
    /// non-recurring events.
    pub recurrence_id: Option<String>,

    /// Id of the [`Alert`](crate::Alert) object within the CalendarEvent that is firing.
    pub alert_id: String,
}

/// Filter condition for `CalendarEventNotification/query`
/// (draft-ietf-jmap-calendars-26 §7.4.1).
///
/// All fields are optional; a condition with no fields set matches every
/// CalendarEventNotification.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationFilterCondition {
    /// Notification `created` time must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,

    /// Notification `created` time must be before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,

    /// Notification `type` must equal this string.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,

    /// Notification `calendarEventId` must be in this list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_event_ids: Option<Vec<Id>>,
}
