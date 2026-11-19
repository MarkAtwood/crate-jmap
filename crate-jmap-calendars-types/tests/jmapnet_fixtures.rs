//! Cross-implementation fixtures from JMAP.Net (github.com/JMAP-Net/JMAP.Net
//! at db60d47, MIT, Copyright (c) 2025 JMAP.Net), bd JMAP-48o0. They are
//! that library's serializer output, checked by its own test suite (165 of
//! 166 passing; the failure is an HTTP limit test, not a fixture). JMAP.Net
//! targets draft-ietf-jmap-calendars-26 and RFC 8620; where it and this
//! crate disagree the draft text decides (calendars-34 here), and each
//! disagreement is asserted with its citation. See tests/fixtures/jmapnet/README.md.

use jmap_calendars_types::{
    Calendar, CalendarEvent, CalendarEventNotification, CalendarsAccountCapability,
    ParticipantIdentity, PrincipalCalendarsCapability,
};
use jmap_types::GetResponse;
use serde_json::{Map, Value};

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/jmapnet/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("JSON")
}

/// `v` without its top-level members whose value is `null`.
fn without_nulls(v: &Value) -> Value {
    let m: Map<String, Value> = v
        .as_object()
        .expect("object")
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Value::Object(m)
}

/// A CalendarEvent (calendars-34 §5) parses with its JMAP and JSCalendar
/// properties, and round-trips except that JMAP.Net writes `null` for
/// absent JSCalendar properties. RFC 8984 types most of them without
/// `null` (e.g. §4.1.3 `relatedTo`: `String[Relation]`), so this crate
/// omits them. The exception is `method`, which this crate does not model
/// (calendars-34 L1253: stored events "MUST NOT have a "method" property")
/// and so keeps as an unknown member, `null` included. Nothing else
/// changes.
#[test]
fn calendar_event_round_trips_except_null_members() {
    let original = fixture("calendar-event");
    let event: CalendarEvent = serde_json::from_value(original.clone()).expect("CalendarEvent");
    let back = serde_json::to_value(&event).unwrap();
    assert_eq!(back["title"], "Design Review");
    assert_eq!(back["calendarIds"]["cal1"], true);
    assert_eq!(back["participants"]["alice"]["roles"]["owner"], true);
    let mut expected = without_nulls(&original);
    expected["method"] = Value::Null;
    assert_eq!(back, expected);
}

/// The same object inside a `/get` response (RFC 8620 §5.1).
#[test]
fn calendar_event_get_response_parses() {
    let r: GetResponse<CalendarEvent> =
        serde_json::from_value(fixture("calendar-event-get-response")).expect("/get");
    assert!(!r.list.is_empty());
}

/// CalendarEventNotification (calendars-34 §6) round-trips, except that this
/// crate writes `changedBy.calendarAddress: null`: calendars-34 L2706 adds
/// "*calendarAddress*: String|null" to Person, which calendars-26 did not
/// have. JMAP.Net's `scheduleId` is kept as an unknown member.
#[test]
fn notification_round_trips_with_calendar_address() {
    let original = fixture("calendar-event-notification");
    let n: CalendarEventNotification =
        serde_json::from_value(original.clone()).expect("CalendarEventNotification");
    let mut expected = original;
    expected["changedBy"]["calendarAddress"] = Value::Null;
    assert_eq!(serde_json::to_value(n).unwrap(), expected);
}

/// The Principal `urn:ietf:params:jmap:calendars` capability round-trips.
#[test]
fn principal_calendars_capability_round_trips() {
    let original = fixture("calendar-principal-capability");
    let c: PrincipalCalendarsCapability =
        serde_json::from_value(original.clone()).expect("capability");
    assert_eq!(serde_json::to_value(c).unwrap(), original);
}

/// The account-level `urn:ietf:params:jmap:calendars` capability from
/// JMAP.Net's Session parses.
#[test]
fn account_capability_from_the_session_parses() {
    let session = fixture("jmap-session-with-calendar-capabilities");
    let cap =
        &session["accounts"]["account1"]["accountCapabilities"]["urn:ietf:params:jmap:calendars"];
    let _: CalendarsAccountCapability = serde_json::from_value(cap.clone()).expect("capability");
}

/// Disagreement, JMAP.Net is wrong: its CalendarRights has `mayAdmin` and
/// no `mayShare`. Both calendars-26 (L947) and calendars-34 (L996) define
/// "*mayShare*: Boolean" in CalendarRights and no `mayAdmin`. This crate
/// requires `mayShare`; with `mayAdmin` renamed, the Calendar parses and
/// the renamed object round-trips.
#[test]
fn calendar_rights_need_may_share_not_may_admin() {
    let original = fixture("calendar");
    let err = serde_json::from_value::<Calendar>(original.clone()).expect_err("mayShare missing");
    assert!(err.to_string().contains("mayShare"), "{err}");

    let mut fixed = original;
    let rename = |rights: &mut Value| {
        let m = rights.as_object_mut().unwrap();
        let admin = m.remove("mayAdmin").expect("JMAP.Net writes mayAdmin");
        m.insert("mayShare".to_owned(), admin);
    };
    rename(&mut fixed["myRights"]);
    rename(&mut fixed["shareWith"]["principal1"]);
    let cal: Calendar = serde_json::from_value(fixed.clone()).expect("Calendar");
    fixed["defaultAlertsWithTime"] = Value::Null;
    fixed["defaultAlertsWithoutTime"] = Value::Null;
    assert_eq!(serde_json::to_value(cal).unwrap(), fixed);
}

/// Disagreement, a draft revision apart: JMAP.Net's ParticipantIdentity is
/// calendars-26's (`scheduleId`, `sendTo`); calendars-34 L646 replaced them
/// with "*calendarAddress*: String". This crate follows -34, so the -26
/// object is missing `calendarAddress`.
#[test]
fn participant_identity_is_a_revision_apart() {
    let original = fixture("participant-identity");
    let err = serde_json::from_value::<ParticipantIdentity>(original.clone())
        .expect_err("calendars-26 shape");
    assert!(err.to_string().contains("calendarAddress"), "{err}");

    let mut fixed = original;
    let m = fixed.as_object_mut().unwrap();
    let address = m.remove("scheduleId").expect("calendars-26 scheduleId");
    m.remove("sendTo");
    m.insert("calendarAddress".to_owned(), address);
    let _: ParticipantIdentity = serde_json::from_value(fixed).expect("-34 shape");
}
