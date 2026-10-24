//! `#creationId` references in chat `/set` Id-typed properties, through
//! the full dispatcher (bd:JMAP-1p88).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls. In the case of records with
//! references to the same type, the server MUST order the creates and
//! updates within a single method call so that creates happen before their
//! creation ids are referenced by another create/update/destroy in the same
//! call." draft-atwood-jmap-chat-00 §Chat/set: `memberIds` are
//! "ChatContact.ids"; §Message: `replyTo` is a Message id; §Chat:
//! `pinnedMessageIds` "Each id MUST be the id of a Message in this Chat".
//!
//! Ids nested in entry objects (bd:JMAP-3x9e), at jmap-chat-spec 29e9a41:
//! draft-atwood-jmap-chat-00 L1118-1119 "`addMembers` (Object[]): Each
//! entry: `id` (String, ChatContact.id)"; L1126-1127 "`updateMemberRoles`
//! (Object[]): Each entry: `id` (String) and `role` (String)"; L429-430 a
//! Mention `id` is "The `id` of the mentioned participant's ChatContact";
//! §Space/set L1352-1363 Space `addMembers` entry "`id` (ChatContact.id,
//! String)", `removeMembers` "ChatContact.ids to remove", `updateMembers`
//! entry `id`. draft-atwood-jmap-chat-filenode-00 L186-189: `filenodeId`
//! is "The `id` of a FileNode"; "If validation fails, the server MUST
//! return a SetError of type `invalidProperties` naming `filenodeId`."
//!
//! Each assertion compares the referencing property with the id the server
//! reported for the referenced record, or with a hand-written SetError
//! shape.

mod common;

use std::sync::Arc;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::{handle_message_get, handle_message_set};
use jmap_chat_server::register_chat_handlers;
use jmap_server::{Dispatcher, HandlerOutput};
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

fn dispatcher() -> Dispatcher<()> {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let mut dispatcher = Dispatcher::new();
    register_chat_handlers(&mut dispatcher, Arc::new(backend));
    dispatcher
}

async fn run(dispatcher: &Dispatcher<()>, method_calls: Value) -> JmapResponse {
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created(resp: &JmapResponse, i: usize, key: &str) -> Value {
    let entry = args(resp, i)["created"][key].clone();
    assert!(entry["id"].is_string(), "created {key}: {}", args(resp, i));
    entry
}

fn message(chat: &str, extra: Value) -> Value {
    let mut obj = json!({
        "chatId": chat,
        "body": "x",
        "bodyType": "text/plain",
        "sentAt": "2024-01-01T00:00:00Z"
    });
    for (k, v) in extra.as_object().expect("object") {
        obj[k] = v.clone();
    }
    obj
}

/// ChatContact/set, Chat/set naming the contact in `memberIds`, and
/// Message/set whose reply sorts before the message it replies to; then a
/// second request that creates a Message and pins it by creation id.
#[tokio::test]
async fn chat_and_message_references_resolve() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["ChatContact/set", { "accountId": ACCOUNT_ID, "create": {
                "bob": { "identity": "bob@example.net" }
            } }, "0"],
            ["Chat/set", { "accountId": ACCOUNT_ID, "create": {
                "g": { "kind": "group", "name": "G", "memberIds": ["#bob"] }
            } }, "1"],
            ["Message/set", { "accountId": ACCOUNT_ID, "create": {
                "a-reply": message("#g", json!({ "replyTo": "#z-first" })),
                "z-first": message("#g", json!({}))
            } }, "2"]
        ]),
    )
    .await;
    let bob = created(&resp, 0, "bob");
    let chat = created(&resp, 1, "g");
    let first = created(&resp, 2, "z-first");
    let reply = created(&resp, 2, "a-reply");
    assert_eq!(reply["chatId"], chat["id"], "{reply}");
    assert_eq!(first["chatId"], chat["id"], "{first}");
    assert_eq!(reply["replyTo"], first["id"], "{reply}");
    let chat_id = chat["id"].as_str().expect("chat id").to_owned();

    let resp = run(
        &d,
        json!([
            ["Message/set", { "accountId": ACCOUNT_ID, "create": {
                "p": message(&chat_id, json!({}))
            } }, "0"],
            ["Chat/set", { "accountId": ACCOUNT_ID, "update": {
                (chat_id.clone()): { "pinnedMessageIds": ["#p"] }
            } }, "1"],
            ["Chat/get", { "accountId": ACCOUNT_ID, "ids": [chat_id],
                "properties": ["members", "pinnedMessageIds"] }, "2"]
        ]),
    )
    .await;
    let pinned = created(&resp, 0, "p");
    let stored = &args(&resp, 2)["list"][0];
    assert_eq!(
        stored["pinnedMessageIds"],
        json!([pinned["id"]]),
        "{stored}"
    );
    let members = stored["members"].as_array().cloned().unwrap_or_default();
    assert!(
        members.iter().any(|m| m["id"] == bob["id"]),
        "members must include the resolved ChatContact id: {stored}"
    );
}

