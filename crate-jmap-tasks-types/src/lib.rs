//! JMAP Tasks extension data types.
//!
//! Implements the data model from draft-ietf-jmap-tasks-06 and the JSCalendar
//! Task subset of RFC 8984.  Types only — no method handlers, no async, no
//! network I/O.
//!
//! ## Crate family position
//!
//! ```text
//! jmap-types (RFC 8620 wire primitives)
//!     └── jmap-tasks-types  ← this crate
//!             ├── jmap-tasks-server (method handlers)
//!             └── jmap-tasks-client (client extension trait)
//! ```
//!
//! ## Modules
//!
//! | Module | Contents |
//! |---|---|
//! | `task_list` | [`TaskList`], [`TaskRights`], [`TaskListRole`] |
//! | `task` | [`Task`], [`TaskProgress`], [`Checklist`], [`CheckItem`], [`Comment`], [`Person`], [`TaskFilterCondition`] |
//! | `notification` | [`TaskNotification`], [`NotificationType`], [`TaskNotificationFilterCondition`] |
//! | `filter` | [`TaskComparator`], [`TaskComparatorProperty`], [`TaskNotificationComparator`], [`TaskNotificationComparatorProperty`] |
//! | `ownership` | [`is_task_owner`], [`task_has_owner`] (the `mayWriteOwn` owner, as Calendars defines it) |
//! | Crate root | [`TasksCapability`], [`TasksAccountCapability`], URI constants, extension capabilities |
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types ([`Task`],
//! [`Comment`], [`Person`], [`Checklist`], [`CheckItem`], [`TaskNotification`],
//! and the re-exported [`Participant`] / [`Location`]). `tracing` or `log`
//! macros that interpolate one of these values with `?` (debug) or `%`
//! (display) emit its full contents to the configured sink: task titles and
//! descriptions, comments, assignee names and addresses, and locations. That
//! data is typically regulated as personal data under GDPR, CCPA, and similar
//! privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks title, description, comments, and assignees.
//! tracing::debug!(task = ?task, "updated task");
//!
//! // GOOD — emits only the opaque id.
//! tracing::debug!(task_id = ?task.id, "updated task");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod backend;
mod capability;
mod filter;
mod notification;
mod ownership;
mod task;
mod task_list;

// --- Top-level re-exports ---

pub use backend::{
    PER_USER_TASK_PROPERTIES, TaskListProperty, TaskNotificationProperty, TaskProperty,
    is_per_user_task_property,
};
pub use capability::{
    JMAP_TASKS_ALERTS_URI, JMAP_TASKS_ASSIGNEES_URI, JMAP_TASKS_CUSTOMTIMEZONES_URI,
    JMAP_TASKS_MULTILINGUAL_URI, JMAP_TASKS_RECURRENCES_URI, JMAP_TASKS_URI,
    PrincipalTasksCapability, TasksAccountCapability, TasksAlertsAccountCapability,
    TasksAlertsCapability, TasksAssigneesAccountCapability, TasksAssigneesCapability,
    TasksCapability, TasksCustomTimeZonesAccountCapability, TasksCustomTimeZonesCapability,
    TasksMultilingualAccountCapability, TasksMultilingualCapability,
    TasksRecurrencesAccountCapability, TasksRecurrencesCapability,
};
pub use filter::{
    TaskComparator, TaskComparatorProperty, TaskNotificationComparator,
    TaskNotificationComparatorProperty,
};
pub use jmap_jscalendar_types::{
    AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
    OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, TimeZone, TimeZoneRule,
    VirtualLocation,
};
pub use notification::{
    NotificationType, TaskNotification, TaskNotificationFilter, TaskNotificationFilterCondition,
};
pub use ownership::{is_task_owner, task_has_owner};
pub use task::{
    CheckItem, Checklist, Comment, Person, Task, TaskFilter, TaskFilterCondition, TaskProgress,
};
pub use task_list::{TaskList, TaskListRole, TaskRights};

/// Generic filter algebra from `jmap-types` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-tasks-types` do not need a direct
/// dependency on `jmap-types`. Mirrors the same root
/// re-exports in `jmap-mail-types`, the workspace canonical
/// extension-types template.
pub use jmap_types::{Filter, FilterOperator, Operator};
