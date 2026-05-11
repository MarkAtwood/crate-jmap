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
    Alert, AlertTrigger, BusyPeriod, Calendar, CalendarAlert, CalendarEvent,
    CalendarEventFilterCondition, CalendarEventNotification, CalendarRights,
    CalendarsAccountCapability, CalendarsCapability, IncludeInAvailability, Link, Participant,
    ParticipantIdentity, Person, PrincipalCalendarsCapability, VirtualLocation, JMAP_CALENDARS_URI,
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
    assert!(*cals.values().next().expect("first value"));
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

/// CalendarEvent with recurrenceOverrides round-trips through the typed
/// `HashMap<String, PatchObject>` envelope.
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
    // Verify the key exists in the typed PatchObject envelope.
    let patch = overrides
        .get("2024-06-22T09:00:00")
        .expect("override key present");
    assert_eq!(
        patch.as_map().get("title"),
        Some(&serde_json::Value::String("Rescheduled meeting".to_owned()))
    );
}

/// Wire format for `recurrenceOverrides` is byte-identical between the old
/// `Option<Value>` shape and the new `Option<HashMap<String, PatchObject>>`
/// shape, because `PatchObject` is `#[serde(transparent)]`.
/// Oracle: hand-written JSON literal (independent of the type under test).
#[test]
fn calendar_event_recurrence_overrides_wire_format_is_object_map() {
    let json = r#"{
        "id": "ev-rt-overrides",
        "calendarIds": {"cal-001": true},
        "recurrenceOverrides": {
            "2024-06-22T09:00:00": {"title": "Rescheduled"}
        }
    }"#;
    let ev: CalendarEvent = serde_json::from_str(json).expect("deserialize");
    let out: serde_json::Value = serde_json::to_value(&ev).expect("serialize");
    let expected: serde_json::Value = serde_json::from_str(json).expect("expected as Value");
    // Compare structurally — both serializations carry the same JSON object map.
    assert_eq!(
        out["recurrenceOverrides"], expected["recurrenceOverrides"],
        "wire format must be byte-identical for object-map input"
    );
}

/// CalendarEvent with localizations round-trips through the typed
/// `HashMap<String, PatchObject>` envelope.
/// Oracle: RFC 8984 §4.6 localizations description.
#[test]
fn calendar_event_localizations_passthrough() {
    let json = r#"{
        "id": "ev-loc",
        "calendarIds": {"cal-001": true},
        "title": "Lunch",
        "localizations": {
            "fr": {"title": "Déjeuner"},
            "es": {"title": "Almuerzo"}
        }
    }"#;
    let ev: CalendarEvent = serde_json::from_str(json).expect("localizations CalendarEvent");
    let locs = ev.localizations.as_ref().expect("localizations");
    let fr = locs.get("fr").expect("fr override");
    assert_eq!(
        fr.as_map().get("title"),
        Some(&serde_json::Value::String("Déjeuner".to_owned()))
    );
    let es = locs.get("es").expect("es override");
    assert_eq!(
        es.as_map().get("title"),
        Some(&serde_json::Value::String("Almuerzo".to_owned()))
    );
    // Wire format byte-identical via PatchObject's #[serde(transparent)].
    let out: serde_json::Value = serde_json::to_value(&ev).expect("serialize");
    let expected: serde_json::Value = serde_json::from_str(json).expect("expected as Value");
    assert_eq!(out["localizations"], expected["localizations"]);
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
    assert_eq!(
        patch.as_map().get("title"),
        Some(&serde_json::Value::String("New Title".to_owned()))
    );
    // Wire format byte-identical via PatchObject's #[serde(transparent)].
    let out: serde_json::Value = serde_json::to_value(&n).expect("serialize notification");
    assert_eq!(out["eventPatch"]["title"], "New Title");
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
            assert_eq!(t.offset.as_ref(), "-PT15M");
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
        AlertTrigger::OffsetTrigger(t) => assert_eq!(t.offset.as_ref(), "-PT15M"),
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

// ─── BusyPeriod (draft-ietf-jmap-calendars-26 §2.2) ─────────────────────────

/// Deserialize a BusyPeriod with all fields set.
///
/// Oracle: field types from draft-ietf-jmap-calendars-26 §2.2 field descriptions.
/// busyStatus values: "confirmed", "tentative", "unavailable".
#[test]
fn busy_period_full_deserialize() {
    let json = r#"{
        "utcStart": "2024-06-15T09:00:00Z",
        "utcEnd": "2024-06-15T10:00:00Z",
        "busyStatus": "confirmed",
        "event": {
            "@type": "Event",
            "uid": "evt-001",
            "title": "Team meeting"
        },
        "accountId": "acc-42"
    }"#;

    let bp: BusyPeriod = serde_json::from_str(json).expect("BusyPeriod deserialize");
    assert_eq!(bp.utc_start, "2024-06-15T09:00:00Z");
    assert_eq!(bp.utc_end, "2024-06-15T10:00:00Z");
    assert_eq!(bp.busy_status.as_deref(), Some("confirmed"));
    assert!(bp.event.is_some(), "event must be deserialized");
    assert_eq!(bp.event.as_ref().unwrap()["title"], "Team meeting");
    assert!(bp.account_id.is_some(), "accountId must be present");
    assert_eq!(bp.account_id.as_ref().unwrap().as_ref(), "acc-42");
}

