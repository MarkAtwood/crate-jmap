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
//! | [`filter`] | [`TaskComparator`], [`TaskComparatorProperty`], [`TaskNotificationComparator`], [`TaskNotificationComparatorProperty`] |
//! | [`capability`] | [`TasksCapability`], [`TasksAccountCapability`], URI constants, extension capabilities |

#![forbid(unsafe_code)]

pub mod backend;
pub mod capability;
pub mod filter;
pub mod notification;
pub mod task;
pub mod task_list;

/// RFC 8984 JSCalendar typed sub-objects from [`jmap_jscalendar_types`].
///
/// The sub-objects (`Location`, `Participant`, `RecurrenceRule`, `Alert`,
/// etc.) used in this crate's sloppy-`Value` fields live in the
/// `jmap-jscalendar-types` crate and are re-exported here for symmetry
/// with `jmap_calendars_types::jscalendar`. New code may import from the
/// top-level re-exports (`jmap_tasks_types::Location`), this module
/// (`jmap_tasks_types::jscalendar::Location`), or the source crate
/// directly (`jmap_jscalendar_types::Location`).
///
/// **Bounded surface.** This module re-exports exactly the names of the
/// top-level re-export below, not the whole `jmap_jscalendar_types`
/// crate, so an addition there does not widen this crate's public API
/// without an intentional change to both lists (bd:JMAP-y6a8).
pub mod jscalendar {
    pub use jmap_jscalendar_types::{
        AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
        OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, TimeZone,
        TimeZoneRule, VirtualLocation,
    };
}

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

/// Generic filter algebra from `jmap-types::query` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-tasks-types` do not need a direct
/// dependency on `jmap-types`. Mirrors the canonical
/// [`jmap_mail_types::query`] re-exports from the workspace canonical
/// extension-types template.
///
/// [`jmap_mail_types::query`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/index.html
pub use jmap_types::query::{Filter, FilterOperator, Operator};
