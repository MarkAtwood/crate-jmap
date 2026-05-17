//! Task object and its auxiliary types.
//!
//! Normative references:
//!   - draft-ietf-jmap-tasks-06 §4 — JMAP-specific additions
//!   - RFC 8984 §4 — JSCalendar common properties
//!   - RFC 8984 §5.2 — JSCalendar Task-specific properties

use std::collections::HashMap;

use jmap_types::{impl_string_enum, Id, PatchObject, UTCDate};
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

// Serde-default functions for the `@type` discriminator on Person /
// CheckItem / Checklist / Comment. draft-tasks-06 §4.2.3 / §4.2.4
// mandate the literal type-name string on the wire; we supply it as a
// default so deserialize is liberal in what it accepts (spec-violating
// vendor input missing `@type` does not fail the whole parent's
// deserialize). See bd:JMAP-ky8g.1.

fn person_at_type_default() -> String {
    "Person".to_owned()
}

fn check_item_at_type_default() -> String {
    "CheckItem".to_owned()
}

fn checklist_at_type_default() -> String {
    "Checklist".to_owned()
}

fn comment_at_type_default() -> String {
    "Comment".to_owned()
}

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
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Person"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.1.
    #[serde(rename = "@type", default = "person_at_type_default")]
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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single item within a [`Checklist`] (draft-tasks-06 §4.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    /// Object type discriminator; MUST be `"CheckItem"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"CheckItem"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.1.
    #[serde(rename = "@type", default = "check_item_at_type_default")]
    pub at_type: String,

    /// Title / description of this checklist item.
    pub title: String,

    /// Client UI sort position within the checklist.  0 ≤ n < 2^31.
    #[serde(default)]
    pub sort_order: u32,

    /// When this item was last updated (UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<UTCDate>,

    /// Whether this item has been completed.
    pub is_complete: bool,

    /// Person this item is assigned to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Person>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A named checklist attached to a Task (draft-tasks-06 §4.2.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checklist {
    /// Object type discriminator; MUST be `"Checklist"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Checklist"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.1.
    #[serde(rename = "@type", default = "checklist_at_type_default")]
    pub at_type: String,

    /// Optional title for the checklist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Ordered list of items in this checklist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_items: Option<Vec<CheckItem>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A free-text comment attached to a Task (draft-tasks-06 §4.2.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    /// Object type discriminator; MUST be `"Comment"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"Comment"` rather than
    /// failing the whole parent object's deserialize. Serialize always
    /// emits the field. See bd:JMAP-ky8g.1.
    #[serde(rename = "@type", default = "comment_at_type_default")]
    pub at_type: String,

    /// The comment text.
    pub message: String,

    /// When this comment was created (UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<UTCDate>,

    /// When this comment was last updated (UTCDateTime).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<UTCDate>,

    /// Author of this comment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Person>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
/// Link, Alert, Participant, RecurrenceRule, TimeZone) are stored as
/// `serde_json::Value` to preserve the wire shape (including vendor
/// extensions permitted by RFC 8984 §3.3) and avoid coupling this crate to
/// a full JSCalendar type library.
///
/// Typed access to these sub-objects is available via the
/// [`jmap-jscalendar-types`](jmap_jscalendar_types) crate, re-exported by
/// this crate as the [`jscalendar`](crate::jscalendar) module alias and at
/// the crate root. Callers obtain typed views with
/// `serde_json::from_value::<Location>(task.locations.clone().unwrap())`
/// (and analogous for the other sub-types).
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
    pub utc_start: Option<UTCDate>,

    /// UTC due time, computed by the server.  Not in default response.
    #[serde(rename = "utcDue", skip_serializing_if = "Option::is_none")]
    pub utc_due: Option<UTCDate>,

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
    pub created: Option<UTCDate>,

    /// When this task was last updated (UTCDateTime; RFC 8984 §4.1.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<UTCDate>,

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
    pub progress_updated: Option<UTCDate>,

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

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Filter condition for `Task/query` (draft-tasks-06 §4.13).
///
/// All fields are optional; a condition with no fields set matches every Task.
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
    pub after: Option<UTCDate>,

    /// Task `due` date must be before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<UTCDate>,

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

/// Concrete filter type for Task/query (draft-ietf-jmap-tasks-06 §4.6).
///
/// Alias for `jmap_types::query::Filter<TaskFilterCondition>` provided so
/// callers do not have to reach into `jmap-types` directly. Mirrors the
/// canonical [`jmap_mail_types::EmailFilter`] shape from the workspace
/// canonical extension-types template.
///
/// [`jmap_mail_types::EmailFilter`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/type.EmailFilter.html
pub type TaskFilter = jmap_types::query::Filter<TaskFilterCondition>;
