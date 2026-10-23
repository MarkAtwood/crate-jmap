//! `SpaceBan/set` and `SpaceBan/changes` argument, validation and
//! backend-failure branches (bd:JMAP-w8fu.2).
//!
//! Oracles: draft-atwood-jmap-chat-00 §SpaceBan and §SpaceBan Methods
//! (jmap-chat-spec 29e9a41 L966-989, L1515-1527) and RFC 8620 §1.1, §1.2,
//! §3.6, §5.2 and §5.3, quoted on each test; expected responses are
//! hand-written.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, INJECTABLE_BACKEND_CANARY, InjectableBackend, SPACE_ID};
use jmap_chat_server::{handle_ban_changes, handle_ban_set};
use jmap_server::HandlerOutput;
use jmap_types::JmapError;
use serde_json::{Value, json};

const BAN_ID: &str = "ban1";

/// A backend holding one stored SpaceBan. Its authorization hooks are
/// the fail-open defaults, so only the branch under test can fail.
fn seeded() -> InjectableBackend {
    let backend = InjectableBackend::new();
    backend.inner.insert_object_for_test(
        "SpaceBan",
        ACCOUNT_ID,
        BAN_ID,
        json!({
            "id": BAN_ID,
            "spaceId": SPACE_ID,
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
    handle_ban_set(backend, &(), &BTreeMap::new(), args)
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

/// Draft §SpaceBan/changes: "Standard JMAP `/changes`." RFC 8620 §5.2:
/// `sinceState` is required; §3.6.2 `invalidArguments`: "a required
/// argument is missing."
#[tokio::test]
async fn changes_without_since_state_is_invalid_arguments() {
    let err = handle_ban_changes(&seeded(), &(), json!({ "accountId": ACCOUNT_ID }))
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

/// RFC 8620 §5.3: a `#creationId` reference names a record created
/// earlier in the request. No record was created under `nope`, so the
/// create fails with `invalidProperties` naming `spaceId`.
#[tokio::test]
async fn create_with_unknown_creation_reference_is_invalid_properties() {
    let resp = set(
        &seeded(),
        create(json!({ "spaceId": "#nope", "userId": "target2" })),
    )
    .await
    .expect("SpaceBan/set");
    assert_eq!(resp["created"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notCreated",
        "b0",
        "invalidProperties",
        Some(json!(["spaceId"])),
    );
}

/// Draft §SpaceBan: "`bannedBy` (String, immutable, server-set)". RFC 8620
/// §1.1: for a server-set property "The client MUST NOT send this property
/// when creating a new object of this type."
#[tokio::test]
async fn create_supplying_banned_by_is_invalid_properties() {
    let resp = set(
        &seeded(),
        create(json!({ "spaceId": SPACE_ID, "userId": "target2", "bannedBy": "someone" })),
    )
    .await
    .expect("SpaceBan/set");
    assert_eq!(resp["created"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notCreated",
        "b0",
        "invalidProperties",
        Some(json!(["bannedBy"])),
    );
}

/// RFC 8620 §1.2: an Id "MUST only contain characters from the 'URL and
/// Filename Safe' base64 alphabet"; `"bad id"` contains a space, so each
/// Id-typed create property fails naming itself.
#[tokio::test]
async fn create_with_malformed_ids_is_invalid_properties() {
    for (body, prop) in [
        (
            json!({ "spaceId": "bad id", "userId": "target2" }),
            "spaceId",
        ),
        (json!({ "spaceId": SPACE_ID, "userId": "bad id" }), "userId"),
    ] {
        let resp = set(&seeded(), create(body)).await.expect("SpaceBan/set");
        assert_eq!(resp["created"], Value::Null, "{prop}: {resp}");
        assert_set_error(
            &resp,
            "notCreated",
            "b0",
            "invalidProperties",
            Some(json!([prop])),
        );
    }
}

/// RFC 8620 §1.2 malformed id on update: `"bad id"` fails with
/// `invalidProperties` naming `id`.
#[tokio::test]
async fn update_with_malformed_id_is_invalid_properties() {
    let resp = set(
        &seeded(),
        json!({ "update": { "bad id": { "reason": "x" } } }),
    )
    .await
    .expect("SpaceBan/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notUpdated",
        "bad id",
        "invalidProperties",
        Some(json!(["id"])),
    );
}

/// Draft §SpaceBan: "`spaceId` (String, immutable)". RFC 8620 §1.1: an
/// immutable value "MUST NOT change after the object is created"; the
/// update fails with `invalidProperties` naming `spaceId`.
#[tokio::test]
async fn update_of_immutable_space_id_is_invalid_properties() {
    let resp = set(
        &seeded(),
        json!({ "update": { BAN_ID: { "spaceId": "s2", "reason": "x" } } }),
    )
    .await
    .expect("SpaceBan/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notUpdated",
        BAN_ID,
        "invalidProperties",
        Some(json!(["spaceId"])),
    );
}

/// RFC 8620 §5.3 `update` is "Id[PatchObject]|null"; `invalidPatch`: "The
/// PatchObject given to update the record was not a valid patch". A
/// string is not a PatchObject, and draft §SpaceBan/set "`update`
/// supports: `reason`, `expiresAt`", so a patch naming neither is not a
/// valid SpaceBan patch either.
#[tokio::test]
async fn update_with_invalid_patch_is_invalid_patch() {
    for patch in [json!("spam"), json!({ "unknownProperty": 1 })] {
        let resp = set(&seeded(), json!({ "update": { BAN_ID: patch.clone() } }))
            .await
            .expect("SpaceBan/set");
        assert_eq!(resp["updated"], Value::Null, "{patch}: {resp}");
        assert_set_error(&resp, "notUpdated", BAN_ID, "invalidPatch", None);
    }
}

/// RFC 8620 §5.3 update: "notFound: The id given to update/destroy cannot
/// be found."
#[tokio::test]
async fn update_of_unknown_ban_is_not_found() {
    let resp = set(
        &seeded(),
        json!({ "update": { "missing": { "reason": "x" } } }),
    )
    .await
    .expect("SpaceBan/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_set_error(&resp, "notUpdated", "missing", "notFound", None);
}

/// RFC 8620 §5.3 `destroy` is "Id[]|null"; a number element is the wrong
/// type (§3.6.2 `invalidArguments`: "One of the arguments is of the wrong
/// type").
#[tokio::test]
async fn destroy_with_non_string_element_is_invalid_arguments() {
    let err = set(&seeded(), json!({ "destroy": [BAN_ID, 7] }))
        .await
        .expect_err("non-string destroy element");
    assert_eq!(err.error_type.to_string(), "invalidArguments", "{err:?}");
}

/// RFC 8620 §1.2 malformed id on destroy: `"bad id"` fails with
/// `invalidProperties` naming `id`.
#[tokio::test]
async fn destroy_with_malformed_id_is_invalid_properties() {
    let resp = set(&seeded(), json!({ "destroy": ["bad id"] }))
        .await
        .expect("SpaceBan/set");
    assert_eq!(resp["destroyed"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notDestroyed",
        "bad id",
        "invalidProperties",
        Some(json!(["id"])),
    );
}

/// RFC 8620 §3.6.1 `serverFail`: "An unexpected error occurred during the
/// processing of the call." Each injected backend fault fails only its
/// target as `serverFail` (§5.3), without leaking the backend's error
/// text.
#[tokio::test]
async fn backend_faults_fail_the_target_with_server_fail() {
    let cases: [(&str, Value, &str, &str); 4] = [
        (
            "create",
            create(json!({ "spaceId": SPACE_ID, "userId": "target2" })),
            "notCreated",
            "b0",
        ),
        (
            "get",
            json!({ "update": { BAN_ID: { "reason": "x" } } }),
            "notUpdated",
            BAN_ID,
        ),
        (
            "get",
            json!({ "destroy": [BAN_ID] }),
            "notDestroyed",
            BAN_ID,
        ),
        (
            "destroy",
            json!({ "destroy": [BAN_ID] }),
            "notDestroyed",
            BAN_ID,
        ),
    ];
    for (op, body, map, key) in cases {
        let backend = seeded();
        backend.inject("SpaceBan", op);
        let resp = set(&backend, body).await.expect("SpaceBan/set");
        assert_eq!(
            resp[map][key]["type"], "serverFail",
            "SpaceBan/{op}: {resp}"
        );
        assert!(
            !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
            "SpaceBan/{op} leaked backend text: {resp}"
        );
    }
}
