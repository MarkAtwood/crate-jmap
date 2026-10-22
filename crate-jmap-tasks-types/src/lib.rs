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
//! | Crate root | [`TasksCapability`], [`TasksAccountCapability`], URI constants, extension capabilities |

#![forbid(unsafe_code)]

mod backend;
mod capability;
mod filter;
mod notification;
mod task;
mod task_list;

// --- Top-level re-exports ---

pub use backend::{
    is_per_user_task_property, TaskListProperty, TaskNotificationProperty, TaskProperty,
    PER_USER_TASK_PROPERTIES,
};
pub use capability::{
    PrincipalTasksCapability, TasksAccountCapability, TasksAlertsAccountCapability,
    TasksAlertsCapability, TasksAssigneesAccountCapability, TasksAssigneesCapability,
    TasksCapability, TasksCustomTimeZonesAccountCapability, TasksCustomTimeZonesCapability,
    TasksMultilingualAccountCapability, TasksMultilingualCapability,
    TasksRecurrencesAccountCapability, TasksRecurrencesCapability, JMAP_TASKS_ALERTS_URI,
    JMAP_TASKS_ASSIGNEES_URI, JMAP_TASKS_CUSTOMTIMEZONES_URI, JMAP_TASKS_MULTILINGUAL_URI,
    JMAP_TASKS_RECURRENCES_URI, JMAP_TASKS_URI,
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