/// Deserialize a BusyPeriod where event and accountId are absent (no detail access).
///
/// Oracle: §2.2 — when showDetails=false or user lacks mayReadItems, event and
/// accountId are null (absent) in the response.
#[test]
fn busy_period_no_detail_access() {
    let json = r#"{
        "utcStart": "2024-06-15T14:00:00Z",
        "utcEnd": "2024-06-15T15:30:00Z",
        "busyStatus": "unavailable"
    }"#;

    let bp: BusyPeriod = serde_json::from_str(json).expect("BusyPeriod minimal deserialize");
    assert_eq!(bp.utc_start, "2024-06-15T14:00:00Z");
    assert_eq!(bp.utc_end, "2024-06-15T15:30:00Z");
    assert_eq!(bp.busy_status.as_deref(), Some("unavailable"));
    assert!(bp.event.is_none());
    assert!(bp.account_id.is_none());

    // Optional fields absent from wire when None.
    let v = serde_json::to_value(&bp).expect("serialize");
    let obj = v.as_object().expect("object");
    assert!(!obj.contains_key("event"), "event must be absent when None");
    assert!(
        !obj.contains_key("accountId"),
        "accountId must be absent when None"
    );
}

/// BusyPeriod round-trip: serialize then re-deserialize, verify equality.
#[test]
fn busy_period_roundtrip() {
    let json = r#"{
        "utcStart": "2024-07-01T08:00:00Z",
        "utcEnd": "2024-07-01T09:00:00Z",
        "busyStatus": "tentative",
        "event": { "@type": "Event", "uid": "rt-evt" },
        "accountId": "acc-rt"
    }"#;

    let original: BusyPeriod = serde_json::from_str(json).expect("first deserialize");
    let serialized = serde_json::to_string(&original).expect("serialize");
    let back: BusyPeriod = serde_json::from_str(&serialized).expect("re-deserialize");

    assert_eq!(original.utc_start, back.utc_start);
    assert_eq!(original.utc_end, back.utc_end);
    assert_eq!(original.busy_status, back.busy_status);
    assert_eq!(original.account_id, back.account_id);
    assert_eq!(original.event, back.event);
}

/// Wire key names for BusyPeriod match spec camelCase names.
#[test]
fn busy_period_wire_names() {
    let json = r#"{
        "utcStart": "2024-08-01T10:00:00Z",
        "utcEnd": "2024-08-01T11:00:00Z",
        "busyStatus": "confirmed",
        "accountId": "acc-wn"
    }"#;
    let bp: BusyPeriod = serde_json::from_str(json).expect("deserialize");
    let v = serde_json::to_value(&bp).expect("serialize");
    let obj = v.as_object().expect("object");

    assert!(obj.contains_key("utcStart"), "wire key must be utcStart");
    assert!(obj.contains_key("utcEnd"), "wire key must be utcEnd");
    assert!(
        obj.contains_key("busyStatus"),
        "wire key must be busyStatus"
    );
    assert!(obj.contains_key("accountId"), "wire key must be accountId");
}

// ─── PrincipalCalendarsCapability (calendars-26 §2.1) ────────────────────────

/// Deserialize PrincipalCalendarsCapability with accountId populated.
///
/// Oracle: field names from draft-ietf-jmap-calendars-26 §2.1 field descriptions.
#[test]
fn principal_calendars_capability_with_account_id() {
    let json = r#"{
        "accountId": "acc-alice",
        "mayGetAvailability": true,
        "mayShareWith": true,
        "calendarAddress": "mailto:alice@example.com"
    }"#;

    let cap: PrincipalCalendarsCapability =
        serde_json::from_str(json).expect("PrincipalCalendarsCapability deserialize");
    assert_eq!(
        cap.account_id.as_ref().map(|id| id.as_ref()),
        Some("acc-alice")
    );
    assert!(cap.may_get_availability);
    assert!(cap.may_share_with);
    assert_eq!(cap.calendar_address, "mailto:alice@example.com");
}