#[tokio::test]
async fn unknown_reference_is_invalid_properties() {
    let resp = run(
        &dispatcher(),
        json!([["Message/set", { "accountId": ACCOUNT_ID, "create": {
            "m": message("#nope", json!({}))
        } }, "0"]]),
    )
    .await;
    let err = &args(&resp, 0)["notCreated"]["m"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["chatId"]), "{err}");
}

/// Chat/set update `addMembers` and `updateMemberRoles` entry ids name a
/// ChatContact created earlier in the request.
#[tokio::test]
async fn chat_member_entry_references_resolve() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["Chat/set", { "accountId": ACCOUNT_ID, "create": {
            "g": { "kind": "group", "name": "G" }
        } }, "0"]]),
    )
    .await;
    let chat_id = created(&resp, 0, "g")["id"]
        .as_str()
        .expect("chat id")
        .to_owned();

    let resp = run(
        &d,
        json!([
            ["ChatContact/set", { "accountId": ACCOUNT_ID, "create": {
                "dave": { "identity": "dave@example.net" }
            } }, "0"],
            ["Chat/set", { "accountId": ACCOUNT_ID, "update": {
                (chat_id.clone()): {
                    "addMembers": [{ "id": "#dave" }],
                    "updateMemberRoles": [{ "id": "#dave", "role": "admin" }]
                }
            } }, "1"],
            ["Chat/get", { "accountId": ACCOUNT_ID, "ids": [chat_id],
                "properties": ["members"] }, "2"]
        ]),
    )
    .await;
    let dave = created(&resp, 0, "dave");
    assert!(
        args(&resp, 1)["updated"]
            .as_object()
            .is_some_and(|m| m.len() == 1),
        "{}",
        args(&resp, 1)
    );
    let stored = &args(&resp, 2)["list"][0];
    let members = stored["members"].as_array().cloned().unwrap_or_default();
    assert!(
        members
            .iter()
            .any(|m| m["id"] == dave["id"] && m["role"] == "admin"),
        "members must hold the resolved ChatContact id as admin: {stored}"
    );
}

#[tokio::test]
async fn chat_member_entry_unknown_reference_names_the_key() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["Chat/set", { "accountId": ACCOUNT_ID, "create": {
            "g": { "kind": "group", "name": "G" }
        } }, "0"]]),
    )
    .await;
    let chat_id = created(&resp, 0, "g")["id"]
        .as_str()
        .expect("chat id")
        .to_owned();
    let resp = run(
        &d,
        json!([["Chat/set", { "accountId": ACCOUNT_ID, "update": {
            (chat_id.clone()): { "updateMemberRoles": [{ "id": "#nope", "role": "admin" }] }
        } }, "0"]]),
    )
    .await;
    let err = &args(&resp, 0)["notUpdated"][chat_id.as_str()];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["updateMemberRoles"]), "{err}");
}

