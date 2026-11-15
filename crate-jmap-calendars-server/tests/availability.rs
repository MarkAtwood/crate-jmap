//! Principal/getAvailability against the memory backend
//! (draft-ietf-jmap-calendars-34 §2.2, JMAP-lehj.11).
//!
//! Oracle: the §2.2 relevance rules, BusyPeriod `busyStatus` definitions
//! and the merge rule ("confirmed > unavailable > tentative"), quoted at
//! each test; expected periods are worked out by hand.

mod common;

use common::MemoryBackend;
use jmap_calendars_server::handle_principal_get_availability;
use jmap_types::Id;
use serde_json::{Value, json};

const ACC: &str = "acc1";
const ME: &str = "mailto:alice@example.com";

fn rights(free_busy: bool, items: bool) -> Value {
    json!({
        "mayReadFreeBusy": free_busy, "mayReadItems": items, "mayWriteAll": true,
        "mayWriteOwn": true, "mayUpdatePrivate": true, "mayRSVP": true,
        "mayShare": true, "mayDelete": true, "mayAdmin": true
    })
}

fn calendar(id: &str, subscribed: bool, include: &str, rights: Value) -> Value {
    json!({ "id": id, "name": id, "sortOrder": 0, "isSubscribed": subscribed,
            "isVisible": true, "isDefault": false, "includeInAvailability": include,
            "myRights": rights })
}

/// A UTC event on `cal` at `start` (LocalDateTime) for `duration`.
fn event(id: &str, cal: &str, start: &str, duration: &str, extra: Value) -> Value {
    let mut e = json!({ "@type": "Event", "id": id, "calendarIds": { cal: true },
                        "title": id, "start": start, "duration": duration,
                        "timeZone": "Etc/UTC" });
    e.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    e
}

fn backend(events: &[Value]) -> MemoryBackend {
    let acc = Id::from(ACC);
    let b = MemoryBackend::new().with_account(&acc);
    for c in [
        calendar("work", true, "all", rights(true, true)),
        calendar("attend", true, "attending", rights(true, true)),
        calendar("hidden", false, "all", rights(true, true)),
        calendar("nofb", true, "all", rights(false, true)),
    ] {
        let id = c["id"].as_str().unwrap().to_owned();
        b.seed_object(&acc, "Calendar", &Id::from(id.as_str()), c);
    }
    b.seed_object(
        &acc,
        "ParticipantIdentity",
        &Id::from("pi1"),
        json!({ "id": "pi1", "name": "Alice", "calendarAddress": ME, "isDefault": true }),
    );
    for e in events {
        let id = e["id"].as_str().unwrap().to_owned();
        b.seed_object(&acc, "CalendarEvent", &Id::from(id.as_str()), e.clone());
    }
    b
}

async fn availability(b: &MemoryBackend, show_details: bool) -> Result<Value, String> {
    handle_principal_get_availability(
        b,
        &(),
        json!({ "accountId": ACC, "id": ACC, "utcStart": "2026-11-16T00:00:00Z",
                "utcEnd": "2026-11-17T00:00:00Z", "showDetails": show_details }),
    )
    .await
    .map(|o| o.primary["list"].clone())
    .map_err(|e| e.error_type.to_string())
}

fn me(status: &str) -> Value {
    json!({ "participants": { "p1": { "@type": "Participant", "calendarAddress": ME,
                                       "participationStatus": status } } })
}

