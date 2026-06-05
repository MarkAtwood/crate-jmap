//! Integration tests for `jmap-filenode-server` using MemoryBackend.
//!
//! All expected values are derived from the spec (draft-ietf-jmap-filenode-14),
//! not from the code under test.

mod common;

use common::MemoryBackend;
use jmap_filenode_server::filenode::{
    handle_filenode_changes, handle_filenode_get, handle_filenode_query, handle_filenode_set,
};
use jmap_filenode_types::FileNode;
use serde_json::json;

// ---------------------------------------------------------------------------
// Helper: build a minimal valid FileNode for seeding.
// ---------------------------------------------------------------------------

fn make_dir(id: &str, name: &str, parent_id: Option<&str>) -> FileNode {
    // FileNode is #[non_exhaustive], so it cannot be constructed with a struct
    // literal outside its defining crate.  Build via JSON deserialization instead.
    let v = json!({
        "id": id,
        "parentId": parent_id,
        "nodeType": "directory",
        "blobId": null,
        "target": null,
        "size": null,
        "name": name,
        "type": null,
        "shareWith": null,
        "role": null
    });
    serde_json::from_value(v).expect("make_dir: deserialization must succeed")
}

// ---------------------------------------------------------------------------
// Test 1: create without nodeType → inferred as directory
// Oracle: draft-ietf-jmap-filenode-14 §3.1 (nodeType inference).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_create_directory_nodetype_inferred() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Create a node without specifying nodeType, blobId, or target.
    // §3.1: if all three are absent/null, the server infers "directory".
    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "name": "mydir",
                "parentId": null,
                "role": null
            }
        }
    });

    let (resp, _) = handle_filenode_set(&backend, &(), args)
        .await
        .expect("set must not return top-level error");

    assert!(
        resp["notCreated"].is_null(),
        "directory inference must succeed — notCreated must be null: {resp}"
    );
    assert!(
        resp["created"].is_object(),
        "created must contain the new node: {resp}"
    );

    // The returned object must have nodeType = "directory".
    let created_obj = &resp["created"]["c1"];
    assert_eq!(
        created_obj["nodeType"], "directory",
        "nodeType must be inferred as 'directory': {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: create with nodeType=file but no blobId → invalidProperties
// Oracle: draft-ietf-jmap-filenode-14 §3.1 (file node requires blobId).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_create_file_requires_blobid() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Explicitly set nodeType=file but leave blobId null.
    // §3.1: "If type is 'file', blobId MUST be non-null."
    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "name": "myfile.txt",
                "nodeType": "file",
                "blobId": null,
                "parentId": null,
                "role": null
            }
        }
    });

    let (resp, _) = handle_filenode_set(&backend, &(), args)
        .await
        .expect("set must not return top-level error");

    let not_created = &resp["notCreated"];
    assert!(
        not_created.is_object(),
        "notCreated must be present: {resp}"
    );
    assert_eq!(
        not_created["c1"]["type"], "invalidProperties",
        "file without blobId must produce invalidProperties: {resp}"
    );
    let props = &not_created["c1"]["properties"];
    assert!(
        props
            .as_array()
            .map(|a| a.contains(&json!("blobId")))
            .unwrap_or(false),
        "blobId must be listed in properties: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: destroy a leaf node succeeds
// Oracle: draft-ietf-jmap-filenode-14 §3.2.3 (basic destroy).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_destroy_leaf_node_succeeds() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed a leaf node (no children).
    backend.seed_node("acc1", make_dir("leaf-1", "leafdir", None));

    let args = json!({
        "accountId": "acc1",
        "destroy": ["leaf-1"]
    });

    let (resp, _) = handle_filenode_set(&backend, &(), args)
        .await
        .expect("set must not return top-level error");

    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");
    assert!(
        destroyed.contains(&json!("leaf-1")),
        "leaf-1 must appear in destroyed: {resp}"
    );
    assert!(
        resp["notDestroyed"].is_null()
            || resp["notDestroyed"]
                .as_object()
                .is_none_or(serde_json::Map::is_empty),
        "notDestroyed must be empty when full-coverage destroy succeeds: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: destroy parent with onDestroyRemoveChildren=false (default) → nodeHasChildren
// Oracle: draft-ietf-jmap-filenode-14 §3.2.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_destroy_parent_node_has_children() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed a parent and a child.
    backend.seed_node("acc1", make_dir("parent-1", "parent", None));
    backend.seed_node("acc1", make_dir("child-1", "child", Some("parent-1")));

    // Destroy only the parent; child is not in the destroy list.
    // §3.2.3: if onDestroyRemoveChildren is false, this MUST return nodeHasChildren.
    let args = json!({
        "accountId": "acc1",
        "destroy": ["parent-1"]
    });

    let (resp, _) = handle_filenode_set(&backend, &(), args)
        .await
        .expect("set must not return top-level error");

    assert!(
        resp["destroyed"].is_null(),
        "parent must not be destroyed: {resp}"
    );
    let not_destroyed = &resp["notDestroyed"];
    assert!(
        not_destroyed.is_object(),
        "notDestroyed must be present: {resp}"
    );
    assert_eq!(
        not_destroyed["parent-1"]["type"], "nodeHasChildren",
        "must return nodeHasChildren: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: destroy with onDestroyRemoveChildren=true cascades to children
// Oracle: draft-ietf-jmap-filenode-14 §3.2.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_destroy_with_remove_children_cascades() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed parent and child.
    backend.seed_node("acc1", make_dir("parent-2", "parent", None));
    backend.seed_node("acc1", make_dir("child-2", "child", Some("parent-2")));

    // With onDestroyRemoveChildren=true, both parent and child must be destroyed.
    let args = json!({
        "accountId": "acc1",
        "onDestroyRemoveChildren": true,
        "destroy": ["parent-2"]
    });

    let (resp, _) = handle_filenode_set(&backend, &(), args)
        .await
        .expect("set must not return top-level error");

    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert!(
        destroyed.contains(&json!("child-2")),
        "child-2 must appear in destroyed (cascade): {resp}"
    );
    assert!(
        destroyed.contains(&json!("parent-2")),
        "parent-2 must appear in destroyed: {resp}"
    );
    assert!(
        resp["notDestroyed"].is_null(),
        "notDestroyed must be null when cascade succeeds: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 5b: multi-level destroy WITHOUT onDestroyRemoveChildren: destroy set
// {A, B} where A is grandparent, B is A's child, and B has its own child C
// not in the destroy set. Both A and B MUST report nodeHasChildren because
// the transitive descendant C is uncovered. Verdict (no destroys happen)
// is correct; error attribution is at every uncovered ancestor.
// Regression for bd:JMAP-510h.12 — documents the current partial-tree
// behaviour so a future refactor that reorders destroys cannot silently
// regress it.
// Oracle: draft-ietf-jmap-filenode-14 §3.2.3 nodeHasChildren — MUST when
// any descendant is not in the destroy set.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_destroy_partial_multilevel_reports_node_has_children() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Build a three-level tree: A -> B -> C.
    backend.seed_node("acc1", make_dir("A", "A", None));
    backend.seed_node("acc1", make_dir("B", "B", Some("A")));
    backend.seed_node("acc1", make_dir("C", "C", Some("B")));

    // Destroy {A, B} but NOT C. C is uncovered, so both A and B must
    // appear in notDestroyed with nodeHasChildren.
    let args = json!({
        "accountId": "acc1",
        "destroy": ["A", "B"]
    });

    let (resp, _) = handle_filenode_set(&backend, &(), args)
        .await
        .expect("set must not return top-level error");

    let not_destroyed = resp["notDestroyed"]
        .as_object()
        .expect("notDestroyed must be an object when entries fail");
    assert!(
        not_destroyed.contains_key("A"),
        "A must be in notDestroyed (uncovered descendant C): {resp}"
    );
    assert_eq!(
        not_destroyed["A"]["type"], "nodeHasChildren",
        "A must report nodeHasChildren: {resp}"
    );
    assert!(
        not_destroyed.contains_key("B"),
        "B must be in notDestroyed (uncovered descendant C): {resp}"
    );
    assert_eq!(
        not_destroyed["B"]["type"], "nodeHasChildren",
        "B must report nodeHasChildren: {resp}"
    );

    // None of A, B, C should be destroyed.
    assert!(
        resp["destroyed"].is_null() || resp["destroyed"].as_array().is_none_or(Vec::is_empty),
        "destroyed must be empty/null: {resp}"
    );

    // C must still exist.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "ids": ["A", "B", "C"]
        }),
    )
    .await
    .expect("get must succeed");
    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 3, "all three nodes must survive: {get_resp}");
}

