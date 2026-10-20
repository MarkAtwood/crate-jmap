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
    /// The `id` property (draft-ietf-jmap-tasks-06 §3).
    Id,
    /// The `role` property (draft-ietf-jmap-tasks-06 §3).
    Role,
    /// The `name` property (draft-ietf-jmap-tasks-06 §3).
    Name,
    /// The `description` property (draft-ietf-jmap-tasks-06 §3).
    Description,
    /// The `color` property (draft-ietf-jmap-tasks-06 §3).
    Color,
    /// The `keywordColors` property (draft-ietf-jmap-tasks-06 §3).
    KeywordColors,
    /// The `categoryColors` property (draft-ietf-jmap-tasks-06 §3).
    CategoryColors,
    /// The `sortOrder` property (draft-ietf-jmap-tasks-06 §3).
    SortOrder,
    /// The `isSubscribed` property (draft-ietf-jmap-tasks-06 §3).
    IsSubscribed,
    /// The `timeZone` property (draft-ietf-jmap-tasks-06 §3).
    TimeZone,
    /// The `workflowStatuses` property (draft-ietf-jmap-tasks-06 §3).
    WorkflowStatuses,
    /// The `shareWith` property (draft-ietf-jmap-tasks-06 §3).
    ShareWith,
    /// The `myRights` property (draft-ietf-jmap-tasks-06 §3).
    MyRights,
    /// The `defaultAlertsWithTime` property (draft-ietf-jmap-tasks-06 §3.1).
    DefaultAlertsWithTime,
    /// The `defaultAlertsWithoutTime` property (draft-ietf-jmap-tasks-06 §3.1).
    DefaultAlertsWithoutTime,
}

/// Property selector for [`crate::Task`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskProperty {
    /// The `id` property (draft-ietf-jmap-tasks-06 §4).
    Id,
    /// The `taskListId` property (draft-ietf-jmap-tasks-06 §4).
    TaskListId,
    /// The `isDraft` property (draft-ietf-jmap-tasks-06 §4).
    IsDraft,
    /// The `utcStart` property (draft-ietf-jmap-tasks-06 §4).
    UtcStart,
    /// The `utcDue` property (draft-ietf-jmap-tasks-06 §4).
    UtcDue,
    /// The `sortOrder` property (draft-ietf-jmap-tasks-06 §4).
    SortOrder,
    /// The `workflowStatus` property (draft-ietf-jmap-tasks-06 §4).
    WorkflowStatus,
    /// The `baseTaskId` property (draft-ietf-jmap-tasks-06 §4.4).
    BaseTaskId,
    /// The `isOrigin` property (draft-ietf-jmap-tasks-06 §4.5).
    IsOrigin,
    /// The `mayInviteSelf` property (draft-ietf-jmap-tasks-06 §4.5.3.1).
    MayInviteSelf,
    /// The `mayInviteOthers` property (draft-ietf-jmap-tasks-06 §4.5.3.2).
    MayInviteOthers,
    /// The `hideAttendees` property (draft-ietf-jmap-tasks-06 §4.5.3.3).
    HideAttendees,
    /// The `@type` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.1.1).
    AtType,
    /// The `uid` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.1.2).
    Uid,
    /// The `relatedTo` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.1.3).
    RelatedTo,
    /// The `prodId` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.1.4).
    ProdId,
    /// The `created` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.1.5).
    Created,
    /// The `updated` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.1.6).
    Updated,
    /// The `sequence` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.1.7).
    Sequence,
    /// The `title` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.1).
    Title,
    /// The `description` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.2).
    Description,
    /// The `descriptionContentType` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.3).
    DescriptionContentType,
    /// The `showWithoutTime` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.4).
    ShowWithoutTime,
    /// The `locations` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.5).
    Locations,
    /// The `virtualLocations` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.6).
    VirtualLocations,
    /// The `links` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.7).
    Links,
    /// The `locale` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.8).
    Locale,
    /// The `keywords` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.9).
    Keywords,
    /// The `categories` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.10).
    Categories,
    /// The `color` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.2.11).
    Color,
    /// The `recurrenceId` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.3.1).
    RecurrenceId,
    /// The `recurrenceIdTimeZone` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.3.2).
    RecurrenceIdTimeZone,
    /// The `recurrenceRules` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.3.3).
    RecurrenceRules,
    /// The `excludedRecurrenceRules` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.3.4).
    ExcludedRecurrenceRules,
    /// The `recurrenceOverrides` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.3.5).
    RecurrenceOverrides,
    /// The `excluded` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.3.6).
    Excluded,
    /// The `priority` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.4.1).
    Priority,
    /// The `freeBusyStatus` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.4.2).
    FreeBusyStatus,
    /// The `privacy` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.4.3).
    Privacy,
    /// The `replyTo` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.4.4).
    ReplyTo,
    /// The `sentBy` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.4.5).
    SentBy,
    /// The `participants` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.4.6).
    Participants,
    /// The `requestStatus` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.4.7).
    RequestStatus,
    /// The `useDefaultAlerts` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.5.1).
    UseDefaultAlerts,
    /// The `alerts` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.5.2).
    Alerts,
    /// The `localizations` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.6.1).
    Localizations,
    /// The `timeZone` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.7.1).
    TimeZone,
    /// The `timeZones` property, inherited from the JSCalendar JSTask object (RFC 8984 §4.7.2).
    TimeZones,
    /// The `due` property, inherited from the JSCalendar JSTask object (RFC 8984 §5.2.1).
    Due,
    /// The `start` property, inherited from the JSCalendar JSTask object (RFC 8984 §5.2.2).
    Start,
    /// The `estimatedDuration` property, inherited from the JSCalendar JSTask object (RFC 8984 §5.2.3).
    EstimatedDuration,
    /// The `percentComplete` property, inherited from the JSCalendar JSTask object (RFC 8984 §5.2.4).
    PercentComplete,
    /// The `progress` property, inherited from the JSCalendar JSTask object (RFC 8984 §5.2.5).
    Progress,
    /// The `progressUpdated` property, inherited from the JSCalendar JSTask object (RFC 8984 §5.2.6).
    ProgressUpdated,
    /// The `estimatedWork` property (draft-ietf-jmap-tasks-06 §4.2.1).
    EstimatedWork,
    /// The `impact` property (draft-ietf-jmap-tasks-06 §4.2.2).
    Impact,
    /// The `checklists` property (draft-ietf-jmap-tasks-06 §4.2.3).
    Checklists,
    /// The `comments` property (draft-ietf-jmap-tasks-06 §4.2.4).
    Comments,
}