/// §2.2 relevance: subscribed, includeInAvailability, mayReadFreeBusy, the
/// window, privacy not "secret", freeBusyStatus "busy", status not
/// "cancelled", not declined/delegated, and "attending" requires
/// accepted/tentative. Only the two qualifying events produce periods.
#[tokio::test]
async fn relevance_rules() {
    let b = backend(&[
        event("ok", "work", "2026-11-16T09:00:00", "PT1H", json!({})),
        event("unsub", "hidden", "2026-11-16T10:00:00", "PT1H", json!({})),
        event("nofb", "nofb", "2026-11-16T10:00:00", "PT1H", json!({})),
        event("before", "work", "2026-11-15T22:00:00", "PT2H", json!({})),
        event(
            "secret",
            "work",
            "2026-11-16T11:00:00",
            "PT1H",
            json!({ "privacy": "secret" }),
        ),
        event(
            "free",
            "work",
            "2026-11-16T11:00:00",
            "PT1H",
            json!({ "freeBusyStatus": "free" }),
        ),
        event(
            "cancel",
            "work",
            "2026-11-16T11:00:00",
            "PT1H",
            json!({ "status": "cancelled" }),
        ),
        event(
            "declined",
            "work",
            "2026-11-16T11:00:00",
            "PT1H",
            me("declined"),
        ),
        event("notme", "attend", "2026-11-16T12:00:00", "PT1H", json!({})),
        event(
            "accepted",
            "attend",
            "2026-11-16T14:00:00",
            "PT30M",
            me("accepted"),
        ),
    ]);
    let list = availability(&b, false).await.unwrap();
    // §2.2: `event` and `accountId` are present and null (JMAP-lehj.13).
    for p in list.as_array().unwrap() {
        assert_eq!(p["event"], Value::Null, "{p}");
        assert!(p.as_object().unwrap().contains_key("event"), "{p}");
        assert!(p.as_object().unwrap().contains_key("accountId"), "{p}");
    }
    let core: Vec<Value> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| json!([p["utcStart"], p["utcEnd"], p["busyStatus"]]))
        .collect();
    assert_eq!(
        core,
        [
            json!(["2026-11-16T09:00:00Z", "2026-11-16T10:00:00Z", "confirmed"]),
            json!(["2026-11-16T14:00:00Z", "2026-11-16T14:30:00Z", "confirmed"]),
        ]
    );
}

/// "the server MUST merge and split BusyPeriod objects where the "event"
/// property is null ... choose the value in the following order:
/// confirmed > unavailable > tentative."
#[tokio::test]
async fn merge_and_split_by_status() {
    let b = backend(&[
        event(
            "t",
            "work",
            "2026-11-16T09:00:00",
            "PT3H",
            json!({ "status": "tentative" }),
        ),
        event("c", "work", "2026-11-16T10:00:00", "PT1H", json!({})),
        event("c2", "work", "2026-11-16T11:00:00", "PT30M", json!({})),
    ]);
    let list = availability(&b, false).await.unwrap();
    let spans: Vec<(String, String, String)> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["utcStart"].as_str().unwrap()[11..16].to_owned(),
                p["utcEnd"].as_str().unwrap()[11..16].to_owned(),
                p["busyStatus"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let s = |a: &str, b: &str, c: &str| (a.to_owned(), b.to_owned(), c.to_owned());
    assert_eq!(
        spans,
        [
            s("09:00", "10:00", "tentative"),
            s("10:00", "11:30", "confirmed"),
            s("11:30", "12:00", "tentative"),
        ]
    );
}

/// "event" is the event when showDetails is true, unless it is "private"
/// or the user lacks mayReadItems; `accountId` follows it.
#[tokio::test]
async fn show_details() {
    let b = backend(&[
        event("pub", "work", "2026-11-16T09:00:00", "PT1H", json!({})),
        event(
            "priv",
            "work",
            "2026-11-16T13:00:00",
            "PT1H",
            json!({ "privacy": "private" }),
        ),
    ]);
    let list = availability(&b, true).await.unwrap();
    let with: Vec<&Value> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| !p["event"].is_null())
        .collect();
    assert_eq!(with.len(), 1, "{list}");
    assert_eq!(with[0]["event"]["id"], "pub");
    assert_eq!(with[0]["accountId"], ACC);
}

/// Decision on JMAP-lehj.11: an event the backend cannot evaluate (a
/// recurrence rule or a non-UTC time zone) fails the call instead of
/// being left out, which would report the Principal as free.
#[tokio::test]
async fn unsupported_events_fail_rather_than_vanish() {
    for extra in [
        json!({ "recurrenceRules": [{ "@type": "RecurrenceRule", "frequency": "weekly" }] }),
        json!({ "timeZone": "Europe/Berlin" }),
    ] {
        let b = backend(&[event("x", "work", "2026-11-16T09:00:00", "PT1H", extra)]);
        assert_eq!(availability(&b, false).await, Err("serverFail".to_owned()));
    }
}
