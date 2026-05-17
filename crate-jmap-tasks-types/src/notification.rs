//! TaskNotification object and related types.
//!
//! Normative reference: draft-ietf-jmap-tasks-06 §5.
//!
//! TaskNotification records changes made by external entities to tasks in
//! task lists the user is subscribed to.  Notifications are stored in the
//! same Account as the Task that was changed.

use jmap_types::{impl_string_enum, Id, PatchObject, UTCDate};
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
    pub created: UTCDate,

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
    ///
    /// Encoded as a `PatchObject` (RFC 8620 §5.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_patch: Option<PatchObject>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Filter condition for `TaskNotification/query` (draft-tasks-06 §5.5.1).
///
/// All fields are optional; a condition with no fields set matches every
/// TaskNotification.
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
/// `Metadata` / `Annotation` companion object. Implemented in `jmap-metadata-types`,
/// `jmap-metadata-server`, and `jmap-metadata-client` (bd JMAP-06zp).
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
pub struct TaskNotificationFilterCondition {
    /// Notification `created` time must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<UTCDate>,

    /// Notification `created` time must be before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<UTCDate>,

    /// Notification `type` must equal this string.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,

    /// Notification `taskId` must be in this list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_ids: Option<Vec<Id>>,
}

/// Concrete filter type for TaskNotification/query
/// (draft-ietf-jmap-tasks-06 §5).
///
/// Alias for `jmap_types::query::Filter<TaskNotificationFilterCondition>`
/// provided so callers do not have to reach into `jmap-types` directly.
/// Mirrors the canonical [`jmap_mail_types::EmailFilter`] shape from the
/// workspace canonical extension-types template.
///
/// [`jmap_mail_types::EmailFilter`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/type.EmailFilter.html
pub type TaskNotificationFilter = jmap_types::query::Filter<TaskNotificationFilterCondition>;
