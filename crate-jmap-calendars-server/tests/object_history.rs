//! Object history for Calendar, CalendarEvent and ParticipantIdentity
//! through the dispatcher and the memory backend (bd JMAP-tna1.3.10).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3 (an
//! `onDestroyRemoveEvents` cascade counts as destroyed), §3.1 (versions,
//! `replaced`), §4 (onlyDestroyed; arguments for current objects only are
//! ignored; an unsupported comparator is `unsupportedSort`), §5.2 (a
//! missing container fails as an update would; restore it in an earlier
//! call), and the -04 editor's copy §5.1 (Foo/changes compares sinceState
//! with now).

use std::sync::Arc;

use jmap_calendars_server::memory::MemoryBackend;
use jmap_calendars_server::{
    register_calendars_handlers, register_calendars_handlers_with_history,
};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: [&str; 3] = [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:calendars",
    "urn:ietf:params:jmap:object-history",
];
const A: &str = "abc";
/// 2026-03-01T00:00:00Z.
const T0: i64 = 1_772_323_200_000;
const TYPES: [&str; 3] = ["Calendar", "CalendarEvent", "ParticipantIdentity"];

fn backend() -> MemoryBackend<()> {
    let b = MemoryBackend::<()>::default()
        .with_account(&Id::from(A))
        .with_object_history(
            serde_json::from_value(json!({"maxHistoryDuration": null, "supportedTypes": TYPES}))
                .unwrap(),
            HistoryRetention::new(),
        );
    b.set_history_clock_for_test(T0);
    b
}

struct Server {
    b: MemoryBackend<()>,
    d: Dispatcher<()>,
}

