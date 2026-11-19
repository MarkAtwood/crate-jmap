//! `ChatBan/set` and `ChatBan/changes` argument, validation and
//! backend-failure branches (bd:JMAP-it7g.1).
//!
//! Oracles: draft-atwood-jmap-chat-00 §ChatBan and §ChatBan Methods and
//! RFC 8620 §1.1, §1.2, §3.6, §5.2 and §5.3, quoted on each test; expected
//! responses are hand-written.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, INJECTABLE_BACKEND_CANARY, InjectableBackend};
use jmap_chat_server::{handle_chat_ban_changes, handle_chat_ban_set};
use jmap_server::HandlerOutput;
use jmap_types::JmapError;
use serde_json::{Value, json};

const BAN_ID: &str = "ban1";
const CHAT_ID: &str = "g1";

/// A backend holding one group Chat where the owner is an admin and
/// `target` a member, and one stored ChatBan in it.
fn seeded() -> InjectableBackend {
    let backend = InjectableBackend::new();
    backend.inner.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        CHAT_ID,
        json!({
            "id": CHAT_ID,
            "kind": "group",
            "name": "Team",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "members": [
                { "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "target2", "role": "member", "joinedAt": "2026-01-01T00:00:00Z" },
            ],
        }),
    );
    backend.inner.insert_object_for_test(
        "ChatBan",
        ACCOUNT_ID,
        BAN_ID,
        json!({
            "id": BAN_ID,
            "chatId": CHAT_ID,
            "userId": "target",
            "bannedBy": ACCOUNT_ID,
            "createdAt": "2026-01-01T00:00:00Z",
        }),
    );
    backend
}

async fn set(backend: &InjectableBackend, body: Value) -> Result<Value, JmapError> {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_chat_ban_set(backend, &(), &BTreeMap::new(), args)
        .await
        .map(|HandlerOutput { primary, .. }| primary)
}

/// Assert `resp[map][key]` is a SetError of `error_type` and, when given,
/// that its `properties` equal `properties`.
fn assert_set_error(
    resp: &Value,
    map: &str,
    key: &str,
    error_type: &str,
    properties: Option<Value>,
) {
    assert_eq!(resp[map][key]["type"], error_type, "{resp}");
    if let Some(props) = properties {
        assert_eq!(resp[map][key]["properties"], props, "{resp}");
    }
}

fn create(body: Value) -> Value {
    json!({ "create": { "b0": body } })
}

/// Draft §ChatBan/changes: "Standard JMAP `/changes`." RFC 8620 §5.2:
/// `sinceState` is required; §3.6.2 `invalidArguments`: "a required
/// argument is missing."
#[tokio::test]
async fn changes_without_since_state_is_invalid_arguments() {
    let err = handle_chat_ban_changes(&seeded(), &(), json!({ "accountId": ACCOUNT_ID }))
        .await
        .expect_err("missing sinceState");
    assert_eq!(err.error_type.to_string(), "invalidArguments", "{err:?}");
}

/// RFC 8620 §5.3 `ifInState`: "If supplied, the string must match the
/// current state; otherwise, the method will be aborted and a
/// "stateMismatch" error returned."
#[tokio::test]
async fn set_with_stale_if_in_state_is_state_mismatch() {
    let err = set(
        &seeded(),
        json!({ "ifInState": "not-the-state", "destroy": [BAN_ID] }),
    )
    .await
    .expect_err("stale ifInState");
    assert_eq!(err.error_type.to_string(), "stateMismatch", "{err:?}");
}

/// Draft §ChatBan/set: "`create` accepts: `chatId` (String, required),
/// `userId` (String, required)". A missing one is `invalidProperties`
/// naming it (RFC 8620 §5.3).
#[tokio::test]
async fn create_without_required_property_is_invalid_properties() {
    for (body, prop) in [
        (json!({ "userId": "target2" }), "chatId"),
        (json!({ "chatId": CHAT_ID }), "userId"),
    ] {
        let resp = set(&seeded(), create(body)).await.expect("ChatBan/set");
        assert_set_error(
            &resp,
            "notCreated",
            "b0",
            "invalidProperties",
            Some(json!([prop])),
        );
    }
}

/// A `chatId` that names no Chat cannot be a group Chat; the draft names
/// no error for it, so it is `invalidProperties` naming `chatId` (RFC 8620
/// §5.3: "The record given is invalid in some way"). A `#creationId`
/// reference to no record created in the request fails the same way.
#[tokio::test]
async fn create_with_unknown_chat_is_invalid_properties() {
    for chat in ["missing", "#nope"] {
        let resp = set(
            &seeded(),
            create(json!({ "chatId": chat, "userId": "target2" })),
        )
        .await
        .expect("ChatBan/set");
        assert_eq!(resp["created"], Value::Null, "{chat}: {resp}");
        assert_set_error(
            &resp,
            "notCreated",
            "b0",
            "invalidProperties",
            Some(json!(["chatId"])),
        );
    }
}

/// Draft §ChatBan: "`bannedBy` (String, immutable, server-set)" and
/// "`createdAt` (UTCDate, immutable, server-set)". RFC 8620 §1.1: for a
/// server-set property "The client MUST NOT send this property when
/// creating a new object of this type."
#[tokio::test]
async fn create_supplying_server_set_property_is_invalid_properties() {
    for prop in ["bannedBy", "createdAt"] {
        let mut body = json!({ "chatId": CHAT_ID, "userId": "target2" });
        body[prop] = json!("2026-01-01T00:00:00Z");
        let resp = set(&seeded(), create(body)).await.expect("ChatBan/set");
        assert_set_error(
            &resp,
            "notCreated",
            "b0",
            "invalidProperties",
            Some(json!([prop])),
        );
    }
}

