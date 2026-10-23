//! `destroy_space_file_tree`: §Space Deletion.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 33702a5 ("filenode Lnnn") and RFC 8620:
//!
//! - filenode L214: "When a Space is destroyed via `Space/set` destroy,
//!   the server SHOULD also destroy the associated file tree and all
//!   FileNodes within it."
//! - filenode L216: "The server MAY instead retain the file tree for an
//!   implementation-defined grace period". Decided 2026-10-20: no grace
//!   period, so the tree is destroyed with the Space.
//! - filenode L126: "A Space's file tree is rooted at a
//!   single FileNode identified by the well-known `role` value
//!   `"chatspace"` and the `spaceId` extension property".
//! - RFC 8620 §5.1 `notFound`: "the list of ids passed to the method for
//!   records that do not exist"; §5.2 `destroyed`: "an array of ids for
//!   records that have been destroyed since the old state".
//!
//! Records are hand-written JSON from the drafts' field lists.

use std::collections::HashSet;

use jmap_chat_filenode_server::destroy_space_file_tree;
use jmap_filenode_server::memory::MemoryBackend;
use jmap_filenode_server::{handle_filenode_changes, handle_filenode_get};
use jmap_filenode_types::FileNode;
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "A1";
const ROLE: &str = "chatspace";

fn node(v: Value) -> FileNode {
    serde_json::from_value(v).expect("FileNode fixture")
}

/// Space S1's tree: root `r1` > `f1`, `d1` > `f2`. Space S2's tree: root
/// `r2` > `f3`. A plain top-level directory `plain` > `f4`.
fn fixture() -> MemoryBackend {
    let files = MemoryBackend::new().with_account(&Id::from(ACCOUNT));
    for v in [
        json!({ "id": "r1", "@type": "Directory", "name": "r1", "parentId": null,
                "role": ROLE, "spaceId": "S1" }),
        json!({ "id": "f1", "@type": "File", "name": "f1", "parentId": "r1",
                "blobId": "b-f1", "size": 1 }),
        json!({ "id": "d1", "@type": "Directory", "name": "d1", "parentId": "r1" }),
        json!({ "id": "f2", "@type": "File", "name": "f2", "parentId": "d1",
                "blobId": "b-f2", "size": 1 }),
        json!({ "id": "r2", "@type": "Directory", "name": "r2", "parentId": null,
                "role": ROLE, "spaceId": "S2" }),
        json!({ "id": "f3", "@type": "File", "name": "f3", "parentId": "r2",
                "blobId": "b-f3", "size": 1 }),
        json!({ "id": "plain", "@type": "Directory", "name": "plain", "parentId": null }),
        json!({ "id": "f4", "@type": "File", "name": "f4", "parentId": "plain",
                "blobId": "b-f4", "size": 1 }),
    ] {
        files.seed_node(&Id::from(ACCOUNT), node(v));
    }
    files
}

fn ids(v: &Value) -> HashSet<String> {
    v.as_array()
        .expect("id array")
        .iter()
        .map(|id| id.as_str().expect("id string").to_owned())
        .collect()
}

fn set(list: &[&str]) -> HashSet<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

async fn not_found(files: &MemoryBackend, list: &[&str]) -> HashSet<String> {
    let out = handle_filenode_get(files, &(), json!({ "accountId": ACCOUNT, "ids": list }))
        .await
        .expect("FileNode/get");
    ids(&out.primary["notFound"])
}

/// L214, L216 (no grace period): the root "and all FileNodes within it"
/// are gone, at every depth.
#[tokio::test]
async fn destroys_the_root_and_every_node_within_it() {
    let files = fixture();
    let destroyed = destroy_space_file_tree(&files, &(), &Id::from(ACCOUNT), &Id::from("S1"))
        .await
        .expect("destroy");
    let destroyed: HashSet<String> = destroyed.iter().map(|id| id.as_ref().to_owned()).collect();
    assert_eq!(destroyed, set(&["r1", "f1", "d1", "f2"]));
    assert_eq!(
        not_found(&files, &["r1", "f1", "d1", "f2"]).await,
        set(&["r1", "f1", "d1", "f2"])
    );
}

/// L214 destroys "the associated file tree" only (L126: the tree whose
/// root carries that Space's `spaceId`): another Space's tree and a
/// non-Space directory survive.
#[tokio::test]
async fn leaves_other_trees_alone() {
    let files = fixture();
    destroy_space_file_tree(&files, &(), &Id::from(ACCOUNT), &Id::from("S1"))
        .await
        .expect("destroy");
    assert!(
        not_found(&files, &["r2", "f3", "plain", "f4"])
            .await
            .is_empty()
    );
}

/// RFC 8620 §5.2: destroying the tree destroys FileNode records, so
/// `FileNode/changes` from the state before it reports every node as
/// `destroyed`.
#[tokio::test]
async fn destroy_is_a_filenode_state_change() {
    let files = fixture();
    let before = handle_filenode_changes(
        &files,
        &(),
        json!({ "accountId": ACCOUNT, "sinceState": "0" }),
    )
    .await
    .expect("FileNode/changes");
    let since = before.primary["newState"].clone();

    destroy_space_file_tree(&files, &(), &Id::from(ACCOUNT), &Id::from("S1"))
        .await
        .expect("destroy");

    let out = handle_filenode_changes(
        &files,
        &(),
        json!({ "accountId": ACCOUNT, "sinceState": since }),
    )
    .await
    .expect("FileNode/changes");
    assert_ne!(out.primary["newState"], since);
    assert_eq!(
        ids(&out.primary["destroyed"]),
        set(&["r1", "f1", "d1", "f2"])
    );
}

/// L214 applies to "the associated file tree"; a Space that never had
/// one destroys nothing.
#[tokio::test]
async fn space_without_a_tree_destroys_nothing() {
    let files = fixture();
    let destroyed = destroy_space_file_tree(&files, &(), &Id::from(ACCOUNT), &Id::from("S9"))
        .await
        .expect("destroy");
    assert!(destroyed.is_empty());
    assert!(not_found(&files, &["r1", "r2", "plain"]).await.is_empty());
}
