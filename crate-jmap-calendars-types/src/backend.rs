//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP Calendars types.
//!
//! These are defined here so that `jmap-calendars-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the calendars types
//! are local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Calendar`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarProperty {
    /// The `id` property (draft-ietf-jmap-calendars-33 §4).
    Id,
    /// The `name` property (draft-ietf-jmap-calendars-33 §4).
    Name,
    /// The `description` property (draft-ietf-jmap-calendars-33 §4).
    Description,
    /// The `color` property (draft-ietf-jmap-calendars-33 §4).
    Color,
    /// The `sortOrder` property (draft-ietf-jmap-calendars-33 §4).
    SortOrder,
    /// The `isSubscribed` property (draft-ietf-jmap-calendars-33 §4).
    IsSubscribed,
    /// The `isVisible` property (draft-ietf-jmap-calendars-33 §4).
    IsVisible,
    /// The `isDefault` property (draft-ietf-jmap-calendars-33 §4).
    IsDefault,
    /// The `includeInAvailability` property (draft-ietf-jmap-calendars-33 §4).
    IncludeInAvailability,
    /// The `defaultAlertsWithTime` property (draft-ietf-jmap-calendars-33 §4).
    DefaultAlertsWithTime,
    /// The `defaultAlertsWithoutTime` property (draft-ietf-jmap-calendars-33 §4).
    DefaultAlertsWithoutTime,
    /// The `timeZone` property (draft-ietf-jmap-calendars-33 §4).
    TimeZone,
    /// The `shareWith` property (draft-ietf-jmap-calendars-33 §4).
    ShareWith,
    /// The `myRights` property (draft-ietf-jmap-calendars-33 §4).
    MyRights,
}

/// Property selector for [`crate::CalendarEvent`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarEventProperty {
    /// The `id` property (draft-ietf-jmap-calendars-33 §5).
    Id,
    /// The `baseEventId` property (draft-ietf-jmap-calendars-33 §5).
    BaseEventId,
    /// The `calendarIds` property (draft-ietf-jmap-calendars-33 §5).
    CalendarIds,
    /// The `isDraft` property (draft-ietf-jmap-calendars-33 §5).
    IsDraft,
    /// The `isOrigin` property (draft-ietf-jmap-calendars-33 §5).
    IsOrigin,
    /// The `utcStart` property (draft-ietf-jmap-calendars-33 §5).
    UtcStart,
    /// The `utcEnd` property (draft-ietf-jmap-calendars-33 §5).
    UtcEnd,
    /// The `useDefaultAlerts` property (draft-ietf-jmap-calendars-33 §5).
    UseDefaultAlerts,
    /// The `mayInviteSelf` property (draft-ietf-jmap-calendars-33 §5.1.1).
    MayInviteSelf,
    /// The `mayInviteOthers` property (draft-ietf-jmap-calendars-33 §5.1.2).
    MayInviteOthers,
    /// The `hideAttendees` property (draft-ietf-jmap-calendars-33 §5.1.3).
    HideAttendees,
    /// The `blobId` property (draft-ietf-jmap-calendars-33 §5.3).
    BlobId,
    /// The `uid` property, inherited from the JSCalendar Event object (RFC 8984 §4.1.2).
    Uid,
    /// The `title` property, inherited from the JSCalendar Event object (RFC 8984 §4.2.1).
    Title,
    /// The `description` property, inherited from the JSCalendar Event object (RFC 8984 §4.2.2).
    Description,
    /// The `start` property, inherited from the JSCalendar Event object (RFC 8984 §5.1.1).
    Start,
    /// The `duration` property, inherited from the JSCalendar Event object (RFC 8984 §5.1.2).
    Duration,
    /// The `status` property, inherited from the JSCalendar Event object (RFC 8984 §5.1.3).
    Status,
}

