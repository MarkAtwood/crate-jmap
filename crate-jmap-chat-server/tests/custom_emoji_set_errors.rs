//! `CustomEmoji/set` and `CustomEmoji/changes` argument, validation,
//! authorization-gate and backend-failure branches (bd:JMAP-w8fu.1).
//!
//! Oracles: draft-atwood-jmap-chat-00 §CustomEmoji and §CustomEmoji
//! Methods (jmap-chat-spec 29e9a41 L913-934, L1456-1476) and RFC 8620
//! §1.1, §1.2, §3.6, §5.2 and §5.3, quoted on each test; expected
//! responses are hand-written.

mod common;

use std::collections::BTreeMap;

use common::{
    ACCOUNT_ID, INJECTABLE_BACKEND_CANARY, InjectableBackend, MemoryBackend, SPACE_ID,
    TrackingBackend,
};
use jmap_chat_server::{ChatBackend, EmojiSetOp, handle_emoji_changes, handle_emoji_set};
use jmap_server::HandlerOutput;
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

/// A stored emoji scoped to Space [`SPACE_ID`].
const SPACE_EMOJI: &str = "emojiSpace";
/// A stored server-global emoji (no `spaceId`).
const GLOBAL_EMOJI: &str = "emojiGlobal";

/// Seed one Space-scoped and one server-global CustomEmoji.
fn seed(inner: &MemoryBackend) {
    for (id, space_id) in [(SPACE_EMOJI, Some(SPACE_ID)), (GLOBAL_EMOJI, None)] {
        let mut emoji = json!({
            "id": id,
            "name": id.to_lowercase(),
            "blobId": "blob1",
            "createdBy": ACCOUNT_ID,
            "createdAt": "2026-01-01T00:00:00Z",
        });
        if let Some(s) = space_id {
            emoji["spaceId"] = json!(s);
        }
        inner.insert_object_for_test("CustomEmoji", ACCOUNT_ID, id, emoji);
    }
}

fn injectable() -> InjectableBackend {
    let backend = InjectableBackend::new();
    seed(&backend.inner);
    backend
}

fn tracking(backend: TrackingBackend) -> TrackingBackend {
    seed(backend.inner());
    backend
}

async fn set<B: ChatBackend<CallerCtx = ()>>(backend: &B, body: Value) -> Result<Value, JmapError> {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_emoji_set(backend, &(), &BTreeMap::new(), args)
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
    json!({ "create": { "e0": body } })
}

fn space_id() -> Id {
    Id::new_validated(SPACE_ID).expect("valid Id")
}

/// Draft §CustomEmoji/changes: "Standard JMAP `/changes`." RFC 8620
/// §5.2: `sinceState` is required; §3.6.2 `invalidArguments`: "a
/// required argument is missing."
#[tokio::test]
async fn changes_without_since_state_is_invalid_arguments() {
    let err = handle_emoji_changes(&injectable(), &(), json!({ "accountId": ACCOUNT_ID }))
        .await
        .expect_err("missing sinceState");
    assert_eq!(err.error_type.to_string(), "invalidArguments", "{err:?}");
}

/// RFC 8620 §5.2 (rfc8620.txt L1761-1768): over `maxChanges` "the server
/// SHOULD generate an update to take the client to an intermediate state,
/// from which the client can continue to call "Foo/changes" until it is
/// fully up to date." Two creates against `maxChanges` 1 come back as two
/// pages of one id each.
#[tokio::test]
async fn changes_over_max_changes_returns_intermediate_state() {
    let backend = injectable();
    let resp = set(
        &backend,
        json!({ "create": {
            "e0": { "name": "catjam", "blobId": "blob1" },
            "e1": { "name": "partyparrot", "blobId": "blob1" },
        } }),
    )
    .await
    .expect("CustomEmoji/set");
    let created: Vec<Value> = resp["created"]
        .as_object()
        .expect("created map")
        .values()
        .map(|e| e["id"].clone())
        .collect();
    assert_eq!(created.len(), 2, "{resp}");

    let first = handle_emoji_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": resp["oldState"], "maxChanges": 1 }),
    )
    .await
    .map(|HandlerOutput { primary, .. }| primary)
    .expect("first page");
    assert_eq!(first["hasMoreChanges"], json!(true), "{first}");
    assert_eq!(
        first["created"].as_array().map(Vec::len),
        Some(1),
        "{first}"
    );

    let rest = handle_emoji_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": first["newState"], "maxChanges": 1 }),
    )
    .await
    .map(|HandlerOutput { primary, .. }| primary)
    .expect("second page");
    assert_eq!(rest["hasMoreChanges"], json!(false), "{rest}");
    assert_eq!(rest["newState"], resp["newState"], "{rest}");
    let mut paged = vec![first["created"][0].clone(), rest["created"][0].clone()];
    let mut want = created;
    paged.sort_by_key(Value::to_string);
    want.sort_by_key(Value::to_string);
    assert_eq!(paged, want);
}

