//! Object history for SceneAsset, SceneRegion and SceneObject through the
//! dispatcher and the memory backend (bd JMAP-tna1.3.20).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3 (a cascade counts as
//! destroyed), §5.2 (a missing container fails as an update would), §8.2
//! (current access applies to history); the -04 editor's copy §5.1
//! (referenced-first restore order); draft-atwood-jmap-scene-00 (a region
//! destroy removes its objects; `invitedUserIds` only for a caller who may
//! update the region); docs/design/DECISIONS.md decision 22 (SceneAvatar
//! keeps no history).

mod common;

use std::sync::Arc;

use common::{ACCOUNT, ALICE, backend};
use jmap_object_history_server::HistoryRetention;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{register_scene_handlers, register_scene_handlers_with_history};
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, State};
use serde_json::{Value, json};

const TYPES: [&str; 3] = ["SceneAsset", "SceneRegion", "SceneObject"];

fn history_backend() -> MemoryBackend {
    let b = backend().with_object_history(
        serde_json::from_value(json!({"maxHistoryDuration": null, "supportedTypes": TYPES}))
            .unwrap(),
        HistoryRetention::new(),
    );
    b.set_history_clock_for_test(1_000);
    b
}

struct Server {
    b: MemoryBackend,
    d: Dispatcher<String>,
}

impl Server {
    fn new() -> Self {
        let b = history_backend();
        let mut d = Dispatcher::new();
        register_scene_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn call(&self, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(ACCOUNT);
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene",
                      "urn:ietf:params:jmap:object-history"],
            "methodCalls": [[method, args, "0"]]
        }))
        .unwrap();
        let resp = serde_json::to_value(
            self.d
                .dispatch(req, ALICE.to_owned(), State::from("s"))
                .await,
        )
        .unwrap();
        resp["methodResponses"][0].clone()
    }

    async fn one(&self, method: &str, args: Value) -> Value {
        let r = self.call(method, args).await;
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
}

fn region() -> Value {
    json!({"name": "Plaza", "bounds": {"min": [-500, -10, -500], "max": [500, 200, 500]}})
}

#[tokio::test]
async fn region_cascade_and_restores() {
    let s = Server::new();
    let r = s.create("SceneRegion/set", region()).await;
    let parent = s
        .create(
            "SceneObject/set",
            json!({"regionId": &r, "position": [0, 0, 0]}),
        )
        .await;
    let child = s
        .create(
            "SceneObject/set",
            json!({"regionId": &r, "parentId": &parent, "position": [0, 1, 0]}),
        )
        .await;
    s.b.set_history_clock_for_test(2_000);
    s.one(
        "SceneObject/set",
        json!({"update": {&parent: {"position": [1, 0, 0]}}}),
    )
    .await;
    let got = s
        .one(
            "SceneObject/get",
            json!({"ids": [&parent], "includeReplaced": true, "properties": ["position"]}),
        )
        .await;
    let positions: Vec<&Value> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["position"])
        .collect();
    assert_eq!(
        positions,
        [&json!([0.0, 0.0, 0.0]), &json!([1.0, 0.0, 0.0])],
        "{got}"
    );

    s.b.set_history_clock_for_test(3_000);
    s.one("SceneRegion/set", json!({"destroy": [&r]})).await;
    // The objects went with the region (§3).
    let got = s
        .one(
            "SceneObject/get",
            json!({"ids": [&parent, &child], "includeDestroyed": true}),
        )
        .await;
    assert_eq!(
        got["list"].as_array().unwrap().len() + got["notFound"].as_array().unwrap().len(),
        2
    );

    // Objects wait for their region (§5.2).
    let out = s
        .one(
            "SceneObject/set",
            json!({"restore": {&child: null, &parent: null}}),
        )
        .await;
    assert_eq!(
        out["notRestored"][&parent]["properties"],
        json!(["regionId"]),
        "{out}"
    );
    s.one("SceneRegion/set", json!({"restore": {&r: null}}))
        .await;
    // Child key first, parent restored first (-04 §5.1).
    let mut restore = serde_json::Map::new();
    let (first, second) = if child < parent {
        (&child, &parent)
    } else {
        (&parent, &child)
    };
    restore.insert(first.clone(), Value::Null);
    restore.insert(second.clone(), Value::Null);
    let out = s.one("SceneObject/set", json!({"restore": restore})).await;
    assert_eq!(out["notRestored"], Value::Null, "{out}");
}

#[test]
fn registration_has_no_gaps() {
    let mut d: Dispatcher<String> = Dispatcher::new();
    register_scene_handlers_with_history(&mut d, Arc::new(history_backend()));
    assert!(d.object_history_gaps(&TYPES).is_empty());
    let mut d: Dispatcher<String> = Dispatcher::new();
    register_scene_handlers(&mut d, Arc::new(history_backend()));
    assert_eq!(d.object_history_gaps(&["SceneObject"]).len(), 4);
}

/// SceneAvatar keeps no history (decision 22).
#[tokio::test]
async fn scene_avatar_has_no_history() {
    let s = Server::new();
    let r = s
        .call(
            "SceneAvatar/get",
            json!({"ids": null, "includeReplaced": true}),
        )
        .await;
    assert_eq!(
        (&r[0], &r[1]["type"]),
        (&json!("error"), &json!("invalidArguments"))
    );
}
