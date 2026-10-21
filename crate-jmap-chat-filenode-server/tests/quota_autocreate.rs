//! `check_space_quota` and `create_space_file_tree`: the account-level
//! capability fields.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 33702a5 ("filenode Lnnn"):
//!
//! - L119: "Maximum total blob storage in bytes across all FileNodes in a
//!   single Space's file tree. ... When a `FileNode/set` create would
//!   cause the Space file tree to exceed this limit, the server MUST
//!   return an `overQuota` SetError (as defined in {{RFC8620}} Section
//!   5.3)."
//! - L122: "When `true`, the server automatically creates a Space file
//!   tree root for each new Space at Space creation time and includes the
//!   new FileNode in subsequent `FileNode/changes` responses."
//! - L126: "A Space's file tree is rooted at a single FileNode
//!   identified by the well-known `role` value `"chatspace"` and the
//!   `spaceId` extension property".
//! - L149: "When a file tree exists for the Space, exactly one matching
//!   FileNode is returned."
//!
//! Records are hand-written JSON from the drafts' field lists.

use jmap_chat_filenode_server::{check_space_quota, create_space_file_tree};
use jmap_filenode_server::memory::MemoryBackend;
use jmap_filenode_server::{handle_filenode_changes, handle_filenode_query};
use jmap_filenode_types::FileNode;
use jmap_types::Id;
use serde_json::{json, Value};

const ACCOUNT: &str = "A1";
const ROLE: &str = "chatspace";
const LIMIT: u64 = 100;

fn node(v: Value) -> FileNode {
    serde_json::from_value(v).expect("FileNode fixture")
}

fn dir(id: &str, parent: Option<&str>) -> FileNode {
    node(json!({ "id": id, "@type": "Directory", "name": id, "parentId": parent }))
}

fn space_root(id: &str, space_id: &str) -> FileNode {
    node(
        json!({ "id": id, "@type": "Directory", "name": id, "parentId": null,
                 "role": ROLE, "spaceId": space_id }),
    )
}

fn file(id: &str, parent: &str, size: u64) -> FileNode {
    node(
        json!({ "id": id, "@type": "File", "name": id, "parentId": parent,
                 "blobId": format!("b-{id}"), "size": size }),
    )
}

/// Space S1's tree: root `r1` > `f1` (30 bytes), `d1` > `f2` (30 bytes),
/// 60 bytes in all. Space S2's tree: root `r2` > `f3` (1000 bytes). A
/// plain top-level directory `plain` > `f4` (1000 bytes).
fn fixture() -> MemoryBackend {
    let files = MemoryBackend::new().with_account(ACCOUNT);
    for n in [
        space_root("r1", "S1"),
        file("f1", "r1", 30),
        dir("d1", Some("r1")),
        file("f2", "d1", 30),
        space_root("r2", "S2"),
        file("f3", "r2", 1000),
        dir("plain", None),
        file("f4", "plain", 1000),
    ] {
        files.seed_node(ACCOUNT, n);
    }
    files
}

/// The SetError for a create of `new`, as wire JSON, or `None` if the
/// create may proceed.
async fn check(files: &MemoryBackend, new: FileNode) -> Option<Value> {
    check_space_quota(files, &(), &Id::from(ACCOUNT), &new, LIMIT)
        .await
        .expect("storage")
        .err()
        .map(|e| serde_json::to_value(e).expect("SetError serializes"))
}

/// L119: only a create that would "exceed" the limit is refused; 60 + 40
/// reaches 100 without exceeding it.
#[tokio::test]
async fn create_reaching_the_limit_passes() {
    let files = fixture();
    assert_eq!(check(&files, file("new", "r1", 40)).await, None);
}

/// L119: "the server MUST return an `overQuota` SetError": 60 + 41 > 100.
#[tokio::test]
async fn create_exceeding_the_limit_is_over_quota() {
    let files = fixture();
    let err = check(&files, file("new", "r1", 41)).await.expect("refused");
    assert_eq!(err["type"], "overQuota");
}

/// L119: the total is "across all FileNodes in a single Space's file
/// tree", so a create deep in the tree counts the whole tree.
#[tokio::test]
async fn nested_create_counts_the_whole_tree() {
    let files = fixture();
    let err = check(&files, file("new", "d1", 41)).await.expect("refused");
    assert_eq!(err["type"], "overQuota");
}

/// L119: the limit is per "single Space's file tree": S2's 1000 bytes do
/// not count against S1, and S2 alone is already over.
#[tokio::test]
async fn other_spaces_tree_is_not_counted() {
    let files = fixture();
    assert_eq!(check(&files, file("new", "d1", 40)).await, None);
    let err = check(&files, file("new", "r2", 1)).await.expect("refused");
    assert_eq!(err["type"], "overQuota");
}

/// L119, L126: the limit applies to Space file trees only, those rooted
/// at a node with role `chatspace`.
#[tokio::test]
async fn node_outside_a_space_tree_is_not_checked() {
    let files = fixture();
    assert_eq!(check(&files, file("new", "plain", 1000)).await, None);
    assert_eq!(check(&files, dir("top", None)).await, None);
}

/// L122: the root is created "at Space creation time" and is the Space's
/// file tree root (L126), found by the discovery query (L149).
#[tokio::test]
async fn auto_created_root_is_the_spaces_root() {
    let files = MemoryBackend::new().with_account(ACCOUNT);
    let (id, root) = create_space_file_tree(&files, &(), &Id::from(ACCOUNT), &Id::from("S1"))
        .await
        .expect("create");
    let wire = serde_json::to_value(&root).expect("FileNode serializes");
    assert_eq!(wire["@type"], "Directory");
    assert_eq!(wire["parentId"], Value::Null);
    assert_eq!(wire["role"], ROLE);
    assert_eq!(wire["spaceId"], "S1");

    let out = handle_filenode_query(
        &files,
        &(),
        json!({ "accountId": ACCOUNT, "filter": { "role": ROLE, "spaceId": "S1" } }),
    )
    .await
    .expect("FileNode/query");
    assert_eq!(out.primary["ids"], json!([id]));
}

/// L122: the server "includes the new FileNode in subsequent
/// `FileNode/changes` responses."
#[tokio::test]
async fn auto_created_root_is_in_filenode_changes() {
    let files = MemoryBackend::new().with_account(ACCOUNT);
    let before = handle_filenode_changes(
        &files,
        &(),
        json!({ "accountId": ACCOUNT, "sinceState": "0" }),
    )
    .await
    .expect("FileNode/changes");
    let since = before.primary["newState"].clone();

    let (id, _) = create_space_file_tree(&files, &(), &Id::from(ACCOUNT), &Id::from("S1"))
        .await
        .expect("create");

    let out = handle_filenode_changes(
        &files,
        &(),
        json!({ "accountId": ACCOUNT, "sinceState": since }),
    )
    .await
    .expect("FileNode/changes");
    assert_eq!(out.primary["created"], json!([id]), "{}", out.primary);
}
