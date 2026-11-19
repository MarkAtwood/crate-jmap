//! Object history for the chat types through the dispatcher and the
//! memory backend (bd JMAP-tna1.3.18).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3 and §5.1;
//! draft-atwood-jmap-chat-00 §Message (once a Message is deleted or
//! hard-deleted, a server listing Message "SHOULD NOT return any prior or
//! destroyed version" and "SHOULD permanently remove those versions") and
//! §Space/set (a Space destroy "Hard-deletes all records; no tombstones are
//! retained"), as docs/design/DECISIONS.md decision 22 applies them; and
//! decision 22's exclusion of ReadPosition and PresenceStatus.

mod common;

use std::sync::Arc;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::{register_chat_handlers, register_chat_handlers_with_history};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const TYPES: [&str; 8] = [
    "Chat",
    "Message",
    "Space",
    "ChatContact",
    "CustomEmoji",
    "SpaceBan",
    "ChatBan",
    "SpaceInvite",
];

fn backend() -> MemoryBackend {
    let b = MemoryBackend::new().with_object_history(
        serde_json::from_value(json!({"maxHistoryDuration": null, "supportedTypes": TYPES}))
            .unwrap(),
        HistoryRetention::new(),
    );
    b.register_account(&Id::from(ACCOUNT_ID));
    b
}

struct Server {
    b: MemoryBackend,
    d: Dispatcher<()>,
}

impl Server {
    fn new() -> Self {
        let b = backend();
        b.set_history_clock_for_test(1_000);
        let mut d = Dispatcher::new();
        register_chat_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn call(&self, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(ACCOUNT_ID);
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat",
                      "urn:ietf:params:jmap:object-history"],
            "methodCalls": [[method, args, "0"]]
        }))
        .unwrap();
        let resp = serde_json::to_value(self.d.dispatch(req, (), State::from("s")).await).unwrap();
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

    fn tick(&self, ms: i64) {
        self.b.set_history_clock_for_test(ms);
    }
}

async fn group(s: &Server) -> String {
    let bob = s
        .create("ChatContact/set", json!({"identity": "bob@example.net"}))
        .await;
    s.create(
        "Chat/set",
        json!({"kind": "group", "name": "G", "memberIds": [bob]}),
    )
    .await
}

fn message(chat: &str) -> Value {
    json!({"chatId": chat, "body": "first", "bodyType": "text/plain",
           "sentAt": "2024-01-01T00:00:00Z"})
}

/// A group Chat's renames are history; destroyed, it is found and
/// restored under its id.
#[tokio::test]
async fn chat_history_and_restore() {
    let s = Server::new();
    let chat = group(&s).await;
    s.tick(2_000);
    s.one("Chat/set", json!({"update": {&chat: {"name": "G2"}}}))
        .await;
    let got = s
        .one(
            "Chat/get",
            json!({"ids": [&chat], "includeReplaced": true, "properties": ["name"]}),
        )
        .await;
    let names: Vec<&str> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["G", "G2"]);

    s.tick(3_000);
    s.one("Chat/set", json!({"destroy": [&chat]})).await;
    let q = s.one("Chat/query", json!({"onlyDestroyed": true})).await;
    assert_eq!(q["ids"], json!([&chat]));
    let out = s.one("Chat/set", json!({"restore": {&chat: null}})).await;
    assert!(out["restored"].get(&chat).is_some(), "{out}");
    let got = s
        .one("Chat/get", json!({"ids": [&chat], "properties": ["name"]}))
        .await;
    assert_eq!(got["list"][0]["name"], "G2");
}

/// chat-00 §IANA registers ChatBan with "Supports History: Yes": a ban's
/// `reason` edits are history, and a lifted ban is restored under its id.
#[tokio::test]
async fn chat_ban_history_and_restore() {
    let s = Server::new();
    let chat = group(&s).await;
    let ban = s
        .create(
            "ChatBan/set",
            json!({"chatId": &chat, "userId": "mallory", "reason": "r1"}),
        )
        .await;
    s.tick(2_000);
    s.one("ChatBan/set", json!({"update": {&ban: {"reason": "r2"}}}))
        .await;
    let got = s
        .one(
            "ChatBan/get",
            json!({"ids": [&ban], "includeReplaced": true, "properties": ["reason"]}),
        )
        .await;
    let reasons: Vec<&str> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons, ["r1", "r2"]);

    s.tick(3_000);
    s.one("ChatBan/set", json!({"destroy": [&ban]})).await;
    let out = s.one("ChatBan/set", json!({"restore": {&ban: null}})).await;
    assert!(out["restored"].get(&ban).is_some(), "{out}");
    let got = s.one("ChatBan/get", json!({"ids": [&ban]})).await;
    assert_eq!(got["list"][0]["reason"], "r2");
}

