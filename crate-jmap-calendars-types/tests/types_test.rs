//! Integration tests for jmap-calendars-types.
//!
//! Test oracles are hand-written JSON fixtures constructed directly from:
//!   - draft-ietf-jmap-calendars-26 §4 (Calendar), §5 (CalendarEvent),
//!     §7 (CalendarEventNotification), §3 (ParticipantIdentity)
//!   - RFC 8984 §4 (JSCalendar sub-objects), §5.1 (Event-specific properties)
//!
//! No expected values are derived from the code under test.
//!
//! NOTE: JSON containing the sequence `"#` (e.g. CSS hex colors) requires
//! `r##"..."##` raw string delimiters because `"#` would terminate `r#"..."#`.

use jmap_calendars_types::{
    Alert, AlertTrigger, Calendar, CalendarEvent, CalendarEventFilterCondition,
    CalendarEventNotification, CalendarRights, CalendarsAccountCapability, CalendarsCapability,
    IncludeInAvailability, ParticipantIdentity, Person, JMAP_CALENDARS_URI,
};
use jmap_types::Id;

// ─── Calendar ────────────────────────────────────────────────────────────────

/// Deserialize a full Calendar from hand-written JSON and verify key fields.
/// Oracle derived from draft-ietf-jmap-calendars-26 §4 field descriptions.
#[test]
fn calendar_deserialize_full() {
    // r##"..."## needed because color value contains `"#`.
    let json = r##"{
        "id": "cal-001",
        "name": "Personal",
        "description": "My personal calendar",
        "color": "#4287f5",
        "sortOrder": 5,
        "isSubscribed": true,
        "isVisible": true,
        "isDefault": true,
        "includeInAvailability": "all",
        "defaultAlertsWithTime": null,
        "defaultAlertsWithoutTime": null,
        "timeZone": "America/New_York",
        "shareWith": null,
        "myRights": {
            "mayReadFreeBusy": true,
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayWriteOwn": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "mayShare": true,
            "mayDelete": true
        }
    }"##;

    let cal: Calendar = serde_json::from_str(json).expect("Calendar deserialize");

    assert_eq!(cal.id.as_ref(), "cal-001");
    assert_eq!(cal.name, "Personal");
    assert_eq!(cal.description.as_deref(), Some("My personal calendar"));
    assert_eq!(cal.color.as_deref(), Some("#4287f5"));
    assert_eq!(cal.sort_order, 5);
    assert!(cal.is_subscribed);
    assert!(cal.is_visible);
    assert!(cal.is_default);
    assert_eq!(cal.include_in_availability, IncludeInAvailability::All);
    assert!(cal.default_alerts_with_time.is_none());
    assert!(cal.default_alerts_without_time.is_none());
    assert_eq!(cal.time_zone.as_deref(), Some("America/New_York"));
    assert!(cal.share_with.is_none());
    assert!(cal.my_rights.may_read_free_busy);
    assert!(cal.my_rights.may_write_all);
    assert!(cal.my_rights.may_share);
    assert!(cal.my_rights.may_delete);
}

/// Calendar round-trip: serialize then re-deserialize, verify equality.
#[test]
fn calendar_roundtrip() {
    let json = r#"{
        "id": "cal-rt",
        "name": "RT Calendar",
        "description": null,
        "color": null,
        "sortOrder": 0,
        "isSubscribed": false,
        "isVisible": true,
        "isDefault": false,
        "includeInAvailability": "none",
        "defaultAlertsWithTime": null,
        "defaultAlertsWithoutTime": null,
        "timeZone": null,
        "shareWith": null,
        "myRights": {
            "mayReadFreeBusy": false,
            "mayReadItems": false,
            "mayWriteAll": false,
            "mayWriteOwn": false,
            "mayUpdatePrivate": false,
            "mayRSVP": false,
            "mayShare": false,
            "mayDelete": false
        }
    }"#;
    let original: Calendar = serde_json::from_str(json).expect("first deserialize");
    let serialized = serde_json::to_string(&original).expect("serialize");
    let recovered: Calendar = serde_json::from_str(&serialized).expect("second deserialize");
    assert_eq!(original, recovered);
}