/// Deserialize PrincipalCalendarsCapability with accountId null.
///
/// Oracle: §2.1 — accountId is `Id|null`; null when the principal has no
/// accessible calendar account.
#[test]
fn principal_calendars_capability_account_id_null() {
    let json = r#"{
        "accountId": null,
        "mayGetAvailability": false,
        "mayShareWith": false,
        "calendarAddress": "mailto:service@example.com"
    }"#;

    let cap: PrincipalCalendarsCapability =
        serde_json::from_str(json).expect("deserialize with null accountId");
    assert!(cap.account_id.is_none(), "accountId must be None for null");
    assert!(!cap.may_get_availability);
    assert!(!cap.may_share_with);
}

/// accountId serializes as null (not absent) when None.
///
/// Oracle: §2.1 types accountId as `Id|null` — required-nullable, must always
/// appear on the wire.
#[test]
fn principal_calendars_capability_account_id_null_serializes() {
    let json = r#"{
        "accountId": null,
        "mayGetAvailability": false,
        "mayShareWith": false,
        "calendarAddress": "mailto:noaccount@example.com"
    }"#;
    let cap: PrincipalCalendarsCapability = serde_json::from_str(json).expect("deserialize");
    let v = serde_json::to_value(&cap).expect("serialize");
    let obj = v.as_object().expect("object");

    assert!(
        obj.contains_key("accountId"),
        "accountId must be present in wire JSON (required-nullable)"
    );
    assert!(
        obj["accountId"].is_null(),
        "accountId must serialize as null"
    );
}

/// PrincipalCalendarsCapability round-trip.
#[test]
fn principal_calendars_capability_roundtrip() {
    let json = r#"{
        "accountId": "acc-rt",
        "mayGetAvailability": true,
        "mayShareWith": false,
        "calendarAddress": "mailto:rt@example.com"
    }"#;

    let original: PrincipalCalendarsCapability =
        serde_json::from_str(json).expect("first deserialize");
    let serialized = serde_json::to_string(&original).expect("serialize");
    let back: PrincipalCalendarsCapability =
        serde_json::from_str(&serialized).expect("re-deserialize");

    assert_eq!(original.account_id, back.account_id);
    assert_eq!(original.may_get_availability, back.may_get_availability);
    assert_eq!(original.may_share_with, back.may_share_with);
    assert_eq!(original.calendar_address, back.calendar_address);
}

// ─── Participant — scheduleSequence / scheduleUpdated (RFC 8984 §5.2) ────────

/// Verify Participant carries scheduleSequence and scheduleUpdated.
///
/// Oracle: RFC 8984 §5.2.1 / §5.2.2 — both fields have Context: Participant.
/// JSON constructed from the spec field descriptions: scheduleSequence is a
/// UnsignedInt, scheduleUpdated is a UTCDateTime string.
#[test]
fn participant_schedule_fields_roundtrip() {
    let json = r#"{
        "@type": "Participant",
        "email": "alice@example.com",
        "roles": { "attendee": true },
        "scheduleSequence": 3,
        "scheduleUpdated": "2024-06-15T10:30:00Z"
    }"#;

    let p: Participant = serde_json::from_str(json).expect("Participant deserialize");
    assert_eq!(p.schedule_sequence, Some(3));
    // schedule_updated is Option<UTCDate>; UTCDate does not impl Deref<Target=str>
    // so we cannot use Option::as_deref. Take an AsRef<str> view via map instead.
    assert_eq!(
        p.schedule_updated.as_ref().map(AsRef::as_ref),
        Some("2024-06-15T10:30:00Z")
    );

    // Round-trip: serialize then re-deserialize.
    let serialized = serde_json::to_string(&p).expect("serialize");
    let back: Participant = serde_json::from_str(&serialized).expect("re-deserialize");
    assert_eq!(back.schedule_sequence, Some(3));
    assert_eq!(
        back.schedule_updated.as_ref().map(AsRef::as_ref),
        Some("2024-06-15T10:30:00Z")
    );

    // Wire names: camelCase.
    let v: serde_json::Value = serde_json::from_str(&serialized).expect("parse");
    assert_eq!(v["scheduleSequence"], serde_json::json!(3_u64));
    assert_eq!(
        v["scheduleUpdated"],
        serde_json::json!("2024-06-15T10:30:00Z")
    );
}

/// Participant with no schedule fields omits them from wire (optional).
///
/// Oracle: RFC 8984 §5.2 — these fields are optional per-participant iTIP
/// tracking fields; absent when not involved in scheduling.
#[test]
fn participant_schedule_fields_absent_when_none() {
    let json = r#"{
        "@type": "Participant",
        "email": "bob@example.com",
        "roles": { "attendee": true }
    }"#;

    let p: Participant = serde_json::from_str(json).expect("Participant deserialize");
    assert_eq!(p.schedule_sequence, None);
    assert_eq!(p.schedule_updated, None);
    assert!(
        p.schedule_status.is_none(),
        "scheduleStatus must be None when absent"
    );

    let serialized = serde_json::to_string(&p).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&serialized).expect("parse");
    let obj = v.as_object().expect("object");
    assert!(
        !obj.contains_key("scheduleSequence"),
        "scheduleSequence must be absent when None"
    );
    assert!(
        !obj.contains_key("scheduleUpdated"),
        "scheduleUpdated must be absent when None"
    );
    assert!(
        !obj.contains_key("scheduleStatus"),
        "scheduleStatus must be absent when None"
    );
}