/// RFC 8620 §5.3 `ifInState`: "If supplied, the string must match the
/// current state; otherwise, the method will be aborted and a
/// "stateMismatch" error returned."
#[tokio::test]
async fn set_with_stale_if_in_state_is_state_mismatch() {
    let err = set(
        &injectable(),
        json!({ "ifInState": "not-the-state", "destroy": [GLOBAL_EMOJI] }),
    )
    .await
    .expect_err("stale ifInState");
    assert_eq!(err.error_type.to_string(), "stateMismatch", "{err:?}");
}

/// Draft §CustomEmoji `name`: "MUST contain only lowercase alphanumeric
/// characters, hyphens, and underscores"; §CustomEmoji/set "`create`
/// accepts: `name` (String, required), `blobId` (String, required)".
/// RFC 8620 §5.3 `invalidProperties`: "The record given is invalid in
/// some way."
#[tokio::test]
async fn create_with_invalid_or_missing_required_properties_is_invalid_properties() {
    for (body, prop) in [
        (json!({ "name": "CatJam", "blobId": "blob1" }), "name"),
        (json!({ "name": "cat jam", "blobId": "blob1" }), "name"),
        (json!({ "name": "", "blobId": "blob1" }), "name"),
        (json!({ "blobId": "blob1" }), "name"),
        (json!({ "name": "catjam" }), "blobId"),
    ] {
        let resp = set(&injectable(), create(body.clone()))
            .await
            .expect("CustomEmoji/set");
        assert_eq!(resp["created"], Value::Null, "{body}: {resp}");
        assert_set_error(
            &resp,
            "notCreated",
            "e0",
            "invalidProperties",
            Some(json!([prop])),
        );
    }
}