/// shareWith with actual rights deserializes correctly.
#[test]
fn calendar_share_with() {
    let json = r#"{
        "id": "cal-shared",
        "name": "Shared",
        "description": null,
        "color": null,
        "sortOrder": 0,
        "isSubscribed": true,
        "isVisible": true,
        "isDefault": false,
        "includeInAvailability": "attending",
        "defaultAlertsWithTime": null,
        "defaultAlertsWithoutTime": null,
        "timeZone": null,
        "shareWith": {
            "principal-42": {
                "mayReadFreeBusy": true,
                "mayReadItems": true,
                "mayWriteAll": false,
                "mayWriteOwn": false,
                "mayUpdatePrivate": false,
                "mayRSVP": false,
                "mayShare": false,
                "mayDelete": false
            }
        },
        "myRights": {
            "mayReadFreeBusy": true,
            "mayReadItems": true,
            "mayWriteAll": true,
            "mayWriteOwn": true,
            "mayUpdatePrivate": true,
            "mayRSVP": true,
            "mayShare": true,
            "mayDelete": true
        }
    }"#;
    let cal: Calendar = serde_json::from_str(json).expect("calendar with shareWith");
    let share = cal.share_with.as_ref().expect("shareWith not None");
    let rights = share.values().next().expect("at least one entry");
    assert!(rights.may_read_free_busy);
    assert!(rights.may_read_items);
    assert!(!rights.may_write_all);
}

// ─── CalendarRights ───────────────────────────────────────────────────────────

/// `Default` produces all-false (most restrictive).
#[test]
fn calendar_rights_default_all_false() {
    let r = CalendarRights::default();
    assert!(!r.may_read_free_busy);
    assert!(!r.may_read_items);
    assert!(!r.may_write_all);
    assert!(!r.may_write_own);
    assert!(!r.may_update_private);
    assert!(!r.may_rsvp);
    assert!(!r.may_share);
    assert!(!r.may_delete);
}

/// All eight boolean fields serialize to the correct camelCase wire names.
/// Oracle: field names from draft-ietf-jmap-calendars-26 §4.
#[test]
fn calendar_rights_wire_names() {
    // Deserialize from known-good JSON then re-serialize to check wire names.
    let json = r#"{
        "mayReadFreeBusy": true,
        "mayReadItems": true,
        "mayWriteAll": true,
        "mayWriteOwn": true,
        "mayUpdatePrivate": true,
        "mayRSVP": true,
        "mayShare": true,
        "mayDelete": true
    }"#;
    let rights: CalendarRights = serde_json::from_str(json).expect("deserialize rights");
    let out = serde_json::to_value(&rights).expect("serialize");

    // Verify every wire field name against the spec.
    assert_eq!(out["mayReadFreeBusy"], true);
    assert_eq!(out["mayReadItems"], true);
    assert_eq!(out["mayWriteAll"], true);
    assert_eq!(out["mayWriteOwn"], true);
    assert_eq!(out["mayUpdatePrivate"], true);
    assert_eq!(out["mayRSVP"], true);
    assert_eq!(out["mayShare"], true);
    assert_eq!(out["mayDelete"], true);
}

// ─── IncludeInAvailability ────────────────────────────────────────────────────