// ---------------------------------------------------------------------------
// Test 5c: full-coverage destroy set: A, B, C all in destroy set. RFC
// §3.2.3 MUST NOT return nodeHasChildren since every descendant is also
// being destroyed in the same request. All three must end up in destroyed.
// Regression for bd:JMAP-510h.12 — full-coverage case.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_destroy_full_coverage_multilevel_succeeds() {
    let backend = MemoryBackend::new().with_account("acc1");

    backend.seed_node("acc1", make_dir("FA", "FA", None));
    backend.seed_node("acc1", make_dir("FB", "FB", Some("FA")));
    backend.seed_node("acc1", make_dir("FC", "FC", Some("FB")));

    let args = json!({
        "accountId": "acc1",
        "destroy": ["FA", "FB", "FC"]
    });

    let (resp, _) = handle_filenode_set(&backend, &(), args)
        .await
        .expect("set must not return top-level error");

    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be array when entries succeed");
    let destroyed_strs: Vec<&str> = destroyed.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        destroyed_strs.contains(&"FA")
            && destroyed_strs.contains(&"FB")
            && destroyed_strs.contains(&"FC"),
        "all three must be in destroyed when fully covered: {resp}"
    );
    assert!(
        resp["notDestroyed"].is_null()
            || resp["notDestroyed"]
                .as_object()
                .is_none_or(|o| o.is_empty()),
        "notDestroyed must be empty when full-coverage destroy succeeds: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: FileNode/get returns a created node by id
// Oracle: draft-ietf-jmap-filenode-14 §3.2.1.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_get_returns_created_node() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Create a node via FileNode/set.
    let set_args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "name": "getme",
                "parentId": null,
                "role": null
            }
        }
    });
    let (set_resp, _) = handle_filenode_set(&backend, &(), set_args)
        .await
        .expect("set must succeed");

    let server_id = set_resp["created"]["c1"]["id"]
        .as_str()
        .expect("created node must have an id")
        .to_owned();

    // Fetch the node by its server-assigned id.
    let get_args = json!({
        "accountId": "acc1",
        "ids": [&server_id]
    });
    let (get_resp, _) = handle_filenode_get(&backend, &(), get_args)
        .await
        .expect("get must succeed");

    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(
        list.len(),
        1,
        "exactly one node must be returned: {get_resp}"
    );
    assert_eq!(
        list[0]["id"], server_id,
        "returned node id must match: {get_resp}"
    );
    assert_eq!(
        list[0]["name"], "getme",
        "returned node name must match: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 7: fetchParents=true returns parent in the list
// Oracle: draft-ietf-jmap-filenode-14 §3.2.1 fetchParents.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_get_fetch_parents_returns_ancestor() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Create a parent node.
    let (parent_resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "p1": { "name": "parent", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("create parent must succeed");

    let parent_id = parent_resp["created"]["p1"]["id"]
        .as_str()
        .expect("parent must have id")
        .to_owned();

    // Create a child under the parent.
    let (child_resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "child", "parentId": &parent_id, "role": null }
            }
        }),
    )
    .await
    .expect("create child must succeed");

    let child_id = child_resp["created"]["c1"]["id"]
        .as_str()
        .expect("child must have id")
        .to_owned();

    // Get the child with fetchParents=true.
    // §3.2.1: the ancestor chain must be appended to the list.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "ids": [&child_id],
            "fetchParents": true
        }),
    )
    .await
    .expect("get must succeed");

    let list = get_resp["list"].as_array().expect("list must be array");
    let ids: Vec<&str> = list.iter().filter_map(|v| v["id"].as_str()).collect();

    assert!(
        ids.contains(&child_id.as_str()),
        "child must be in list: {get_resp}"
    );
    assert!(
        ids.contains(&parent_id.as_str()),
        "parent must appear in list when fetchParents=true: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 7a: non-object update value yields invalidPatch BEFORE the cycle
// check inspects parentId. Before the fix for bd:JMAP-510h.13 the cycle
// check ran on the raw wire Value (which silently no-op'd on a non-object)
// and the handler then re-parsed the same value as PatchObject, surfacing
// invalidPatch second. The ordering matters for forward-compat: any future
// PatchObject deserializer change that normalises keys must not reshape
// what the cycle guard sees.
// Oracle: RFC 8620 §5.3 — a PatchObject MUST be a JSON Object;
// non-object values produce invalidPatch.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_update_non_object_patch_returns_invalid_patch() {
    let backend = MemoryBackend::new().with_account("acc1");
    let node_id = create_node(&backend, "acc1", "doc", None, "s").await;

    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "update": {
                &node_id: ["this is not a JSON object"]
            }
        }),
    )
    .await
    .expect("set must succeed at the method level");

    let not_updated = &resp["notUpdated"][&node_id];
    assert_eq!(
        not_updated["type"], "invalidPatch",
        "non-object patch must produce invalidPatch (RFC 8620 §5.3); got: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 7b: fetchParents=true with multiple sibling ids that share a parent
// must dedup the parent in the response list, per the FileNodeBackend::
// get_ancestors union+dedup contract. Regression for bd JMAP-510h.32.
// Oracle: the get_ancestors trait doc requires the union of all ancestor
// chains, deduplicated by node id.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_get_fetch_parents_dedup_shared_ancestor() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Build a parent with two children sharing it.
    let parent_id = create_node(&backend, "acc1", "parent", None, "p1").await;
    let child_a = create_node(&backend, "acc1", "a", Some(&parent_id), "c1").await;
    let child_b = create_node(&backend, "acc1", "b", Some(&parent_id), "c2").await;

    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "ids": [&child_a, &child_b],
            "fetchParents": true
        }),
    )
    .await
    .expect("get must succeed");

    let list = get_resp["list"].as_array().expect("list must be array");
    let ids: Vec<&str> = list.iter().filter_map(|v| v["id"].as_str()).collect();

    // Both children present.
    assert!(
        ids.contains(&child_a.as_str()) && ids.contains(&child_b.as_str()),
        "both children must be in list: {get_resp}"
    );
    // Shared parent present exactly once — proves dedup.
    let parent_count = ids.iter().filter(|id| **id == parent_id.as_str()).count();
    assert_eq!(
        parent_count, 1,
        "shared parent must appear exactly once after dedup; appeared {parent_count}x in: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: query with depth=1 returns directory and its direct children
// Oracle: draft-ietf-jmap-filenode-14 §3.2.5 (depth parameter).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_query_depth_one_returns_children() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Create a top-level directory.
    let (dir_resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "d1": { "name": "topdir", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("create dir must succeed");

    let dir_id = dir_resp["created"]["d1"]["id"]
        .as_str()
        .expect("dir must have id")
        .to_owned();

    // Create two children under topdir.
    handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "ch1": { "name": "alpha", "parentId": &dir_id, "role": null },
                "ch2": { "name": "beta",  "parentId": &dir_id, "role": null }
            }
        }),
    )
    .await
    .expect("create children must succeed");

    // Query isTopLevel=true with depth=1.
    // §3.2.5: depth=1 expands one level below the initial result.
    let (q_resp, _) = handle_filenode_query(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "filter": { "isTopLevel": true },
            "sort": null,
            "depth": 1
        }),
    )
    .await
    .expect("query must succeed");

    let ids = q_resp["ids"].as_array().expect("ids must be array");
    // Must contain the directory AND its two children → 3 ids.
    assert_eq!(
        ids.len(),
        3,
        "depth=1 must include dir and both children: {q_resp}"
    );
    let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        id_strs.contains(&dir_id.as_str()),
        "topdir must be in result: {q_resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 9: FileNode/changes after create shows the new id in the created list
