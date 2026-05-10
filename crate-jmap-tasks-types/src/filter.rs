//! Comparator types for Task and TaskNotification queries.
//!
//! Normative references:
//!   - draft-ietf-jmap-tasks-06 §4.13 (Task/query)
//!   - draft-ietf-jmap-tasks-06 §5.5.2 (TaskNotification/query sorting)
//!
//! `TaskFilterCondition` lives in `task.rs` alongside the `Task` type.
//! `TaskNotificationFilterCondition` lives in `notification.rs` alongside
//! `TaskNotification`.  This module contains only the comparator types used
//! for sorting query results.

use serde::{Deserialize, Serialize};

/// Comparator for `Task/query` (draft-tasks-06 §4.13).
///
/// Mirrors the CalendarEvent/query comparators from the Calendars draft.
/// The spec §4.13 is a stub referencing the Calendars spec, so common JMAP
/// comparator fields are included based on JMAP base §5.5 and the Calendars
/// analogue.
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field, and its `property`
/// field is consumed by backend dispatch to determine sort order. See
/// `TaskFilterCondition` in `task.rs` for the rationale and for the two
/// recommended paths (`draft-ietf-jmap-metadata`, bd JMAP-06zp; or the
/// pre-IETF sloppy-value escape).
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskComparator {
    /// Property name to sort by.
    pub property: String,

    /// If true, sort ascending; if false, sort descending.
    /// Defaults to true per JMAP base §5.5.
    #[serde(default = "default_ascending")]
    pub is_ascending: bool,

    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

/// Comparator for `TaskNotification/query` (draft-tasks-06 §5.5.2).
///
/// The spec mandates that the `"created"` property MUST be supported for
/// sorting.
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field, and its `property`
/// field is consumed by backend dispatch to determine sort order. See
/// `TaskNotificationFilterCondition` in `notification.rs` for the
/// rationale and for the two recommended paths
/// (`draft-ietf-jmap-metadata`, bd JMAP-06zp; or the pre-IETF
/// sloppy-value escape).
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNotificationComparator {
    /// Property name to sort by.  `"created"` MUST be supported.
    pub property: String,

    /// If true, sort ascending; if false, sort descending.
    #[serde(default = "default_ascending")]
    pub is_ascending: bool,

    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

fn default_ascending() -> bool {
    true
}
