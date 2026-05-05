//! Capability types for the JMAP Tasks extension.
//!
//! Normative reference: draft-ietf-jmap-tasks-06 §1.6.
//!
//! The Tasks extension defines six capability URIs.  Each has both a
//! session-level capability (value in the JMAP Session `capabilities` map)
//! and an account-level capability (value in the account's
//! `accountCapabilities` map).  Session-level values are empty objects for
//! all Tasks capabilities.

use std::collections::HashMap;

use jmap_types::Id;
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

/// Account-level capability for the JMAP Tasks Alerts extension
/// (draft-ietf-jmap-tasks-06 §1.6.4).
///
/// Empty object — all server capabilities for alerts are signalled at
/// session level by [`TasksAlertsCapability`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksAlertsAccountCapability {}

/// Session-level capability for the multilingual extension (draft-tasks-06 §1.6.5).
///
/// Value is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksMultilingualCapability {}

/// Account-level capability for the JMAP Tasks Multilingual extension
/// (draft-ietf-jmap-tasks-06 §1.6.5).
///
/// Empty object — all server capabilities for multilingual support are
/// signalled at session level by [`TasksMultilingualCapability`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksMultilingualAccountCapability {}

/// Session-level capability for the custom time zones extension
/// (draft-tasks-06 §1.6.6).
///
/// Value is an empty object `{}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksCustomTimeZonesCapability {}

/// Account-level capability for the JMAP Tasks Custom Time Zones extension
/// (draft-ietf-jmap-tasks-06 §1.6.6).
///
/// Empty object — all server capabilities for custom time zones are
/// signalled at session level by [`TasksCustomTimeZonesCapability`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TasksCustomTimeZonesAccountCapability {}

/// Capability placed on a JMAP Sharing Principal's capabilities map
/// under `"urn:ietf:params:jmap:tasks"` (draft-ietf-jmap-tasks-06 §2.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalTasksCapability {
    /// Id of the account this principal may use for JMAP Tasks,
    /// or `null` if the principal has no Tasks account.
    ///
    /// Required-nullable (serializes as `null` when `None`, not absent).
    pub account_id: Option<Id>,

    /// Whether the client may invite this principal to share a task list.
    pub may_share_with: bool,

    /// Method types to which the principal has a send address, and the
    /// address for each.  `null` or absent when the principal cannot be
    /// sent to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_to: Option<HashMap<String, String>>,
}
