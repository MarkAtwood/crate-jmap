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

/// draft-ietf-jmap-tasks-06 §3 (lines 383, 496): `id` is "(immutable;
/// server-set)", `myRights` is "(server-set)".
impl SetObject for crate::TaskList {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "myRights"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "role",
        "name",
        "description",
        "color",
        "keywordColors",
        "categoryColors",
        "sortOrder",
        "isSubscribed",
        "timeZone",
        "workflowStatuses",
        "shareWith",
        "defaultAlertsWithTime",
        "defaultAlertsWithoutTime",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl JmapObject for crate::Task {
    const TYPE_NAME: &'static str = "Task";
    type Property = TaskProperty;
}

impl GetObject for crate::Task {}

/// draft-ietf-jmap-tasks-06 §4 (lines 715, 922, 941): `id` and
/// `baseTaskId` are "(immutable; server-set)", `isOrigin` is
/// "(server-set)". No other property, JMAP or JSCalendar, carries either
/// attribute.
impl SetObject for crate::Task {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "baseTaskId", "isOrigin"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "taskListId",
        "isDraft",
        "utcStart",
        "utcDue",
        "sortOrder",
        "workflowStatus",
        "mayInviteSelf",
        "mayInviteOthers",
        "hideAttendees",
        "@type",
        "uid",
        "relatedTo",
        "prodId",
        "created",
        "updated",
        "sequence",
        "title",
        "description",
        "descriptionContentType",
        "showWithoutTime",
        "locations",
        "virtualLocations",
        "links",
        "locale",
        "keywords",
        "categories",
        "color",
        "recurrenceId",
        "recurrenceIdTimeZone",
        "recurrenceRules",
        "excludedRecurrenceRules",
        "recurrenceOverrides",
        "excluded",
        "priority",
        "freeBusyStatus",
        "privacy",
        "replyTo",
        "sentBy",
        "participants",
        "requestStatus",
        "useDefaultAlerts",
        "alerts",
        "localizations",
        "timeZone",
        "timeZones",
        "due",
        "start",
        "estimatedDuration",
        "percentComplete",
        "progress",
        "progressUpdated",
        "estimatedWork",
        "impact",
        "checklists",
        "comments",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::Task {
    type Filter = crate::TaskFilter;
    type Comparator = crate::TaskComparator;
}

impl JmapObject for crate::TaskNotification {
    const TYPE_NAME: &'static str = "TaskNotification";
    type Property = TaskNotificationProperty;
}

impl GetObject for crate::TaskNotification {}

/// draft-ietf-jmap-tasks-06 §5.4: "Only destroy is supported; any attempt
/// to create/update MUST be rejected", so every property is server-set.
impl SetObject for crate::TaskNotification {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "created",
        "changedBy",
        "comment",
        "type",
        "taskId",
        "isDraft",
        "task",
        "taskPatch",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::TaskNotification {
    type Filter = crate::TaskNotificationFilter;
    type Comparator = crate::TaskNotificationComparator;
}

#[cfg(test)]
mod tests {
    use super::{is_per_user_task_property, PER_USER_TASK_PROPERTIES};
    use jmap_types::{check_property_classification, Id, PatchObject, UTCDate};
    use serde_json::{json, Map, Value};
    use std::collections::BTreeMap;

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    fn rights() -> crate::TaskRights {
        de(json!({
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayWriteOwn": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "mayAdmin": true,
            "mayDelete": true,
        }))
    }

    fn date() -> UTCDate {
        UTCDate::from("2026-01-01T00:00:00Z")
    }

    // Each classification test builds a fully populated object with a
    // struct literal, so a new field is a compile error until it is set,
    // then a classification failure until the `SetObject` lists name it.