// ─── Id usage in tests ────────────────────────────────────────────────────────

/// Verify Id::from works correctly (used implicitly via serde deserialize above).
#[test]
fn id_from_str_works() {
    let id = Id::from("test-id-123");
    assert_eq!(id.as_ref(), "test-id-123");
}

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

#[test]
fn link_with_href_roundtrip() {
    // Oracle: RFC 8984 §1.4.11 field definitions.
    let json = r#"{"@type":"Link","href":"https://example.com/file.pdf","contentType":"application/pdf","size":4096,"rel":"enclosure","display":"report.pdf"}"#;
    let link: Link =
        serde_json::from_str(json).expect("link_with_href_roundtrip: must deserialize");
    assert_eq!(link.at_type, "Link");
    assert_eq!(link.href.as_deref(), Some("https://example.com/file.pdf"));
    assert_eq!(link.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(link.size, Some(4096));
    assert_eq!(link.rel.as_deref(), Some("enclosure"));
    assert_eq!(link.display.as_deref(), Some("report.pdf"));
    assert!(
        link.blob_id.is_none(),
        "blob_id must be absent when not in JSON"
    );

    // Verify blobId absent from serialized output.
    let out = serde_json::to_string(&link).expect("link_with_href_roundtrip: must serialize");
    assert!(
        !out.contains("blobId"),
        "blobId must not appear when None: {out}"
    );
}

#[test]
fn link_with_blob_id_roundtrip() {
    // Oracle: draft-ietf-jmap-calendars-26 §5.3 — blobId may be set instead of href.
    let json = r#"{"@type":"Link","blobId":"blob-abc123","contentType":"image/png","size":2048,"rel":"enclosure"}"#;
    let link: Link =
        serde_json::from_str(json).expect("link_with_blob_id_roundtrip: must deserialize");
    assert_eq!(link.blob_id, Some(Id::from("blob-abc123")));
    assert!(link.href.is_none(), "href must be absent when not in JSON");

    // Verify blobId is present in serialized output but href is not.
    let out = serde_json::to_value(&link).expect("link_with_blob_id_roundtrip: must serialize");
    assert_eq!(
        out["blobId"], "blob-abc123",
        "blobId must serialize to wire name"
    );
    assert!(
        out.get("href").is_none() || out["href"].is_null(),
        "href must be absent when None"
    );
}

#[test]
fn link_blob_id_absent_when_none() {
    // Oracle: skip_serializing_if = Option::is_none — optional field absent when not set.
    // Construct via deserialization (Link is #[non_exhaustive]).
    let json = r#"{"@type":"Link","href":"https://example.com/doc.pdf"}"#;
    let link: Link =
        serde_json::from_str(json).expect("link_blob_id_absent_when_none: must deserialize");
    assert!(
        link.blob_id.is_none(),
        "blob_id must be None when absent from JSON"
    );
    let out = serde_json::to_string(&link).expect("link_blob_id_absent_when_none: must serialize");
    assert!(
        !out.contains("blobId"),
        "blobId must not appear when None: {out}"
    );
}

#[test]
fn link_wire_names() {
    // Oracle: #[serde(rename_all = "camelCase")] on Link struct.
    // All camelCase wire keys must be present; snake_case must not appear.
    let json = r#"{"@type":"Link","href":"https://example.com/","contentType":"text/html","size":100,"rel":"describedby","display":"Page","blobId":"b1"}"#;
    let link: Link = serde_json::from_str(json).expect("link_wire_names: must deserialize");
    let out = serde_json::to_value(&link).expect("link_wire_names: must serialize");
    // camelCase keys must be present
    assert!(
        out.get("blobId").is_some(),
        "blobId must be camelCase wire key"
    );
    assert!(
        out.get("contentType").is_some(),
        "contentType must be camelCase wire key"
    );
    assert!(out.get("rel").is_some(), "rel must be present");
    assert!(out.get("display").is_some(), "display must be present");
    // snake_case must NOT appear
    assert!(
        out.get("blob_id").is_none(),
        "snake_case blob_id must not appear on wire"
    );
    assert!(
        out.get("content_type").is_none(),
        "snake_case content_type must not appear on wire"
    );
}

// ---------------------------------------------------------------------------
// CalendarAlert
// ---------------------------------------------------------------------------

#[test]
fn calendar_alert_with_recurrence_id_roundtrip() {
    // Oracle: draft-ietf-jmap-calendars-26 §6.4 CalendarAlert push-notification object.
    // A recurring-event alert: recurrenceId is present and non-null.
    let json = r#"{"@type":"CalendarAlert","accountId":"acc-1","calendarEventId":"ev-42","uid":"abc-uid-123","recurrenceId":"2024-06-15T10:00:00","alertId":"alert-1"}"#;
    let alert: CalendarAlert = serde_json::from_str(json)
        .expect("calendar_alert_with_recurrence_id_roundtrip: must deserialize");
    assert_eq!(
        alert.at_type, "CalendarAlert",
        "@type field must be 'CalendarAlert'"
    );
    assert_eq!(alert.account_id.as_ref(), "acc-1");
    assert_eq!(alert.calendar_event_id.as_ref(), "ev-42");
    assert_eq!(alert.uid, "abc-uid-123");
    assert_eq!(
        alert.recurrence_id.as_deref(),
        Some("2024-06-15T10:00:00"),
        "recurrenceId must be present for recurring events"
    );
    assert_eq!(alert.alert_id, "alert-1");

    // Round-trip: serialize and re-deserialize.
    let serialized = serde_json::to_string(&alert)
        .expect("calendar_alert_with_recurrence_id_roundtrip: must serialize");
    let recovered: CalendarAlert = serde_json::from_str(&serialized)
        .expect("calendar_alert_with_recurrence_id_roundtrip: round-trip must deserialize");
    assert_eq!(alert, recovered);
}

#[test]
fn calendar_alert_recurrence_id_null_for_non_recurring() {
    // Oracle: draft-ietf-jmap-calendars-26 §6.4 — recurrenceId is null for non-recurring events.
    // It MUST serialize as null (not be omitted) so receivers can distinguish
    // recurring from non-recurring events.
    let json = r#"{"@type":"CalendarAlert","accountId":"acc-2","calendarEventId":"ev-99","uid":"xyz-uid-999","recurrenceId":null,"alertId":"alert-2"}"#;
    let alert: CalendarAlert = serde_json::from_str(json)
        .expect("calendar_alert_recurrence_id_null_for_non_recurring: must deserialize");
    assert!(
        alert.recurrence_id.is_none(),
        "recurrenceId must be None when null"
    );

    // Verify it serializes back as null (not omitted).
    let out = serde_json::to_value(&alert)
        .expect("calendar_alert_recurrence_id_null_for_non_recurring: must serialize");
    assert!(
        out.get("recurrenceId").is_some(),
        "recurrenceId key must be present in serialized output (not omitted)"
    );
    assert!(
        out["recurrenceId"].is_null(),
        "recurrenceId must serialize as null for non-recurring events"
    );
}

#[test]
fn calendar_alert_wire_names() {
    // Oracle: #[serde(rename_all = "camelCase")] + #[serde(rename = "@type")] on CalendarAlert.
    // All camelCase wire keys must be present; snake_case must not appear.
    let json = r#"{"@type":"CalendarAlert","accountId":"A1","calendarEventId":"EV1","uid":"uid-1","recurrenceId":"2024-01-01T09:00:00","alertId":"al-1"}"#;
    let alert: CalendarAlert =
        serde_json::from_str(json).expect("calendar_alert_wire_names: must deserialize");
    let out = serde_json::to_value(&alert).expect("calendar_alert_wire_names: must serialize");
    // Required camelCase wire keys
    assert!(out.get("@type").is_some(), "@type must be present");
    assert!(
        out.get("accountId").is_some(),
        "accountId must be camelCase"
    );
    assert!(
        out.get("calendarEventId").is_some(),
        "calendarEventId must be camelCase"
    );
    assert!(out.get("alertId").is_some(), "alertId must be camelCase");
    // snake_case must NOT appear on wire
    assert!(
        out.get("account_id").is_none(),
        "account_id must not appear on wire"
    );
    assert!(
        out.get("calendar_event_id").is_none(),
        "calendar_event_id must not appear on wire"
    );
    assert!(
        out.get("alert_id").is_none(),
        "alert_id must not appear on wire"
    );
    assert!(
        out.get("at_type").is_none(),
        "at_type must not appear on wire (must be @type)"
    );
}

// ---------------------------------------------------------------------------
// VirtualLocation — mandatory uri field (RFC 8984 §4.2.6)
// ---------------------------------------------------------------------------

#[test]
fn virtual_location_uri_required() {
    // Oracle: RFC 8984 §4.2.6 — uri is a mandatory String field.
    // A well-formed VirtualLocation must include uri; deserialization must succeed
    // and preserve the uri value.
    let json = r#"{"@type":"VirtualLocation","name":"Team Call","uri":"https://meet.example.com/room-42","features":{"video":true}}"#;
    let vl: VirtualLocation =
        serde_json::from_str(json).expect("virtual_location_uri_required: must deserialize");
    assert_eq!(
        vl.uri, "https://meet.example.com/room-42",
        "uri must round-trip correctly"
    );
    assert_eq!(vl.name.as_deref(), Some("Team Call"));
}

#[test]
fn virtual_location_uri_missing_fails_deserialization() {
    // Oracle: RFC 8984 §4.2.6 — uri is mandatory; a VirtualLocation with no uri
    // must fail deserialization (the crate uses String, not Option<String>).
    let json = r#"{"@type":"VirtualLocation","name":"Nameless Location"}"#;
    let result: Result<VirtualLocation, _> = serde_json::from_str(json);
    assert!(
        result.is_err(),
        "VirtualLocation without uri must fail deserialization (uri is mandatory per RFC 8984 §4.2.6)"
    );
}

#[test]
fn virtual_location_wire_names() {
    // Oracle: #[serde(rename_all = "camelCase")] on VirtualLocation.
    let json = r#"{"@type":"VirtualLocation","uri":"https://meet.example.com/xyz","features":{"audio":true}}"#;
    let vl: VirtualLocation =
        serde_json::from_str(json).expect("virtual_location_wire_names: must deserialize");
    let out = serde_json::to_value(&vl).expect("virtual_location_wire_names: must serialize");
    assert!(out.get("uri").is_some(), "uri must be present on wire");
    assert!(out.get("@type").is_some(), "@type must be present on wire");
    // snake_case must not appear
    assert!(
        out.get("at_type").is_none(),
        "at_type must not appear on wire"
    );
}

// ---------------------------------------------------------------------------
// Link.cid and Link.title fields (RFC 8984 §1.4.11)
// ---------------------------------------------------------------------------

#[test]
fn link_cid_roundtrip() {
    // Oracle: RFC 8984 §1.4.11 — cid is an optional field for inline image
    // references in text/html event descriptions via cid: URLs.
    let json = r#"{"@type":"Link","href":"https://example.com/logo.png","cid":"logo@example.com","contentType":"image/png"}"#;
    let link: Link = serde_json::from_str(json).expect("link_cid_roundtrip: must deserialize");
    assert_eq!(
        link.cid.as_deref(),
        Some("logo@example.com"),
        "cid must round-trip correctly"
    );
    // Verify wire name is "cid" (camelCase would be identical here).
    let out = serde_json::to_value(&link).expect("link_cid_roundtrip: must serialize");
    assert_eq!(
        out["cid"], "logo@example.com",
        "cid wire name must be 'cid'"
    );
}

#[test]
fn link_cid_absent_when_none() {
    // Oracle: skip_serializing_if = Option::is_none — cid must be absent when not set.
    let json = r#"{"@type":"Link","href":"https://example.com/doc.pdf"}"#;
    let link: Link =
        serde_json::from_str(json).expect("link_cid_absent_when_none: must deserialize");
    assert!(link.cid.is_none(), "cid must be None when absent from JSON");
    let out = serde_json::to_string(&link).expect("link_cid_absent_when_none: must serialize");
    assert!(
        !out.contains("\"cid\""),
        "cid must not appear in output when None: {out}"
    );
}

#[test]
fn link_title_roundtrip() {
    // Oracle: RFC 8984 §1.4.11 — title is an optional human-readable description
    // of the linked resource, distinct from display (file name).
    let json = r#"{"@type":"Link","href":"https://example.com/report.pdf","title":"Annual Report 2024","display":"report.pdf"}"#;
    let link: Link = serde_json::from_str(json).expect("link_title_roundtrip: must deserialize");
    assert_eq!(
        link.title.as_deref(),
        Some("Annual Report 2024"),
        "title must round-trip correctly"
    );
    let out = serde_json::to_value(&link).expect("link_title_roundtrip: must serialize");
    assert_eq!(
        out["title"], "Annual Report 2024",
        "title wire name must be 'title'"
    );
}

#[test]
fn link_title_absent_when_none() {
    // Oracle: skip_serializing_if = Option::is_none — title must be absent when not set.
    let json = r#"{"@type":"Link","href":"https://example.com/"}"#;
    let link: Link =
        serde_json::from_str(json).expect("link_title_absent_when_none: must deserialize");
    assert!(
        link.title.is_none(),
        "title must be None when absent from JSON"
    );
    let out = serde_json::to_string(&link).expect("link_title_absent_when_none: must serialize");
    assert!(
        !out.contains("\"title\""),
        "title must not appear in output when None: {out}"
    );
}

// ---------------------------------------------------------------------------
// Participant.scheduleStatus (RFC 8984 §4.4.6)
// ---------------------------------------------------------------------------

#[test]
fn participant_schedule_status_roundtrip() {
    // Oracle: RFC 8984 §4.4.6 — scheduleStatus is String[] (optional),
    // contains iTIP status codes (e.g. "1.0", "2.0").
    let json =
        r#"{"@type":"Participant","roles":{"attendee":true},"scheduleStatus":["1.0","2.0"]}"#;
    let p: Participant = serde_json::from_str(json)
        .expect("participant_schedule_status_roundtrip: must deserialize");
    let status = p
        .schedule_status
        .as_ref()
        .expect("scheduleStatus must be Some");
    assert_eq!(status.len(), 2, "must have 2 status codes");
    assert_eq!(status[0], "1.0");
    assert_eq!(status[1], "2.0");

    let out =
        serde_json::to_value(&p).expect("participant_schedule_status_roundtrip: must serialize");
    assert_eq!(
        out["scheduleStatus"][0], "1.0",
        "scheduleStatus[0] must be '1.0'"
    );
    assert_eq!(
        out["scheduleStatus"][1], "2.0",
        "scheduleStatus[1] must be '2.0'"
    );
}

// ---------------------------------------------------------------------------
// CalendarEvent.iCalComponent (calendars-26 §5.7)
// ---------------------------------------------------------------------------

#[test]
fn calendar_event_ical_component_roundtrip() {
    // Oracle: draft-ietf-jmap-calendars-26 §5.7 — iCalComponent is returned
    // only when explicitly requested. The value is base64-encoded iCalendar data.
    let sample_base64 = "QkVHSU46VkNBTEVOREFSCk...(truncated)..."; // not real base64, just a string value
    let json = format!(r#"{{"id":"ev1","iCalComponent":"{sample_base64}"}}"#);
    let event: CalendarEvent = serde_json::from_str(&json)
        .expect("calendar_event_ical_component_roundtrip: must deserialize");
    assert_eq!(
        event.ical_component.as_deref(),
        Some(sample_base64),
        "iCalComponent must round-trip correctly"
    );

    let out = serde_json::to_value(&event)
        .expect("calendar_event_ical_component_roundtrip: must serialize");
    assert_eq!(
        out["iCalComponent"], sample_base64,
        "iCalComponent wire name must be 'iCalComponent'"
    );
}

#[test]
fn calendar_event_ical_component_absent_when_none() {
    // Oracle: skip_serializing_if = Option::is_none — iCalComponent must be absent
    // from the wire when not explicitly set (it is never returned by default).
    let json = r#"{"id":"ev1"}"#;
    let event: CalendarEvent = serde_json::from_str(json)
        .expect("calendar_event_ical_component_absent_when_none: must deserialize");
    assert!(
        event.ical_component.is_none(),
        "iCalComponent must be None when absent from JSON"
    );
    let out = serde_json::to_string(&event)
        .expect("calendar_event_ical_component_absent_when_none: must serialize");
    assert!(
        !out.contains("iCalComponent"),
        "iCalComponent must not appear in output when None: {out}"
    );
}

// ── Extras-preservation policy tests (JMAP-lbdy.4) ──────────────────────────
//
// One round-trip preservation test per migrated type. Each asserts that
// an unknown vendor / site / private-extension field survives
// deserialize/serialize unchanged. Per workspace AGENTS.md
// "Extras-preservation policy for vendor/site fields".

/// `BusyPeriod.extra` captures vendor fields and preserves them.
#[test]
fn busy_period_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "utcStart": "2024-06-01T09:00:00Z",
        "utcEnd": "2024-06-01T10:00:00Z",
        "acmeCorpFreeBusyTag": "client-meeting"
    });
    let b: BusyPeriod = serde_json::from_value(raw).unwrap();
    assert_eq!(
        b.extra.get("acmeCorpFreeBusyTag").and_then(|v| v.as_str()),
        Some("client-meeting")
    );
    let back = serde_json::to_value(&b).unwrap();
    assert_eq!(back["acmeCorpFreeBusyTag"], "client-meeting");
}