// Oracle: draft-ietf-jmap-filenode-14 §3.2.2 (changes after mutation).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_changes_after_create_shows_in_created_list() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Record state before any creation.
    let pre_state = {
        let get_args = json!({
            "accountId": "acc1",
            "ids": []
        });
        let (resp, _) = handle_filenode_get(&backend, &(), get_args)
            .await
            .expect("get must succeed");
        resp["state"]
            .as_str()
            .expect("state must be present")
            .to_owned()
    };

    // Create a node.
    let (set_resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "newnode", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("set must succeed");

    let created_id = set_resp["created"]["c1"]["id"]
        .as_str()
        .expect("created node must have id")
        .to_owned();

    // Call FileNode/changes with sinceState = pre_state.
    // §3.2.2: the new id must appear in the `created` list.
    let (ch_resp, _) = handle_filenode_changes(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "sinceState": &pre_state
        }),
    )
    .await
    .expect("changes must succeed");

    let ch_created = ch_resp["created"]
        .as_array()
        .expect("created must be array");
    assert!(
        ch_created.contains(&json!(&created_id)),
        "newly created id must appear in changes.created: {ch_resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 9b: FileNode/changes with an unparseable sinceState must return
// `cannotCalculateChanges`, not silently fall back to state=0.
// Oracle: RFC 8620 §5.2 — when the server cannot calculate the changes
// between the given state and the current state, it MUST return an error
// of type `cannotCalculateChanges`. Regression for bd JMAP-510h.62.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_changes_unparseable_since_state_cannot_calculate() {
    let backend = MemoryBackend::new().with_account("acc1");

    let err = handle_filenode_changes(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "sinceState": "bogus-non-numeric-state"
        }),
    )
    .await
    .expect_err("unparseable sinceState must produce a JmapError");

    assert_eq!(
        err.error_type.as_str(),
        "cannotCalculateChanges",
        "unparseable sinceState must surface as cannotCalculateChanges, not silently \
         fall back to state=0 (see bd:JMAP-510h.62); got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/set create — onExists='replace' + onDestroyRemoveChildren cascade
// Oracle: draft-ietf-jmap-filenode-14 §3.2.3 lines 565-570 — "if the
// replaced item is a directory which has children, then the server MUST
// respond with a nodeHasChildren error to this action UNLESS
// onDestroyRemoveChildren is true". Regression for bd JMAP-510h.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_create_replace_with_remove_children_cascades() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed: parent "old_dir" at root, with one child "old_child".
    let (resp_parent, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "p": { "name": "shared_name", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("seed parent must succeed");
    let parent_id = resp_parent["created"]["p"]["id"]
        .as_str()
        .expect("parent id")
        .to_owned();

    let (_resp_child, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "c": { "name": "child", "parentId": &parent_id, "role": null }
            }
        }),
    )
    .await
    .expect("seed child must succeed");
    let child_id = _resp_child["created"]["c"]["id"]
        .as_str()
        .expect("child id")
        .to_owned();

    // Attempt to create a new node at the SAME parent (root) with the
    // SAME name ("shared_name"). With onExists="replace" AND
    // onDestroyRemoveChildren=true, the existing "shared_name" directory
    // (which has children) MUST be destroyed along with its descendants,
    // and the new node MUST be created.
    let (replace_resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "onDestroyRemoveChildren": true,
            "onExists": "replace",
            "create": {
                "new": { "name": "shared_name", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("replace+cascade must succeed");

    // The new node must be in `created`, not `notCreated`.
    assert!(
        replace_resp["created"].is_object() && replace_resp["created"]["new"].is_object(),
        "new node must be created: {replace_resp}"
    );
    assert!(
        replace_resp["notCreated"].is_null()
            || !replace_resp["notCreated"]
                .as_object()
                .is_some_and(|m| m.contains_key("new")),
        "must NOT be in notCreated: {replace_resp}"
    );

    // The destroyed list MUST include the old parent and its child.
    let destroyed = replace_resp["destroyed"]
        .as_array()
        .expect("destroyed must be array: {replace_resp}");
    let destroyed_strs: Vec<&str> = destroyed.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        destroyed_strs.contains(&parent_id.as_str()),
        "destroyed must include the replaced parent id {parent_id}: {replace_resp}"
    );
    assert!(
        destroyed_strs.contains(&child_id.as_str()),
        "destroyed must include the cascaded child id {child_id}: {replace_resp}"
    );
}

#[tokio::test]
async fn filenode_set_create_replace_without_flag_returns_node_has_children() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed: parent + child.
    let (resp_parent, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "p": { "name": "shared_name", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("seed parent must succeed");
    let parent_id = resp_parent["created"]["p"]["id"]
        .as_str()
        .expect("parent id")
        .to_owned();

    let _ = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "c": { "name": "child", "parentId": &parent_id, "role": null }
            }
        }),
    )
    .await
    .expect("seed child must succeed");

    // Attempt replace without onDestroyRemoveChildren=true: MUST return
    // nodeHasChildren (the no-cascade guard is preserved).
    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "onExists": "replace",
            "create": {
                "new": { "name": "shared_name", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("must not return top-level error");

    let not_created = &resp["notCreated"];
    assert!(
        not_created.is_object() && not_created["new"].is_object(),
        "new node must be in notCreated when replace would orphan children: {resp}"
    );
    assert_eq!(
        not_created["new"]["type"], "nodeHasChildren",
        "without onDestroyRemoveChildren, replace of non-empty dir must \
         return nodeHasChildren: {resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — onExists / compareCaseInsensitively / onDestroyRemoveChildren
// Oracle: draft-ietf-jmap-filenode-14 §3.2.4 — "This is a standard
// Foo/copy function with the same additional top-level arguments as
// FileNode/set, onDestroyRemoveChildren and onExists, with the same
// behaviour." Regression for bd JMAP-510h.1.
// ---------------------------------------------------------------------------

use jmap_filenode_server::filenode::handle_filenode_copy;

/// Helper: create a node and return its id.
async fn create_node(
    backend: &MemoryBackend,
    account: &str,
    name: &str,
    parent_id: Option<&str>,
    create_id: &str,
) -> String {
    let (resp, _) = handle_filenode_set(
        backend,
        &(),
        json!({
            "accountId": account,
            "create": {
                create_id: { "name": name, "parentId": parent_id, "role": null }
            }
        }),
    )
    .await
    .expect("seed node must succeed");
    resp["created"][create_id]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("seed node {create_id} must have id: {resp}"))
        .to_owned()
}

#[tokio::test]
async fn filenode_copy_default_collision_returns_already_exists() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    let src_id = create_node(&backend, "src", "doc", None, "s").await;
    // Pre-seed a collision in the destination.
    let _existing = create_node(&backend, "dst", "doc", None, "e").await;

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    // onExists absent → Reject (alreadyExists in notCreated).
    let not_created = &resp["notCreated"];
    assert!(
        not_created.is_object() && not_created["c"].is_object(),
        "must be in notCreated: {resp}"
    );
    assert_eq!(
        not_created["c"]["type"], "alreadyExists",
        "default onExists is reject → alreadyExists: {resp}"
    );
}

#[tokio::test]
async fn filenode_copy_on_exists_rename_succeeds_with_suffixed_name() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    let src_id = create_node(&backend, "src", "doc", None, "s").await;
    // Pre-seed a collision in the destination.
    let _existing = create_node(&backend, "dst", "doc", None, "e").await;

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "onExists": "rename",
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    // Must succeed: onExists=rename → a non-colliding name is used.
    assert!(
        resp["created"].is_object() && resp["created"]["c"].is_object(),
        "rename must produce a created entry: {resp}"
    );
    let created_name = resp["created"]["c"]["name"]
        .as_str()
        .expect("created.c.name must be present");
    // Name must have a suffix (the renamer appends -N).
    assert_ne!(
        created_name, "doc",
        "rename must produce a non-colliding name, got '{created_name}': {resp}"
    );
    assert!(
        created_name.starts_with("doc-"),
        "rename should produce 'doc-N', got '{created_name}': {resp}"
    );
}