/// A Message create's Mention `id`, and an edit's, name a ChatContact
/// created earlier in the request.
#[tokio::test]
async fn mention_references_resolve() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["ChatContact/set", { "accountId": ACCOUNT_ID, "create": {
                "bob": { "identity": "bob@example.net" }
            } }, "0"],
            ["Chat/set", { "accountId": ACCOUNT_ID, "create": {
                "g": { "kind": "group", "name": "G", "memberIds": ["#bob"] }
            } }, "1"],
            ["Message/set", { "accountId": ACCOUNT_ID, "create": {
                "m": message("#g", json!({ "mentions": [{ "id": "#bob" }] }))
            } }, "2"]
        ]),
    )
    .await;
    let bob = created(&resp, 0, "bob");
    let msg_id = created(&resp, 2, "m")["id"]
        .as_str()
        .expect("message id")
        .to_owned();
    let got = run(
        &d,
        json!([["Message/get", { "accountId": ACCOUNT_ID, "ids": [msg_id],
            "properties": ["mentions"] }, "0"]]),
    )
    .await;
    assert_eq!(
        args(&got, 0)["list"][0]["mentions"],
        json!([{ "id": bob["id"] }]),
        "{}",
        args(&got, 0)
    );

    let resp = run(
        &d,
        json!([
            ["ChatContact/set", { "accountId": ACCOUNT_ID, "create": {
                "carol": { "identity": "carol@example.net" }
            } }, "0"],
            ["Message/set", { "accountId": ACCOUNT_ID, "update": {
                (msg_id.clone()): { "mentions": [{ "id": "#carol" }] }
            } }, "1"],
            ["Message/get", { "accountId": ACCOUNT_ID, "ids": [msg_id],
                "properties": ["mentions"] }, "2"]
        ]),
    )
    .await;
    let carol = created(&resp, 0, "carol");
    assert_eq!(
        args(&resp, 2)["list"][0]["mentions"],
        json!([{ "id": carol["id"] }]),
        "{}",
        args(&resp, 1)
    );
}

#[tokio::test]
async fn mention_unknown_reference_names_mentions() {
    let resp = run(
        &dispatcher(),
        json!([
            ["Chat/set", { "accountId": ACCOUNT_ID, "create": {
                "g": { "kind": "group", "name": "G" }
            } }, "0"],
            ["Message/set", { "accountId": ACCOUNT_ID, "create": {
                "m": message("#g", json!({ "mentions": [{ "id": "#nope" }] }))
            } }, "1"]
        ]),
    )
    .await;
    let err = &args(&resp, 1)["notCreated"]["m"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 1));
    assert_eq!(err["properties"], json!(["mentions"]), "{err}");
}

/// SHA-256("abc"), FIPS 180-2 Appendix B.1 / RFC 6234 §8.5 TEST1.
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// One Message/set create in `chat_id` whose single Attachment has
/// `filenodeId`, resolved against `created_ids`; returns the response.
async fn create_with_filenode(
    backend: &MemoryBackend,
    chat_id: &str,
    filenode_id: &str,
    created_ids: &BTreeMap<Id, Id>,
) -> Value {
    let HandlerOutput { primary, .. } = handle_message_set(
        backend,
        &(),
        created_ids,
        json!({ "accountId": ACCOUNT_ID, "create": { "m": message(chat_id, json!({
            "attachments": [{
                "blobId": "B1",
                "filename": "report.txt",
                "contentType": "text/plain",
                "size": 3,
                "sha256": SHA256_ABC,
                "filenodeId": filenode_id
            }]
        })) } }),
        &[],
    )
    .await
    .expect("Message/set");
    primary
}

