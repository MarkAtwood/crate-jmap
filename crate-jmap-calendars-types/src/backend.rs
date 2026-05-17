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
    /// The `id` property (draft-ietf-jmap-calendars-26 §4).
    Id,
    /// The `name` property (draft-ietf-jmap-calendars-26 §4).
    Name,
    /// The `description` property (draft-ietf-jmap-calendars-26 §4).
    Description,
    /// The `color` property (draft-ietf-jmap-calendars-26 §4).
    Color,
    /// The `sortOrder` property (draft-ietf-jmap-calendars-26 §4).
    SortOrder,
    /// The `isSubscribed` property (draft-ietf-jmap-calendars-26 §4).
    IsSubscribed,
    /// The `isVisible` property (draft-ietf-jmap-calendars-26 §4).
    IsVisible,
    /// The `isDefault` property (draft-ietf-jmap-calendars-26 §4).
    IsDefault,
    /// The `includeInAvailability` property (draft-ietf-jmap-calendars-26 §4).
    IncludeInAvailability,
    /// The `defaultAlertsWithTime` property (draft-ietf-jmap-calendars-26 §4).
    DefaultAlertsWithTime,
    /// The `defaultAlertsWithoutTime` property (draft-ietf-jmap-calendars-26 §4).
    DefaultAlertsWithoutTime,
    /// The `timeZone` property (draft-ietf-jmap-calendars-26 §4).
    TimeZone,
    /// The `shareWith` property (draft-ietf-jmap-calendars-26 §4).
    ShareWith,
    /// The `myRights` property (draft-ietf-jmap-calendars-26 §4).
    MyRights,
}

/// Property selector for [`crate::CalendarEvent`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CalendarEventProperty {
    /// The `id` property (draft-ietf-jmap-calendars-26 §5).
    Id,
    /// The `baseEventId` property (draft-ietf-jmap-calendars-26 §5).
    BaseEventId,
    /// The `calendarIds` property (draft-ietf-jmap-calendars-26 §5).
    CalendarIds,
    /// The `isDraft` property (draft-ietf-jmap-calendars-26 §5).
    IsDraft,
    /// The `isOrigin` property (draft-ietf-jmap-calendars-26 §5).
    IsOrigin,
    /// The `utcStart` property (draft-ietf-jmap-calendars-26 §5).
    UtcStart,
    /// The `utcEnd` property (draft-ietf-jmap-calendars-26 §5).
    UtcEnd,
    /// The `useDefaultAlerts` property (draft-ietf-jmap-calendars-26 §5).
    UseDefaultAlerts,
    /// The `mayInviteSelf` property (draft-ietf-jmap-calendars-26 §5.1.1).
    MayInviteSelf,
    /// The `mayInviteOthers` property (draft-ietf-jmap-calendars-26 §5.1.2).
    MayInviteOthers,
    /// The `hideAttendees` property (draft-ietf-jmap-calendars-26 §5.1.3).
    HideAttendees,
    /// The `blobId` property (draft-ietf-jmap-calendars-26 §10.9.14).
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
/// Calendars draft (draft-ietf-jmap-calendars-26 §5.4) classifies as
/// **per-user**.
///
/// Per-user properties belong to the authenticated user's view of the event;
/// patching them MUST NOT change the shared `updated` timestamp on the
/// underlying object. Backends serving multiple users SHOULD store these
/// separately from the shared event body.
///
/// This list mirrors the IANA-registered set in §10.8.2 of the draft.
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

/// Per-user property wire-names from draft-ietf-jmap-calendars-26 §5.4
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
/// property name per draft-ietf-jmap-calendars-26 §5.4.
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
    /// The `id` property (draft-ietf-jmap-calendars-26 §7).
    Id,
    /// The `created` property (draft-ietf-jmap-calendars-26 §7).
    Created,
    /// The `changedBy` property (draft-ietf-jmap-calendars-26 §7).
    ChangedBy,
    /// The `comment` property (draft-ietf-jmap-calendars-26 §7).
    Comment,
    /// The `type` property (draft-ietf-jmap-calendars-26 §7).
    Type,
    /// The `calendarEventId` property (draft-ietf-jmap-calendars-26 §7).
    CalendarEventId,
    /// The `isDraft` property (draft-ietf-jmap-calendars-26 §7).
    IsDraft,
    /// The `event` property (draft-ietf-jmap-calendars-26 §7).
    Event,
    /// The `eventPatch` property (draft-ietf-jmap-calendars-26 §7).
    EventPatch,
}

/// Property selector for [`crate::ParticipantIdentity`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParticipantIdentityProperty {
    /// The `id` property (draft-ietf-jmap-calendars-26 §3).
    Id,
    /// The `name` property (draft-ietf-jmap-calendars-26 §3).
    Name,
    /// The `calendarAddress` property (draft-ietf-jmap-calendars-26 §3).
    CalendarAddress,
    /// The `isDefault` property (draft-ietf-jmap-calendars-26 §3).
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

impl SetObject for crate::Calendar {
    type Patch = PatchObject;
}

impl QueryObject for crate::Calendar {
    type Filter = crate::CalendarFilterCondition;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::CalendarEvent {
    const TYPE_NAME: &'static str = "CalendarEvent";
    type Property = CalendarEventProperty;
}

impl GetObject for crate::CalendarEvent {}

impl SetObject for crate::CalendarEvent {
    type Patch = PatchObject;
}

impl QueryObject for crate::CalendarEvent {
    type Filter = crate::CalendarEventFilterCondition;
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
impl SetObject for crate::CalendarEventNotification {
    type Patch = PatchObject;
}

impl QueryObject for crate::CalendarEventNotification {
    type Filter = crate::NotificationFilterCondition;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::ParticipantIdentity {
    const TYPE_NAME: &'static str = "ParticipantIdentity";
    type Property = ParticipantIdentityProperty;
}

impl GetObject for crate::ParticipantIdentity {}

impl SetObject for crate::ParticipantIdentity {
    type Patch = PatchObject;
}

impl QueryObject for crate::ParticipantIdentity {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinning test: the per-user property list MUST match the IANA-registered
    /// set in draft-ietf-jmap-calendars-26 §5.4 / §10.8.2 exactly. Update this
    /// table only when the spec list itself changes.
    #[test]
    fn per_user_calendar_event_properties_match_spec() {
        assert_eq!(
            PER_USER_CALENDAR_EVENT_PROPERTIES,
            &[
                "keywords",
                "color",
                "freeBusyStatus",
                "useDefaultAlerts",
                "alerts"
            ]
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
}