/// Draft §CustomEmoji: "`createdBy` (String, immutable, server-set)".
/// RFC 8620 §1.1: for a server-set property "The client MUST NOT send
/// this property when creating a new object of this type."
#[tokio::test]
async fn create_supplying_created_by_is_invalid_properties() {
    let resp = set(
        &injectable(),
        create(json!({ "name": "catjam", "blobId": "blob1", "createdBy": "someone" })),
    )
    .await
    .expect("CustomEmoji/set");
    assert_eq!(resp["created"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notCreated",
        "e0",
        "invalidProperties",
        Some(json!(["createdBy"])),
    );
}

/// RFC 8620 §1.2: an Id "MUST only contain characters from the 'URL and
/// Filename Safe' base64 alphabet"; `"bad id"` contains a space. RFC 8620
/// §5.3: a `#creationId` reference names a record created earlier in the
/// request, and none was created under `nope`. Both fail naming
/// `spaceId`.
#[tokio::test]
async fn create_with_bad_space_id_is_invalid_properties() {
    for space in ["bad id", "#nope"] {
        let resp = set(
            &injectable(),
            create(json!({ "name": "catjam", "blobId": "blob1", "spaceId": space })),
        )
        .await
        .expect("CustomEmoji/set");
        assert_eq!(resp["created"], Value::Null, "{space}: {resp}");
        assert_set_error(
            &resp,
            "notCreated",
            "e0",
            "invalidProperties",
            Some(json!(["spaceId"])),
        );
    }
}

/// RFC 8620 §1.2 malformed id on update and destroy: `"bad id"` fails
/// with `invalidProperties` naming `id`.
#[tokio::test]
async fn update_and_destroy_with_malformed_id_are_invalid_properties() {
    let resp = set(
        &injectable(),
        json!({ "update": { "bad id": { "name": "x" } }, "destroy": ["bad id"] }),
    )
    .await
    .expect("CustomEmoji/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_eq!(resp["destroyed"], Value::Null, "{resp}");
    for map in ["notUpdated", "notDestroyed"] {
        assert_set_error(
            &resp,
            map,
            "bad id",
            "invalidProperties",
            Some(json!(["id"])),
        );
    }
}

/// Draft §CustomEmoji/set: "`update` supports: `name`, `blobId`";
/// `spaceId` is accepted only by `create`. RFC 8620 §5.3
/// `invalidProperties` for a property the update may not set.
#[tokio::test]
async fn update_of_space_id_is_invalid_properties() {
    let resp = set(
        &injectable(),
        json!({ "update": { GLOBAL_EMOJI: { "spaceId": SPACE_ID, "name": "x" } } }),
    )
    .await
    .expect("CustomEmoji/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_set_error(
        &resp,
        "notUpdated",
        GLOBAL_EMOJI,
        "invalidProperties",
        Some(json!(["spaceId"])),
    );
}

/// RFC 8620 §5.3 `update` is "Id[PatchObject]|null"; `invalidPatch`: "The
/// PatchObject given to update the record was not a valid patch". A
/// string is not a PatchObject, and a patch naming neither `name` nor
/// `blobId` (draft §CustomEmoji/set) is not a valid CustomEmoji patch.
#[tokio::test]
async fn update_with_invalid_patch_is_invalid_patch() {
    for patch in [json!("rename"), json!({ "unknownProperty": 1 })] {
        let resp = set(
            &injectable(),
            json!({ "update": { GLOBAL_EMOJI: patch.clone() } }),
        )
        .await
        .expect("CustomEmoji/set");
        assert_eq!(resp["updated"], Value::Null, "{patch}: {resp}");
        assert_set_error(&resp, "notUpdated", GLOBAL_EMOJI, "invalidPatch", None);
    }
}

/// RFC 8620 §5.3 `destroy` is "Id[]|null"; a number element is the wrong
/// type (§3.6.2 `invalidArguments`: "One of the arguments is of the wrong
/// type").
#[tokio::test]
async fn destroy_with_non_string_element_is_invalid_arguments() {
    let err = set(&injectable(), json!({ "destroy": [GLOBAL_EMOJI, 7] }))
        .await
        .expect_err("non-string destroy element");
    assert_eq!(err.error_type.to_string(), "invalidArguments", "{err:?}");
}

/// RFC 8620 §5.3: "notFound: The id given to update/destroy cannot be
/// found." A missing destroy target is `notFound`, not the `forbidden`
/// the denying gate would produce for an existing emoji.
#[tokio::test]
async fn destroy_of_unknown_emoji_is_not_found_even_when_gate_denies() {
    let backend = tracking(TrackingBackend::with_emoji_set_denied());
    let resp = set(&backend, json!({ "destroy": ["ghost"] }))
        .await
        .expect("CustomEmoji/set");
    assert_eq!(resp["destroyed"], Value::Null, "{resp}");
    assert_set_error(&resp, "notDestroyed", "ghost", "notFound", None);
}

/// Draft §CustomEmoji: an emoji is "server- or Space-scoped", and
/// §CustomEmoji/set requires `forbidden` "when the caller is not
/// authorized to act on the targeted emoji". The authorization decision
/// therefore needs the targeted emoji's scope: the `spaceId` the create
/// supplies, and the stored `spaceId` (absent for a server-global emoji)
/// on update and destroy.
#[tokio::test]
async fn gate_receives_the_targeted_emoji_scope_and_op() {
    let backend = tracking(TrackingBackend::new());
    let resp = set(
        &backend,
        json!({
            "create": { "e0": { "name": "catjam", "blobId": "blob1", "spaceId": SPACE_ID } },
            "update": { GLOBAL_EMOJI: { "name": "renamed" } },
            "destroy": [SPACE_EMOJI],
        }),
    )
    .await
    .expect("CustomEmoji/set");
    assert_eq!(resp["created"]["e0"]["spaceId"], SPACE_ID, "{resp}");
    assert!(resp["updated"].get(GLOBAL_EMOJI).is_some(), "{resp}");
    assert_eq!(resp["destroyed"], json!([SPACE_EMOJI]), "{resp}");
    assert_eq!(
        backend.emoji_set_calls(),
        vec![
            (Some(space_id()), EmojiSetOp::Create),
            (None, EmojiSetOp::Update),
            (Some(space_id()), EmojiSetOp::Destroy),
        ]
    );
}

/// RFC 8620 §5.3: a create that fails validation is `invalidProperties`
/// whatever the caller's rights, so the authorization gate has nothing
/// to decide for it.
#[tokio::test]
async fn invalid_create_does_not_consult_gate() {
    let backend = tracking(TrackingBackend::with_emoji_set_denied());
    let resp = set(
        &backend,
        create(json!({ "name": "Bad!", "blobId": "blob1" })),
    )
    .await
    .expect("CustomEmoji/set");
    assert_set_error(
        &resp,
        "notCreated",
        "e0",
        "invalidProperties",
        Some(json!(["name"])),
    );
    assert_eq!(backend.emoji_set_calls(), vec![]);
}

/// RFC 8620 §3.6.1 `serverFail`: "An unexpected error occurred during the
/// processing of the call." When the authorization gate itself fails,
/// each target fails as `serverFail` (§5.3), without leaking the
/// backend's error text, and nothing is created, updated or destroyed.
#[tokio::test]
async fn gate_failure_fails_each_target_with_server_fail() {
    let backend = tracking(TrackingBackend::with_emoji_set_failing());
    let resp = set(
        &backend,
        json!({
            "create": { "e0": { "name": "catjam", "blobId": "blob1" } },
            "update": { GLOBAL_EMOJI: { "name": "renamed" } },
            "destroy": [SPACE_EMOJI],
        }),
    )
    .await
    .expect("CustomEmoji/set");
    for (map, key) in [
        ("notCreated", "e0"),
        ("notUpdated", GLOBAL_EMOJI),
        ("notDestroyed", SPACE_EMOJI),
    ] {
        assert_set_error(&resp, map, key, "serverFail", None);
    }
    for map in ["created", "updated", "destroyed"] {
        assert_eq!(resp[map], Value::Null, "{map}: {resp}");
    }
    assert!(
        !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
        "leaked backend text: {resp}"
    );
}

/// RFC 8620 §3.6.1 `serverFail`: "An unexpected error occurred during the
/// processing of the call." Each injected storage fault (the create, the
/// pre-fetch of the target before update or destroy, the update, the
/// destroy) fails only its target as `serverFail` (§5.3), without leaking
/// the backend's error text.
#[tokio::test]
async fn backend_faults_fail_the_target_with_server_fail() {
    let cases: [(&str, Value, &str, &str); 5] = [
        (
            "create",
            create(json!({ "name": "catjam", "blobId": "blob1" })),
            "notCreated",
            "e0",
        ),
        (
            "get",
            json!({ "update": { GLOBAL_EMOJI: { "name": "renamed" } } }),
            "notUpdated",
            GLOBAL_EMOJI,
        ),
        (
            "update",
            json!({ "update": { GLOBAL_EMOJI: { "name": "renamed" } } }),
            "notUpdated",
            GLOBAL_EMOJI,
        ),
        (
            "get",
            json!({ "destroy": [SPACE_EMOJI] }),
            "notDestroyed",
            SPACE_EMOJI,
        ),
        (
            "destroy",
            json!({ "destroy": [SPACE_EMOJI] }),
            "notDestroyed",
            SPACE_EMOJI,
        ),
    ];
    for (op, body, map, key) in cases {
        let backend = injectable();
        backend.inject("CustomEmoji", op);
        let resp = set(&backend, body).await.expect("CustomEmoji/set");
        assert_eq!(
            resp[map][key]["type"], "serverFail",
            "CustomEmoji/{op}: {resp}"
        );
        assert!(
            !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
            "CustomEmoji/{op} leaked backend text: {resp}"
        );
    }
}
