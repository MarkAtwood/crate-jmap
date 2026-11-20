//! Conditional calendars Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_calendars_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.10; docs/design/conditional.md §5.1, §7).
//!
//! Oracles: conditional-02 §3.1 and §4.2 ("the server MUST NOT make any
//! of the changes"), draft-ietf-jmap-calendars-34 §4.4
//! (`onDestroyRemoveEvents` destroys the Calendar's events with it).
//!
//! The crate has no cross-object cap or uniqueness rule for an atomic
//! commit to check: the default Calendar is one pointer, and
//! `calendarHasEvent` is judged within a Calendar/set, which moves no
//! events. So there is no final-state cap test here; rollback covers the
//! cascades. `onSuccessSetIsDefault` applies only when every change
//! succeeded (§4.3), so an atomic failure never reaches it.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_calendars_server::{register_calendars_conditional, register_calendars_handlers};
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:calendars",
    "urn:ietf:params:jmap:conditional",
];

fn calendar(id: &str, name: &str) -> Value {
    json!({
        "id": id, "name": name, "description": null, "color": null, "sortOrder": 0,
        "isSubscribed": true, "isVisible": true, "isDefault": false,
        "includeInAvailability": "all", "defaultAlertsWithTime": null,
        "defaultAlertsWithoutTime": null, "timeZone": null, "shareWith": null,
        "myRights": {
            "mayReadFreeBusy": true, "mayReadItems": true, "mayWriteAll": true,
            "mayWriteOwn": true, "mayUpdatePrivate": true, "mayRSVP": true,
            "mayShare": true, "mayDelete": true, "mayAdmin": true
        }
    })
}

/// Account acc1 with Calendars cal1 "Work" (holding event evt1) and cal2
/// "Home".
fn backend() -> Arc<MemoryBackend<()>> {
    let acc = Id::from("acc1");
    let b = MemoryBackend::<()>::default().with_account(&acc);
    b.seed_object(
        &acc,
        "Calendar",
        &Id::from("cal1"),
        calendar("cal1", "Work"),
    );
    b.seed_object(
        &acc,
        "Calendar",
        &Id::from("cal2"),
        calendar("cal2", "Home"),
    );
    b.seed_object(
        &acc,
        "CalendarEvent",
        &Id::from("evt1"),
        json!({"id": "evt1", "@type": "Event", "uid": "evt1-uid", "title": "Standup",
               "calendarIds": {"cal1": true}}),
    );
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend<()>>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_calendars_handlers(&mut d, Arc::clone(b));
    register_calendars_conditional(&mut d, Arc::clone(b));
    d
}

async fn call(d: &Dispatcher<()>, method: &str, mut args: Value) -> (String, Value) {
    args["accountId"] = json!("acc1");
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// The ids `method` (a `/get`) returns, sorted, and the state string.
async fn ids(d: &Dispatcher<()>, method: &str) -> (Vec<String>, Value) {
    let (name, r) = call(d, method, json!({"properties": ["id"]})).await;
    assert_eq!(name, method, "{r}");
    let mut ids: Vec<String> = r["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    (ids, r["state"].clone())
}

#[tokio::test]
async fn precondition_on_a_calendar_name() {
    let b = backend();
    let d = dispatcher(&b);
    let rename_if = |expected: &str| json!({"ifUnchangedBy": {"cal1": {"name": expected}}, "update": {"cal1": {"name": "Office"}}});
    let (name, r) = call(&d, "Calendar/set", rename_if("Personal")).await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "stateMismatch", "failed": ["cal1"]})
        )
    );
    let (name, r) = call(&d, "Calendar/set", rename_if("Work")).await;
    assert_eq!(name, "Calendar/set", "{r}");
    assert!(r["updated"].get("cal1").is_some(), "{r}");
}

/// §4.2 with a cascade: the `onDestroyRemoveEvents` destroy of cal1 and
/// its event roll back with the failing update, state strings included.
#[tokio::test]
async fn atomic_failure_rolls_back_a_destroy_cascade() {
    let b = backend();
    let d = dispatcher(&b);
    let calendars_before = ids(&d, "Calendar/get").await;
    let events_before = ids(&d, "CalendarEvent/get").await;
    assert_eq!(events_before.0, ["evt1"]);

    let (name, r) = call(
        &d,
        "Calendar/set",
        json!({
            "atomic": true,
            "onDestroyRemoveEvents": true,
            "destroy": ["cal1"],
            "update": {"nope": {"name": "x"}}
        }),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notUpdated"]["nope"]["type"], "notFound", "{r}");
    assert_eq!(ids(&d, "Calendar/get").await, calendars_before);
    assert_eq!(ids(&d, "CalendarEvent/get").await, events_before);

    // Without the failing change the cascade commits.
    let (name, r) = call(
        &d,
        "Calendar/set",
        json!({"atomic": true, "onDestroyRemoveEvents": true, "destroy": ["cal1"]}),
    )
    .await;
    assert_eq!(name, "Calendar/set", "{r}");
    assert!(ids(&d, "CalendarEvent/get").await.0.is_empty());
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = backend();
    let mut d = Dispatcher::new();
    register_calendars_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps().len(), 4, "{:?}", d.conditional_gaps());
    register_calendars_conditional(&mut d, b);
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());
}