/// Names of [`CalendarEvent`](crate::CalendarEvent) properties that the JMAP
/// Calendars draft (draft-ietf-jmap-calendars-33 §5.4) classifies as
/// **per-user**.
///
/// Per-user properties belong to the authenticated user's view of the event;
/// patching them MUST NOT change the shared `updated` timestamp on the
/// underlying object. Backends serving multiple users SHOULD store these
/// separately from the shared event body.
///
/// This list mirrors the properties with "Is Per-User" set to "Yes" in
/// the IANA "JSCalendar Properties" registry (draft-ietf-calext-jscalendarbis-22
/// §7.4.2), which draft-ietf-jmap-calendars-33 §5.4 now refers to.
///
/// **Maintainer note (internal layout).** Internally this list is split
/// into two private halves — `PER_USER_PROPERTIES_IN_ENUM` and
/// `PER_USER_PROPERTIES_NOT_YET_IN_ENUM` — because
/// [`CalendarEventProperty`] is deliberately a subset of the spec
/// property set: `keywords`, `color`, `freeBusyStatus`, and `alerts`
/// are reserved as future additions but not yet enumerated. When you
/// add one of those as a `CalendarEventProperty` variant, move its
/// wire-name from `PER_USER_PROPERTIES_NOT_YET_IN_ENUM` to
/// `PER_USER_PROPERTIES_IN_ENUM` in the same commit, and update the
/// `classify` match in the drift-guard test
/// `per_user_in_enum_matches_enum_variants`. The
/// `per_user_const_is_union_of_two_halves` drift-guard test enforces
/// that this public const equals the disjoint union of the two halves.
pub const PER_USER_CALENDAR_EVENT_PROPERTIES: &[&str] = &[
    "keywords",
    "color",
    "freeBusyStatus",
    "useDefaultAlerts",
    "alerts",
];

/// Wire-names of per-user [`CalendarEventProperty`] variants that already
/// exist in the enum.
///
/// When you add a per-user property as a new `CalendarEventProperty`
/// variant, move its wire-name string here from
/// [`PER_USER_PROPERTIES_NOT_YET_IN_ENUM`].
///
/// Invariant (enforced by the `per_user_const_is_union_of_two_halves`
/// drift-guard test):
///
/// ```text
/// PER_USER_CALENDAR_EVENT_PROPERTIES
///     = PER_USER_PROPERTIES_IN_ENUM ∪ PER_USER_PROPERTIES_NOT_YET_IN_ENUM
/// ```
///
/// and the two halves are disjoint.
///
/// `#[allow(dead_code)]`: this const exists as the documented other half
/// of the split layout; it is read by drift-guard tests but not by
/// runtime code (the public [`PER_USER_CALENDAR_EVENT_PROPERTIES`] is
/// the runtime source of truth).
#[allow(dead_code)]
const PER_USER_PROPERTIES_IN_ENUM: &[&str] = &["useDefaultAlerts"];

/// Per-user property wire-names from draft-ietf-jmap-calendars-33 §5.4
/// that do **not** yet have a corresponding [`CalendarEventProperty`]
/// variant.
///
/// When you add one of these as a `CalendarEventProperty` variant, move
/// its wire-name string out of this list and into
/// [`PER_USER_PROPERTIES_IN_ENUM`], and add a `Variant => "wireName"`
/// arm to the `classify` match in the drift-guard test
/// `per_user_in_enum_matches_enum_variants`.
///
/// `#[allow(dead_code)]`: see [`PER_USER_PROPERTIES_IN_ENUM`].
#[allow(dead_code)]
const PER_USER_PROPERTIES_NOT_YET_IN_ENUM: &[&str] =
    &["keywords", "color", "freeBusyStatus", "alerts"];

/// Returns `true` if `name` is a per-user [`CalendarEvent`](crate::CalendarEvent)
/// property name per draft-ietf-jmap-calendars-33 §5.4.
///
/// See [`PER_USER_CALENDAR_EVENT_PROPERTIES`] for the full set. This is a
/// wire-protocol property classification: the spec list is fixed by IANA
/// registration and backends MUST NOT redefine it.
#[must_use]
pub fn is_per_user_calendar_event_property(name: &str) -> bool {
    PER_USER_CALENDAR_EVENT_PROPERTIES.contains(&name)
}

