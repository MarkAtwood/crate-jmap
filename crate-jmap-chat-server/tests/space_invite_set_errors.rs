//! `SpaceInvite/set` and `SpaceInvite/changes` argument, validation and
//! backend-failure branches (bd:JMAP-w8fu.9).
//!
//! Oracles: draft-atwood-jmap-chat-00 §SpaceInvite/set and RFC 8620 §3.6.1,
//! §5.2 and §5.3, quoted on each test; expected responses are hand-written.

mod common;

use common::{
    ACCOUNT_ID, INJECTABLE_BACKEND_CANARY, IdentityBackend, InjectableBackend, SPACE_ID, seed_space,
};
use jmap_chat_server::{handle_invite_changes, handle_invite_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const INVITE_ID: &str = "inv1";

/// A Space whose owner (member entry = accountId, draft §Terminology)
/// holds `manage_members`, plus one stored SpaceInvite.
fn seeded() -> IdentityBackend {
    let backend = IdentityBackend::new();
    seed_space(
        &backend,
        json!([{ "id": "r-mm", "name": "Mod", "permissions": ["manage_members"], "position": 10 }]),
        json!([{ "id": ACCOUNT_ID, "roleIds": ["r-mm"], "joinedAt": "2026-01-01T00:00:00Z" }]),
    );
    backend.inner().insert_object_for_test(
        "SpaceInvite",
        ACCOUNT_ID,
        INVITE_ID,
        json!({
            "id": INVITE_ID,
            "code": "code1",
            "spaceId": SPACE_ID,
            "createdBy": ACCOUNT_ID,
            "uses": 0,
            "createdAt": "2026-01-01T00:00:00Z",
        }),
    );
    backend
}

fn with_account(body: Value) -> Value {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    let (Value::Object(args_map), Value::Object(body_map)) = (&mut args, body) else {
        unreachable!("both are object literals");
    };
    args_map.extend(body_map);
    args
}

async fn set(backend: &IdentityBackend, body: Value) -> Result<Value, jmap_types::JmapError> {
    handle_invite_set(
        backend,
        &Id::from(ACCOUNT_ID),
        &BTreeMap::new(),
        with_account(body),
    )
    .await
    .map(|HandlerOutput { primary, .. }| primary)
}

async fn set_injected(backend: &InjectableBackend, body: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_invite_set(backend, &(), &BTreeMap::new(), with_account(body))
            .await
            .expect("SpaceInvite/set");
    primary
}

/// RFC 8620 §5.2: `sinceState` is a required argument of `/changes`.
#[tokio::test]
async fn changes_without_since_state_is_invalid_arguments() {
    let err = handle_invite_changes(&seeded(), &Id::from(ACCOUNT_ID), with_account(json!({})))
        .await
        .expect_err("missing sinceState");
    assert_eq!(err.error_type.to_string(), "invalidArguments", "{err:?}");
}

/// RFC 8620 §5.3 `ifInState`: "If supplied, the string must match the
/// current state, otherwise the method will be aborted and a
/// `stateMismatch` error returned."
#[tokio::test]
async fn set_with_stale_if_in_state_is_state_mismatch() {
    let err = set(
        &seeded(),
        json!({ "ifInState": "not-the-state", "destroy": [INVITE_ID] }),
    )
    .await
    .expect_err("stale ifInState");
    assert_eq!(err.error_type.to_string(), "stateMismatch", "{err:?}");
}

/// RFC 8620 §5.3: an unresolvable `#creationId` reference fails that
/// create with `invalidProperties` naming the property.
#[tokio::test]
async fn create_with_unknown_creation_reference_is_invalid_properties() {
    let resp = set(
        &seeded(),
        json!({ "create": { "i0": { "spaceId": "#nope" } } }),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(resp["created"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notCreated"]["i0"]["type"], "invalidProperties",
        "{resp:?}"
    );
    assert_eq!(
        resp["notCreated"]["i0"]["properties"],
        json!(["spaceId"]),
        "{resp:?}"
    );
}

/// Draft §SpaceInvite/set: "The server assigns both `id` and `code`;
/// clients MUST NOT supply either." RFC 8620 §5.3: a server-set property
/// in a create is `invalidProperties`.
#[tokio::test]
async fn create_supplying_code_is_invalid_properties() {
    let resp = set(
        &seeded(),
        json!({ "create": { "i0": { "spaceId": SPACE_ID, "code": "mine" } } }),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(resp["created"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notCreated"]["i0"]["type"], "invalidProperties",
        "{resp:?}"
    );
    assert_eq!(
        resp["notCreated"]["i0"]["properties"],
        json!(["code"]),
        "{resp:?}"
    );
}

/// Draft §SpaceInvite/set: `create` accepts "`defaultChannelId` (String,
/// optional), `expiresAt` (UTCDate, optional)"; both are stored as given.
#[tokio::test]
async fn create_stores_default_channel_and_expiry() {
    let resp = set(
        &seeded(),
        json!({ "create": { "i0": {
            "spaceId": SPACE_ID,
            "defaultChannelId": "ch1",
            "expiresAt": "2030-01-01T00:00:00Z",
        } } }),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(resp["notCreated"], Value::Null, "{resp:?}");
    let created = &resp["created"]["i0"];
    assert_eq!(created["defaultChannelId"], "ch1", "{resp:?}");
    assert_eq!(created["expiresAt"], "2030-01-01T00:00:00Z", "{resp:?}");
}

/// RFC 8620 §5.3: `destroy` is an `Id[]`; a non-string element is an
/// `invalidArguments` method error.
#[tokio::test]
async fn destroy_with_non_string_element_is_invalid_arguments() {
    let err = set(&seeded(), json!({ "destroy": [42] }))
        .await
        .expect_err("non-string destroy element");
    assert_eq!(err.error_type.to_string(), "invalidArguments", "{err:?}");
}

/// RFC 8620 §1.2: an Id is 1-255 characters of `A-Za-z0-9-_`; a
/// malformed destroy id fails with `invalidProperties`.
#[tokio::test]
async fn destroy_with_malformed_id_is_invalid_properties() {
    let resp = set(&seeded(), json!({ "destroy": ["bad id!"] }))
        .await
        .expect("SpaceInvite/set");
    assert_eq!(resp["destroyed"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notDestroyed"]["bad id!"]["type"], "invalidProperties",
        "{resp:?}"
    );
}

/// RFC 8620 §5.3 destroy: "notFound: The id given to update/destroy
/// cannot be found."
#[tokio::test]
async fn destroy_of_unknown_invite_is_not_found() {
    let resp = set(&seeded(), json!({ "destroy": ["missing"] }))
        .await
        .expect("SpaceInvite/set");
    assert_eq!(resp["destroyed"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notDestroyed"]["missing"]["type"], "notFound",
        "{resp:?}"
    );
}

/// RFC 8620 §3.6.1 `serverFail`: "An unexpected error occurred during the
/// processing of the call." Each injected backend fault fails only its
/// target as `serverFail`, without leaking the backend's error text.
#[tokio::test]
async fn backend_faults_fail_the_target_with_server_fail() {
    let cases: [(&str, &str, Value, &str, &str); 4] = [
        (
            "Space",
            "get",
            json!({ "create": { "i0": { "spaceId": SPACE_ID } } }),
            "notCreated",
            "i0",
        ),
        (
            "SpaceInvite",
            "create",
            json!({ "create": { "i0": { "spaceId": SPACE_ID } } }),
            "notCreated",
            "i0",
        ),
        (
            "SpaceInvite",
            "get",
            json!({ "destroy": [INVITE_ID] }),
            "notDestroyed",
            INVITE_ID,
        ),
        (
            "SpaceInvite",
            "destroy",
            json!({ "destroy": [INVITE_ID] }),
            "notDestroyed",
            INVITE_ID,
        ),
    ];
    for (type_name, op, body, map, key) in cases {
        let backend = InjectableBackend::new();
        backend.inner.insert_object_for_test(
            "SpaceInvite",
            ACCOUNT_ID,
            INVITE_ID,
            json!({
                "id": INVITE_ID,
                "code": "code1",
                "spaceId": SPACE_ID,
                "createdBy": ACCOUNT_ID,
                "uses": 0,
                "createdAt": "2026-01-01T00:00:00Z",
            }),
        );
        backend.inject(type_name, op);

        let resp = set_injected(&backend, body).await;
        assert_eq!(
            resp[map][key]["type"], "serverFail",
            "{type_name}/{op}: {resp:?}"
        );
        assert!(
            !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
            "{type_name}/{op} leaked backend text: {resp:?}"
        );
    }
}