impl Server {
    fn new() -> Self {
        let b = backend();
        let mut d = Dispatcher::new();
        register_calendars_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn run(&self, calls: Value) -> Vec<Value> {
        let req: JmapRequest =
            serde_json::from_value(json!({"using": USING, "methodCalls": calls})).unwrap();
        let resp = serde_json::to_value(self.d.dispatch(req, (), State::from("s")).await).unwrap();
        resp["methodResponses"].as_array().unwrap().clone()
    }

    async fn one(&self, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(A);
        let r = self.run(json!([[method, args, "0"]])).await.remove(0);
        assert_ne!(r[0], "error", "{r}");
        r[1].clone()
    }

    async fn create(&self, method: &str, object: Value) -> String {
        let out = self.one(method, json!({"create": {"x": object}})).await;
        out["created"]["x"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{out}"))
            .to_owned()
    }

    async fn state(&self, t: &str) -> Value {
        self.one(&format!("{t}/get"), json!({"ids": []})).await["state"].clone()
    }
}

fn calendar(name: &str) -> Value {
    json!({"name": name, "isSubscribed": true, "includeInAvailability": "all"})
}

fn event(calendar: &str, title: &str) -> Value {
    json!({"@type": "Event", "uid": title, "title": title, "start": "2026-01-05T09:00:00",
           "calendarIds": {calendar: true}})
}

#[tokio::test]
async fn event_versions_cascade_and_restore() {
    let s = Server::new();
    let cal = s.create("Calendar/set", calendar("Work")).await;
    let ev = s.create("CalendarEvent/set", event(&cal, "Standup")).await;
    s.b.set_history_clock_for_test(T0 + 60_000);
    s.one(
        "CalendarEvent/set",
        json!({"update": {&ev: {"title": "Daily standup"}}}),
    )
    .await;
    let got = s
        .one(
            "CalendarEvent/get",
            json!({"ids": [&ev], "includeReplaced": true, "properties": ["title"]}),
        )
        .await;
    let titles: Vec<(&str, &Value)> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["title"].as_str().unwrap(),
                &e["objectHistory"]["replaced"],
            )
        })
        .collect();
    assert_eq!(
        titles,
        [
            ("Standup", &json!("2026-03-01T00:01:00Z")),
            ("Daily standup", &Value::Null)
        ]
    );

    let since_cal = s.state("Calendar").await;
    let since_ev = s.state("CalendarEvent").await;
    s.b.set_history_clock_for_test(T0 + 120_000);
    let out = s
        .one(
            "Calendar/set",
            json!({"destroy": [&cal], "onDestroyRemoveEvents": true}),
        )
        .await;
    assert_eq!(out["destroyed"], json!([&cal]), "{out}");

    // §4: the cascaded event is destroyed; expandRecurrences is ignored,
    // an explicit sort is not supported.
    let q = s
        .one(
            "CalendarEvent/query",
            json!({"onlyDestroyed": true, "expandRecurrences": true,
                   "after": "2026-01-01T00:00:00Z", "before": "2026-02-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(q["ids"], json!([&ev]), "{q}");
    let r = s
        .run(
            json!([["CalendarEvent/query", {"accountId": A, "onlyDestroyed": true,
            "sort": [{"property": "start"}]}, "0"]]),
        )
        .await;
    assert_eq!(r[0][1]["type"], "unsupportedSort", "{}", r[0]);
    let q = s
        .one(
            "CalendarEvent/query",
            json!({"onlyDestroyed": true, "filter": {"inCalendar": &cal}}),
        )
        .await;
    assert_eq!(q["ids"], json!([&ev]));

    // §5.2: the event waits for its Calendar.
    let out = s
        .one("CalendarEvent/set", json!({"restore": {&ev: null}}))
        .await;
    assert_eq!(
        out["notRestored"][&ev]["properties"],
        json!(["calendarIds"])
    );
    let r = s
        .run(json!([
            ["Calendar/set", {"accountId": A, "restore": {&cal: null}}, "0"],
            ["CalendarEvent/set", {"accountId": A, "restore": {&ev: null}}, "1"]
        ]))
        .await;
    assert_eq!(r[0][1]["restored"], json!({&cal: null}), "{}", r[0]);
    assert_eq!(r[1][1]["restored"], json!({&ev: null}), "{}", r[1]);

    // -04 §5.1: existed at sinceState and now ⇒ updated.
    let ch = s
        .one("Calendar/changes", json!({"sinceState": since_cal}))
        .await;
    assert_eq!(
        (&ch["updated"], &ch["destroyed"]),
        (&json!([&cal]), &json!([])),
        "{ch}"
    );
    let ch = s
        .one("CalendarEvent/changes", json!({"sinceState": since_ev}))
        .await;
    assert_eq!(
        (&ch["updated"], &ch["destroyed"]),
        (&json!([&ev]), &json!([])),
        "{ch}"
    );
}

#[tokio::test]
async fn participant_identity_restore() {
    let s = Server::new();
    let id = s
        .create(
            "ParticipantIdentity/set",
            json!({"name": "Me", "calendarAddress": "mailto:me@example.com"}),
        )
        .await;
    s.one("ParticipantIdentity/set", json!({"destroy": [&id]}))
        .await;
    let got = s
        .one(
            "ParticipantIdentity/get",
            json!({"ids": [&id], "includeDestroyed": true}),
        )
        .await;
    assert_eq!(got["list"][0]["name"], "Me", "{got}");
    let out = s
        .one("ParticipantIdentity/set", json!({"restore": {&id: null}}))
        .await;
    assert!(out["restored"].get(&id).is_some(), "{out}");
}

#[test]
fn registration_has_no_gaps() {
    let mut d = Dispatcher::new();
    register_calendars_handlers_with_history(&mut d, Arc::new(backend()));
    assert!(d.object_history_gaps(&TYPES).is_empty());
    let mut d = Dispatcher::new();
    register_calendars_handlers(&mut d, Arc::new(backend()));
    assert_eq!(
        d.object_history_gaps(&["ParticipantIdentity"]),
        ["ParticipantIdentity/get", "ParticipantIdentity/set"]
    );
}