/// All three known values serialize to the correct lowercase wire strings.
/// Oracle: spec §4 field description.
#[test]
fn include_in_availability_known_values() {
    let all = serde_json::to_string(&IncludeInAvailability::All).expect("serialize All");
    let attending =
        serde_json::to_string(&IncludeInAvailability::Attending).expect("serialize Attending");
    let none = serde_json::to_string(&IncludeInAvailability::None).expect("serialize None");

    assert_eq!(all, r#""all""#);
    assert_eq!(attending, r#""attending""#);
    assert_eq!(none, r#""none""#);
}

/// All three known values deserialize from the correct wire strings.
#[test]
fn include_in_availability_deserialize() {
    let all: IncludeInAvailability = serde_json::from_str(r#""all""#).expect("all");
    let attending: IncludeInAvailability =
        serde_json::from_str(r#""attending""#).expect("attending");
    let none: IncludeInAvailability = serde_json::from_str(r#""none""#).expect("none");

    assert_eq!(all, IncludeInAvailability::All);
    assert_eq!(attending, IncludeInAvailability::Attending);
    assert_eq!(none, IncludeInAvailability::None);
}

// ─── CalendarEvent ────────────────────────────────────────────────────────────

/// Minimal CalendarEvent with only id and calendarIds deserializes without error.
/// Oracle: draft §5 mandatory fields.
#[test]
fn calendar_event_minimal() {
    let json = r#"{
        "id": "event-001",
        "calendarIds": {
            "cal-001": true
        }
    }"#;
    let ev: CalendarEvent = serde_json::from_str(json).expect("minimal CalendarEvent");
    let id_val = ev.id.as_ref().expect("id present");
    assert_eq!(id_val.as_ref(), "event-001");
    let cals = ev.calendar_ids.as_ref().expect("calendarIds");
    assert_eq!(cals.len(), 1);
    assert_eq!(*cals.values().next().expect("first value"), true);
}

/// An all-fields-None (empty) CalendarEvent deserializes without error
/// (partial response per RFC 8620 §5.1).
#[test]
fn calendar_event_all_none() {
    let ev: CalendarEvent = serde_json::from_str("{}").expect("empty CalendarEvent");
    assert!(ev.id.is_none());
    assert!(ev.calendar_ids.is_none());
    assert!(ev.title.is_none());
    assert!(ev.start.is_none());
    assert!(ev.recurrence_rules.is_none());
}

/// A richer CalendarEvent with JSCalendar scalar fields deserializes correctly.
/// Oracle: RFC 8984 §6.1 simple event example (adapted).
#[test]
fn calendar_event_scalar_fields() {
    let json = r#"{
        "@type": "Event",
        "uid": "2a358cee-6489-4f14-a57f-c104db4dc357",
        "id": "ev-42",
        "calendarIds": {
            "cal-main": true
        },
        "title": "Team meeting",
        "description": "Monthly all-hands",
        "start": "2024-06-15T09:00:00",
        "duration": "PT1H",
        "timeZone": "America/New_York",
        "status": "confirmed",
        "isDraft": false,
        "isOrigin": true,
        "showWithoutTime": false,
        "priority": 5,
        "freeBusyStatus": "busy",
        "privacy": "public",
        "created": "2024-05-01T12:00:00Z",
        "updated": "2024-06-01T08:00:00Z",
        "sequence": 0,
        "color": "red"
    }"#;

    let ev: CalendarEvent = serde_json::from_str(json).expect("CalendarEvent deserialize");

    assert_eq!(ev.at_type.as_deref(), Some("Event"));
    assert_eq!(
        ev.uid.as_deref(),
        Some("2a358cee-6489-4f14-a57f-c104db4dc357")
    );
    assert_eq!(ev.title.as_deref(), Some("Team meeting"));
    assert_eq!(ev.description.as_deref(), Some("Monthly all-hands"));
    assert_eq!(ev.start.as_deref(), Some("2024-06-15T09:00:00"));
    assert_eq!(ev.duration.as_deref(), Some("PT1H"));
    assert_eq!(ev.time_zone.as_deref(), Some("America/New_York"));
    assert_eq!(ev.status.as_deref(), Some("confirmed"));
    assert_eq!(ev.is_draft, Some(false));
    assert_eq!(ev.is_origin, Some(true));
    assert_eq!(ev.show_without_time, Some(false));
    assert_eq!(ev.priority, Some(5));
    assert_eq!(ev.free_busy_status.as_deref(), Some("busy"));
    assert_eq!(ev.privacy.as_deref(), Some("public"));
    assert_eq!(ev.sequence, Some(0));
    assert_eq!(ev.color.as_deref(), Some("red"));
}

/// CalendarEvent with recurrenceOverrides as serde_json::Value round-trips.
/// Oracle: RFC 8984 §4.3.2 recurrenceOverrides description.
#[test]
fn calendar_event_recurrence_overrides_passthrough() {
    let json = r#"{
        "id": "ev-recur",
        "calendarIds": {"cal-001": true},
        "recurrenceRules": [
            {
                "@type": "RecurrenceRule",
                "frequency": "weekly",
                "count": 4
            }
        ],
        "recurrenceOverrides": {
            "2024-06-22T09:00:00": {
                "title": "Rescheduled meeting",
                "start": "2024-06-22T11:00:00"
            }
        }
    }"#;

    let ev: CalendarEvent = serde_json::from_str(json).expect("recurrence CalendarEvent");
    let overrides = ev
        .recurrence_overrides
        .as_ref()
        .expect("recurrenceOverrides");
    // Verify the key exists in the passthrough value.
    let key = &overrides["2024-06-22T09:00:00"];
    assert_eq!(key["title"], "Rescheduled meeting");
}

/// CalendarEvent round-trip preserves all present fields.
#[test]
fn calendar_event_roundtrip() {
    let json = r#"{
        "@type": "Event",
        "id": "ev-rt",
        "calendarIds": {"cal-rt": true},
        "uid": "uid-rt-001",
        "title": "RT event",
        "start": "2024-07-04T10:00:00",
        "duration": "PT2H",
        "timeZone": "UTC",
        "updated": "2024-07-01T00:00:00Z",
        "isDraft": false
    }"#;
    let original: CalendarEvent = serde_json::from_str(json).expect("first deserialize");
    let serialized = serde_json::to_string(&original).expect("serialize");
    let recovered: CalendarEvent = serde_json::from_str(&serialized).expect("second deserialize");
    assert_eq!(original, recovered);
}

// ─── CalendarEventFilterCondition ─────────────────────────────────────────────

/// An empty CalendarEventFilterCondition deserializes from `{}`.
#[test]
fn calendar_event_filter_condition_empty() {
    let fc: CalendarEventFilterCondition =
        serde_json::from_str("{}").expect("empty FilterCondition");
    assert!(fc.in_calendar.is_none());
    assert!(fc.after.is_none());
    assert!(fc.before.is_none());
    assert!(fc.text.is_none());
    assert!(fc.title.is_none());
    assert!(fc.description.is_none());
    assert!(fc.location.is_none());
    assert!(fc.owner.is_none());
    assert!(fc.attendee.is_none());
    assert!(fc.uid.is_none());
}

/// CalendarEventFilterCondition serializes set fields to correct wire names.
/// Oracle: draft §5.11.1 filter field names.
#[test]
fn calendar_event_filter_condition_wire_names() {
    let json = r#"{
        "inCalendar": "cal-001",
        "after": "2024-01-01T00:00:00",
        "before": "2024-12-31T23:59:59",
        "text": "meeting",
        "title": "team"
    }"#;
    let fc: CalendarEventFilterCondition = serde_json::from_str(json).expect("filter condition");

    assert_eq!(
        fc.in_calendar.as_ref().map(|id| id.as_ref()),
        Some("cal-001")
    );
    assert_eq!(fc.after.as_deref(), Some("2024-01-01T00:00:00"));
    assert_eq!(fc.before.as_deref(), Some("2024-12-31T23:59:59"));
    assert_eq!(fc.text.as_deref(), Some("meeting"));
    assert_eq!(fc.title.as_deref(), Some("team"));
    assert!(fc.description.is_none());
    assert!(fc.owner.is_none());

    // Verify round-trip serialization uses correct wire names.
    let out = serde_json::to_value(&fc).expect("serialize");
    assert_eq!(out["inCalendar"], "cal-001");
    assert_eq!(out["after"], "2024-01-01T00:00:00");
    assert_eq!(out["before"], "2024-12-31T23:59:59");
}

// ─── CalendarEventNotification ────────────────────────────────────────────────

/// Deserialize a full CalendarEventNotification from hand-written JSON.
/// Oracle: draft-ietf-jmap-calendars-26 §7 field descriptions.
#[test]
fn notification_deserialize() {
    let json = r#"{
        "id": "notif-001",
        "created": "2024-06-15T10:05:00Z",
        "changedBy": {
            "name": "Alice Smith",
            "email": "alice@example.com",
            "principalId": "principal-alice",
            "calendarAddress": "mailto:alice@example.com"
        },
        "comment": null,
        "type": "updated",
        "calendarEventId": "event-001",
        "isDraft": false,
        "event": {
            "@type": "Event",
            "uid": "uid-event-001",
            "title": "Original Title"
        },
        "eventPatch": {
            "title": "New Title"
        }
    }"#;

    let n: CalendarEventNotification =
        serde_json::from_str(json).expect("CalendarEventNotification deserialize");

    assert_eq!(n.id.as_ref(), "notif-001");
    assert_eq!(n.created, "2024-06-15T10:05:00Z");
    assert_eq!(n.changed_by.name, "Alice Smith");
    assert_eq!(n.changed_by.email.as_deref(), Some("alice@example.com"));
    assert_eq!(
        n.changed_by.principal_id.as_ref().map(|id| id.as_ref()),
        Some("principal-alice")
    );
    assert_eq!(
        n.changed_by.calendar_address.as_deref(),
        Some("mailto:alice@example.com")
    );
    assert!(n.comment.is_none());
    assert_eq!(
        n.notification_type,
        jmap_calendars_types::NotificationType::Updated
    );
    assert_eq!(n.calendar_event_id.as_ref(), "event-001");
    assert_eq!(n.is_draft, Some(false));
    assert_eq!(n.event["title"], "Original Title");
    let patch = n.event_patch.as_ref().expect("eventPatch");
    assert_eq!(patch["title"], "New Title");
}