/// Names of top-level [`Task`](crate::Task) properties that
/// draft-ietf-jmap-tasks-06 §4.5.1 classifies as **per-user**.
///
/// In shared task lists these MUST be stored per-user, and a patch that
/// writes only these properties MUST store the `updated` timestamp just for
/// that user rather than changing the shared one.
pub const PER_USER_TASK_PROPERTIES: &[&str] = &[
    "keywords",
    "color",
    "freeBusyStatus",
    "useDefaultAlerts",
    "alerts",
];

/// Returns `true` if `name` is a per-user top-level [`Task`](crate::Task)
/// property name per draft-ietf-jmap-tasks-06 §4.5.1.
///
/// See [`PER_USER_TASK_PROPERTIES`] for the full set. This is a
/// wire-protocol classification fixed by the spec; backends MUST NOT
/// redefine it. Property paths such as `"alerts/abc"` are not top-level
/// names and return `false`; callers classify the leading path segment.
#[must_use]
pub fn is_per_user_task_property(name: &str) -> bool {
    PER_USER_TASK_PROPERTIES.contains(&name)
}

/// Property selector for [`crate::TaskNotification`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskNotificationProperty {
    /// The `id` property (draft-ietf-jmap-tasks-06 §5.1).
    Id,
    /// The `created` property (draft-ietf-jmap-tasks-06 §5.1).
    Created,
    /// The `changedBy` property (draft-ietf-jmap-tasks-06 §5.1).
    ChangedBy,
    /// The `comment` property (draft-ietf-jmap-tasks-06 §5.1).
    Comment,
    /// The `type` property (draft-ietf-jmap-tasks-06 §5.1).
    NotificationType,
    /// The `taskId` property (draft-ietf-jmap-tasks-06 §5.1).
    TaskId,
    /// The `isDraft` property (draft-ietf-jmap-tasks-06 §5.1).
    IsDraft,
    /// The `task` property (draft-ietf-jmap-tasks-06 §5.1).
    Task,
    /// The `taskPatch` property (draft-ietf-jmap-tasks-06 §5.1).
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

// The filter is untyped JSON, so the generic handlers cannot reject an
// unknown key; the backend must answer it with RFC 8620 §5.5
// `unsupportedFilter` itself.
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

#[cfg(test)]
mod tests {
    use super::{is_per_user_task_property, PER_USER_TASK_PROPERTIES};

    /// Oracle: draft-ietf-jmap-tasks-06 §4.5.1 "Per-user properties",
    /// quoted verbatim:
    ///
    /// > In shared task lists, any top-level property registered in the
    /// > IANA registry as "Is Per-User: yes" (see Section XXX) MUST be
    /// > stored per-user.  This includes:
    /// >
    /// > *  keywords
    /// > *  color
    /// > *  freeBusyStatus
    /// > *  useDefaultAlerts
    /// > *  alerts
    #[test]
    fn per_user_task_properties_match_spec() {
        const SPEC_PER_USER_PROPERTIES_VERBATIM: &str = "\
   *  keywords
   *  color
   *  freeBusyStatus
   *  useDefaultAlerts
   *  alerts
";
        let spec_list: Vec<&str> = SPEC_PER_USER_PROPERTIES_VERBATIM
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("*  ").map(str::trim))
            .collect();
        assert_eq!(
            spec_list.len(),
            5,
            "spec-quote parser sanity: {spec_list:?}"
        );
        assert_eq!(PER_USER_TASK_PROPERTIES, spec_list.as_slice());
        for name in spec_list {
            assert!(is_per_user_task_property(name), "{name} must be per-user");
        }
    }

    /// Shared Task properties from draft-ietf-jmap-tasks-06 (and the
    /// JSCalendar base, RFC 8984) are not per-user.
    #[test]
    fn shared_properties_are_not_per_user() {
        for shared in [
            "id",
            "title",
            "start",
            "due",
            "progress",
            "taskListId",
            "uid",
        ] {
            assert!(
                !is_per_user_task_property(shared),
                "{shared} must be shared"
            );
        }
    }

    #[test]
    fn unknown_names_and_paths_are_not_per_user() {
        assert!(!is_per_user_task_property(""));
        assert!(!is_per_user_task_property("notARealProperty"));
        assert!(!is_per_user_task_property("Keywords"));
        assert!(!is_per_user_task_property("keywords/done"));
        assert!(!is_per_user_task_property("alerts/abc"));
    }
}
