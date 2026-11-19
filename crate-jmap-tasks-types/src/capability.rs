//! Capability types for the JMAP Tasks extension.
//!
//! Normative reference: draft-ietf-jmap-tasks-06 §1.6.
//!
//! The Tasks extension defines six capability URIs.  Each has both a
//! session-level capability (value in the JMAP Session `capabilities` map)
//! and an account-level capability (value in the account's
//! `accountCapabilities` map).  Session-level values are empty objects for
//! all Tasks capabilities.

use jmap_jscalendar_types::{Duration, LocalDateTime};
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
    /// Earliest date-time the server accepts for any Task date.
    ///
    /// Typed `LocalDate` by draft-tasks-06 §1.6.1 (no time offset, per §1.2),
    /// so this is a [`LocalDateTime`], not a `UTCDate`.
    pub min_date_time: LocalDateTime,

    /// Latest date-time the server accepts for any Task date.
    ///
    /// Typed `LocalDate` by draft-tasks-06 §1.6.1 (no time offset, per §1.2),
    /// so this is a [`LocalDateTime`], not a `UTCDate`.
    pub max_date_time: LocalDateTime,

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
    /// (draft-tasks-06 §1.6.2: "maxExpandedQueryDuration: Duration").
    pub max_expanded_query_duration: Duration,
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
///
/// Participant identity follows JMAP Calendars rather than -06. -06 §2.1
/// gives the principal a `sendTo` map; draft-ietf-jmap-calendars-34
/// replaced that with a single `calendarAddress` (§2.1) and matches a
/// participant to a ParticipantIdentity by `calendarAddress` (§3). This
/// type carries `calendarAddress` in place of `sendTo`, as jmapio/jmap #470
/// proposes, so a server doing both Tasks and Calendars has one way to
/// decide whether a participant is the user.
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

    /// The `calendarAddress` to give a participant that is this principal,
    /// or `null` when it may not be added as a participant to a task
    /// (draft-ietf-jmap-calendars-34 §2.1: "If this Principal may be added
    /// as a participant to an event, this is the calendarAddress to use to
    /// receive iTIP scheduling messages; otherwise, this is null").
    ///
    /// Required-and-nullable: always present on the wire, as `null` or a
    /// String, as on the Calendars principal capability.
    pub calendar_address: Option<String>,
}