/// A `created` notification has no eventPatch.
#[test]
fn notification_created_no_patch() {
    let json = r#"{
        "id": "notif-002",
        "created": "2024-06-16T08:00:00Z",
        "changedBy": {
            "name": "Bob Jones",
            "email": null,
            "principalId": null,
            "calendarAddress": null
        },
        "type": "created",
        "calendarEventId": "event-002",
        "isDraft": true,
        "event": {"@type": "Event", "uid": "uid-event-002", "title": "Draft event"}
    }"#;

    let n: CalendarEventNotification = serde_json::from_str(json).expect("created notification");
    assert_eq!(
        n.notification_type,
        jmap_calendars_types::NotificationType::Created
    );
    assert!(n.event_patch.is_none());
    assert_eq!(n.is_draft, Some(true));
}

// ─── NotificationType ─────────────────────────────────────────────────────────

/// All three known notification types serialize to correct wire strings.
#[test]
fn notification_type_known_values() {
    use jmap_calendars_types::NotificationType;

    let created = serde_json::to_string(&NotificationType::Created).expect("created");
    let updated = serde_json::to_string(&NotificationType::Updated).expect("updated");
    let destroyed = serde_json::to_string(&NotificationType::Destroyed).expect("destroyed");

    assert_eq!(created, r#""created""#);
    assert_eq!(updated, r#""updated""#);
    assert_eq!(destroyed, r#""destroyed""#);
}

/// An unknown notification type maps to `Other(String)` and round-trips.
#[test]
fn notification_type_other_roundtrip() {
    use jmap_calendars_types::NotificationType;

    let other: NotificationType = serde_json::from_str(r#""rescheduled""#).expect("other type");
    match &other {
        NotificationType::Other(s) => assert_eq!(s, "rescheduled"),
        _ => panic!("expected Other variant"),
    }
    let back = serde_json::to_string(&other).expect("re-serialize");
    assert_eq!(back, r#""rescheduled""#);
}

// ─── ParticipantIdentity ──────────────────────────────────────────────────────

/// ParticipantIdentity round-trip with isDefault true.
/// Oracle: draft §3 field descriptions.
#[test]
fn participant_identity_roundtrip() {
    let json = r#"{
        "id": "pi-001",
        "name": "Joe Bloggs",
        "calendarAddress": "mailto:joe@example.com",
        "isDefault": true
    }"#;

    let pi: ParticipantIdentity = serde_json::from_str(json).expect("ParticipantIdentity");

    assert_eq!(pi.id.as_ref(), "pi-001");
    assert_eq!(pi.name, "Joe Bloggs");
    assert_eq!(pi.calendar_address, "mailto:joe@example.com");
    assert!(pi.is_default);

    // Round-trip.
    let serialized = serde_json::to_string(&pi).expect("serialize");
    let recovered: ParticipantIdentity = serde_json::from_str(&serialized).expect("re-deserialize");
    assert_eq!(pi, recovered);
}

/// ParticipantIdentity wire field names match spec.
/// Oracle: draft §3 field list.
#[test]
fn participant_identity_wire_names() {
    let json = r#"{
        "id": "pi-002",
        "name": "Jane Doe",
        "calendarAddress": "mailto:jane@example.com",
        "isDefault": false
    }"#;
    let pi: ParticipantIdentity = serde_json::from_str(json).expect("ParticipantIdentity");
    let out = serde_json::to_value(&pi).expect("serialize");
    assert_eq!(out["id"], "pi-002");
    assert_eq!(out["name"], "Jane Doe");
    assert_eq!(out["calendarAddress"], "mailto:jane@example.com");
    assert_eq!(out["isDefault"], false);
}

// ─── Capability ───────────────────────────────────────────────────────────────

/// CalendarsCapability serializes to an empty object.
#[test]
fn calendars_capability_empty_object() {
    let cap = CalendarsCapability::default();
    let json = serde_json::to_string(&cap).expect("serialize");
    assert_eq!(json, "{}");
}

/// CalendarsAccountCapability round-trip with all required fields.
/// Oracle: draft §1.5.1 field descriptions.
#[test]
fn calendars_account_capability_roundtrip() {
    let json = r#"{
        "maxCalendarsPerEvent": 5,
        "minDateTime": "1970-01-01T00:00:00Z",
        "maxDateTime": "2099-12-31T23:59:59Z",
        "maxExpandedQueryDuration": "P1Y",
        "maxParticipantsPerEvent": null,
        "mayCreateCalendar": true
    }"#;

    let cap: CalendarsAccountCapability =
        serde_json::from_str(json).expect("CalendarsAccountCapability");

    assert_eq!(cap.max_calendars_per_event, Some(5));
    assert_eq!(cap.min_date_time, "1970-01-01T00:00:00Z");
    assert_eq!(cap.max_date_time, "2099-12-31T23:59:59Z");
    assert_eq!(cap.max_expanded_query_duration, "P1Y");
    assert!(cap.max_participants_per_event.is_none());
    assert!(cap.may_create_calendar);

    // Round-trip.
    let s = serde_json::to_string(&cap).expect("serialize");
    let r: CalendarsAccountCapability = serde_json::from_str(&s).expect("re-deserialize");
    assert_eq!(cap, r);
}

/// JMAP_CALENDARS_URI constant matches spec §1.5.1.
#[test]
fn calendars_uri_constant() {
    assert_eq!(JMAP_CALENDARS_URI, "urn:ietf:params:jmap:calendars");
}

// ─── AlertTrigger ─────────────────────────────────────────────────────────────

/// OffsetTrigger serializes and deserializes correctly.
/// Oracle: RFC 8984 §4.5.2.
#[test]
fn alert_offset_trigger_roundtrip() {
    let json = r#"{
        "@type": "OffsetTrigger",
        "offset": "-PT15M",
        "relativeTo": "start"
    }"#;

    let trigger: AlertTrigger = serde_json::from_str(json).expect("OffsetTrigger");
    match &trigger {
        AlertTrigger::OffsetTrigger(t) => {
            assert_eq!(t.at_type, "OffsetTrigger");
            assert_eq!(t.offset, "-PT15M");
            assert_eq!(t.relative_to.as_deref(), Some("start"));
        }
        _ => panic!("expected OffsetTrigger variant"),
    }

    let back = serde_json::to_string(&trigger).expect("serialize");
    let recovered: AlertTrigger = serde_json::from_str(&back).expect("re-deserialize");
    assert_eq!(trigger, recovered);
}

/// AbsoluteTrigger serializes and deserializes correctly.
/// Oracle: RFC 8984 §4.5.2.
#[test]
fn alert_absolute_trigger_roundtrip() {
    let json = r#"{
        "@type": "AbsoluteTrigger",
        "when": "2024-06-15T08:45:00Z"
    }"#;

    let trigger: AlertTrigger = serde_json::from_str(json).expect("AbsoluteTrigger");
    match &trigger {
        AlertTrigger::AbsoluteTrigger(t) => {
            assert_eq!(t.at_type, "AbsoluteTrigger");
            assert_eq!(t.when, "2024-06-15T08:45:00Z");
        }
        _ => panic!("expected AbsoluteTrigger variant"),
    }

    let back = serde_json::to_string(&trigger).expect("serialize");
    let recovered: AlertTrigger = serde_json::from_str(&back).expect("re-deserialize");
    assert_eq!(trigger, recovered);
}