/// `CalendarRights.extra` captures vendor fields and preserves them.
#[test]
fn calendar_rights_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "mayReadFreeBusy": true,
        "mayReadItems": true,
        "mayWriteAll": false,
        "mayWriteOwn": true,
        "mayUpdatePrivate": false,
        "mayRSVP": true,
        "mayShare": false,
        "mayDelete": false,
        "acmeCorpMayPublish": false
    });
    let r: CalendarRights = serde_json::from_value(raw).unwrap();
    assert_eq!(
        r.extra.get("acmeCorpMayPublish").and_then(|v| v.as_bool()),
        Some(false)
    );
    let back = serde_json::to_value(&r).unwrap();
    assert_eq!(back["acmeCorpMayPublish"], false);
}

/// `Calendar.extra` captures vendor fields and preserves them.
#[test]
fn calendar_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "id": "c1",
        "name": "Work",
        "description": null,
        "color": null,
        "sortOrder": 0,
        "isSubscribed": true,
        "isVisible": true,
        "isDefault": false,
        "includeInAvailability": "all",
        "defaultAlertsWithTime": null,
        "defaultAlertsWithoutTime": null,
        "timeZone": null,
        "shareWith": null,
        "myRights": {
            "mayReadFreeBusy": true, "mayReadItems": true, "mayWriteAll": true,
            "mayWriteOwn": true, "mayUpdatePrivate": true, "mayRSVP": true,
            "mayShare": true, "mayDelete": true
        },
        "acmeCorpDepartment": "engineering"
    });
    let c: Calendar = serde_json::from_value(raw).unwrap();
    assert_eq!(
        c.extra.get("acmeCorpDepartment").and_then(|v| v.as_str()),
        Some("engineering")
    );
    let back = serde_json::to_value(&c).unwrap();
    assert_eq!(back["acmeCorpDepartment"], "engineering");
}

