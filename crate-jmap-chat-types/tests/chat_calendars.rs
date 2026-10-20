//! draft-atwood-jmap-chat-calendars-00 types: capability, `Space.calendarId`
//! accessor, and the calendar-event / availability MessageAction metadata.
//!
//! Oracle: sentences quoted from
//! /home/mark/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-calendars-00.md
//! (line numbers at HEAD) and hand-written JSON built from the field names
//! they define.

use jmap_chat_types::{
    AvailabilityMetadata, CalendarEventMetadata, ChatCalendarsCapability, LookupWindow,
    MessageAction, Space, JMAP_CHAT_CALENDARS_URI, SPACE_CALENDAR_ID_PROPERTY,
};
use serde_json::{json, Value};

#[test]
fn capability_uri_matches_draft() {
    // L87: "Servers supporting this specification MUST advertise the
    // `urn:ietf:params:jmap:chat:calendars` capability in the JMAP Session
    // object."
    assert_eq!(
        JMAP_CHAT_CALENDARS_URI,
        "urn:ietf:params:jmap:chat:calendars"
    );
}

#[test]
fn account_capability_round_trips_three_booleans() {
    // L95-104: the account-level object has `mayBindCalendar`,
    // `supportsIcsParsing` and `supportsAvailabilityLookup`, each "(Boolean)".
    let raw = json!({
        "mayBindCalendar": true,
        "supportsIcsParsing": false,
        "supportsAvailabilityLookup": true
    });
    let cap: ChatCalendarsCapability = serde_json::from_value(raw.clone()).expect("deserialize");
    assert!(cap.may_bind_calendar);
    assert!(!cap.supports_ics_parsing);
    assert!(cap.supports_availability_lookup);
    assert!(cap.extra.is_empty());
    assert_eq!(serde_json::to_value(&cap).expect("serialize"), raw);
}

#[test]
fn account_capability_requires_each_boolean() {
    // L95-104 list all three fields without marking any optional.
    let full = json!({
        "mayBindCalendar": false,
        "supportsIcsParsing": false,
        "supportsAvailabilityLookup": false
    });
    for key in [
        "mayBindCalendar",
        "supportsIcsParsing",
        "supportsAvailabilityLookup",
    ] {
        let mut raw = full.clone();
        raw.as_object_mut().expect("object").remove(key);
        assert!(
            serde_json::from_value::<ChatCalendarsCapability>(raw).is_err(),
            "{key} missing must not deserialize"
        );
    }
}

#[test]
fn account_capability_preserves_vendor_fields() {
    let raw = json!({
        "mayBindCalendar": true,
        "supportsIcsParsing": false,
        "supportsAvailabilityLookup": false,
        "acmeCorpTier": "gold"
    });
    let cap: ChatCalendarsCapability = serde_json::from_value(raw.clone()).expect("deserialize");
    assert_eq!(serde_json::to_value(&cap).expect("serialize"), raw);
}

/// A Space with the draft-atwood-jmap-chat-00 core properties plus `ext`.
fn space(ext: Value) -> Value {
    let mut v = json!({
        "id": "S1",
        "name": "Team",
        "roles": [],
        "members": [],
        "categories": [],
        "uncategorizedChannelIds": [],
        "createdAt": "2026-10-20T00:00:00Z",
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "memberCount": 0
    });
    v.as_object_mut()
        .expect("object")
        .extend(ext.as_object().expect("object").clone());
    v
}

#[test]
fn calendar_id_property_name_matches_draft() {
    // L111: "`calendarId` (String, optional)".
    assert_eq!(SPACE_CALENDAR_ID_PROPERTY, "calendarId");
}

#[test]
fn calendar_id_present_round_trips() {
    // L111-112: "A JMAP Calendars `Calendar.id` ... bound to this Space."
    let raw = space(json!({ "calendarId": "cal1" }));
    let s: Space = serde_json::from_value(raw.clone()).expect("deserialize");
    assert_eq!(
        s.calendar_id().expect("string").as_ref().map(AsRef::as_ref),
        Some("cal1")
    );
    assert_eq!(serde_json::to_value(&s).expect("serialize"), raw);
}

#[test]
fn calendar_id_absent_or_null_means_unbound() {
    // L112: "When absent, this Space has no bound Calendar."
    // L118: "setting it to `null` unbinds the Space."
    for raw in [space(json!({})), space(json!({ "calendarId": null }))] {
        let s: Space = serde_json::from_value(raw).expect("deserialize");
        assert_eq!(s.calendar_id().expect("absent or null"), None);
    }
}

#[test]
fn calendar_id_non_string_is_an_error() {
    // L111: "`calendarId` (String, optional)".
    let s: Space = serde_json::from_value(space(json!({ "calendarId": 7 }))).expect("deserialize");
    assert!(s.calendar_id().is_err());
}

