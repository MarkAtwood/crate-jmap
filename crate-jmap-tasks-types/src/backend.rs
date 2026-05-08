//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP Tasks types.
//!
//! These are defined here so that `jmap-tasks-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the tasks types are
//! local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::TaskList`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskListProperty {
    Id,
    Role,
    Name,
    Description,
    Color,
    KeywordColors,
    CategoryColors,
    SortOrder,
    IsSubscribed,
    TimeZone,
    WorkflowStatuses,
    ShareWith,
    MyRights,
    DefaultAlertsWithTime,
    DefaultAlertsWithoutTime,
}

/// Property selector for [`crate::Task`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskProperty {
    Id,
    TaskListId,
    IsDraft,
    UtcStart,
    UtcDue,
    SortOrder,
    WorkflowStatus,
    BaseTaskId,
    IsOrigin,
    MayInviteSelf,
    MayInviteOthers,
    HideAttendees,
    AtType,
    Uid,
    RelatedTo,
    ProdId,
    Created,
    Updated,
    Sequence,
    Title,
    Description,
    DescriptionContentType,
    ShowWithoutTime,
    Locations,
    VirtualLocations,
    Links,
    Locale,
    Keywords,
    Categories,
    Color,
    RecurrenceId,
    RecurrenceIdTimeZone,
    RecurrenceRules,
    ExcludedRecurrenceRules,
    RecurrenceOverrides,
    Excluded,
    Priority,
    FreeBusyStatus,
    Privacy,
    ReplyTo,
    SentBy,
    Participants,
    RequestStatus,
    UseDefaultAlerts,
    Alerts,
    Localizations,
    TimeZone,
    TimeZones,
    Due,
    Start,
    EstimatedDuration,
    PercentComplete,
    Progress,
    ProgressUpdated,
    EstimatedWork,
    Impact,
    Checklists,
    Comments,
}

/// Property selector for [`crate::TaskNotification`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskNotificationProperty {
    Id,
    Created,
    ChangedBy,
    Comment,
    NotificationType,
    TaskId,
    IsDraft,
    Task,
    TaskPatch,
}

// ---------------------------------------------------------------------------
// JmapObject impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::TaskList {
    const TYPE_NAME: &'static str = "TaskList";
    type Property = TaskListProperty;
}

impl GetObject for crate::TaskList {}

impl SetObject for crate::TaskList {
    type Patch = PatchObject;
}

impl QueryObject for crate::TaskList {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::Task {
    const TYPE_NAME: &'static str = "Task";
    type Property = TaskProperty;
}

impl GetObject for crate::Task {}

impl SetObject for crate::Task {
    type Patch = PatchObject;
}

impl QueryObject for crate::Task {
    type Filter = crate::TaskFilterCondition;
    type Comparator = crate::TaskComparator;
}

impl JmapObject for crate::TaskNotification {
    const TYPE_NAME: &'static str = "TaskNotification";
    type Property = TaskNotificationProperty;
}

impl GetObject for crate::TaskNotification {}

impl SetObject for crate::TaskNotification {
    type Patch = PatchObject;
}

impl QueryObject for crate::TaskNotification {
    type Filter = crate::TaskNotificationFilterCondition;
    type Comparator = crate::TaskNotificationComparator;
}
