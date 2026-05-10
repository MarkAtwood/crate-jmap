//! Task object and its auxiliary types.
//!
//! Normative references:
//!   - draft-ietf-jmap-tasks-06 §4 — JMAP-specific additions
//!   - RFC 8984 §4 — JSCalendar common properties
//!   - RFC 8984 §5.2 — JSCalendar Task-specific properties

use std::collections::HashMap;

use jmap_types::{impl_string_enum, Id, PatchObject};
use serde::{Deserialize, Serialize};

/// Progress status of a Task (RFC 8984 §5.2.5).
///
/// Wire values use the hyphenated form mandated by RFC 8984 (e.g. `"needs-action"`).
/// Vendor-specific values are preserved via `Other(String)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskProgress {
    /// Task has not yet been started.
    NeedsAction,
    /// Task is currently being worked on.
    InProcess,
    /// Task has been completed.
    Completed,
    /// Task has failed.
    Failed,
    /// Task has been cancelled.
    Cancelled,
    /// Any progress string not recognised by this implementation.
    Other(String),
}

impl_string_enum!(TaskProgress, "a JSCalendar Task progress string",
    "needs-action" => NeedsAction,
    "in-process"   => InProcess,
    "completed"    => Completed,
    "failed"       => Failed,
    "cancelled"    => Cancelled,
);

/// A person reference used in CheckItem assignee and Comment author fields
/// (draft-tasks-06 §4.2.3).
///
/// Either `uri` or `principal_id` MUST be set per the spec; this is enforced
/// by the handler layer, not the type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// Object type discriminator; MUST be `"Person"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Display name of the person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// URI (typically the scheduleId of the corresponding Participant).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Id of the Principal corresponding to this person, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
}

/// A single item within a [`Checklist`] (draft-tasks-06 §4.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    /// Object type discriminator; MUST be `"CheckItem"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Title / description of this checklist item.
    pub title: String,

    /// Client UI sort position within the checklist.  0 ≤ n < 2^31.
    #[serde(default)]
    pub sort_order: u32,

    /// When this item was last updated (UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,

    /// Whether this item has been completed.
    pub is_complete: bool,

    /// Person this item is assigned to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Person>,
}

/// A named checklist attached to a Task (draft-tasks-06 §4.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checklist {
    /// Object type discriminator; MUST be `"Checklist"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// Optional title for the checklist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Ordered list of items in this checklist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_items: Option<Vec<CheckItem>>,
}

/// A free-text comment attached to a Task (draft-tasks-06 §4.2.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    /// Object type discriminator; MUST be `"Comment"` on the wire.
    #[serde(rename = "@type")]
    pub at_type: String,

    /// The comment text.
    pub message: String,

    /// When this comment was created (UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    /// When this comment was last updated (UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,

    /// Author of this comment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Person>,
}