/// `CalendarEvent.extra` captures vendor fields and preserves them.
#[test]
fn calendar_event_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "@type": "Event",
        "uid": "event-1",
        "title": "Meeting",
        "start": "2024-06-01T10:00:00",
        "duration": "PT1H",
        "acmeCorpMeetingNotes": "https://wiki/n/42"
    });
    let e: CalendarEvent = serde_json::from_value(raw).unwrap();
    assert_eq!(
        e.extra.get("acmeCorpMeetingNotes").and_then(|v| v.as_str()),
        Some("https://wiki/n/42")
    );
    let back = serde_json::to_value(&e).unwrap();
    assert_eq!(back["acmeCorpMeetingNotes"], "https://wiki/n/42");
}

/// `Person.extra` captures vendor fields and preserves them.
#[test]
fn person_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "name": "Alice",
        "email": "alice@example.com",
        "principalId": null,
        "calendarAddress": null,
        "acmeCorpEmployeeId": "emp-42"
    });
    let p: Person = serde_json::from_value(raw).unwrap();
    assert_eq!(
        p.extra.get("acmeCorpEmployeeId").and_then(|v| v.as_str()),
        Some("emp-42")
    );
    let back = serde_json::to_value(&p).unwrap();
    assert_eq!(back["acmeCorpEmployeeId"], "emp-42");
}