#[tokio::test]
async fn filenode_copy_on_exists_replace_destroys_existing_then_creates() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    let src_id = create_node(&backend, "src", "doc", None, "s").await;
    let existing_id = create_node(&backend, "dst", "doc", None, "e").await;

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "onExists": "replace",
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    // The new copy must be in `created`.
    assert!(
        resp["created"].is_object() && resp["created"]["c"].is_object(),
        "replace must produce a created entry: {resp}"
    );

    // And the existing node must actually be gone from the destination.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "dst",
            "ids": [&existing_id]
        }),
    )
    .await
    .expect("get must succeed");
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    let not_found_strs: Vec<&str> = not_found.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        not_found_strs.contains(&existing_id.as_str()),
        "the existing node must have been destroyed by replace: {get_resp}"
    );
}

#[tokio::test]
async fn filenode_copy_on_exists_replace_without_flag_node_has_children() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    let src_id = create_node(&backend, "src", "doc", None, "s").await;
    // Pre-seed a colliding directory WITH a child in the destination.
    let existing_parent = create_node(&backend, "dst", "doc", None, "ep").await;
    let _existing_child = create_node(&backend, "dst", "child", Some(&existing_parent), "ec").await;

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "onExists": "replace",
            // onDestroyRemoveChildren absent (defaults to false)
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    let not_created = &resp["notCreated"];
    assert!(
        not_created.is_object() && not_created["c"].is_object(),
        "replace without flag against directory-with-children must \
         go to notCreated: {resp}"
    );
    assert_eq!(
        not_created["c"]["type"], "nodeHasChildren",
        "must return nodeHasChildren: {resp}"
    );
}