/// An unknown trigger type is preserved via the Unknown variant.
/// Oracle: RFC 8984 §4.5.2 "Implementations MUST preserve unknown trigger types."
#[test]
fn alert_unknown_trigger_roundtrip() {
    let json = r#"{
        "@type": "CustomTrigger",
        "customField": "customValue"
    }"#;

    let trigger: AlertTrigger = serde_json::from_str(json).expect("unknown trigger");
    match &trigger {
        AlertTrigger::Unknown(v) => {
            assert_eq!(v["@type"], "CustomTrigger");
            assert_eq!(v["customField"], "customValue");
        }
        _ => panic!("expected Unknown variant"),
    }

    let back = serde_json::to_string(&trigger).expect("re-serialize");
    let recovered: AlertTrigger = serde_json::from_str(&back).expect("re-deserialize");
    match &recovered {
        AlertTrigger::Unknown(v) => assert_eq!(v["@type"], "CustomTrigger"),
        _ => panic!("round-trip should remain Unknown"),
    }
}

/// Full Alert with OffsetTrigger deserializes correctly.
#[test]
fn alert_with_offset_trigger() {
    let json = r#"{
        "@type": "Alert",
        "trigger": {
            "@type": "OffsetTrigger",
            "offset": "-PT15M"
        },
        "action": "display"
    }"#;

    let alert: Alert = serde_json::from_str(json).expect("Alert");
    assert_eq!(alert.at_type, "Alert");
    assert_eq!(alert.action.as_deref(), Some("display"));
    assert!(alert.acknowledged.is_none());
    match &alert.trigger {
        AlertTrigger::OffsetTrigger(t) => assert_eq!(t.offset, "-PT15M"),
        _ => panic!("expected OffsetTrigger"),
    }
}

