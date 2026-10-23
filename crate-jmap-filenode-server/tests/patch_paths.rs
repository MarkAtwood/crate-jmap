//! RFC 8620 §5.3 PatchObject path rules on `FileNode/set` update
//! (bd:JMAP-4vf8.20).
//!
//! Oracle: RFC 8620 §5.3 — "All parts prior to the last (i.e., the value
//! after the final slash) MUST already exist on the object being
//! patched"; on violation "the update MUST be rejected with an
//! "invalidPatch" error". Expected values come from that text, not from
//! the patch applier under test.

mod common;

use common::MemoryBackend;
use jmap_filenode_server::handle_filenode_set;
use jmap_filenode_types::FileNode;
use jmap_server::{HandlerOutput, JmapBackend};
use jmap_types::Id;
use serde_json::{Value, json};

/// Seed a File node `id` whose `shareWith` holds a single "existing" entry.
fn seed_file(backend: &MemoryBackend, id: &str) {
    let node: FileNode = serde_json::from_value(json!({
        "id": id,
        "name": id,
        "parentId": null,
        "role": null,
        "@type": "File",
        "target": null,
        "blobId": "B-blob1",
        "mediaType": "text/plain",
        "size": 7,
        "shareWith": {
            "existing": {
                "mayRead": true,
                "mayAddChildren": false,
                "mayRename": false,
                "mayDelete": false,
                "mayModifyContent": false,
                "mayShare": false
            }
        },
        "myRights": null,
        "created": "2026-01-01T00:00:00Z",
        "modified": "2026-01-01T00:00:00Z",
        "accessed": null
    }))
    .expect("seed FileNode must deserialize");
    backend.seed_node(&Id::from("acc1"), node);
}

async fn stored(backend: &MemoryBackend, id: &str) -> Value {
    let (nodes, _) = backend
        .get_objects::<FileNode>(&(), &Id::from("acc1"), Some(&[Id::from(id)]), None)
        .await
        .expect("get_objects must succeed");
    serde_json::to_value(&nodes[0]).expect("FileNode serializes")
}

/// Send `bad_patch` to node "bad" and `ok_patch` to control node "ok";
/// assert "bad" is invalidPatch and unchanged, and "ok" is updated.
async fn assert_invalid_patch(bad_patch: Value, ok_patch: Value) {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    seed_file(&backend, "bad");
    seed_file(&backend, "ok");
    let before = stored(&backend, "bad").await;

    let HandlerOutput { primary: resp, .. } = handle_filenode_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": "acc1",
            "update": { "bad": bad_patch, "ok": ok_patch }
        }),
        &[],
    )
    .await
    .expect("must not return top-level error");

    assert_eq!(resp["notUpdated"]["bad"]["type"], "invalidPatch", "{resp}");
    assert!(
        resp["updated"]
            .as_object()
            .is_some_and(|m| m.contains_key("ok") && !m.contains_key("bad")),
        "valid pointer patch must apply: {resp}"
    );
    assert_eq!(
        stored(&backend, "bad").await,
        before,
        "rejected patch must not modify the stored node"
    );
}

/// "mediaType" exists but is a String, so "mediaType/x" has no existing
/// object part to set "x" in.
#[tokio::test]
async fn pointer_through_string_property_is_invalid_patch() {
    assert_invalid_patch(
        json!({ "mediaType/x": "y" }),
        json!({ "shareWith/existing/mayShare": true }),
    )
    .await;
}

/// "shareWith/alice" does not exist, so "shareWith/alice/mayRead" is
/// rejected even though the final part would be a create.
#[tokio::test]
async fn pointer_through_absent_key_is_invalid_patch() {
    assert_invalid_patch(
        json!({ "shareWith/alice/mayRead": true }),
        json!({ "shareWith/existing/mayRead": false }),
    )
    .await;
}

/// The null no-op rule ("If the key is not present in the parent, this a
/// no-op") covers only the final part; an absent prior part is still
/// invalidPatch.
#[tokio::test]
async fn null_pointer_through_absent_key_is_invalid_patch() {
    assert_invalid_patch(
        json!({ "shareWith/alice/mayRead": null }),
        json!({ "shareWith/alice": null }),
    )
    .await;
}

/// Oracle: RFC 8620 §5.3 — "There MUST NOT be two patches in the
/// PatchObject where the pointer of one is the prefix of the pointer of
/// the other, e.g., "alerts/1/offset" and "alerts"".
///
/// The "shareWith" value keeps "existing", so each entry alone would
/// apply; only the prefix rule rejects the pair.
#[tokio::test]
async fn prefix_conflicting_pointers_are_invalid_patch() {
    let existing = json!({
        "mayRead": true,
        "mayAddChildren": false,
        "mayRename": false,
        "mayDelete": false,
        "mayModifyContent": false,
        "mayShare": false
    });
    assert_invalid_patch(
        json!({
            "shareWith": { "existing": existing },
            "shareWith/existing/mayShare": true
        }),
        json!({ "shareWith/existing/mayShare": true }),
    )
    .await;
}
