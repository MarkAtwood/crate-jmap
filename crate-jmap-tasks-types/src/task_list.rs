//! TaskList object, TaskRights, and TaskListRole.
//!
//! Normative reference: draft-ietf-jmap-tasks-06 §3.

use std::collections::HashMap;

use jmap_types::{impl_string_enum, Id};
use serde::{Deserialize, Serialize};

/// Special role of a TaskList (draft-tasks-06 §3).
///
/// A TaskList may optionally hold one well-known role that identifies its
/// common purpose.  Roles map to iCalendar special-use mailbox semantics.
/// An account MUST NOT have more than one TaskList with any given role.
///
/// An unrecognised role is preserved via `Other(String)` so the wire value
/// round-trips without loss and can be echoed back to the server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskListRole {
    /// The principal's default task list (inbox for new tasks).
    Inbox,
    /// Task list holding tasks the user has discarded (trash).
    Trash,
    /// Any role string not recognised by this implementation.
    Other(String),
}

impl_string_enum!(TaskListRole, "a JMAP TaskList role string",
    "inbox" => Inbox,
    "trash" => Trash,
);

impl TaskListRole {
    /// Return the wire-format string for this role.
    pub fn to_wire_str(&self) -> &str {
        match self {
            Self::Inbox => "inbox",
            Self::Trash => "trash",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// Access control rights the authenticated user holds for a TaskList
/// (draft-tasks-06 §3).
///
/// `Default` produces all-false (no access), which is the most restrictive
/// valid value and a safe starting point when constructing rights in tests
/// or server code.
///
/// ## Invariant (spec §3)
///
/// If `may_write_all` is `true`, then `may_write_own`, `may_update_private`,
/// and `may_rsvp` MUST also be `true`.  This invariant is enforced by the
/// handler/backend layer, not this type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRights {
    /// User may fetch the tasks in this task list.
    pub may_read_items: bool,

    /// User may create, modify, or destroy all tasks in this task list, or
    /// move tasks to/from this task list.  If true, `may_write_own`,
    /// `may_update_private`, and `may_rsvp` MUST also be true.
    pub may_write_all: bool,

    /// User may create, modify, or destroy a task if they are the owner or
    /// the task has no owner.
    pub may_write_own: bool,

    /// User may modify per-user properties (e.g. `keywords`, `color`,
    /// `useDefaultAlerts`, `alerts`) on all tasks in this task list.
    pub may_update_private: bool,

    /// User may modify `participationStatus`, `participationComment`, and
    /// `expectReply` on Participant objects that correspond to their own
    /// ParticipantIdentity objects.
    #[serde(rename = "mayRSVP")]
    pub may_rsvp: bool,

    /// User may modify sharing for this task list (set `shareWith`).
    pub may_admin: bool,

    /// User may delete this task list itself (server-set).
    pub may_delete: bool,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP TaskList object (draft-tasks-06 §3).
///
/// A TaskList is a named collection of tasks.  All tasks belong to exactly
/// one TaskList.  The `id` is immutable and server-set.
///
/// ## Per-user properties
///
/// The following properties are stored per-user and may be set by any
/// subscribed user: `name`, `color`, `sort_order`, `time_zone`,
/// `default_alerts_with_time`, `default_alerts_without_time`.
///
/// ## `workflow_statuses` default
///
/// The spec defines a default of `["completed", "failed", "in-process",
/// "needs-action", "cancelled", "pending"]` (draft-tasks-06 §3).  This is a
/// server-side responsibility; the type does not bake in a default.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskList {
    /// Server-assigned immutable identifier.
    pub id: Id,

    /// Well-known role identifying the task list's common purpose, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<TaskListRole>,

    /// User-visible name for this task list (1–255 bytes UTF-8).
    pub name: String,

    /// Optional longer-form description for shared environments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// CSS color name or `#rrggbb` value, or `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Map of keyword strings to display colors (CSS color name or `#rrggbb`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_colors: Option<HashMap<String, String>>,

    /// Map of category strings to display colors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_colors: Option<HashMap<String, String>>,

    /// Client UI sort position; lower values sort first.  0 ≤ n < 2^31.
    pub sort_order: u32,

    /// Whether the user has subscribed to this task list.
    pub is_subscribed: bool,

    /// IANA Time Zone Database id, or `null` to inherit from the account principal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,

    /// Allowed values for `workflowStatus` on tasks in this list.
    ///
    /// Default per spec: `["completed", "failed", "in-process", "needs-action",
    /// "cancelled", "pending"]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_statuses: Option<Vec<String>>,

    /// Map of Principal id → rights for principals this list is shared with.
    /// `null` means the list is not shared.  May only be modified by a user
    /// with `may_admin` right.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_with: Option<HashMap<Id, TaskRights>>,

    /// Access rights the authenticated user holds for this list (server-set).
    pub my_rights: TaskRights,

    /// Map of alert id → Alert object (RFC 8984 §4.5.2) for timed tasks
    /// when `useDefaultAlerts` is true.  Alerts extension only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_alerts_with_time: Option<HashMap<Id, serde_json::Value>>,

    /// Map of alert id → Alert object for all-day tasks when
    /// `useDefaultAlerts` is true.  Alerts extension only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_alerts_without_time: Option<HashMap<Id, serde_json::Value>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