/// Property selector for [`crate::CalendarEventNotification`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarEventNotificationProperty {
    /// The `id` property (draft-ietf-jmap-calendars-33 §7).
    Id,
    /// The `created` property (draft-ietf-jmap-calendars-33 §7).
    Created,
    /// The `changedBy` property (draft-ietf-jmap-calendars-33 §7).
    ChangedBy,
    /// The `comment` property (draft-ietf-jmap-calendars-33 §7).
    Comment,
    /// The `type` property (draft-ietf-jmap-calendars-33 §7).
    Type,
    /// The `calendarEventId` property (draft-ietf-jmap-calendars-33 §7).
    CalendarEventId,
    /// The `isDraft` property (draft-ietf-jmap-calendars-33 §7).
    IsDraft,
    /// The `event` property (draft-ietf-jmap-calendars-33 §7).
    Event,
    /// The `eventPatch` property (draft-ietf-jmap-calendars-33 §7).
    EventPatch,
}

/// Property selector for [`crate::ParticipantIdentity`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParticipantIdentityProperty {
    /// The `id` property (draft-ietf-jmap-calendars-33 §3).
    Id,
    /// The `name` property (draft-ietf-jmap-calendars-33 §3).
    Name,
    /// The `calendarAddress` property (draft-ietf-jmap-calendars-33 §3).
    CalendarAddress,
    /// The `isDefault` property (draft-ietf-jmap-calendars-33 §3).
    IsDefault,
}

// ---------------------------------------------------------------------------
// JmapObject impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::Calendar {
    const TYPE_NAME: &'static str = "Calendar";
    type Property = CalendarProperty;
}

impl GetObject for crate::Calendar {}

/// draft-ietf-jmap-calendars-33 §4 (lines 747, 816, 907): `id` is
/// "(immutable; server-set)"; `isDefault` and `myRights` are
/// "(server-set)".
impl SetObject for crate::Calendar {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "isDefault", "myRights"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "name",
        "description",
        "color",
        "sortOrder",
        "isSubscribed",
        "isVisible",
        "includeInAvailability",
        "defaultAlertsWithTime",
        "defaultAlertsWithoutTime",
        "timeZone",
        "shareWith",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl JmapObject for crate::CalendarEvent {
    const TYPE_NAME: &'static str = "CalendarEvent";
    type Property = CalendarEventProperty;
}

impl GetObject for crate::CalendarEvent {}

/// draft-ietf-jmap-calendars-33 §5 (lines 1133, 1138, 1151): `id` and
/// `baseEventId` are "(immutable; server-set)", `isOrigin` is
/// "(server-set)". No other property, JMAP or JSCalendar, carries either
/// attribute.
impl SetObject for crate::CalendarEvent {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "baseEventId", "isOrigin"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "calendarIds",
        "isDraft",
        "utcStart",
        "utcEnd",
        "useDefaultAlerts",
        "mayInviteSelf",
        "mayInviteOthers",
        "hideAttendees",
        "blobId",
        "iCalendar",
        "@type",
        "version",
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
        "organizerCalendarAddress",
        "participants",
        "requestStatus",
        "alerts",
        "localizations",
        "timeZone",
        "timeZones",
        "start",
        "duration",
        "status",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::CalendarEvent {
    type Filter = crate::CalendarEventFilter;
    type Comparator = crate::CalendarEventComparator;
}

impl JmapObject for crate::CalendarEventNotification {
    const TYPE_NAME: &'static str = "CalendarEventNotification";
    type Property = CalendarEventNotificationProperty;
}

impl GetObject for crate::CalendarEventNotification {}

/// `SetObject` for `CalendarEventNotification` is destroy-only.
/// The `Patch` type is never used in practice; [`PatchObject`] is a
/// safe placeholder that satisfies the trait bound while keeping the
/// type-system contract aligned with sibling types (RFC 8620 §5.3).
///
/// draft-ietf-jmap-calendars-33 §7.3: "Only destroy is supported; any
/// attempt to create/update MUST be rejected", so every property is
/// server-set; `id` (line 2601) is also "(immutable; server-set)".
impl SetObject for crate::CalendarEventNotification {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "created",
        "changedBy",
        "comment",
        "type",
        "calendarEventId",
        "isDraft",
        "event",
        "eventPatch",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::CalendarEventNotification {
    type Filter = crate::NotificationFilter;
    type Comparator = crate::CalendarEventNotificationComparator;
}

impl JmapObject for crate::ParticipantIdentity {
    const TYPE_NAME: &'static str = "ParticipantIdentity";
    type Property = ParticipantIdentityProperty;
}

impl GetObject for crate::ParticipantIdentity {}

/// draft-ietf-jmap-calendars-33 §3 (lines 627, 638): `id` is "(immutable;
/// server-set)", `isDefault` is "(server-set)".
impl SetObject for crate::ParticipantIdentity {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "isDefault"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["name", "calendarAddress"];
    const IMMUTABLE: &'static [&'static str] = &[];
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_types::{check_property_classification, Id, UTCDate};
    use serde_json::{json, Map, Value};
    use std::collections::BTreeMap;

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    fn rights() -> crate::CalendarRights {
        de(json!({
            "mayReadFreeBusy": true,
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayWriteOwn": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "mayShare": true,
            "mayDelete": true,
        }))
    }

