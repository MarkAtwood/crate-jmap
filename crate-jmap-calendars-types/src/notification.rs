//! CalendarEventNotification, Person, and NotificationType.
//!
//! Normative reference: draft-ietf-jmap-calendars-33 §7.

use jmap_types::{Id, PatchObject, UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};

/// Who made the change that triggered a [`CalendarEventNotification`]
/// (draft-ietf-jmap-calendars-33 §7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// Display name of the person who made the change.
    pub name: String,

    /// Email address of the person, or `null` if not available.
    pub email: Option<String>,

    /// Id of the Principal corresponding to this person, or `null` if
    /// the change came from an external entity (e.g. an iTIP invitation).
    pub principal_id: Option<Id>,

    /// CalendarAddress URI of the person, or `null`.
    pub calendar_address: Option<String>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Type of change recorded by a [`CalendarEventNotification`]
/// (draft-ietf-jmap-calendars-33 §7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotificationType {
    /// A calendar event was created.
    Created,
    /// A calendar event was updated.
    Updated,
    /// A calendar event was destroyed.
    Destroyed,
    /// Any notification type string not recognised by this implementation.
    Other(String),
}

impl_string_enum!(NotificationType, "a JMAP CalendarEventNotification type string",
    "created"   => Created,
    "updated"   => Updated,
    "destroyed" => Destroyed,
);

/// A JMAP CalendarEventNotification object
/// (draft-ietf-jmap-calendars-33 §7).
///
/// Records a change made to a calendar event by an external entity or
/// another client.  Apart from `hideAttendees` filtering, a notification
/// has no per-user data, so one object may be shared by all users with
/// access to the calendar; but the server MUST NOT create one for anyone
/// other than the calendar's owner when the event's `privacy` is
/// `"private"` or `"secret"` before or after the change (§7).
///
/// Only `destroyed` notifications may be explicitly managed via
/// `CalendarEventNotification/set`; `created` and `updated` notifications
/// are server-set.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventNotification {
    /// Server-assigned identifier for this notification.
    pub id: Id,

    /// UTC date-time when this notification was created.
    pub created: UTCDate,

    /// Who made the change.
    pub changed_by: Person,

    /// Comment sent along with the change (e.g. iTIP COMMENT property), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,

    /// Type of change recorded.
    ///
    /// Wire field name is `type`.
    #[serde(rename = "type")]
    pub notification_type: NotificationType,

    /// Id of the CalendarEvent this notification is about (always the base
    /// event id, even for single-instance changes).
    pub calendar_event_id: Id,

    /// Whether the event is a draft.  Present for `created` and `updated`
    /// notifications only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_draft: Option<bool>,

    /// The CalendarEvent data before the change (for `updated` or `destroyed`),
    /// or the data after creation (for `created`).
    pub event: serde_json::Value,

    /// A PatchObject encoding the change between `event` and the state after
    /// the update.  Present for `updated` notifications only.
    ///
    /// Wire format is byte-identical to a plain JSON object via
    /// [`PatchObject`]'s `#[serde(transparent)]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_patch: Option<PatchObject>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// Serde-default function for the `@type` discriminator on
// CalendarAlert. draft-ietf-jmap-calendars-33 §6.4 marks the field as
// mandatory on the wire; we supply the literal as a default so
// deserialize is liberal in what it accepts (spec-violating vendor
// input missing `@type` does not fail). Mirrors the same pattern in
// `jmap-jscalendar-types` (bd:JMAP-ky8g.10) and `jmap-tasks-types`
// (bd:JMAP-ky8g.1).

fn calendar_alert_at_type_default() -> String {
    "CalendarAlert".to_owned()
}

/// Push-notification payload emitted when a calendar alert fires
/// (draft-ietf-jmap-calendars-33 §6.4).
///
/// Sent over the JMAP push channel when the server determines that an alert
/// for a CalendarEvent should fire.  Clients receiving this payload should
/// trigger the appropriate local notification (e.g. an OS alert).
///
/// `recurrenceId` is `null` for non-recurring events and MUST serialize as
/// `null` (not be omitted) — the receiver uses its presence to distinguish
/// recurring from non-recurring.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarAlert {
    /// Object type discriminator; always `"CalendarAlert"` on the wire.
    ///
    /// Deserialize is liberal: if `@type` is absent (spec-violating
    /// vendor input), this field defaults to `"CalendarAlert"` rather
    /// than failing the whole parent object's deserialize. Serialize
    /// always emits the field. See bd:JMAP-ky8g.10.
    #[serde(rename = "@type", default = "calendar_alert_at_type_default")]
    pub at_type: String,

    /// The account that owns the calendar event.
    pub account_id: Id,

    /// Id of the CalendarEvent for which this alert is firing.
    pub calendar_event_id: Id,

    /// The UID property of the iCalendar object underlying the calendar event.
    pub uid: String,

    /// The recurrenceId of the specific occurrence, or `null` for non-recurring
    /// events (draft-ietf-jmap-calendars-33 §6.4).
    ///
    /// Serializes as `null` when `None` — intentionally NOT marked
    /// `skip_serializing_if` so the receiver can distinguish recurring from
    /// non-recurring events.
    ///
    /// **Maintainer note: do NOT add `#[serde(skip_serializing_if = "Option::is_none")]`
    /// to this field.** Adding it would silently break the spec contract:
    /// per draft-ietf-jmap-calendars-33 §6.4, the receiver uses the
    /// **presence** of `recurrenceId` to determine whether the alert
    /// fired for a recurring or non-recurring event. An omitted field
    /// is indistinguishable from a sender that forgot to include it; an
    /// explicit `null` carries the "non-recurring" signal. This is the
    /// same "required-and-nullable" wire shape used by
    /// `PrincipalCalendarsCapability.account_id` in `capability.rs:121`.
    ///
    /// A "clippy clean-up", "consistency PR", or pattern-based code
    /// audit may flag this field as inconsistent with neighbours that
    /// use `skip_serializing_if` and propose to "fix" it. That is a
    /// silent wire-protocol break. The regression test
    /// `calendar_alert_recurrence_id_null_for_non_recurring` (in
    /// `tests/types_test.rs`) pins the current behavior. Design defense:
    /// bd:JMAP-1rwf.7.
    pub recurrence_id: Option<String>,

    /// Id of the [`Alert`](crate::Alert) object within the CalendarEvent that is firing.
    pub alert_id: String,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Filter condition for `CalendarEventNotification/query`
