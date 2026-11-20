//! Object history for VTCCall, VTCRecording and VTCLivestream through the
//! dispatcher and the memory backend (bd JMAP-tna1.3.21).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3, §5.2 (a missing
//! container fails as an update would), §8.2 ("never a version unreadable
//! when current"); draft-atwood-jmap-vtc-00 §VTCLivestream (`streamKey`
//! and `streamUri` are "" for a non-moderator); docs/design/DECISIONS.md
//! decision 22 (VTCParticipant keeps no history).

mod common;

use std::sync::Arc;

use common::{ACCOUNT, BOB, EVE, media_backend, seed_as, stored_livestream, stored_recording};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, State};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{register_vtc_handlers, register_vtc_handlers_with_history};
use serde_json::{Value, json};

const TYPES: [&str; 3] = ["VTCCall", "VTCRecording", "VTCLivestream"];

fn history_backend() -> MemoryBackend {
    let b = media_backend().with_object_history(
        serde_json::from_value(json!({"maxHistoryDuration": null, "supportedTypes": TYPES}))
            .unwrap(),
        HistoryRetention::new(),
    );
    b.set_history_clock_for_test(1_000);
    b
}

struct Server {
    b: MemoryBackend,
    d: Dispatcher<jmap_types::Id>,
}

impl Server {
    fn new() -> Self {
        let b = history_backend();
        let mut d = Dispatcher::new();
        register_vtc_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn call(&self, who: &str, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(ACCOUNT);
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:vtc",
                      "urn:ietf:params:jmap:object-history"],
            "methodCalls": [[method, args, "0"]]
        }))
        .unwrap();
        let resp = serde_json::to_value(
            self.d
                .dispatch(req, jmap_types::Id::from(who), State::from("s"))
                .await,
        )
        .unwrap();
        resp["methodResponses"][0].clone()
    }

    async fn one(&self, who: &str, method: &str, args: Value) -> Value {
        let r = self.call(who, method, args).await;
        assert_ne!(r[0], "error", "{r}");
        r[1].clone()
    }
}

#[tokio::test]
async fn call_versions() {
    let s = Server::new();
    s.b.set_history_clock_for_test(2_000);
    s.one(
        BOB,
        "VTCCall/set",
        json!({"update": {"C1": {"subject": "Standup"}}}),
    )
    .await;
    let got = s
        .one(
            BOB,
            "VTCCall/get",
            json!({"ids": ["C1"], "includeReplaced": true, "properties": ["subject"]}),
        )
        .await;
    let subjects: Vec<&Value> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["subject"])
        .collect();
    assert_eq!(subjects, [&Value::Null, &json!("Standup")], "{got}");
}

/// §8.2: a non-moderator gets `streamKey` and `streamUri` blanked in every
/// history entry, as in the current object.
#[tokio::test]
async fn livestream_history_is_redacted() {
    let s = Server::new();
    seed_as(
        &s.b,
        "VTCLivestream",
        stored_livestream("L1", "live", json!("2026-06-05T14:31:00Z")),
    );
    let eve = s
        .one(
            EVE,
            "VTCLivestream/get",
            json!({"ids": ["L1"], "includeReplaced": true}),
        )
        .await;
    let entry = &eve["list"][0];
    assert_eq!(
        (&entry["streamKey"], &entry["streamUri"]),
        (&json!(""), &json!("")),
        "{eve}"
    );
    assert!(entry.get("objectHistory").is_some());
    let bob = s
        .one(
            BOB,
            "VTCLivestream/get",
            json!({"ids": ["L1"], "includeReplaced": true}),
        )
        .await;
    assert_eq!(bob["list"][0]["streamKey"], "sk_live_abc123def456");
}

/// §5.2: a recording's restore needs its call.
#[tokio::test]
async fn recording_restore_needs_its_call() {
    let s = Server::new();
    seed_as(
        &s.b,
        "VTCRecording",
        stored_recording("R1", "stopped", "2026-06-05T14:31:00Z"),
    );
    let out = s
        .one(BOB, "VTCRecording/set", json!({"destroy": ["R1"]}))
        .await;
    assert_eq!(out["destroyed"], json!(["R1"]), "{out}");
    let q = s
        .one(BOB, "VTCRecording/query", json!({"onlyDestroyed": true}))
        .await;
    assert_eq!(q["ids"], json!(["R1"]));
    let out = s
        .one(
            BOB,
            "VTCRecording/set",
            json!({"restore": {"R1": {"callId": "C9"}}}),
        )
        .await;
    assert_eq!(
        out["notRestored"]["R1"]["properties"],
        json!(["callId"]),
        "{out}"
    );
    let out = s
        .one(BOB, "VTCRecording/set", json!({"restore": {"R1": null}}))
        .await;
    assert!(out["restored"].get("R1").is_some(), "{out}");
}

#[tokio::test]
async fn participant_has_no_history() {
    let s = Server::new();
    let r = s
        .call(
            BOB,
            "VTCParticipant/get",
            json!({"ids": null, "includeReplaced": true}),
        )
        .await;
    assert_eq!(
        (&r[0], &r[1]["type"]),
        (&json!("error"), &json!("invalidArguments"))
    );
}

#[test]
fn registration_has_no_gaps() {
    let mut d: Dispatcher<jmap_types::Id> = Dispatcher::new();
    register_vtc_handlers_with_history(&mut d, Arc::new(history_backend()));
    assert!(d.object_history_gaps(&TYPES).is_empty());
    let mut d: Dispatcher<jmap_types::Id> = Dispatcher::new();
    register_vtc_handlers(&mut d, Arc::new(history_backend()));
    assert_eq!(d.object_history_gaps(&["VTCCall"]).len(), 4);
}