#[test]
fn calendar_event_metadata_round_trips_draft_keys() {
    // L161-166: `metadata` "MAY include the following type-specific keys":
    // `startsAt` (UTCDate), `endsAt` (UTCDate), `location` (String),
    // `participantCount` (UnsignedInt).
    let action = json!({
        "type": "urn:jmap:chat:cap:calendar-event",
        "uri": "jmap:calendarevent:A1:E42",
        "label": "Sprint review",
        "metadata": {
            "startsAt": "2026-10-21T16:00:00Z",
            "endsAt": "2026-10-21T17:00:00Z",
            "location": "Room 4",
            "participantCount": 12
        }
    });
    let a: MessageAction = serde_json::from_value(action.clone()).expect("action");
    let m: CalendarEventMetadata =
        serde_json::from_value(a.metadata.clone().expect("metadata")).expect("metadata shape");
    assert_eq!(
        m.starts_at.as_ref().map(AsRef::as_ref),
        Some("2026-10-21T16:00:00Z")
    );
    assert_eq!(
        m.ends_at.as_ref().map(AsRef::as_ref),
        Some("2026-10-21T17:00:00Z")
    );
    assert_eq!(m.location.as_deref(), Some("Room 4"));
    assert_eq!(m.participant_count, Some(12));
    assert!(m.extra.is_empty());
    assert_eq!(
        serde_json::to_value(&m).expect("serialize"),
        action["metadata"]
    );
}

#[test]
fn calendar_event_metadata_keys_are_optional() {
    // L162: "MAY include the following type-specific keys".
    let m: CalendarEventMetadata = serde_json::from_value(json!({})).expect("empty");
    assert_eq!(m, CalendarEventMetadata::default());
    assert_eq!(serde_json::to_value(&m).expect("serialize"), json!({}));
}

#[test]
fn calendar_event_metadata_keeps_unknown_keys() {
    // L167: "Servers MAY include other type-specific metadata keys; clients
    // MUST ignore unknown keys."
    let raw = json!({ "startsAt": "2026-10-21T16:00:00Z", "acmeRoomCode": "R4" });
    let m: CalendarEventMetadata = serde_json::from_value(raw.clone()).expect("deserialize");
    assert_eq!(m.extra.get("acmeRoomCode"), Some(&json!("R4")));
    assert_eq!(serde_json::to_value(&m).expect("serialize"), raw);
}

#[test]
fn calendar_event_participant_count_is_unsigned() {
    // L166: "`participantCount` (UnsignedInt)".
    assert!(
        serde_json::from_value::<CalendarEventMetadata>(json!({ "participantCount": -1 })).is_err()
    );
}

#[test]
fn availability_metadata_round_trips_lookup_window() {
    // L240-252: for `urn:jmap:chat:cap:availability`, `uri` is
    // `jmap:principal:<accountId>:<principalId>`, and `metadata` "MAY
    // include `lookupWindow` (Object, with `startsAt` and `endsAt`
    // fields)".
    let action = json!({
        "type": "urn:jmap:chat:cap:availability",
        "uri": "jmap:principal:A1:P7",
        "label": "Alice",
        "metadata": {
            "lookupWindow": {
                "startsAt": "2026-10-22T09:00:00Z",
                "endsAt": "2026-10-22T17:00:00Z"
            }
        }
    });
    let a: MessageAction = serde_json::from_value(action.clone()).expect("action");
    let m: AvailabilityMetadata =
        serde_json::from_value(a.metadata.clone().expect("metadata")).expect("metadata shape");
    let w = m.lookup_window.as_ref().expect("lookupWindow");
    assert_eq!(w.starts_at.as_ref(), "2026-10-22T09:00:00Z");
    assert_eq!(w.ends_at.as_ref(), "2026-10-22T17:00:00Z");
    assert_eq!(
        serde_json::to_value(&m).expect("serialize"),
        action["metadata"]
    );
}

#[test]
fn availability_metadata_lookup_window_is_optional() {
    // L252: "MAY include `lookupWindow`".
    let m: AvailabilityMetadata = serde_json::from_value(json!({})).expect("empty");
    assert!(m.lookup_window.is_none());
}

#[test]
fn lookup_window_new_serializes_both_fields() {
    // L252: "(Object, with `startsAt` and `endsAt` fields)".
    let w = LookupWindow::new("2026-10-22T09:00:00Z".into(), "2026-10-22T17:00:00Z".into());
    assert_eq!(
        serde_json::to_value(&w).expect("serialize"),
        json!({ "startsAt": "2026-10-22T09:00:00Z", "endsAt": "2026-10-22T17:00:00Z" })
    );
}

#[test]
fn lookup_window_requires_both_fields() {
    // L252: "(Object, with `startsAt` and `endsAt` fields)".
    assert!(
        serde_json::from_value::<LookupWindow>(json!({ "startsAt": "2026-10-22T09:00:00Z" }))
            .is_err()
    );
    assert!(
        serde_json::from_value::<LookupWindow>(json!({ "endsAt": "2026-10-22T17:00:00Z" }))
            .is_err()
    );
}