/// chat-00 §Message: an edited Message has history until it is deleted;
/// then none, and it is never a destroyed object.
#[tokio::test]
async fn message_versions_go_with_the_message() {
    let s = Server::new();
    let chat = group(&s).await;
    let m = s.create("Message/set", message(&chat)).await;
    s.tick(2_000);
    s.one("Message/set", json!({"update": {&m: {"body": "edited"}}}))
        .await;
    let got = s
        .one(
            "Message/get",
            json!({"ids": [&m], "includeReplaced": true, "properties": ["body"]}),
        )
        .await;
    let bodies: Vec<&str> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["body"].as_str().unwrap())
        .collect();
    assert_eq!(bodies, ["first", "edited"]);

    // Soft delete: the versions go.
    s.one(
        "Message/set",
        json!({"update": {&m: {"deletedAt": "2026-01-02T00:00:00Z"}}}),
    )
    .await;
    let got = s
        .one(
            "Message/get",
            json!({"ids": [&m], "includeReplaced": true, "properties": ["body"]}),
        )
        .await;
    assert_eq!(
        got["list"].as_array().unwrap().len(),
        1,
        "live version only: {got}"
    );

    // Hard delete: nothing is retained.
    let m2 = s.create("Message/set", message(&chat)).await;
    s.one("Message/set", json!({"update": {&m2: {"body": "edited"}}}))
        .await;
    s.one("Message/set", json!({"destroy": [&m2]})).await;
    let got = s
        .one(
            "Message/get",
            json!({"ids": [&m2], "includeDestroyed": true, "includeReplaced": true}),
        )
        .await;
    assert_eq!(got["notFound"], json!([&m2]), "{got}");
    let q = s.one("Message/query", json!({"onlyDestroyed": true})).await;
    assert_eq!(q["ids"], json!([]));
    let out = s.one("Message/set", json!({"restore": {&m2: null}})).await;
    assert_eq!(out["notRestored"][&m2]["type"], "notFound");
}

/// chat-00 §Space/set: a destroyed Space leaves no history, so its
/// restore is `notFound`; a live Space's earlier versions remain.
#[tokio::test]
async fn space_destroy_purges_its_history() {
    let s = Server::new();
    let space = s.create("Space/set", json!({"name": "S"})).await;
    let got = s
        .one(
            "Space/get",
            json!({"ids": [&space], "includeReplaced": true, "properties": ["name"]}),
        )
        .await;
    assert_eq!(got["list"][0]["name"], "S", "{got}");
    s.one("Space/set", json!({"destroy": [&space]})).await;
    let got = s
        .one(
            "Space/get",
            json!({"ids": [&space], "includeDestroyed": true}),
        )
        .await;
    assert_eq!(got["notFound"], json!([&space]), "{got}");
    let out = s.one("Space/set", json!({"restore": {&space: null}})).await;
    assert_eq!(out["notRestored"][&space]["type"], "notFound");
}

/// Decision 22: ReadPosition and PresenceStatus keep no history.
#[tokio::test]
async fn excluded_types_refuse_history_arguments() {
    let s = Server::new();
    for method in ["ReadPosition/get", "PresenceStatus/get"] {
        let r = s
            .call(method, json!({"ids": null, "includeReplaced": true}))
            .await;
        assert_eq!(
            (&r[0], &r[1]["type"]),
            (&json!("error"), &json!("invalidArguments")),
            "{method}"
        );
    }
}

#[test]
fn registration_has_no_gaps() {
    let mut d = Dispatcher::new();
    register_chat_handlers_with_history(&mut d, Arc::new(backend()));
    assert!(d.object_history_gaps(&TYPES).is_empty());
    let mut d = Dispatcher::new();
    register_chat_handlers(&mut d, Arc::new(backend()));
    assert_eq!(
        d.object_history_gaps(&["SpaceBan"]),
        ["SpaceBan/get", "SpaceBan/set"]
    );
}
