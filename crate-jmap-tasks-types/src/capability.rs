//! Capability types for the JMAP Tasks extension.
//!
//! Normative reference: draft-ietf-jmap-tasks-06 §1.6.
//!
//! The Tasks extension defines six capability URIs.  Each has both a
//! session-level capability (value in the JMAP Session `capabilities` map)
//! and an account-level capability (value in the account's
//! `accountCapabilities` map).  Session-level values are empty objects for
//! all Tasks capabilities.

use serde::{Deserialize, Serialize};

/// Capability URI for core JMAP Tasks support (draft-tasks-06 §1.6.1).
pub const JMAP_TASKS_URI: &str = "urn:ietf:params:jmap:tasks";

/// Capability URI for the Tasks recurrences extension (draft-tasks-06 §1.6.2).
pub const JMAP_TASKS_RECURRENCES_URI: &str = "urn:ietf:params:jmap:tasks:recurrences";

/// Capability URI for the Tasks assignees extension (draft-tasks-06 §1.6.3).
pub const JMAP_TASKS_ASSIGNEES_URI: &str = "urn:ietf:params:jmap:tasks:assignees";

/// Capability URI for the Tasks alerts extension (draft-tasks-06 §1.6.4).
pub const JMAP_TASKS_ALERTS_URI: &str = "urn:ietf:params:jmap:tasks:alerts";

/// Capability URI for the Tasks multilingual extension (draft-tasks-06 §1.6.5).
pub const JMAP_TASKS_MULTILINGUAL_URI: &str = "urn:ietf:params:jmap:tasks:multilingual";

/// Capability URI for the Tasks custom time zones extension (draft-tasks-06 §1.6.6).
pub const JMAP_TASKS_CUSTOMTIMEZONES_URI: &str = "urn:ietf:params:jmap:tasks:customtimezones";

/// Session-level Tasks capability (draft-tasks-06 §1.6.1).
///
/// The value of `capabilities["urn:ietf:params:jmap:tasks"]` in the JMAP
/// Session object.  The spec mandates that this is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksCapability {}

/// Account-level Tasks capability (draft-tasks-06 §1.6.1).
///
/// The value of `accountCapabilities["urn:ietf:params:jmap:tasks"]` for a
/// given account.  Describes server capabilities and account-level permissions
/// for the Tasks extension.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksAccountCapability {
    /// Earliest date-time the server accepts for any Task date (LocalDate).
    pub min_date_time: String,

    /// Latest date-time the server accepts for any Task date (LocalDate).
    pub max_date_time: String,

    /// If true, the user may create a task list in this account.
    pub may_create_task_list: bool,
}

/// Session-level capability for the recurrences extension (draft-tasks-06 §1.6.2).
///
/// Value is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksRecurrencesCapability {}

/// Account-level capability for the recurrences extension (draft-tasks-06 §1.6.2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksRecurrencesAccountCapability {
    /// Maximum duration over which the server will expand recurrences
    /// (ISO 8601 Duration string).
    pub max_expanded_query_duration: String,
}

/// Session-level capability for the assignees extension (draft-tasks-06 §1.6.3).
///
/// Value is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksAssigneesCapability {}

/// Account-level capability for the assignees extension (draft-tasks-06 §1.6.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksAssigneesAccountCapability {
    /// Maximum number of participants per task, or `null` for no limit.
    pub max_participants_per_task: Option<u64>,
}

/// Session-level capability for the alerts extension (draft-tasks-06 §1.6.4).
///
/// Value is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksAlertsCapability {}

/// Session-level capability for the multilingual extension (draft-tasks-06 §1.6.5).
///
/// Value is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksMultilingualCapability {}

/// Session-level capability for the custom time zones extension
/// (draft-tasks-06 §1.6.6).
///
/// Value is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksCustomTimeZonesCapability {}
