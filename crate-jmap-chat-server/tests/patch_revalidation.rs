//! `/set update` re-validates the patched record before storing it
//! (bd:JMAP-yk3i).
//!
//! RFC 8620 §5.3 "invalidProperties": "The record given is invalid in
//! some way. For example: It contains properties that are invalid
//! according to the type specification of this record type." And: "it
//! MUST NOT only commit part of an update to a single record". A patch
//! that sets a required property to `null` or to a wrong-typed value is
//! therefore rejected, and the stored record (and every later `/get`)
//! is unaffected.

mod common;

use common::{ACCOUNT_ID, make_chat_group, seed_group_chat};
use jmap_chat_server::message::{handle_message_get, handle_message_set};
use jmap_chat_server::{
    handle_ban_get, handle_ban_set, handle_chat_get, handle_chat_set, handle_contact_get,
    handle_contact_set, handle_emoji_get, handle_emoji_set, handle_position_get,
    handle_position_set, handle_presence_get, handle_presence_set,
};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// Assert a `/set` response rejected `id` with `invalidProperties`.
fn assert_rejected(resp: &Value, id: &str) {
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notUpdated"][id]["type"], "invalidProperties",
        "{resp:?}"
    );
}

/// Assert a `/set` response rejected `id` with `invalidProperties`
/// naming `prop`.
fn assert_rejected_naming(resp: &Value, id: &str, prop: &str) {
    assert_rejected(resp, id);
    assert_eq!(
        resp["notUpdated"][id]["properties"],
        json!([prop]),
        "{resp:?}"
    );
}

/// Assert a `/get` response succeeded and returned exactly one record.
fn single(resp: Value) -> Value {
    assert_eq!(resp["notFound"], json!([]), "{resp:?}");
    let list = resp["list"].as_array().expect("list");
    assert_eq!(list.len(), 1, "{resp:?}");
    list[0].clone()
}

/// Oracle: draft-atwood-jmap-chat-00 Chat `kind` (String, immutable):
/// "`"direct"`, `"group"`, or `"channel"`." `null` is none of those.
#[tokio::test]
async fn chat_kind_null_rejected_and_get_succeeds() {
    let backend = common::account_backend();
    let id = make_chat_group(&backend, "g").await;

    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "kind": null } } }),
        &[],
    )
    .await
    .expect("Chat/set");
    assert_rejected_naming(&resp, &id, "kind");

    let HandlerOutput { primary: resp, .. } =
        handle_chat_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("Chat/get");
    assert_eq!(single(resp)["kind"], "group");
}

/// Oracle: draft-atwood-jmap-chat-00 Message `body` (String): "Message
/// content. [...] Cleared to empty string when the message is deleted."
/// The property is a String with no default; `null` is not a String.
#[tokio::test]
async fn message_body_null_rejected_and_get_succeeds() {
    let backend = common::account_backend();
    seed_group_chat(&backend, "c1");
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
        &[],
    )
    .await
    .expect("Message/set create");
    let id = resp["created"]["m0"]["id"]
        .as_str()
        .expect("created id")
        .to_owned();

    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "body": null } } }),
        &[],
    )
    .await
    .expect("Message/set update");
    // The handler injects server-set `editedAt` alongside a body edit,
    // so only the description names the culprit.
    assert_rejected(&resp, &id);

    let HandlerOutput { primary: resp, .. } =
        handle_message_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("Message/get");
    assert_eq!(single(resp)["body"], "hi");
}

/// Oracle: draft-atwood-jmap-chat-00 CustomEmoji `blobId` (String) is a
/// required String property with no default; `null` is not a String.
#[tokio::test]
async fn emoji_blob_id_null_rejected_and_get_succeeds() {
    let backend = common::account_backend();
    backend.insert_object_for_test(
        "CustomEmoji",
        ACCOUNT_ID,
        "e1",
        json!({
            "id": "e1",
            "name": "catjam",
            "blobId": "b1",
            "createdBy": "u1",
            "createdAt": "2024-01-01T00:00:00Z",
        }),
    );

    let HandlerOutput { primary: resp, .. } = handle_emoji_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { "e1": { "blobId": null } } }),
    )
    .await
    .expect("CustomEmoji/set");
    assert_rejected_naming(&resp, "e1", "blobId");

    let HandlerOutput { primary: resp, .. } =
        handle_emoji_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("CustomEmoji/get");
    assert_eq!(single(resp)["blobId"], "b1");
}

/// Oracle: draft-atwood-jmap-chat-00 ChatContact `blocked` (Boolean):
/// "When `true`, messages from this ChatContact are silently dropped".
/// A String is not a Boolean.
#[tokio::test]
async fn contact_blocked_wrong_type_rejected_and_get_succeeds() {
    let backend = common::account_backend();
    backend.insert_object_for_test(
        "ChatContact",
        ACCOUNT_ID,
        "ct1",
        json!({
            "id": "ct1",
            "identity": "alice@example.com",
            "login": "alice@example.com",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-01T00:00:00Z",
            "blocked": true,
        }),
    );

    let HandlerOutput { primary: resp, .. } = handle_contact_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { "ct1": { "blocked": "yes" } } }),
    )
    .await
    .expect("ChatContact/set");
    assert_rejected_naming(&resp, "ct1", "blocked");

    let HandlerOutput { primary: resp, .. } =
        handle_contact_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("ChatContact/get");
    assert_eq!(single(resp)["blocked"], true);
}

/// Oracle: draft-atwood-jmap-chat-00 PresenceStatus `presence`
/// (String): "One of `"online"`, `"away"`, `"busy"`, `"invisible"`, or
/// `"offline"`." A Number is none of those.
#[tokio::test]
async fn presence_wrong_type_rejected_and_get_succeeds() {
    let backend = common::account_backend();
    backend.insert_object_for_test(
        "PresenceStatus",
        ACCOUNT_ID,
        "ps1",
        json!({
            "id": "ps1",
            "presence": "busy",
            "receiptSharing": false,
            "updatedAt": "2024-01-01T00:00:00Z",
        }),
    );

    let HandlerOutput { primary: resp, .. } = handle_presence_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { "ps1": { "presence": 5 } } }),
    )
    .await
    .expect("PresenceStatus/set");
    // The handler injects server-set `updatedAt` into every patch, so
    // only the description names the culprit.
    assert_rejected(&resp, "ps1");

    let HandlerOutput { primary: resp, .. } =
        handle_presence_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("PresenceStatus/get");
    let ps = single(resp);
    assert_eq!(ps["presence"], "busy");
    assert_eq!(ps["updatedAt"], "2024-01-01T00:00:00Z");
}

/// Oracle: draft-atwood-jmap-chat-00 ReadPosition `lastReadMessageId`
/// (String, optional): "The `id` of the most recent Message the owner
/// has read in this Chat." A Number is not a String id.
#[tokio::test]
async fn position_wrong_type_rejected_and_get_succeeds() {
    let backend = common::account_backend();
    backend.insert_object_for_test(
        "ReadPosition",
        ACCOUNT_ID,
        "rp1",
        json!({ "id": "rp1", "chatId": "c1", "lastReadMessageId": "m1" }),
    );

    let HandlerOutput { primary: resp, .. } = handle_position_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { "rp1": { "lastReadMessageId": 5 } } }),
    )
    .await
    .expect("ReadPosition/set");
    assert_rejected_naming(&resp, "rp1", "lastReadMessageId");

    let HandlerOutput { primary: resp, .. } =
        handle_position_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("ReadPosition/get");
    assert_eq!(single(resp)["lastReadMessageId"], "m1");
}

/// Oracle: draft-atwood-jmap-chat-00 SpaceBan `reason` (String,
/// optional): "Human-readable reason for the ban." A Number is not a
/// String.
#[tokio::test]
async fn ban_reason_wrong_type_rejected_and_get_succeeds() {
    let backend = common::account_backend();
    backend.insert_object_for_test(
        "SpaceBan",
        ACCOUNT_ID,
        "b1",
        json!({
            "id": "b1",
            "spaceId": "s1",
            "userId": "u2",
            "bannedBy": "u1",
            "createdAt": "2024-01-01T00:00:00Z",
            "reason": "spam",
        }),
    );

    let HandlerOutput { primary: resp, .. } = handle_ban_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { "b1": { "reason": 5 } } }),
    )
    .await
    .expect("SpaceBan/set");
    assert_rejected_naming(&resp, "b1", "reason");

    let HandlerOutput { primary: resp, .. } =
        handle_ban_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("SpaceBan/get");
    assert_eq!(single(resp)["reason"], "spam");
}
