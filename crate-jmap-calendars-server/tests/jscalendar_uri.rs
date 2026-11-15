//! JSCalendar URI members on CalendarEvent/set (RFC 8984 §1.4.11 Link
//! `href` "is a URI [RFC3986]", §4.2.6 VirtualLocation `uri`, §4.4.6
//! Participant `sendTo` values; draft-atwood-jmap-updates-00 §7.1).
//!
//! Oracle: RFC 8984 example values (§6.8 `https://stream.example.com/...`,
//! `tel:+1-555-555-5555`) as valid, and hand-written invalid values.

mod common;

use common::MemoryBackend;
use jmap_calendars_server::handle_calendar_event_set;
use jmap_types::Id;
use serde_json::{Value, json};

async fn set(args: Value) -> Value {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    let mut args = args;
    args["accountId"] = json!("acc1");
    handle_calendar_event_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("/set must not fail at method level")
        .primary
}

fn event() -> Value {
    json!({ "@type": "Event", "calendarIds": { "cal1": true }, "title": "t",
            "start": "2026-11-15T10:00:00", "duration": "PT1H" })
}

#[tokio::test]
async fn create_rejects_each_kind_of_bad_uri() {
    for (property, value) in [
        (
            "links",
            json!({ "l1": { "@type": "Link", "href": "https://example.com/a b" } }),
        ),
        (
            "virtualLocations",
            json!({ "v1": { "@type": "VirtualLocation", "uri": "tel: +1 555" } }),
        ),
        (
            "participants",
            json!({ "p1": { "@type": "Participant", "sendTo": { "imip": "mailto:a b@example.com" } } }),
        ),
        (
            "recurrenceOverrides",
            json!({ "2026-11-22T10:00:00": { "links/l2/href": "x y" } }),
        ),
    ] {
        let mut ev = event();
        ev[property] = value;
        let resp = set(json!({ "create": { "e": ev } })).await;
        assert_eq!(
            resp["notCreated"]["e"]["type"], "invalidProperties",
            "{property}: {resp}"
        );
        assert_eq!(
            resp["notCreated"]["e"]["properties"],
            json!([property]),
            "{resp}"
        );
    }
}

#[tokio::test]
async fn rfc8984_example_uris_are_not_rejected() {
    let mut ev = event();
    ev["virtualLocations"] = json!({
        "v1": { "@type": "VirtualLocation", "uri": "https://stream.example.com/the_band_2020" },
        "v2": { "@type": "VirtualLocation", "uri": "tel:+1-555-555-5555" }
    });
    ev["participants"] =
        json!({ "p1": { "@type": "Participant", "sendTo": { "imip": "mailto:a@example.com" } } });
    let resp = set(json!({ "create": { "e": ev } })).await;
    let props = &resp["notCreated"]["e"]["properties"];
    for p in ["virtualLocations", "participants"] {
        assert!(
            !props.as_array().is_some_and(|a| a.contains(&json!(p))),
            "{resp}"
        );
    }
}

#[tokio::test]
async fn update_rejects_a_bad_uri_by_pointer() {
    let resp = set(
        json!({ "update": { "E1": { "participants/p1/sendTo/imip": "mailto:a b@example.com" } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["E1"]["properties"],
        json!(["participants"]),
        "{resp}"
    );
}
