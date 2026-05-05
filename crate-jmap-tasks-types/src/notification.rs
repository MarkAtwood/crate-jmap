//! TaskNotification object and related types.
//!
//! Normative reference: draft-ietf-jmap-tasks-06 §5.
//!
//! TaskNotification records changes made by external entities to tasks in
//! task lists the user is subscribed to.  Notifications are stored in the
//! same Account as the Task that was changed.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

use crate::task::Person;

/// Type of change recorded by a [`TaskNotification`] (draft-tasks-06 §5.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotificationType {
    /// A task was created.
    Created,
    /// A task was updated.
    Updated,
    /// A task was destroyed.
    Destroyed,
    /// Any notification type string not recognised by this implementation.
    Other(String),
}

impl_string_enum!(NotificationType, "a JMAP TaskNotification type string",
    "created"   => Created,
    "updated"   => Updated,
    "destroyed" => Destroyed,
);

/// A JMAP TaskNotification object (draft-tasks-06 §5.1).
///
/// Records a change made by an external entity to a task in a subscribed
/// task list.  Only destroyed notifications may be explicitly managed via
/// `TaskNotification/set`; created and updated notifications are server-set.
///
/// ## Wire field name note
///
/// The spec §5.1 uses `TaskId` (capital T) as the wire field name.  This
/// appears to be a typographical error — all other JMAP specs (including the
/// Calendars draft from which this spec was copied) use lowercase camelCase
/// `taskId`.  We follow lowercase camelCase here and document the discrepancy.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNotification {
    /// Server-assigned identifier for this notification.
    pub id: Id,

    /// When this notification was created (UTCDate).
    pub created: String,

    /// Who made the change (Person object as defined in draft-tasks-06 §4.2.3).
    pub changed_by: Person,

    /// Comment sent along with the change (e.g. iTIP COMMENT property), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,

    /// Type of change recorded.
    ///
    /// Wire field name is `type`.
    #[serde(rename = "type")]
    pub notification_type: NotificationType,

    /// Id of the Task this notification is about.
    ///
    /// Note: the spec §5.1 lists this as `TaskId` (capital T), which appears
    /// to be a typo.  We use `taskId` (lowercase) per camelCase convention.
    pub task_id: Id,

    /// Whether the task is a draft.  Present for `created` and `updated`
    /// notifications only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,

    /// The Task data before the change (for `updated` or `destroyed`), or
    /// the data after creation (for `created`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<serde_json::Value>,

    /// A patch encoding the change between `task` and the state after the
    /// update.  Present for `updated` notifications only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_patch: Option<serde_json::Value>,
}

/// Filter condition for `TaskNotification/query` (draft-tasks-06 §5.5.1).
///
/// All fields are optional; a condition with no fields set matches every
/// TaskNotification.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNotificationFilterCondition {
    /// Notification `created` time must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,

    /// Notification `created` time must be before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,

    /// Notification `type` must equal this string.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,

    /// Notification `taskId` must be in this list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_ids: Option<Vec<Id>>,
}