// ─── Person ───────────────────────────────────────────────────────────────────

/// Person wire field names match draft §7.
#[test]
fn person_wire_names() {
    let json = r#"{
        "name": "Charlie Brown",
        "email": "charlie@example.com",
        "principalId": "princ-007",
        "calendarAddress": "mailto:charlie@example.com"
    }"#;
    let p: Person = serde_json::from_str(json).expect("Person");
    let out = serde_json::to_value(&p).expect("serialize");
    assert_eq!(out["name"], "Charlie Brown");
    assert_eq!(out["email"], "charlie@example.com");
    assert_eq!(out["principalId"], "princ-007");
    assert_eq!(out["calendarAddress"], "mailto:charlie@example.com");
}

/// Person with null fields serializes correctly (nullable, not absent).
#[test]
fn person_null_fields() {
    let json = r#"{
        "name": "Eve",
        "email": null,
        "principalId": null,
        "calendarAddress": null
    }"#;
    let p: Person = serde_json::from_str(json).expect("Person");
    assert_eq!(p.name, "Eve");
    assert!(p.email.is_none());
    assert!(p.principal_id.is_none());
    assert!(p.calendar_address.is_none());
}

// ─── Id usage in tests ────────────────────────────────────────────────────────

/// Verify Id::from works correctly (used implicitly via serde deserialize above).
#[test]
fn id_from_str_works() {
    let id = Id::from("test-id-123");
    assert_eq!(id.as_ref(), "test-id-123");
}