/// An Attachment `filenodeId` names a FileNode created earlier in the
/// request (for example by FileNode/set): the stored id is the one the
/// request-wide map holds for it.
#[tokio::test]
async fn attachment_filenode_reference_resolves() {
    let backend = common::account_backend();
    let chat_id = common::make_chat_group(&backend, "G").await;
    let created_ids = BTreeMap::from([(Id::from("f"), Id::from("F1"))]);
    let resp = create_with_filenode(&backend, &chat_id, "#f", &created_ids).await;
    let id = resp["created"]["m"]["id"].clone();
    assert!(id.is_string(), "not created: {resp}");
    let HandlerOutput { primary: got, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": ["attachments"] }),
    )
    .await
    .expect("Message/get");
    assert_eq!(
        got["list"][0]["attachments"][0]["filenodeId"], "F1",
        "{got}"
    );
}

#[tokio::test]
async fn attachment_filenode_unknown_reference_names_filenode_id() {
    let backend = common::account_backend();
    let chat_id = common::make_chat_group(&backend, "G").await;
    let resp = create_with_filenode(&backend, &chat_id, "#nope", &BTreeMap::new()).await;
    let err = &resp["notCreated"]["m"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["filenodeId"]), "{err}");
}

/// Space/set update `addMembers` and `updateMembers` entry ids and
/// `removeMembers` elements name ChatContacts created earlier in the
/// request.
#[tokio::test]
async fn space_member_references_resolve() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["Space/set", { "accountId": ACCOUNT_ID, "create": {
            "s": { "name": "S" }
        } }, "0"]]),
    )
    .await;
    let space_id = created(&resp, 0, "s")["id"]
        .as_str()
        .expect("space id")
        .to_owned();

    let resp = run(
        &d,
        json!([
            ["ChatContact/set", { "accountId": ACCOUNT_ID, "create": {
                "bob": { "identity": "bob@example.net" },
                "carol": { "identity": "carol@example.net" }
            } }, "0"],
            ["Space/set", { "accountId": ACCOUNT_ID, "update": {
                (space_id.clone()): {
                    "addMembers": [{ "id": "#bob" }, { "id": "#carol" }]
                }
            } }, "1"],
            ["Space/set", { "accountId": ACCOUNT_ID, "update": {
                (space_id.clone()): {
                    "updateMembers": [{ "id": "#bob", "nick": "B" }],
                    "removeMembers": ["#carol"]
                }
            } }, "2"],
            ["Space/get", { "accountId": ACCOUNT_ID, "ids": [space_id],
                "properties": ["members"] }, "3"]
        ]),
    )
    .await;
    let bob = created(&resp, 0, "bob");
    let carol = created(&resp, 0, "carol");
    for i in [1, 2] {
        assert!(
            args(&resp, i)["updated"]
                .as_object()
                .is_some_and(|m| m.len() == 1),
            "{}",
            args(&resp, i)
        );
    }
    let stored = &args(&resp, 3)["list"][0];
    let members = stored["members"].as_array().cloned().unwrap_or_default();
    assert!(
        members
            .iter()
            .any(|m| m["id"] == bob["id"] && m["nick"] == "B"),
        "{stored}"
    );
    assert!(members.iter().all(|m| m["id"] != carol["id"]), "{stored}");
}

#[tokio::test]
async fn space_member_unknown_reference_names_the_key() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["Space/set", { "accountId": ACCOUNT_ID, "create": {
            "s": { "name": "S" }
        } }, "0"]]),
    )
    .await;
    let space_id = created(&resp, 0, "s")["id"]
        .as_str()
        .expect("space id")
        .to_owned();
    let resp = run(
        &d,
        json!([["Space/set", { "accountId": ACCOUNT_ID, "update": {
            (space_id.clone()): { "addMembers": [{ "id": "#nope" }] }
        } }, "0"]]),
    )
    .await;
    let err = &args(&resp, 0)["notUpdated"][space_id.as_str()];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["addMembers"]), "{err}");
}
