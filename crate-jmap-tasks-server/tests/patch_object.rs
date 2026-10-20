//! Task/set update PatchObject semantics against `MemoryBackend`.
//!
//! Oracle: RFC 8620 §5.3 PatchObject definition (quoted per test).
//!
//! Bead: JMAP-cr0z.

mod common;

use common::MemoryBackend;
use jmap_server::HandlerOutput;
use jmap_tasks_server::{handle_task_get, handle_task_set};
use serde_json::{json, Value};

/// A backend holding Task `t1` with one participant `p1`.
fn backend_with_participant() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "Task",
        "t1",
        json!({
            "id": "t1",
            "@type": "Task",
            "uid": "t1-uid",
            "title": "Review",
            "participants": {
                "p1": {
                    "@type": "Participant",
                    "name": "Alice",
                    "participationStatus": "needs-action"
                }
            }
        }),
    );
    backend
}

async fn set_update(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "t1": patch } });
    handle_task_set(backend, &(), args)
        .await
        .expect("/set must succeed at the top level")
        .primary
}

async fn get_t1(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["t1"] });
    let HandlerOutput { primary: resp, .. } = handle_task_get(backend, &(), args)
        .await
        .expect("/get must succeed");
    resp["list"][0].clone()
}

/// RFC 8620 §5.3: "The keys are a path in JSON Pointer format [RFC6901],
/// with an implicit leading "/" (i.e., prefix each key with "/" before
/// applying the JSON Pointer evaluation algorithm)." -- a path key names a
/// nested member, not a literal property.
#[tokio::test]
async fn path_key_updates_nested_member() {
    let backend = backend_with_participant();
    let resp = set_update(
        &backend,
        json!({ "participants/p1/participationStatus": "accepted" }),
    )
    .await;
    assert!(resp["updated"].get("t1").is_some(), "t1 updated: {resp}");

    let task = get_t1(&backend).await;
    assert_eq!(
        task["participants"]["p1"],
        json!({
            "@type": "Participant",
            "name": "Alice",
            "participationStatus": "accepted"
        }),
        "nested member updated, siblings kept: {task}"
    );
    assert!(
        task.get("participants/p1/participationStatus").is_none(),
        "no literal path-named property: {task}"
    );
}

/// RFC 8620 §5.3: "The value associated with each pointer determines how
/// to apply that patch: If null, set to the default value if specified
/// for this property; otherwise, remove the property from the patched
/// object. If the key is not present in the parent, this a no-op.
/// Anything else: The value to set for this property (this may be a
/// replacement or addition to the object being patched)." -- an object
/// value replaces the property wholesale.
#[tokio::test]
async fn object_value_replaces_property_wholesale() {
    let backend = backend_with_participant();
    let replacement = json!({
        "p1": { "@type": "Participant", "participationStatus": "declined" }
    });
    let resp = set_update(&backend, json!({ "participants": replacement.clone() })).await;
    assert!(resp["updated"].get("t1").is_some(), "t1 updated: {resp}");

    let task = get_t1(&backend).await;
    assert_eq!(
        task["participants"], replacement,
        "participants replaced, not merged (name must be gone): {task}"
    );
}

/// RFC 8620 §5.3: "All parts prior to the last (i.e., the value after the
/// final slash) MUST already exist on the object being patched." and
/// "if there is any violation, the update MUST be rejected with an
/// "invalidPatch" error" -- no part of the patch is applied.
#[tokio::test]
async fn path_through_absent_parent_is_invalid_patch() {
    let backend = backend_with_participant();
    let before = get_t1(&backend).await;
    let resp = set_update(
        &backend,
        json!({ "title": "Changed", "participants/p9/participationStatus": "accepted" }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["t1"]["type"], "invalidPatch", "{resp}");
    assert_eq!(get_t1(&backend).await, before, "stored task unchanged");
}

/// RFC 8620 §5.3: "There MUST NOT be two patches in the PatchObject where
/// the pointer of one is the prefix of the pointer of the other, e.g.,
/// "alerts/1/offset" and "alerts"." and "if there is any violation, the
/// update MUST be rejected with an "invalidPatch" error".
#[tokio::test]
async fn prefix_conflict_is_invalid_patch() {
    let backend = backend_with_participant();
    let before = get_t1(&backend).await;
    let resp = set_update(
        &backend,
        json!({
            "participants": {},
            "participants/p1/participationStatus": "accepted"
        }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["t1"]["type"], "invalidPatch", "{resp}");
    assert_eq!(get_t1(&backend).await, before, "stored task unchanged");
}
