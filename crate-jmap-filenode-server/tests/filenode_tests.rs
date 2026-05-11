//! Integration tests for `jmap-filenode-server` using MemoryBackend.
//!
//! All expected values are derived from the spec (draft-ietf-jmap-filenode-13),
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
// Oracle: draft-ietf-jmap-filenode-13 §3.1 (nodeType inference).
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
// Oracle: draft-ietf-jmap-filenode-13 §3.1 (file node requires blobId).
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
// Oracle: draft-ietf-jmap-filenode-13 §3.2.3 (basic destroy).
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
        resp["notDestroyed"].is_null(),
        "notDestroyed must be null for a leaf: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: destroy parent with onDestroyRemoveChildren=false (default) → nodeHasChildren
// Oracle: draft-ietf-jmap-filenode-13 §3.2.3.
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
// Oracle: draft-ietf-jmap-filenode-13 §3.2.3.
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
// Test 6: FileNode/get returns a created node by id
// Oracle: draft-ietf-jmap-filenode-13 §3.2.1.
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
// Oracle: draft-ietf-jmap-filenode-13 §3.2.1 fetchParents.
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
// Test 8: query with depth=1 returns directory and its direct children
// Oracle: draft-ietf-jmap-filenode-13 §3.2.5 (depth parameter).
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
// Oracle: draft-ietf-jmap-filenode-13 §3.2.2 (changes after mutation).
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