    #[test]
    fn task_list_properties_are_classified() {
        let colors = || Some(BTreeMap::from([("k".to_owned(), "red".to_owned())]));
        let obj = crate::TaskList {
            id: Id::from("l1"),
            role: Some(de(json!("inbox"))),
            name: "n".into(),
            description: Some("d".into()),
            color: Some("red".into()),
            keyword_colors: colors(),
            category_colors: colors(),
            sort_order: 1,
            is_subscribed: true,
            time_zone: Some("Etc/UTC".into()),
            workflow_statuses: Some(vec!["completed".into()]),
            share_with: Some(BTreeMap::from([(Id::from("p1"), rights())])),
            my_rights: rights(),
            default_alerts_with_time: Some(BTreeMap::new()),
            default_alerts_without_time: Some(BTreeMap::new()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn task_properties_are_classified() {
        let v = || Some(json!({}));
        let patch = || -> PatchObject { de(json!({"title": "t"})) };
        let flags = || Some(BTreeMap::from([("k".to_owned(), true)]));
        let obj = crate::Task {
            id: Some(Id::from("t1")),
            task_list_id: Some(Id::from("l1")),
            is_draft: Some(true),
            utc_start: Some(date()),
            utc_due: Some(date()),
            sort_order: Some(1),
            workflow_status: Some("completed".into()),
            base_task_id: Some(Id::from("t0")),
            is_origin: Some(true),
            may_invite_self: Some(true),
            may_invite_others: Some(true),
            hide_attendees: Some(true),
            at_type: Some("Task".into()),
            uid: Some("u".into()),
            related_to: v(),
            prod_id: Some("p".into()),
            created: Some(date()),
            updated: Some(date()),
            sequence: Some(1),
            title: Some("t".into()),
            description: Some("d".into()),
            description_content_type: Some("text/plain".into()),
            show_without_time: Some(true),
            locations: v(),
            virtual_locations: v(),
            links: v(),
            locale: Some("en".into()),
            keywords: flags(),
            categories: flags(),
            color: Some("red".into()),
            recurrence_id: Some("2026-01-01T00:00:00".into()),
            recurrence_id_time_zone: Some("Etc/UTC".into()),
            recurrence_rules: Some(json!([])),
            excluded_recurrence_rules: Some(json!([])),
            recurrence_overrides: Some(BTreeMap::from([("2026-01-02T00:00:00".into(), patch())])),
            excluded: Some(false),
            priority: Some(1),
            free_busy_status: Some("busy".into()),
            privacy: Some("public".into()),
            reply_to: Some(BTreeMap::from([(
                "imip".into(),
                "mailto:a@example.com".into(),
            )])),
            sent_by: Some("mailto:a@example.com".into()),
            participants: v(),
            request_status: Some("2.0;Success".into()),
            use_default_alerts: Some(true),
            alerts: v(),
            localizations: Some(BTreeMap::from([("de".into(), patch())])),
            time_zone: Some("Etc/UTC".into()),
            time_zones: v(),
            due: Some("2026-01-02T00:00:00".into()),
            start: Some("2026-01-01T00:00:00".into()),
            estimated_duration: Some("PT1H".into()),
            percent_complete: Some(50),
            progress: Some(de(json!("in-process"))),
            progress_updated: Some(date()),
            estimated_work: Some(1),
            impact: Some("i".into()),
            checklists: Some(BTreeMap::from([(
                Id::from("c1"),
                de(json!({"title": "c"})),
            )])),
            comments: Some(BTreeMap::from([(
                Id::from("m1"),
                de(json!({"message": "m"})),
            )])),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn task_notification_properties_are_classified() {
        let obj = crate::TaskNotification {
            id: Id::from("n1"),
            created: date(),
            changed_by: de(json!({"name": "A"})),
            comment: Some("c".into()),
            notification_type: de(json!("updated")),
            task_id: Id::from("t1"),
            is_draft: Some(false),
            task: Some(json!({})),
            task_patch: Some(de(json!({"title": "t"}))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

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

    /// Oracle: RFC 8620 §5.5 (line 2368) types the /query `filter` as
    /// "FilterOperator|FilterCondition|null". draft-ietf-jmap-tasks-06
    /// §4.13 makes Task/query the CalendarEvent/query method (Task
    /// property `taskListId`, §4) and §5.5.1 names the TaskNotification
    /// `taskIds` condition. A hand-written NOT tree decodes as each filter.
    #[test]
    fn query_filters_are_operator_trees() {
        use jmap_types::QueryObject;
        /// Decodes `json` as `O`'s filter, which must be the full tree.
        fn decodes_as_operator<O, C>(json: serde_json::Value) -> bool
        where
            O: QueryObject<Filter = jmap_types::Filter<C>>,
            C: serde::de::DeserializeOwned,
        {
            let f: O::Filter = serde_json::from_value(json).expect("operator tree must decode");
            matches!(f, jmap_types::Filter::Operator(_))
        }
        let not = |cond: serde_json::Value| serde_json::json!({ "operator": "NOT", "conditions": [cond] });
        assert!(decodes_as_operator::<crate::Task, _>(not(
            serde_json::json!({ "taskListId": "l1" })
        )));
        assert!(decodes_as_operator::<crate::TaskNotification, _>(not(
            serde_json::json!({ "taskIds": ["t1"] })
        )));
    }
}