/// A JMAP Task object (draft-tasks-06 §4; RFC 8984 §4, §5.2).
///
/// A Task is a JSCalendar Task (RFC 8984) with JMAP-specific additions.  All
/// fields are `Option` because RFC 8620 §5.1 allows partial responses — a
/// `Task/get` response with a `properties` argument returns only the requested
/// fields, and absent fields must not fail deserialization.
///
/// ## Field groupings
///
/// Fields are grouped in source order as they appear in the specs:
/// 1. JMAP additions (draft-tasks-06 §4)
/// 2. JSCalendar Metadata Properties (RFC 8984 §4.1)
/// 3. JSCalendar What/Where Properties (RFC 8984 §4.2)
/// 4. JSCalendar Recurrence Properties (RFC 8984 §4.3)
/// 5. JSCalendar Sharing/Scheduling Properties (RFC 8984 §4.4)
/// 6. JSCalendar Alert Properties (RFC 8984 §4.5)
/// 7. JSCalendar Multilingual Properties (RFC 8984 §4.6)
/// 8. JSCalendar Time Zone Properties (RFC 8984 §4.7)
/// 9. JSCalendar Task-specific Properties (RFC 8984 §5.2)
/// 10. Tasks-draft-defined JSCalendar properties (draft-tasks-06 §4.2)
///
/// ## Complex sub-objects
///
/// JSCalendar sub-objects that are deeply nested (Location, VirtualLocation,
/// Link, Alert, Participant, RecurrenceRule, TimeZone) are represented as
/// `serde_json::Value` to defer parsing cost and avoid maintaining a full
/// JSCalendar type library in this crate.  This can be migrated to typed
/// representations in a future semver-breaking release.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    // --- JMAP additions (draft-tasks-06 §4) ---
    /// Server-assigned immutable identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,

    /// Id of the single TaskList this task belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_list_id: Option<Id>,

    /// If true, this task is a draft and the server will not send alerts.
    /// Once set to false, cannot be reverted to true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,

    /// UTC start time, computed by the server from `start` + `time_zone`.
    /// Not included in default responses; must be requested explicitly.
    #[serde(rename = "utcStart", skip_serializing_if = "Option::is_none")]
    pub utc_start: Option<String>,

    /// UTC due time, computed by the server.  Not in default response.
    #[serde(rename = "utcDue", skip_serializing_if = "Option::is_none")]
    pub utc_due: Option<String>,

    /// Client UI sort position; lower values sort first.  0 ≤ n < 2^31.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<u32>,

    /// Current workflow status.  Must be one of the TaskList's
    /// `workflowStatuses`.  If set, `progress` MUST be null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_status: Option<String>,

    /// For expanded recurring task instances: id of the master Task.
    /// Immutable; server-set.  Recurrences extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_task_id: Option<Id>,

    /// Whether this account is the authoritative source for this task.
    /// Server-set.  Assignees extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_origin: Option<bool>,

    /// If true, any user with access may add themselves as an attendee.
    /// Assignees extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub may_invite_self: Option<bool>,

    /// If true, existing attendees may add new attendees.
    /// Assignees extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub may_invite_others: Option<bool>,

    /// If true, only owners may see the full participant list.
    /// Assignees extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_attendees: Option<bool>,

    // --- JSCalendar Metadata Properties (RFC 8984 §4.1) ---
    /// Object type discriminator; `"Task"` on the wire.
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub at_type: Option<String>,

    /// Globally unique identifier for this task (RFC 8984 §4.1.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Map of related object UIDs to relation type objects (RFC 8984 §4.1.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_to: Option<HashMap<String, serde_json::Value>>,

    /// Product identifier of the software that last modified this task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prod_id: Option<String>,

    /// When this task was first created (UTCDateTime; RFC 8984 §4.1.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    /// When this task was last updated (UTCDateTime; RFC 8984 §4.1.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,

    /// Sequence number for iTIP scheduling (RFC 8984 §4.1.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,

    // --- JSCalendar What/Where Properties (RFC 8984 §4.2) ---
    /// Short summary / title of the task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Extended description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// MIME type of `description`; defaults to `"text/plain"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_content_type: Option<String>,

    /// If true, the task has no associated time; treated as all-day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_without_time: Option<bool>,

    /// Map of location id → Location objects (RFC 8984 §4.2.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<HashMap<Id, serde_json::Value>>,

    /// Map of virtual location id → VirtualLocation objects (RFC 8984 §4.2.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_locations: Option<HashMap<Id, serde_json::Value>>,

    /// Map of link id → Link objects (RFC 8984 §4.2.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<HashMap<Id, serde_json::Value>>,

    /// BCP 47 language tag for the task content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,

    /// Map of keyword strings to `true` (RFC 8984 §4.2.10).
    /// Per-user in shared task lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<HashMap<String, bool>>,

    /// Map of category strings to `true` (RFC 8984 §4.2.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<HashMap<String, bool>>,

    /// CSS color name or `#rrggbb` value; per-user in shared lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    // --- JSCalendar Recurrence Properties (RFC 8984 §4.3) ---
    // These fields are gated on the recurrences extension at the handler layer.
    /// Recurrence id for a specific instance of a recurring task (LocalDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_id: Option<String>,

    /// Time zone for interpreting `recurrence_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_id_time_zone: Option<String>,

    /// List of RecurrenceRule objects (RFC 8984 §4.3.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_rules: Option<Vec<serde_json::Value>>,

    /// List of RecurrenceRule objects for exclusions (RFC 8984 §4.3.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_recurrence_rules: Option<Vec<serde_json::Value>>,

    /// Map of LocalDateTime → PatchObject for per-instance overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_overrides: Option<HashMap<String, PatchObject>>,

    /// If true, this instance is excluded from the recurrence set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded: Option<bool>,

    // --- JSCalendar Sharing/Scheduling Properties (RFC 8984 §4.4) ---
    /// Priority, 0–9 per iCalendar semantics (RFC 8984 §4.4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,

    /// Free/busy status for calendar blocking (RFC 8984 §4.4.2); per-user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_busy_status: Option<String>,

    /// Privacy classification: `"public"`, `"private"`, or `"secret"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy: Option<String>,

    /// Map of method → URI for iTIP scheduling replies (RFC 8984 §4.4.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<HashMap<String, String>>,

    /// Email address of the entity that sent this task on behalf of the
    /// organizer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_by: Option<String>,

    /// Map of participant id → Participant objects (RFC 8984 §4.4.6).
    /// Assignees extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<HashMap<Id, serde_json::Value>>,

    /// iTIP request status (RFC 8984 §4.4.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_status: Option<String>,

    // --- JSCalendar Alert Properties (RFC 8984 §4.5) ---
    // These fields are gated on the alerts extension at the handler layer.
    /// If true, apply the TaskList's default alerts instead of `alerts`.
    /// Per-user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_default_alerts: Option<bool>,

    /// Map of alert id → Alert objects (RFC 8984 §4.5.2).  Per-user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alerts: Option<HashMap<Id, serde_json::Value>>,

    // --- JSCalendar Multilingual Properties (RFC 8984 §4.6) ---
    // Gated on the multilingual extension at the handler layer.
    /// Map of BCP 47 language tag → patch object for localised property values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localizations: Option<HashMap<String, PatchObject>>,

    // --- JSCalendar Time Zone Properties (RFC 8984 §4.7) ---
    /// IANA Time Zone Database id for interpreting `start` and `due`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,

    /// Map of time zone id → custom TimeZone objects (RFC 8984 §4.7.2).
    /// Customtimezones extension.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zones: Option<HashMap<String, serde_json::Value>>,

    // --- JSCalendar Task-specific Properties (RFC 8984 §5.2) ---
    /// Due date/time (LocalDateTime; RFC 8984 §5.2.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,

    /// Start date/time (LocalDateTime; RFC 8984 §5.2.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,

    /// Estimated duration as an ISO 8601 duration string (RFC 8984 §5.2.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_duration: Option<String>,

    /// Percentage of work completed, 0–100 (RFC 8984 §5.2.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<u8>,

    /// Current progress status (RFC 8984 §5.2.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<TaskProgress>,

    /// When `progress` was last updated (UTCDateTime; RFC 8984 §5.2.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_updated: Option<String>,

    // --- Tasks-draft-defined JSCalendar properties (draft-tasks-06 §4.2) ---
    /// Estimated work in story points / complexity units (draft-tasks-06 §4.2.1).
    /// Type: `UnsignedInt|null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_work: Option<u64>,

    /// Impact/severity description (draft-tasks-06 §4.2.2).
    /// Examples: `"minor"`, `"major"`, `"blocking"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,

    /// Map of checklist id → Checklist objects (draft-tasks-06 §4.2.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checklists: Option<HashMap<Id, Checklist>>,

    /// Map of comment id → Comment objects (draft-tasks-06 §4.2.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<HashMap<Id, Comment>>,
}

/// Filter condition for `Task/query` (draft-tasks-06 §4.13).
///
/// All fields are optional; a condition with no fields set matches every Task.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFilterCondition {
    /// Task must belong to this TaskList.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_list_id: Option<Id>,

    /// Task `uid` must equal this string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Task must have this keyword set to `true` in its `keywords` map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_keyword: Option<String>,

    /// Task must NOT have this keyword set to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_keyword: Option<String>,

    /// Free-text search across task fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Task `title` must contain this string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Task `description` must contain this string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Task `due` date must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,

    /// Task `due` date must be before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,

    /// If true/false, filter by whether the task is a draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,

    /// Filter by `progress` value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,

    /// Filter by `workflow_status` value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_status: Option<String>,
}
