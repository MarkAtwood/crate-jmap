//! Object history for FileNode through the dispatcher and the memory
//! backend (bd JMAP-tna1.3.11).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §4 (onlyDestroyed), §5.2 (a
//! missing parent fails as an update naming it would; a conflict with a
//! current object gets the Foo/set error), the -04 editor's copy §5.1
//! ("the server MUST order the restores within a single call so that an
//! object is restored before any restore that refers to it"), and
//! draft-ietf-jmap-filenode-20 (sibling names are unique).

use std::sync::Arc;

use jmap_filenode_server::memory::MemoryBackend;
use jmap_filenode_server::{register_filenode_handlers, register_filenode_handlers_with_history};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const A: &str = "abc";

fn backend() -> MemoryBackend {
    MemoryBackend::new()
        .with_account(&Id::from(A))
        .with_object_history(
            serde_json::from_value(
                json!({"maxHistoryDuration": null, "supportedTypes": ["FileNode"]}),
            )
            .unwrap(),
            HistoryRetention::new(),
        )
}

struct Server(Dispatcher<()>, MemoryBackend);

impl Server {
    fn new() -> Self {
        let b = backend();
        b.set_history_clock_for_test(1_000);
        let mut d = Dispatcher::new();
        register_filenode_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self(d, b)
    }

    async fn one(&self, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(A);
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:filenode",
                      "urn:ietf:params:jmap:object-history"],
            "methodCalls": [[method, args, "0"]]
        }))
        .unwrap();
        let resp = serde_json::to_value(self.0.dispatch(req, (), State::from("s")).await).unwrap();
        let r = &resp["methodResponses"][0];
        assert_ne!(r[0], "error", "{r}");
        r[1].clone()
    }

    async fn dir(&self, name: &str, parent: Option<&str>) -> String {
        let out = self
            .one(
                "FileNode/set",
                json!({"create": {"d": {"@type": "Directory", "name": name, "parentId": parent}}}),
            )
            .await;
        out["created"]["d"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{out}"))
            .to_owned()
    }
}

/// -04 §5.1 and §5.2: a child whose parent is also destroyed fails alone,
/// and succeeds with the parent in one call even when its key sorts first.
#[tokio::test]
async fn parent_restored_before_child_in_one_call() {
    let s = Server::new();
    let p1 = s.dir("p1", None).await;
    let child = s.dir("child", Some(&p1)).await;
    let p2 = s.dir("p2", None).await;
    // Move the child under the later parent, so its id sorts before its
    // parent's.
    s.one(
        "FileNode/set",
        json!({"update": {&child: {"parentId": &p2}}}),
    )
    .await;
    assert!(child < p2, "{child} {p2}");
    s.1.set_history_clock_for_test(60_000);
    s.one("FileNode/set", json!({"destroy": [&child]})).await;
    s.1.set_history_clock_for_test(120_000);
    s.one("FileNode/set", json!({"destroy": [&p2]})).await;

    let q = s
        .one("FileNode/query", json!({"onlyDestroyed": true}))
        .await;
    assert_eq!(
        q["ids"],
        json!([&p2, &child]),
        "most recently destroyed first"
    );

    let out = s
        .one("FileNode/set", json!({"restore": {&child: null}}))
        .await;
    assert_eq!(
        out["notRestored"][&child]["properties"],
        json!(["parentId"]),
        "{out}"
    );

    let out = s
        .one(
            "FileNode/set",
            json!({"restore": {&child: null, &p2: null}}),
        )
        .await;
    assert_eq!(out["notRestored"], Value::Null, "{out}");
    assert!(out["restored"].get(&child).is_some() && out["restored"].get(&p2).is_some());
    let got = s
        .one(
            "FileNode/get",
            json!({"ids": [&child], "properties": ["parentId"]}),
        )
        .await;
    assert_eq!(got["list"][0]["parentId"], json!(p2));
    let q = s
        .one("FileNode/query", json!({"onlyDestroyed": true}))
        .await;
    assert_eq!(q["ids"], json!([]));

    // The history continues: the move is a replaced version.
    let got = s
        .one(
            "FileNode/get",
            json!({"ids": [&child], "includeReplaced": true, "properties": ["parentId"]}),
        )
        .await;
    let parents: Vec<&Value> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["parentId"])
        .collect();
    assert_eq!(parents, [&json!(p1), &json!(p2), &json!(p2)], "{got}");
}

/// §5.2: a sibling with the same name is `alreadyExists`; renaming in the
/// patch avoids it.
#[tokio::test]
async fn name_clash_is_already_exists() {
    let s = Server::new();
    let old = s.dir("docs", None).await;
    s.one("FileNode/set", json!({"destroy": [&old]})).await;
    s.dir("docs", None).await;
    let out = s
        .one("FileNode/set", json!({"restore": {&old: null}}))
        .await;
    assert_eq!(out["notRestored"][&old]["type"], "alreadyExists", "{out}");
    let out = s
        .one(
            "FileNode/set",
            json!({"restore": {&old: {"name": "docs (old)"}}}),
        )
        .await;
    assert!(out["restored"].get(&old).is_some(), "{out}");
}

#[test]
fn registration_has_no_gaps() {
    let mut d = Dispatcher::new();
    register_filenode_handlers_with_history(&mut d, Arc::new(backend()));
    assert!(d.object_history_gaps(&["FileNode"]).is_empty());
    let mut d = Dispatcher::new();
    register_filenode_handlers(&mut d, Arc::new(backend()));
    assert_eq!(d.object_history_gaps(&["FileNode"]).len(), 4);
}
