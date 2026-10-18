//! `/set update` PatchObject semantics in the in-memory backend
//! (RFC 8620 §5.3, bd:JMAP-cr0z).
//!
//! The backend previously applied PatchObjects as RFC 7396 merge
//! patches, storing path keys as literal top-level properties and
//! merging object values. These tests drive `Message/set` against the
//! client-writable `deliveryReceipts` map (draft-atwood-jmap-chat-00
//! §Message), a real object-of-objects property.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_message_get, handle_message_set};
use serde_json::{json, Value};

/// Create a Message and return its server id.
async fn create_message(backend: &MemoryBackend) -> String {
    let (resp, _) = handle_message_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    resp["created"]["m0"]["id"]
        .as_str()
        .expect("created id")
        .to_owned()
}

/// Apply `patch` to message `id` and return the `Message/set` response.
async fn update(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let (resp, _) = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
    )
    .await
    .expect("update");
    resp
}

/// Fetch message `id` as its wire JSON.
async fn get(backend: &MemoryBackend, id: &str) -> Value {
    let (resp, _) = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("get");
    resp["list"][0].clone()
}

/// Create a Message whose `deliveryReceipts` holds a single entry `u1`.
async fn message_with_receipt(backend: &MemoryBackend) -> String {
    let id = create_message(backend).await;
    let resp = update(
        backend,
        &id,
        json!({ "deliveryReceipts": { "u1": { "deliveredAt": "2024-01-01T00:00:01Z" } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    id
}

/// Oracle: RFC 8620 §5.3: "The keys are a path in JSON Pointer format
/// [RFC6901], with an implicit leading "/"". The key
/// `deliveryReceipts/u2` therefore adds member `u2` of the
/// `deliveryReceipts` object, keeping `u1`, rather than creating a
/// property literally named `deliveryReceipts/u2`.
#[tokio::test]
async fn path_key_updates_nested_member() {
    let backend = MemoryBackend::new();
    let id = message_with_receipt(&backend).await;

    let resp = update(
        &backend,
        &id,
        json!({ "deliveryReceipts/u2": { "readAt": "2024-01-01T00:00:02Z" } }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");

    let msg = get(&backend, &id).await;
    assert!(msg.get("deliveryReceipts/u2").is_none(), "{msg:?}");
    assert_eq!(
        msg["deliveryReceipts"],
        json!({
            "u1": { "deliveredAt": "2024-01-01T00:00:01Z" },
            "u2": { "readAt": "2024-01-01T00:00:02Z" },
        })
    );
}

/// Oracle: RFC 8620 §5.3: "Anything else: The value to set for this
/// property (this may be a replacement or addition to the object being
/// patched)." A top-level `deliveryReceipts` value replaces the whole
/// property; the existing `u1` entry is not merged into it.
#[tokio::test]
async fn top_level_object_value_replaces_property() {
    let backend = MemoryBackend::new();
    let id = message_with_receipt(&backend).await;

    let resp = update(
        &backend,
        &id,
        json!({ "deliveryReceipts": { "u2": { "readAt": "2024-01-01T00:00:02Z" } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");

    let msg = get(&backend, &id).await;
    assert_eq!(
        msg["deliveryReceipts"],
        json!({ "u2": { "readAt": "2024-01-01T00:00:02Z" } })
    );
}

/// Oracle: RFC 8620 §5.3: "All parts prior to the last (i.e., the value
/// after the final slash) MUST already exist on the object being
/// patched." and "if there is any violation, the update MUST be
/// rejected with an "invalidPatch" error". A fresh Message has no
/// `deliveryReceipts`, so `deliveryReceipts/u1` is rejected and the
/// stored Message is unchanged.
#[tokio::test]
async fn path_through_absent_parent_is_invalid_patch() {
    let backend = MemoryBackend::new();
    let id = create_message(&backend).await;
    let before = get(&backend, &id).await;
    assert!(before.get("deliveryReceipts").is_none(), "{before:?}");

    let resp = update(
        &backend,
        &id,
        json!({ "deliveryReceipts/u1": { "readAt": "2024-01-01T00:00:02Z" } }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&id]["type"], "invalidPatch", "{resp:?}");
    assert!(resp["updated"].get(&id).is_none(), "{resp:?}");
    assert_eq!(get(&backend, &id).await, before);
}

/// Oracle: RFC 8620 §5.3: "There MUST NOT be two patches in the
/// PatchObject where the pointer of one is the prefix of the pointer of
/// the other, e.g., "alerts/1/offset" and "alerts"." and "if there is
/// any violation, the update MUST be rejected with an "invalidPatch"
/// error". The stored Message is unchanged.
#[tokio::test]
async fn prefix_conflict_is_invalid_patch() {
    let backend = MemoryBackend::new();
    let id = message_with_receipt(&backend).await;
    let before = get(&backend, &id).await;

    let resp = update(
        &backend,
        &id,
        json!({
            "deliveryReceipts": { "u2": { "readAt": "2024-01-01T00:00:02Z" } },
            "deliveryReceipts/u1/readAt": "2024-01-01T00:00:03Z",
        }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&id]["type"], "invalidPatch", "{resp:?}");
    assert!(resp["updated"].get(&id).is_none(), "{resp:?}");
    assert_eq!(get(&backend, &id).await, before);
}

/// `{"k": {"k": ... {}}}` with `levels` object levels.
fn nested(levels: usize) -> Value {
    (1..levels).fold(json!({}), |inner, _| json!({ "k": inner }))
}

/// Oracle: bd:JMAP-wlip.1 caps `/set update` nesting at 32 levels and
/// rejects deeper patches as RFC 8620 §5.3 "invalidPatch", leaving the
/// record unchanged. Because §5.3 makes keys "a path in JSON Pointer
/// format", the path itself reaches into the object, so its segments
/// count toward the cap. Test vector: a 21-segment path into an
/// existing 21-level value, setting another 21-level value, would store
/// 43 levels; each repeat could add 21 more.
#[tokio::test]
async fn path_depth_counts_toward_nesting_cap() {
    let backend = MemoryBackend::new();
    let id = create_message(&backend).await;
    let resp = update(
        &backend,
        &id,
        json!({ "deliveryReceipts": { "u1": nested(21) } }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    let before = get(&backend, &id).await;

    let path = format!("deliveryReceipts/u1{}", "/k".repeat(19));
    let resp = update(&backend, &id, json!({ path: nested(21) })).await;
    assert_eq!(resp["notUpdated"][&id]["type"], "invalidPatch", "{resp:?}");
    assert!(resp["updated"].get(&id).is_none(), "{resp:?}");
    assert_eq!(get(&backend, &id).await, before);
}