#[tokio::test]
async fn filenode_copy_non_string_parent_id_returns_invalid_properties() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    let src_id = create_node(&backend, "src", "doc", None, "s").await;

    // Sending parentId as a number is wire-protocol garbage. The
    // handler must surface this as invalidProperties rather than
    // silently coercing to None (which would copy the source to the
    // destination root, undocumented and with no error signal).
    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "create": {
                "c": { "id": &src_id, "parentId": 42, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    let not_copied = &resp["notCreated"];
    assert!(
        not_copied.is_object() && not_copied["c"].is_object(),
        "non-string parentId must go to notCreated: {resp}"
    );
    assert_eq!(
        not_copied["c"]["type"], "invalidProperties",
        "must surface as invalidProperties: {resp}"
    );
    let props = &not_copied["c"]["properties"];
    assert!(
        props
            .as_array()
            .map(|a| a.contains(&json!("parentId")))
            .unwrap_or(false),
        "parentId must be listed in properties: {resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — arbitrary property overrides from the create entry apply
// to the copied node (not just parentId and name).
// Oracle: RFC 8620 §5.4 — "A map of creation id to a Foo object. [...]
// a copy of the source object with the given properties overridden."
// draft-ietf-jmap-filenode-14 §3.2.4 incorporates by reference. Regression
// for bd JMAP-510h.11.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_copy_applies_arbitrary_property_overrides() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    // Source has role=null. The copy supplies role="documents" override.
    let src_id = create_node(&backend, "src", "doc", None, "s").await;

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "create": {
                "c": {
                    "id": &src_id,
                    "name": "doc",
                    "role": "documents"
                }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    let created = &resp["created"]["c"];
    assert!(created.is_object(), "copy must succeed: {resp}");
    assert_eq!(
        created["role"], "documents",
        "the role override from the create entry must be applied to the copy; \
         got: {resp}"
    );
}

#[tokio::test]
async fn filenode_copy_on_exists_replace_with_flag_cascades() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    let src_id = create_node(&backend, "src", "doc", None, "s").await;
    let existing_parent = create_node(&backend, "dst", "doc", None, "ep").await;
    let existing_child = create_node(&backend, "dst", "child", Some(&existing_parent), "ec").await;

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "onExists": "replace",
            "onDestroyRemoveChildren": true,
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    // Copy must succeed.
    assert!(
        resp["created"].is_object() && resp["created"]["c"].is_object(),
        "replace+cascade must produce a created entry: {resp}"
    );

    // Both the existing parent and its child must be gone in the destination.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "dst",
            "ids": [&existing_parent, &existing_child]
        }),
    )
    .await
    .expect("get must succeed");
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    let not_found_strs: Vec<&str> = not_found.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        not_found_strs.contains(&existing_parent.as_str()),
        "existing parent must have been destroyed: {get_resp}"
    );
    assert!(
        not_found_strs.contains(&existing_child.as_str()),
        "existing child must have been cascade-destroyed: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — ifFromInState mismatch returns stateMismatch (RFC 8620 §5.4)
// Oracle: RFC 8620 §5.4 — "If supplied, the string must match the current
// state of the account referenced by the fromAccountId when reading the data
// to be copied; otherwise, the method will be aborted and a 'stateMismatch'
// error returned." Regression for bd JMAP-510h.57.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_copy_if_from_in_state_mismatch_returns_state_mismatch() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    // Seed a source node so the source account is in a non-trivial state.
    let src_id = create_node(&backend, "src", "doc", None, "s").await;

    // ifFromInState that does not match the current source-account state must
    // abort the method with stateMismatch — not silently proceed.
    let err = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "ifFromInState": "definitely-not-the-current-state",
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect_err("ifFromInState mismatch must produce a JmapError");

    assert_eq!(
        err.error_type.as_str(),
        "stateMismatch",
        "ifFromInState mismatch must surface as stateMismatch (RFC 8620 §5.4); got: {err:?}"
    );

    // And the destination account must not have received any copy.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "dst",
            "ids": null
        }),
    )
    .await
    .expect("get must succeed");
    let list = get_resp["list"].as_array().expect("list must be array");
    assert!(
        list.is_empty(),
        "no copy must have been performed on stateMismatch: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — ifFromInState matching the current source state allows the
// copy to proceed (positive control for the test above).
// Oracle: RFC 8620 §5.4 — when ifFromInState matches the current source
// state, the method proceeds normally. Regression for bd JMAP-510h.57.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_copy_if_from_in_state_match_proceeds() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    let src_id = create_node(&backend, "src", "doc", None, "s").await;

    // Read the current source-account state via FileNode/get on src.
    let (src_state_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "src",
            "ids": []
        }),
    )
    .await
    .expect("get must succeed");
    let src_state = src_state_resp["state"]
        .as_str()
        .expect("state must be present")
        .to_owned();

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "ifFromInState": &src_state,
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("matching ifFromInState must allow the copy to proceed");

    assert!(
        resp["created"].is_object() && resp["created"]["c"].is_object(),
        "copy must succeed when ifFromInState matches: {resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — onSuccessDestroyOriginal destroys each successfully copied
// source record and emits an implicit FileNode/set response.
// Oracle: RFC 8620 §5.4 — "If true, an attempt will be made to destroy the
// original records that were successfully copied: after emitting the
// Foo/copy response, but before processing the next method, the server MUST
// make a single call to Foo/set to destroy the original of each
// successfully copied record." Regression for bd JMAP-510h.56.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_copy_on_success_destroy_original_destroys_source() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    // A single leaf node in src that we will move to dst.
    let src_id = create_node(&backend, "src", "doc", None, "s").await;

    let (resp, extra) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "onSuccessDestroyOriginal": true,
            "create": {
                "c": { "id": &src_id, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("copy must succeed");

    // The copy itself succeeded.
    assert!(
        resp["created"]["c"].is_object(),
        "copy must produce a created entry: {resp}"
    );

    // Exactly one implicit FileNode/set response was appended.
    assert_eq!(
        extra.len(),
        1,
        "onSuccessDestroyOriginal must emit one implicit FileNode/set: extra={extra:?}"
    );
    let (method_name, set_resp, call_id) = &extra[0];
    assert_eq!(method_name, "FileNode/set");
    assert_eq!(
        call_id, "c0",
        "implicit FileNode/set must carry the original call_id"
    );
    assert_eq!(
        set_resp["accountId"], "src",
        "implicit destroy targets the source account"
    );
    let destroyed = set_resp["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    let destroyed_strs: Vec<&str> = destroyed.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        destroyed_strs.contains(&src_id.as_str()),
        "the source node must appear in implicit destroyed list: {set_resp}"
    );

    // The source node must actually be gone from src.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "src",
            "ids": [&src_id]
        }),
    )
    .await
    .expect("get must succeed");
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    let not_found_strs: Vec<&str> = not_found.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        not_found_strs.contains(&src_id.as_str()),
        "source must be destroyed: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — onSuccessDestroyOriginal without onDestroyRemoveChildren