/// `CalendarEventNotification.extra` captures vendor fields and preserves them.
#[test]
fn calendar_event_notification_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "id": "n1",
        "created": "2024-06-01T00:00:00Z",
        "changedBy": {
            "name": "Alice",
            "email": null,
            "principalId": null,
            "calendarAddress": null
        },
        "type": "created",
        "calendarEventId": "e1",
        "event": {},
        "acmeCorpNotificationChannel": "in-app"
    });
    let n: CalendarEventNotification = serde_json::from_value(raw).unwrap();
    assert_eq!(
        n.extra
            .get("acmeCorpNotificationChannel")
            .and_then(|v| v.as_str()),
        Some("in-app")
    );
    let back = serde_json::to_value(&n).unwrap();
    assert_eq!(back["acmeCorpNotificationChannel"], "in-app");
}

/// `CalendarAlert.extra` captures vendor fields and preserves them.
#[test]
fn calendar_alert_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "@type": "CalendarAlert",
        "accountId": "a1",
        "calendarEventId": "e1",
        "uid": "uid-1",
        "recurrenceId": null,
        "alertId": "alrt-1",
        "acmeCorpPushPriority": "high"
    });
    let a: CalendarAlert = serde_json::from_value(raw).unwrap();
    assert_eq!(
        a.extra.get("acmeCorpPushPriority").and_then(|v| v.as_str()),
        Some("high")
    );
    let back = serde_json::to_value(&a).unwrap();
    assert_eq!(back["acmeCorpPushPriority"], "high");
}

/// `ParticipantIdentity.extra` captures vendor fields and preserves them.
#[test]
fn participant_identity_preserves_vendor_extras() {
    let raw = serde_json::json!({
        "id": "pi1",
        "name": "Alice",
        "calendarAddress": "mailto:alice@example.com",
        "isDefault": true,
        "acmeCorpDeliveryHint": "external"
    });
    let pi: ParticipantIdentity = serde_json::from_value(raw).unwrap();
    assert_eq!(
        pi.extra
            .get("acmeCorpDeliveryHint")
            .and_then(|v| v.as_str()),
        Some("external")
    );
    let back = serde_json::to_value(&pi).unwrap();
    assert_eq!(back["acmeCorpDeliveryHint"], "external");
}
