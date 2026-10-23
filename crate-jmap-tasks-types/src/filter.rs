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

use jmap_types::impl_string_enum;
use serde::{Deserialize, Serialize};

/// The property to sort by in a [`TaskComparator`].
///
/// draft-ietf-jmap-tasks-06 §4.13: Task/query "is the
/// "CalendarEvent/query" method as described in
/// [I-D.ietf-jmap-calendars]", so these are the CalendarEvent/query sort
/// properties (draft-ietf-jmap-calendars-26 §5.11.2: `start`, `uid`,
/// `recurrenceId` MUST; `created`, `updated` SHOULD).
///
/// Unknown property names are preserved in
/// [`Other`](TaskComparatorProperty::Other) so they round-trip; a server
/// cannot sort by one it does not know (RFC 8620 §5.5 `unsupportedSort`).
/// Cross-reference: bd JMAP-wf92.11.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskComparatorProperty {
    /// Sort by `start`.
    Start,
    /// Sort by `uid`.
    Uid,
    /// Sort by `recurrenceId`.
    RecurrenceId,
    /// Sort by `created`.
    Created,
    /// Sort by `updated`.
    Updated,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(TaskComparatorProperty, "a Task sort property string",
    "start"        => Start,
    "uid"          => Uid,
    "recurrenceId" => RecurrenceId,
    "created"      => Created,
    "updated"      => Updated,
);

/// The property to sort by in a [`TaskNotificationComparator`]
/// (draft-ietf-jmap-tasks-06 §5.5.2: "The "created" property MUST be
/// supported for sorting.").
///
/// Unknown property names are preserved in
/// [`Other`](TaskNotificationComparatorProperty::Other) so they round-trip;
/// a server cannot sort by one it does not know (RFC 8620 §5.5
/// `unsupportedSort`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskNotificationComparatorProperty {
    /// Sort by `created`.
    Created,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(TaskNotificationComparatorProperty, "a TaskNotification sort property string",
    "created" => Created,
);

/// Comparator for `Task/query` (draft-tasks-06 §4.13).
///
/// Build one with [`TaskComparator::new`]; the struct is
/// `#[non_exhaustive]`.
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
/// recommended paths (`draft-ietf-jmap-metadata`, bd JMAP-ua37; or the
/// pre-IETF sloppy-value escape).
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskComparator {
    /// The property to sort by.
    pub property: TaskComparatorProperty,

    /// If true, sort ascending; if false, sort descending.
    /// Defaults to true per JMAP base §5.5.
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub is_ascending: bool,

    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

/// Comparator for `TaskNotification/query` (draft-tasks-06 §5.5.2).
///
/// Build one with [`TaskNotificationComparator::new`]; the struct is
/// `#[non_exhaustive]`.
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field, and its `property`
/// field is consumed by backend dispatch to determine sort order. See
/// `TaskNotificationFilterCondition` in `notification.rs` for the
/// rationale and for the two recommended paths
/// (`draft-ietf-jmap-metadata`, bd JMAP-ua37; or the pre-IETF
/// sloppy-value escape).
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNotificationComparator {
    /// The property to sort by.
    pub property: TaskNotificationComparatorProperty,

    /// If true, sort ascending; if false, sort descending.
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub is_ascending: bool,

    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl TaskComparator {
    /// An ascending comparator on `property` with no collation
    /// (`isAscending` defaults to `true`, RFC 8620 §5.5).
    pub fn new(property: TaskComparatorProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

impl TaskNotificationComparator {
    /// An ascending comparator on `property` with no collation
    /// (`isAscending` defaults to `true`, RFC 8620 §5.5).
    pub fn new(property: TaskNotificationComparatorProperty) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

fn bool_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}