// MUST report `nodeHasChildren` for a source directory that has children,
// honoring the FileNode-specific top-level flag on the implicit destroy.
// Oracle: draft-ietf-jmap-filenode-14 §3.2.4 ("with the same behaviour" as
// FileNode/set) + §3.2.3 nodeHasChildren semantics. Regression for bd
// JMAP-510h.56.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_copy_on_success_destroy_original_node_has_children() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    // src has a parent directory with a child; we copy the parent only.
    let src_parent = create_node(&backend, "src", "parent", None, "p").await;
    let _src_child = create_node(&backend, "src", "child", Some(&src_parent), "c1").await;

    let (_resp, extra) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "onSuccessDestroyOriginal": true,
            // onDestroyRemoveChildren absent → defaults to false.
            "create": {
                "c": { "id": &src_parent, "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("copy itself must succeed");

    assert_eq!(extra.len(), 1, "implicit FileNode/set must be emitted");
    let (_, set_resp, _) = &extra[0];
    let not_destroyed = &set_resp["notDestroyed"];
    assert!(
        not_destroyed.is_object() && not_destroyed[&src_parent].is_object(),
        "src parent must be in notDestroyed: {set_resp}"
    );
    assert_eq!(
        not_destroyed[&src_parent]["type"], "nodeHasChildren",
        "without onDestroyRemoveChildren, the implicit destroy must report nodeHasChildren: {set_resp}"
    );

    // And the source parent must still exist.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "src",
            "ids": [&src_parent]
        }),
    )
    .await
    .expect("get must succeed");
    let list = get_resp["list"].as_array().expect("list must be array");
    assert!(
        !list.is_empty(),
        "src parent must survive a failed implicit destroy: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — same-account copy under the source node's own descendant
// is rejected with invalidProperties (cycle guard).
// Oracle: bd:JMAP-510h.59 — same-account copy is allowed (RFC 8620 §5.4
// does not prohibit it), but the resulting tree must not have a node copied
// under its own descendant.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_copy_same_account_under_own_descendant_returns_invalid_properties() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Build A -> B in acc1.
    let a_id = create_node(&backend, "acc1", "A", None, "a").await;
    let b_id = create_node(&backend, "acc1", "B", Some(&a_id), "b").await;

    // Try to copy A under B (which is A's descendant) within the same account.
    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "acc1",
            "accountId": "acc1",
            "create": {
                "c": { "id": &a_id, "parentId": &b_id }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    let not_copied = &resp["notCreated"]["c"];
    assert_eq!(
        not_copied["type"], "invalidProperties",
        "copy under own descendant must surface as invalidProperties: {resp}"
    );
    let props = not_copied["properties"]
        .as_array()
        .expect("properties must be array");
    assert!(
        props.contains(&json!("parentId")),
        "parentId must be listed in properties: {resp}"
    );
}

#[tokio::test]
async fn filenode_copy_same_account_to_own_id_returns_invalid_properties() {
    let backend = MemoryBackend::new().with_account("acc1");
    let a_id = create_node(&backend, "acc1", "A", None, "a").await;

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "acc1",
            "accountId": "acc1",
            "create": {
                "c": { "id": &a_id, "parentId": &a_id }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    let not_copied = &resp["notCreated"]["c"];
    assert_eq!(
        not_copied["type"], "invalidProperties",
        "copy under self must surface as invalidProperties: {resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/copy — unknown fromAccountId returns fromAccountNotFound, distinct
// from accountNotFound for an unknown destination accountId.
// Oracle: RFC 8620 §5.4 — Foo/copy defines fromAccountNotFound for the
// source and accountNotFound (inherited from §3.6.2) for the destination
// so a client can tell which side of the copy is misconfigured.
// Regression for bd:JMAP-510h.58.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_copy_unknown_from_account_returns_from_account_not_found() {
    // Only 'dst' is registered; 'src' is unknown.
    let backend = MemoryBackend::new().with_account("dst");

    let err = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "create": {
                "c": { "id": "any-id", "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect_err("unknown fromAccountId must produce a JmapError");

    assert_eq!(
        err.error_type.as_str(),
        "fromAccountNotFound",
        "unknown fromAccountId must surface as fromAccountNotFound, not \
         the generic accountNotFound (RFC 8620 §5.4); got: {err:?}"
    );
}

#[tokio::test]
async fn filenode_copy_unknown_destination_account_returns_account_not_found() {
    // Only 'src' is registered; 'dst' is unknown.
    let backend = MemoryBackend::new().with_account("src");

    let err = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "create": {
                "c": { "id": "any-id", "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect_err("unknown accountId must produce a JmapError");

    assert_eq!(
        err.error_type.as_str(),
        "accountNotFound",
        "unknown destination accountId must surface as accountNotFound \
         (RFC 8620 §3.6.2 inherited); got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/query — role filter actually filters by role on MemoryBackend.
// Oracle: FileNodeFilterCondition.role is exact byte match. Regression
// for bd:JMAP-510h.9 — before the fix, MemoryBackend silently passed
// through any filter condition other than parentId/isTopLevel and the
// query returned every node regardless of role.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_query_role_filter_excludes_non_matching() {
    let backend = MemoryBackend::new().with_account("acc1");

    handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "d1": { "name": "documents", "parentId": null, "role": "documents" },
                "d2": { "name": "downloads", "parentId": null, "role": "downloads" },
                "d3": { "name": "no-role",    "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("create must succeed");

    let (q_resp, _) = handle_filenode_query(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "filter": { "role": "documents" },
            "sort": null
        }),
    )
    .await
    .expect("query must succeed");

    let ids = q_resp["ids"].as_array().expect("ids must be array");
    assert_eq!(
        ids.len(),
        1,
        "role=documents must match exactly one node, not all three; got: {q_resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/query — body (full-text) filter is explicitly unsupported.
// Oracle: bd:JMAP-510h.9 — unsupported conditions MUST surface as a
// backend Err (mapped to serverFail by the handler), not be silently
// passed through. Closes the reference-impl footgun where a downstream
// contributor would copy the silent match-all pattern.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_query_body_filter_returns_server_fail() {
    let backend = MemoryBackend::new().with_account("acc1");
    let _ = create_node(&backend, "acc1", "doc", None, "s").await;

    let result = handle_filenode_query(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "filter": { "body": "search-term" },
            "sort": null
        }),
    )
    .await;

    let err = result.expect_err("body filter must surface as a JmapError, not silently match all");
    assert_eq!(
        err.error_type.as_str(),
        "serverFail",
        "unsupported filter condition must surface as serverFail (the \
         MemoryBackend explicitly returns Err so downstream backends do not \
         silently match-all); got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/set + FileNode/changes — state monotonicity invariants.
// Oracle: PLAN.md FileNodeBackend invariant ('State monotonicity:
// get_state returns a different token after every successful mutation.
// Token does not change on failure.'); RFC 8620 §5.2 + §5.3 oldState /
// newState semantics. Regression guards for bd:JMAP-510h.27.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filenode_set_state_advances_on_successful_create() {
    let backend = MemoryBackend::new().with_account("acc1");

    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "first", "parentId": null, "role": null }
            }
        }),
    )
    .await
    .expect("set must succeed");

    let old = resp["oldState"]
        .as_str()
        .expect("oldState must be a string");
    let new = resp["newState"]
        .as_str()
        .expect("newState must be a string");
    assert_ne!(
        old, new,
        "state MUST advance on a successful create (RFC 8620 §5.3 + \
         PLAN.md FileNodeBackend monotonicity invariant): old={old}, new={new}, resp={resp}"
    );
    assert!(
        resp["created"].is_object() && resp["created"]["c1"].is_object(),
        "the create itself must have succeeded: {resp}"
    );
}

#[tokio::test]
async fn filenode_set_state_stays_when_all_creates_fail() {
    let backend = MemoryBackend::new().with_account("acc1");

    // A file node without blobId is invalidProperties per draft §3.1.
    // The whole /set has no successful mutation, so state MUST NOT
    // advance.
    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "name": "bad-file",
                    "parentId": null,
                    "role": null,
                    "nodeType": "file"
                    // intentionally missing blobId
                }
            }
        }),
    )
    .await
    .expect("set must not return top-level error");

    let old = resp["oldState"]
        .as_str()
        .expect("oldState must be a string");
    let new = resp["newState"]
        .as_str()
        .expect("newState must be a string");
    assert_eq!(
        old, new,
        "state MUST NOT advance when no creation succeeded (PLAN.md \
         FileNodeBackend monotonicity invariant: 'Token does not change \
         on failure'): old={old}, new={new}, resp={resp}"
    );
    assert!(
        resp["created"].is_null(),
        "the create must have failed: {resp}"
    );
    assert!(
        resp["notCreated"].is_object(),
        "the failure must surface in notCreated: {resp}"
    );
}