/// Draft §ChatBan: `reason` is a String and `expiresAt` a UTCDate (RFC
/// 8620 §1.4). A wrongly typed or malformed value is `invalidProperties`
/// naming it, and RFC 8620 §1.2 malformed ids fail the same way.
#[tokio::test]
async fn create_with_bad_values_is_invalid_properties() {
    for (body, prop) in [
        (
            json!({ "chatId": CHAT_ID, "userId": "target2", "reason": 7 }),
            "reason",
        ),
        (
            json!({ "chatId": CHAT_ID, "userId": "target2", "expiresAt": "tomorrow" }),
            "expiresAt",
        ),
        (json!({ "chatId": "bad id", "userId": "target2" }), "chatId"),
        (json!({ "chatId": CHAT_ID, "userId": "bad id" }), "userId"),
    ] {
        let resp = set(&seeded(), create(body)).await.expect("ChatBan/set");
        assert_set_error(
            &resp,
            "notCreated",
            "b0",
            "invalidProperties",
            Some(json!([prop])),
        );
    }
}

/// Draft §ChatBan: "`chatId` (String, immutable)". RFC 8620 §1.1: an
/// immutable value "MUST NOT change after the object is created".
#[tokio::test]
async fn update_of_immutable_chat_id_is_invalid_properties() {
    let resp = set(
        &seeded(),
        json!({ "update": { BAN_ID: { "chatId": "g2", "reason": "x" } } }),
    )
    .await
    .expect("ChatBan/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notUpdated",
        BAN_ID,
        "invalidProperties",
        Some(json!(["chatId"])),
    );
}

/// Draft §ChatBan/set "`update` supports: `reason`, `expiresAt`", so a
/// patch naming neither, or a patch that is not an object, is
/// `invalidPatch` (RFC 8620 §5.3).
#[tokio::test]
async fn update_with_invalid_patch_is_invalid_patch() {
    for patch in [json!("spam"), json!({ "unknownProperty": 1 })] {
        let resp = set(&seeded(), json!({ "update": { BAN_ID: patch.clone() } }))
            .await
            .expect("ChatBan/set");
        assert_eq!(resp["updated"], Value::Null, "{patch}: {resp}");
        assert_set_error(&resp, "notUpdated", BAN_ID, "invalidPatch", None);
    }
}

/// RFC 8620 §5.3: "notFound: The id given to update/destroy cannot be
/// found."
#[tokio::test]
async fn update_and_destroy_of_unknown_ban_are_not_found() {
    let resp = set(
        &seeded(),
        json!({ "update": { "missing": { "reason": "x" } }, "destroy": ["missing2"] }),
    )
    .await
    .expect("ChatBan/set");
    assert_set_error(&resp, "notUpdated", "missing", "notFound", None);
    assert_set_error(&resp, "notDestroyed", "missing2", "notFound", None);
}

/// RFC 8620 §5.3 `destroy` is "Id[]|null"; a number element is the wrong
/// type (§3.6.2 `invalidArguments`).
#[tokio::test]
async fn destroy_with_non_string_element_is_invalid_arguments() {
    let err = set(&seeded(), json!({ "destroy": [BAN_ID, 7] }))
        .await
        .expect_err("non-string destroy element");
    assert_eq!(err.error_type.to_string(), "invalidArguments", "{err:?}");
}

/// RFC 8620 §5.3: "Any server-set properties MAY be included in the patch
/// if their value is identical to the current server value".
#[tokio::test]
async fn update_with_identical_read_only_values_is_updated() {
    let resp = set(
        &seeded(),
        json!({ "update": { BAN_ID: {
            "id": BAN_ID,
            "chatId": CHAT_ID,
            "userId": "target",
            "bannedBy": ACCOUNT_ID,
            "createdAt": "2026-01-01T00:00:00Z",
            "reason": "x",
        } } }),
    )
    .await
    .expect("ChatBan/set");
    assert_eq!(resp["notUpdated"], Value::Null, "{resp}");
    assert!(resp["updated"].get(BAN_ID).is_some(), "{resp}");
}

/// RFC 8620 §3.6.1 `serverFail`: "An unexpected error occurred during the
/// processing of the call." Each injected backend fault fails only its
/// target as `serverFail` (§5.3), without leaking the backend's error
/// text.
#[tokio::test]
async fn backend_faults_fail_the_target_with_server_fail() {
    let cases: [(&str, &str, Value, &str, &str); 5] = [
        (
            "ChatBan",
            "create",
            create(json!({ "chatId": CHAT_ID, "userId": "target2" })),
            "notCreated",
            "b0",
        ),
        (
            "Chat",
            "get",
            create(json!({ "chatId": CHAT_ID, "userId": "target2" })),
            "notCreated",
            "b0",
        ),
        (
            "ChatBan",
            "get",
            json!({ "update": { BAN_ID: { "reason": "x" } } }),
            "notUpdated",
            BAN_ID,
        ),
        (
            "ChatBan",
            "get",
            json!({ "destroy": [BAN_ID] }),
            "notDestroyed",
            BAN_ID,
        ),
        (
            "ChatBan",
            "destroy",
            json!({ "destroy": [BAN_ID] }),
            "notDestroyed",
            BAN_ID,
        ),
    ];
    for (type_name, op, body, map, key) in cases {
        let backend = seeded();
        backend.inject(type_name, op);
        let resp = set(&backend, body).await.expect("ChatBan/set");
        assert_eq!(
            resp[map][key]["type"], "serverFail",
            "{type_name}/{op}: {resp}"
        );
        assert!(
            !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
            "{type_name}/{op} leaked backend text: {resp}"
        );
    }
}