/// (draft-ietf-jmap-calendars-33 §7.4.1).
///
/// All fields are optional; a condition with no fields set matches every
/// CalendarEventNotification.
///
/// # Excluded from extras preservation
///
/// This type is **out of scope** for the workspace extras-preservation
/// policy: it carries no flatten-extras `extra` field. Filter clauses the
/// server does not understand are a query-correctness hazard — silently
/// preserving an unrecognised clause and round-tripping it back to the
/// client can return the wrong set of records with no error signal.
///
/// ## What to do instead
///
/// **IETF-track path.** Vendors who need both capability-level declaration
/// and filterability for custom fields should use
/// `draft-ietf-jmap-metadata` (capability URI
/// `urn:ietf:params:jmap:metadata`), which adds namespace-keyed
/// `metadata` / `privateMetadata` properties to each listed data type,
/// filterable through `metadata*` conditions on that type's own `/query`
/// (draft-ietf-jmap-metadata-04 §2, §3.6). Helpers live in
/// `jmap-metadata-types`, `jmap-metadata-server`, and
/// `jmap-metadata-client`.
///
/// **Pre-IETF escape.** Vendors who cannot wait for the metadata draft can
/// either escape the filter tree to `serde_json::Value` or fork the
/// `FilterCondition` type. See `crate-jmap-calendars-types/PLAN.md` for
/// the hybrid sloppy-value pattern.
///
/// Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationFilterCondition {
    /// Notification `created` time must be on or after this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<UTCDate>,

    /// Notification `created` time must be before this UTCDate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<UTCDate>,

    /// Notification `type` must equal this value.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<NotificationType>,

    /// Notification `calendarEventId` must be in this list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_event_ids: Option<Vec<Id>>,
}

/// Concrete filter type for CalendarEventNotification/query
/// (draft-ietf-jmap-calendars-33 §6).
///
/// Alias for `jmap_types::Filter<NotificationFilterCondition>`
/// provided so callers do not have to reach into `jmap-types` directly.
/// Mirrors the canonical [`jmap_mail_types::EmailFilter`] shape from the
/// workspace canonical extension-types template.
///
/// [`jmap_mail_types::EmailFilter`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/type.EmailFilter.html
pub type NotificationFilter = jmap_types::Filter<NotificationFilterCondition>;

/// The property to sort by in a [`CalendarEventNotificationComparator`]
/// (draft-ietf-jmap-calendars-33 §7.4.2: "The "created" property MUST be
/// supported for sorting.").
///
/// Unknown property names are preserved in
/// [`Other`](CalendarEventNotificationComparatorProperty::Other) so they
/// round-trip; a server cannot sort by one it does not know (RFC 8620 §5.5
/// `unsupportedSort`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CalendarEventNotificationComparatorProperty {
    /// Sort by `created`.
    Created,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(CalendarEventNotificationComparatorProperty,
    "a CalendarEventNotification sort property string",
    "created" => Created,
);

/// Comparator for `CalendarEventNotification/query`
/// (draft-ietf-jmap-calendars-33 §7.4.2).
///
/// Build one with [`CalendarEventNotificationComparator::new`]; the struct
/// is `#[non_exhaustive]`. Out of scope for extras preservation for the
/// same reason as [`crate::CalendarEventComparator`]: backends dispatch on
/// `property`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventNotificationComparator {
    /// The property to sort by.
    pub property: CalendarEventNotificationComparatorProperty,

    /// If `true`, sort ascending; if `false`, sort descending (default `true`).
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub is_ascending: bool,

    /// A collation identifier (RFC 4790) to use when comparing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl CalendarEventNotificationComparator {
    /// An ascending comparator on `property` with no collation
    /// (`isAscending` defaults to `true`, RFC 8620 §5.5).
    pub fn new(property: CalendarEventNotificationComparatorProperty) -> Self {
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

#[cfg(test)]
mod comparator_tests {
    use super::*;

    /// Oracle: draft-ietf-jmap-calendars-33 §7.4.2 names "created" as the
    /// sort property; RFC 8620 §5.5 makes `isAscending` default to true.
    #[test]
    fn created_comparator_decodes_from_spec_name() {
        let c: CalendarEventNotificationComparator =
            serde_json::from_str(r#"{"property":"created"}"#).expect("must parse");
        assert_eq!(
            c.property,
            CalendarEventNotificationComparatorProperty::Created
        );
        assert!(c.is_ascending);
        assert_eq!(c.collation, None);
    }

    /// Oracle: hand-written JSON; an unknown property survives as `Other`
    /// and re-serializes unchanged (AGENTS.md comparator rule).
    #[test]
    fn unknown_property_round_trips() {
        let json = serde_json::json!({"property":"calendarEventId","isAscending":false});
        let c: CalendarEventNotificationComparator =
            serde_json::from_value(json.clone()).expect("must parse");
        assert_eq!(
            c.property,
            CalendarEventNotificationComparatorProperty::Other("calendarEventId".to_owned())
        );
        assert_eq!(serde_json::to_value(&c).unwrap(), json);
    }
}