#[tokio::test]
async fn filenode_set_state_advances_exactly_once_on_mixed_success() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Mixed batch: one valid directory create (will succeed), one file
    // without blobId (will fail with invalidProperties). State must
    // advance exactly once — not zero (one create succeeded), not
    // twice (the per-failed-target is not a mutation).
    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "create": {
                "c_good": { "name": "valid-dir", "parentId": null, "role": null },
                "c_bad": {
                    "name": "bad-file",
                    "parentId": null,
                    "role": null,
                    "nodeType": "file"
                    // intentionally missing blobId
                }
            }
        }),
    )
    .await
    .expect("set must not return top-level error");

    let old = resp["oldState"].as_str().expect("oldState").to_owned();
    let new = resp["newState"].as_str().expect("newState").to_owned();
    assert_ne!(
        old, new,
        "mixed /set with at least one successful create MUST advance state: {resp}"
    );
    assert!(
        resp["created"].is_object(),
        "good create must succeed: {resp}"
    );
    assert!(
        resp["notCreated"].is_object(),
        "bad create must surface in notCreated: {resp}"
    );

    // Run a follow-up changes call with sinceState=new. Since no further
    // mutations happened, the changes response MUST be empty (no new
    // tokens, no created/updated/destroyed deltas).
    let (ch_resp, _) = handle_filenode_changes(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "sinceState": &new
        }),
    )
    .await
    .expect("changes must succeed");

    assert_eq!(
        ch_resp["oldState"].as_str().expect("oldState"),
        new.as_str(),
        "changes since the just-emitted newState must have oldState==newState: {ch_resp}"
    );
    assert_eq!(
        ch_resp["newState"].as_str().expect("newState"),
        new.as_str(),
        "changes since the just-emitted newState must have no further state advance: {ch_resp}"
    );
    let created_arr = ch_resp["created"].as_array().expect("created array");
    let updated_arr = ch_resp["updated"].as_array().expect("updated array");
    let destroyed_arr = ch_resp["destroyed"].as_array().expect("destroyed array");
    assert!(
        created_arr.is_empty() && updated_arr.is_empty() && destroyed_arr.is_empty(),
        "changes since the just-emitted newState must have empty deltas: {ch_resp}"
    );
}

