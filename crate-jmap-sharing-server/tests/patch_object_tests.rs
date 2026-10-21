//! Principal/set `update` PatchObject semantics (RFC 8620 §5.3).
//!
//! Each test dispatches through `register_sharing_handlers` with the
//! in-memory `MemoryBackend` and observes the stored object via
//! Principal/get.

mod common;

use jmap_types::Invocation;
use std::sync::Arc;

use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_sharing_server::{register_sharing_handlers, SharingBackend};
use jmap_sharing_types::Principal;
use jmap_types::Id;
use serde_json::{json, Value};

use common::MemoryBackend;

/// Seed one Principal whose `capabilities` holds `{"urn:example:a": {"v": 1}}`
/// and return a dispatcher plus its id.
async fn setup() -> (Dispatcher<()>, Id) {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let principal = |id: &Id| -> Principal {
        serde_json::from_value(json!({
            "id": id, "type": "individual", "name": "Alice Smith",
            "email": "alice@example.com", "description": null, "timeZone": null,
            "capabilities": { "urn:example:a": { "v": 1 } }, "accounts": null
        }))
        .expect("test fixture must deserialize")
    };
    let (id, _) = backend
        .create_object(&(), &Id::from("acc1"), "seed", principal)
        .await
        .expect("seed must succeed");
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, backend);
    (dispatcher, id)
}

/// Dispatch one method call and return its response arguments.
async fn call(dispatcher: &Dispatcher<()>, method: &str, args: Value) -> Value {
    let req = JmapRequest::new(
        vec!["urn:ietf:params:jmap:principals".into()],
        vec![Invocation::new(method, args, "c0")],
        None,
    );
    let mut resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    resp.method_responses.remove(0).arguments
}

/// Apply `patch` to `id` with Principal/set and return the set response.
async fn update(dispatcher: &Dispatcher<()>, id: &Id, patch: Value) -> Value {
    call(
        dispatcher,
        "Principal/set",
        json!({ "accountId": "acc1", "update": { id.as_ref(): patch } }),
    )
    .await
}

/// Fetch `id` with Principal/get and return the stored object.
async fn get(dispatcher: &Dispatcher<()>, id: &Id) -> Value {
    let args = call(
        dispatcher,
        "Principal/get",
        json!({ "accountId": "acc1", "ids": [id.as_ref()] }),
    )
    .await;
    args["list"][0].clone()
}

/// Oracle: RFC 8620 §5.3 — "The keys are a path in JSON Pointer format
/// [RFC6901], with an implicit leading "/" (i.e., prefix each key with "/"
/// before applying the JSON Pointer evaluation algorithm)."
///
/// `capabilities/urn:example:b` must set member `urn:example:b` of
/// `capabilities`, not create a literal top-level property.
#[tokio::test]
async fn update_path_key_sets_nested_member() {
    let (dispatcher, id) = setup().await;
    let resp = update(
        &dispatcher,
        &id,
        json!({ "capabilities/urn:example:b": { "v": 2 } }),
    )
    .await;
    assert!(resp["updated"].get(id.as_ref()).is_some(), "{resp}");

    let obj = get(&dispatcher, &id).await;
    assert_eq!(
        obj["capabilities"],
        json!({ "urn:example:a": { "v": 1 }, "urn:example:b": { "v": 2 } }),
        "{obj}"
    );
    assert!(obj.get("capabilities/urn:example:b").is_none(), "{obj}");
}

/// Oracle: RFC 8620 §5.3 — for a non-null value: "Anything else: The value
/// to set for this property (this may be a replacement or addition to the
/// object being patched)." A top-level key `capabilities` is set to the
/// given value wholesale; members absent from it are not merged in.
#[tokio::test]
async fn update_top_level_object_value_replaces_property() {
    let (dispatcher, id) = setup().await;
    let resp = update(
        &dispatcher,
        &id,
        json!({ "capabilities": { "urn:example:b": { "v": 2 } } }),
    )
    .await;
    assert!(resp["updated"].get(id.as_ref()).is_some(), "{resp}");

    let obj = get(&dispatcher, &id).await;
    assert_eq!(
        obj["capabilities"],
        json!({ "urn:example:b": { "v": 2 } }),
        "{obj}"
    );
}

/// Oracle: RFC 8620 §5.3 — "All parts prior to the last (i.e., the value
/// after the final slash) MUST already exist on the object being patched."
/// and "if there is any violation, the update MUST be rejected with an
/// "invalidPatch" error".
#[tokio::test]
async fn update_path_through_absent_parent_is_invalid_patch() {
    let (dispatcher, id) = setup().await;
    let before = get(&dispatcher, &id).await;
    let resp = update(
        &dispatcher,
        &id,
        json!({ "capabilities/urn:example:missing/v": 2 }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][id.as_ref()]["type"],
        "invalidPatch",
        "{resp}"
    );
    assert_eq!(get(&dispatcher, &id).await, before);
}

/// Oracle: RFC 8620 §5.3 — "There MUST NOT be two patches in the
/// PatchObject where the pointer of one is the prefix of the pointer of
/// the other, e.g., "alerts/1/offset" and "alerts"." and "if there is any
/// violation, the update MUST be rejected with an "invalidPatch" error".
#[tokio::test]
async fn update_prefix_conflict_is_invalid_patch() {
    let (dispatcher, id) = setup().await;
    let before = get(&dispatcher, &id).await;
    let resp = update(
        &dispatcher,
        &id,
        json!({ "capabilities": {}, "capabilities/urn:example:a": { "v": 2 } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][id.as_ref()]["type"],
        "invalidPatch",
        "{resp}"
    );
    assert_eq!(get(&dispatcher, &id).await, before);
}

/// Oracle: RFC 8620 §5.3 — "invalidProperties": "The record given is
/// invalid in some way. For example: It contains properties that are
/// invalid according to the type specification of this record type."
/// RFC 9670 §2 types Principal `*name*: String` with no default.
///
/// Patching `name` to null must be rejected, and Principal/get must still
/// return the stored Principal.
#[tokio::test]
async fn update_null_required_property_is_invalid_properties() {
    let (dispatcher, id) = setup().await;
    let before = get(&dispatcher, &id).await;
    let resp = update(&dispatcher, &id, json!({ "name": null })).await;
    assert_eq!(
        resp["notUpdated"][id.as_ref()]["type"],
        "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][id.as_ref()]["properties"],
        json!(["name"]),
        "{resp}"
    );
    assert_eq!(get(&dispatcher, &id).await, before);
}