    fn patch() -> PatchObject {
        de(json!({"title": "t"}))
    }

    // Each classification test builds a fully populated object with a
    // struct literal, so a new field is a compile error until it is set,
    // then a classification failure until the `SetObject` lists name it.

    #[test]
    fn calendar_properties_are_classified() {
        let obj = crate::Calendar {
            id: Some(Id::from("c1")),
            name: "n".into(),
            description: Some("d".into()),
            color: Some("red".into()),
            sort_order: 1,
            is_subscribed: true,
            is_visible: true,
            is_default: true,
            include_in_availability: de(json!("all")),
            default_alerts_with_time: Some(BTreeMap::new()),
            default_alerts_without_time: Some(BTreeMap::new()),
            time_zone: Some("Etc/UTC".into()),
            share_with: Some(BTreeMap::from([(Id::from("p1"), rights())])),
            my_rights: rights(),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn calendar_event_properties_are_classified() {
        let v = || Some(json!({}));
        let obj = crate::CalendarEvent {
            id: Some(Id::from("e1")),
            base_event_id: Some(Id::from("e0")),
            calendar_ids: Some(BTreeMap::from([(Id::from("c1"), true)])),
            is_draft: Some(true),
            is_origin: Some(true),
            utc_start: Some(UTCDate::from("2026-01-01T00:00:00Z")),
            utc_end: Some(UTCDate::from("2026-01-01T01:00:00Z")),
            use_default_alerts: Some(true),
            may_invite_self: Some(true),
            may_invite_others: Some(true),
            hide_attendees: Some(true),
            blob_id: Some(Id::from("b1")),
            icalendar: Some(json!([])),
            at_type: Some("Event".into()),
            version: Some("2.0".into()),
            uid: Some("u".into()),
            related_to: v(),
            prod_id: Some("p".into()),
            created: Some(UTCDate::from("2026-01-01T00:00:00Z")),
            updated: Some(UTCDate::from("2026-01-01T00:00:00Z")),
            sequence: Some(1),
            title: Some("t".into()),
            description: Some("d".into()),
            description_content_type: Some("text/plain".into()),
            show_without_time: Some(true),
            locations: v(),
            virtual_locations: v(),
            links: v(),
            locale: Some("en".into()),
            keywords: v(),
            categories: v(),
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
            reply_to: v(),
            sent_by: Some("mailto:a@example.com".into()),
            organizer_calendar_address: Some("mailto:a@example.com".into()),
            participants: v(),
            request_status: Some("2.0;Success".into()),
            alerts: v(),
            localizations: Some(BTreeMap::from([("de".into(), patch())])),
            time_zone: Some("Etc/UTC".into()),
            time_zones: v(),
            start: Some("2026-01-01T00:00:00".into()),
            duration: Some("PT1H".into()),
            status: Some("confirmed".into()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn calendar_event_notification_properties_are_classified() {
        let obj = crate::CalendarEventNotification {
            id: Id::from("n1"),
            created: UTCDate::from("2026-01-01T00:00:00Z"),
            changed_by: de(json!({"name": "A", "email": "a@example.com"})),
            comment: Some("c".into()),
            notification_type: de(json!("updated")),
            calendar_event_id: Id::from("e1"),
            is_draft: Some(false),
            event: json!({}),
            event_patch: Some(patch()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn participant_identity_properties_are_classified() {
        let obj = crate::ParticipantIdentity {
            id: Some(Id::from("p1")),
            name: "A".into(),
            calendar_address: "mailto:a@example.com".into(),
            is_default: true,
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    /// Pinning test: the per-user property list MUST match the IANA-registered
    /// set in draft-ietf-jmap-calendars-26 §10.8.2 exactly.
    ///
    /// draft-ietf-jmap-calendars-33 dropped §10.8 and refers to the "Is
    /// Per-User" column of draft-ietf-calext-jscalendarbis-22 §7.4, which
    /// marks the same five properties "Yes"; the -26 list stays the oracle
    /// because it fixes an order.
    ///
    /// # Independent oracle (verbatim spec quote)
    ///
    /// The expected list is the verbatim bullet list from
    /// draft-ietf-jmap-calendars-26 §10.8.2 "Initial values for existing
    /// registrations":
    ///
    /// > IANA will set "Is per-user: yes" on the following property
    /// > registrations:
    /// >
    /// > *  keywords
    /// > *  color
    /// > *  freeBusyStatus
    /// > *  useDefaultAlerts
    /// > *  alerts
    /// >
    /// > All other existing registrations will have "Is per-user: no".
    ///
    /// The test extracts the bullet item names from a multi-line string
    /// shaped to mirror the spec passage, then asserts equality with the
    /// production const. Updating this test requires the contributor to
    /// edit the spec-shaped string — which a code-review reviewer can
    /// diff against the spec passage above. A contributor who edits the
    /// const without also editing the spec quote will see this test
    /// fail; a contributor who edits both must justify the spec citation
    /// in the same diff.
    #[test]
    fn per_user_calendar_event_properties_match_spec() {
        // Spec-shaped fixture: mirror of draft-ietf-jmap-calendars-26
        // §10.8.2 bullet list. Each "*  <name>" line names one IANA-
        // registered per-user property. Modify ONLY by reconciling
        // against the verbatim spec text in this test's doc comment.
        const SPEC_REGISTERED_PER_USER_PROPERTIES_VERBATIM: &str = "\
*  keywords
*  color
*  freeBusyStatus
*  useDefaultAlerts
*  alerts
";

        // Parse the bullet list independently of the production const.
        // This is the independent-oracle step: the parser does not look
        // at PER_USER_CALENDAR_EVENT_PROPERTIES.
        let spec_list: Vec<&str> = SPEC_REGISTERED_PER_USER_PROPERTIES_VERBATIM
            .lines()
            .filter_map(|line| line.strip_prefix("*  ").map(str::trim))
            .filter(|name| !name.is_empty())
            .collect();

        // Sanity-check the parser before relying on it as the oracle.
        assert_eq!(
            spec_list.len(),
            5,
            "spec-quote parser sanity: §10.8.2 bullet list has exactly 5 entries; \
             parser found {}: {spec_list:?}",
            spec_list.len()
        );

        // The real assertion: production const equals the spec quote.
        assert_eq!(
            PER_USER_CALENDAR_EVENT_PROPERTIES,
            spec_list.as_slice(),
            "PER_USER_CALENDAR_EVENT_PROPERTIES must match \
             draft-ietf-jmap-calendars-26 §10.8.2 verbatim (including order)"
        );
    }

    #[test]
    fn is_per_user_classifies_spec_properties_as_true() {
        for name in PER_USER_CALENDAR_EVENT_PROPERTIES {
            assert!(
                is_per_user_calendar_event_property(name),
                "expected {name} to be classified per-user"
            );
        }
    }

    #[test]
    fn is_per_user_classifies_shared_properties_as_false() {
        // Spot-check a few shared (non-per-user) properties from the draft.
        for shared in &["id", "title", "start", "duration", "calendarIds", "uid"] {
            assert!(
                !is_per_user_calendar_event_property(shared),
                "expected {shared} to be classified shared"
            );
        }
    }

    #[test]
    fn is_per_user_rejects_unknown_property() {
        assert!(!is_per_user_calendar_event_property(""));
        assert!(!is_per_user_calendar_event_property("notARealProperty"));
        // Property-path forms like "alerts/abc" are NOT classified per-user;
        // the routing logic must look at the top-level patch key after
        // expanding any nested path.
        assert!(!is_per_user_calendar_event_property("alerts/abc"));
    }

    /// Drift guard: the public [`PER_USER_CALENDAR_EVENT_PROPERTIES`]
    /// const MUST equal the disjoint union of the two internal halves
    /// [`PER_USER_PROPERTIES_IN_ENUM`] and
    /// [`PER_USER_PROPERTIES_NOT_YET_IN_ENUM`].
    ///
    /// If this test fails, you have either:
    /// - added a name to one half without removing it from the other (the
    ///   disjointness assertion fires), or
    /// - added a name to the public const without placing it in either
    ///   half (the union assertion fires).
    #[test]
    fn per_user_const_is_union_of_two_halves() {
        use std::collections::BTreeSet;

        let in_enum: BTreeSet<&str> = PER_USER_PROPERTIES_IN_ENUM.iter().copied().collect();
        let not_yet: BTreeSet<&str> = PER_USER_PROPERTIES_NOT_YET_IN_ENUM
            .iter()
            .copied()
            .collect();
        let public: BTreeSet<&str> = PER_USER_CALENDAR_EVENT_PROPERTIES.iter().copied().collect();

        // Disjoint: a name MUST NOT appear in both halves.
        let overlap: BTreeSet<&&str> = in_enum.intersection(&not_yet).collect();
        assert!(
            overlap.is_empty(),
            "PER_USER_PROPERTIES_IN_ENUM and PER_USER_PROPERTIES_NOT_YET_IN_ENUM \
             must be disjoint, but both contain: {overlap:?}. When you promote a \
             property to the enum, move its string from NOT_YET_IN_ENUM to IN_ENUM, \
             do not duplicate it."
        );

        // Union equals the public const.
        let union: BTreeSet<&str> = in_enum.union(&not_yet).copied().collect();
        assert_eq!(
            union, public,
            "PER_USER_CALENDAR_EVENT_PROPERTIES must equal the union of the two halves"
        );
    }

    /// Drift guard: [`PER_USER_PROPERTIES_IN_ENUM`] MUST list exactly the
    /// wire-names of [`CalendarEventProperty`] variants that the draft
    /// classifies as per-user.
    ///
    /// The `match` below is exhaustive over the (intra-crate-visible)
    /// `CalendarEventProperty` variants. When you add a new variant the
    /// compiler will force you to add a match arm; classify it as
    /// per-user (`true`) or shared (`false`) per the spec, then update
    /// [`PER_USER_PROPERTIES_IN_ENUM`] (and remove from
    /// [`PER_USER_PROPERTIES_NOT_YET_IN_ENUM`] if it was reserved there)
    /// to match the new ground truth.
    #[test]
    fn per_user_in_enum_matches_enum_variants() {
        use std::collections::BTreeSet;

        // Wire-name + per-user classification for every CalendarEventProperty
        // variant. Adding a variant without updating this match is a compile
        // error in this same crate (#[non_exhaustive] only applies cross-crate).
        fn classify(p: &CalendarEventProperty) -> (&'static str, bool) {
            match p {
                CalendarEventProperty::Id => ("id", false),
                CalendarEventProperty::BaseEventId => ("baseEventId", false),
                CalendarEventProperty::CalendarIds => ("calendarIds", false),
                CalendarEventProperty::IsDraft => ("isDraft", false),
                CalendarEventProperty::IsOrigin => ("isOrigin", false),
                CalendarEventProperty::UtcStart => ("utcStart", false),
                CalendarEventProperty::UtcEnd => ("utcEnd", false),
                CalendarEventProperty::UseDefaultAlerts => ("useDefaultAlerts", true),
                CalendarEventProperty::MayInviteSelf => ("mayInviteSelf", false),
                CalendarEventProperty::MayInviteOthers => ("mayInviteOthers", false),
                CalendarEventProperty::HideAttendees => ("hideAttendees", false),
                CalendarEventProperty::BlobId => ("blobId", false),
                CalendarEventProperty::Uid => ("uid", false),
                CalendarEventProperty::Title => ("title", false),
                CalendarEventProperty::Description => ("description", false),
                CalendarEventProperty::Start => ("start", false),
                CalendarEventProperty::Duration => ("duration", false),
                CalendarEventProperty::Status => ("status", false),
            }
        }

        // Every known variant in turn.
        let variants = [
            CalendarEventProperty::Id,
            CalendarEventProperty::BaseEventId,
            CalendarEventProperty::CalendarIds,
            CalendarEventProperty::IsDraft,
            CalendarEventProperty::IsOrigin,
            CalendarEventProperty::UtcStart,
            CalendarEventProperty::UtcEnd,
            CalendarEventProperty::UseDefaultAlerts,
            CalendarEventProperty::MayInviteSelf,
            CalendarEventProperty::MayInviteOthers,
            CalendarEventProperty::HideAttendees,
            CalendarEventProperty::BlobId,
            CalendarEventProperty::Uid,
            CalendarEventProperty::Title,
            CalendarEventProperty::Description,
            CalendarEventProperty::Start,
            CalendarEventProperty::Duration,
            CalendarEventProperty::Status,
        ];

        let derived_per_user: BTreeSet<&str> = variants
            .iter()
            .filter_map(|p| {
                let (wire, is_per_user) = classify(p);
                is_per_user.then_some(wire)
            })
            .collect();
        let declared_per_user: BTreeSet<&str> =
            PER_USER_PROPERTIES_IN_ENUM.iter().copied().collect();

        assert_eq!(
            derived_per_user, declared_per_user,
            "PER_USER_PROPERTIES_IN_ENUM ({declared_per_user:?}) must match the per-user \
             variants derived from the CalendarEventProperty match in this test \
             ({derived_per_user:?}). Update the const, the match, or both as the spec dictates."
        );

        // Sanity: every wire-name produced by classify() must be unique
        // (a typo that maps two variants to the same wire-name would
        // silently corrupt routing).
        let mut wire_names = BTreeSet::new();
        for p in &variants {
            let (wire, _) = classify(p);
            assert!(
                wire_names.insert(wire),
                "duplicate wire-name {wire:?} in classify(); a CalendarEventProperty \
                 variant has the wrong wire-string"
            );
        }
    }

    /// Oracle: RFC 8620 §5.5 (line 2368) types the /query `filter` as
    /// "FilterOperator|FilterCondition|null". draft-ietf-jmap-calendars-33
    /// §5.11 names the CalendarEvent `inCalendar` condition and §7.4 the
    /// CalendarEventNotification `calendarEventIds` condition. A
    /// hand-written NOT tree decodes as each filter.
    #[test]
    fn query_filters_are_operator_trees() {
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
        assert!(decodes_as_operator::<crate::CalendarEvent, _>(not(
            serde_json::json!({ "inCalendar": "c1" })
        )));
        assert!(decodes_as_operator::<crate::CalendarEventNotification, _>(
            not(serde_json::json!({ "calendarEventIds": ["e1"] }))
        ));
    }
}