// ---------------------------------------------------------------------------
// FileNode/set — onExists="newest"
// Oracle: draft-ietf-jmap-filenode-14 §3.2.3 — "If 'newest', the server
// compares the 'modified' timestamp of the incoming item and the existing
// item. If the incoming item has a strictly later 'modified' value,
// proceed as if 'replace'. Otherwise, reject as if null (alreadyExists)."
// ---------------------------------------------------------------------------

/// Helper: build a directory node with a specific `modified` timestamp.
fn make_dir_with_modified(id: &str, name: &str, parent_id: Option<&str>, modified: &str) -> FileNode {
    let v = json!({
        "id": id,
        "parentId": parent_id,
        "nodeType": "directory",
        "blobId": null,
        "target": null,
        "size": null,
        "name": name,
        "type": null,
        "shareWith": null,
        "role": null,
        "modified": modified
    });
    serde_json::from_value(v).expect("make_dir_with_modified: deserialization must succeed")
}

/// Oracle: incoming modified is strictly later → behaves like "replace".
#[tokio::test]
async fn filenode_set_on_exists_newest_incoming_wins() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed an existing directory with an older modified timestamp.
    backend.seed_node("acc1", make_dir_with_modified("existing-1", "shared", None, "2020-01-01T00:00:00Z"));

    // Create a new node with the same name and a LATER modified timestamp.
    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "onExists": "newest",
            "create": {
                "n": {
                    "name": "shared",
                    "parentId": null,
                    "role": null,
                    "modified": "2025-06-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("must not return top-level error");

    // Incoming is newer → must replace: new node in `created`.
    assert!(
        resp["created"].is_object() && resp["created"]["n"].is_object(),
        "incoming-newer must be created: {resp}"
    );
    // The old node must have been destroyed.
    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    let destroyed_strs: Vec<&str> = destroyed.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        destroyed_strs.contains(&"existing-1"),
        "the existing node must be in the destroyed list: {resp}"
    );
}

/// Oracle: existing modified is later or equal → reject with alreadyExists.
#[tokio::test]
async fn filenode_set_on_exists_newest_existing_wins() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed an existing directory with a LATER modified timestamp.
    backend.seed_node("acc1", make_dir_with_modified("existing-2", "shared", None, "2025-06-01T00:00:00Z"));

    // Attempt to create a node with the same name but an OLDER modified timestamp.
    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "onExists": "newest",
            "create": {
                "n": {
                    "name": "shared",
                    "parentId": null,
                    "role": null,
                    "modified": "2020-01-01T00:00:00Z"
                }
            }
        }),
    )
    .await
    .expect("must not return top-level error");

    // Existing is newer → must reject with alreadyExists.
    let not_created = &resp["notCreated"]["n"];
    assert!(
        not_created.is_object(),
        "existing-newer must produce notCreated: {resp}"
    );
    assert_eq!(
        not_created["type"], "alreadyExists",
        "must be alreadyExists when existing is newer: {resp}"
    );
    assert_eq!(
        not_created["existingId"], "existing-2",
        "alreadyExists must include existingId: {resp}"
    );
}

/// Oracle: when both nodes lack a modified timestamp, the incoming node
/// is NOT strictly later (empty == empty), so reject with alreadyExists.
#[tokio::test]
async fn filenode_set_on_exists_newest_both_missing_modified() {
    let backend = MemoryBackend::new().with_account("acc1");

    // Seed a directory with no modified timestamp.
    backend.seed_node("acc1", make_dir("existing-3", "shared", None));

    // Create a new node with the same name — also no modified timestamp.
    let (resp, _) = handle_filenode_set(
        &backend,
        &(),
        json!({
            "accountId": "acc1",
            "onExists": "newest",
            "create": {
                "n": {
                    "name": "shared",
                    "parentId": null,
                    "role": null
                }
            }
        }),
    )
    .await
    .expect("must not return top-level error");

    // Neither has modified → "" > "" is false → alreadyExists.
    let not_created = &resp["notCreated"]["n"];
    assert!(
        not_created.is_object(),
        "both-missing-modified must produce notCreated: {resp}"
    );
    assert_eq!(
        not_created["type"], "alreadyExists",
        "must be alreadyExists when both lack modified: {resp}"
    );
}

/// Oracle: FileNode/copy with onExists="newest" — incoming source node
/// is strictly later → replace in destination account.
#[tokio::test]
async fn filenode_copy_on_exists_newest_incoming_wins() {
    let backend = MemoryBackend::new().with_account("src").with_account("dst");

    // Source node with a LATER modified timestamp.
    backend.seed_node("src", make_dir_with_modified("src-1", "doc", None, "2025-06-01T00:00:00Z"));
    // Destination node with an OLDER modified timestamp.
    backend.seed_node("dst", make_dir_with_modified("dst-1", "doc", None, "2020-01-01T00:00:00Z"));

    let (resp, _) = handle_filenode_copy(
        &backend,
        &(),
        json!({
            "fromAccountId": "src",
            "accountId": "dst",
            "onExists": "newest",
            "create": {
                "c": { "id": "src-1", "role": null }
            }
        }),
        "c0",
    )
    .await
    .expect("must not return top-level error");

    // Source is newer → must replace: new copy in `created`.
    assert!(
        resp["created"].is_object() && resp["created"]["c"].is_object(),
        "copy newest incoming-wins must produce created entry: {resp}"
    );

    // Verify the old destination node is actually gone.
    let (get_resp, _) = handle_filenode_get(
        &backend,
        &(),
        json!({
            "accountId": "dst",
            "ids": ["dst-1"]
        }),
    )
    .await
    .expect("get must succeed");
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    let not_found_strs: Vec<&str> = not_found.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        not_found_strs.contains(&"dst-1"),
        "the existing destination node must have been destroyed: {get_resp}"
    );
}
