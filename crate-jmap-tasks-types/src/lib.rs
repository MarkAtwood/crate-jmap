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
//! | [`task_list`] | [`TaskList`], [`TaskRights`], [`TaskListRole`] |
//! | [`task`] | [`Task`], [`TaskProgress`], [`Checklist`], [`CheckItem`], [`Comment`], [`Person`], [`TaskFilterCondition`] |
//! | [`notification`] | [`TaskNotification`], [`NotificationType`], [`TaskNotificationFilterCondition`] |
//! | [`filter`] | [`TaskComparator`], [`TaskNotificationComparator`] |
//! | [`capability`] | [`TasksCapability`], [`TasksAccountCapability`], URI constants, extension capabilities |

#![forbid(unsafe_code)]

pub mod backend;
pub mod capability;
pub mod filter;
pub mod notification;
pub mod task;
pub mod task_list;

/// Module alias re-exporting [`jmap_jscalendar_types`].
///
/// The RFC 8984 JSCalendar typed sub-objects (`Location`, `Participant`,
/// `RecurrenceRule`, `Alert`, etc.) used in this crate's sloppy-`Value`
/// fields live in the `jmap-jscalendar-types` crate and are re-exported
/// here for symmetry with `jmap_calendars_types::jscalendar`. New code
/// may import from either the top-level re-exports
/// (`jmap_tasks_types::Location`), the module alias
/// (`jmap_tasks_types::jscalendar::Location`), or the source crate
/// directly (`jmap_jscalendar_types::Location`).
pub use jmap_jscalendar_types as jscalendar;

// --- Top-level re-exports ---

pub use backend::{TaskListProperty, TaskNotificationProperty, TaskProperty};
pub use capability::{
    PrincipalTasksCapability, TasksAccountCapability, TasksAlertsAccountCapability,
    TasksAlertsCapability, TasksAssigneesAccountCapability, TasksAssigneesCapability,
    TasksCapability, TasksCustomTimeZonesAccountCapability, TasksCustomTimeZonesCapability,
    TasksMultilingualAccountCapability, TasksMultilingualCapability,
    TasksRecurrencesAccountCapability, TasksRecurrencesCapability, JMAP_TASKS_ALERTS_URI,
    JMAP_TASKS_ASSIGNEES_URI, JMAP_TASKS_CUSTOMTIMEZONES_URI, JMAP_TASKS_MULTILINGUAL_URI,
    JMAP_TASKS_RECURRENCES_URI, JMAP_TASKS_URI,
};
pub use filter::{TaskComparator, TaskNotificationComparator};
pub use jmap_jscalendar_types::{
    AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
    OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, VirtualLocation,
};
pub use notification::{NotificationType, TaskNotification, TaskNotificationFilterCondition};
pub use task::{CheckItem, Checklist, Comment, Person, Task, TaskFilterCondition, TaskProgress};
pub use task_list::{TaskList, TaskListRole, TaskRights};
