//! A Space root destroy that removes its subtree, refused whole for a
//! member without `"manage_space"`.
//!
//! Oracle: draft-atwood-jmap-chat-filenode-00.md at jmap-chat-spec
//! 29eb435, §Permission Mapping (L172, L182), and RFC 8620:
//!
//! - L172: a member with `"manage_channels"` or `"manage_space"` has "The
//!   above, plus `mayDelete`, `mayRename`, and `mayModifyContent`".
//! - L182: "Creating, renaming, or destroying a Space root requires
//!   `"manage_space"` permission, whatever the root's `myRights` report;
//!   the server SHOULD reject these from other members with `forbidden`.
//!   ... Destroying a Space root follows {{JMAP-FILENODE}} for its
//!   children".
//! - RFC 8620 §5.3 (lines 1947-1948): "Each creation, modification, or
//!   destruction of an object is considered an atomic unit."
//! - draft-ietf-jmap-filenode-20 §3.2.3: with onDestroyRemoveChildren
//!   "destroying a node also destroys all its descendants"; for onExists
//!   "replace" "the server destroys the existing node".
//!
//! Records are hand-written JSON from those lines.

mod common;

use common::{ACCOUNT, Deployment};
use jmap_filenode_server::{handle_filenode_get, handle_filenode_set};
use serde_json::{Value, json};

/// The FileNode/set response for `args` (with `accountId` added).
async fn set(deployment: &Deployment, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_filenode_set(
        deployment,
        &(),
        &std::collections::BTreeMap::new(),
        args,
        &[],
    )
    .await
    .expect("FileNode/set")
    .primary
}

/// The ids of `ids` that FileNode/get still finds, sorted.
async fn surviving(deployment: &Deployment, ids: &[&str]) -> Vec<String> {
    let resp = handle_filenode_get(
        deployment,
        &(),
        json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["id"] }),
        &[],
    )
    .await
    .expect("FileNode/get")
    .primary;
    let mut found: Vec<String> = resp["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|n| n["id"].as_str().expect("id").to_owned())
        .collect();
    found.sort();
    found
}

/// L182 and RFC 8620 §5.3: a `"manage_channels"` member holds mayDelete
/// on the root (L172) but may not destroy it, so the cascade is refused
/// with `forbidden` and the root's descendants stay.
#[tokio::test]
async fn cascade_without_manage_space_destroys_nothing() {
    let deployment = Deployment::new(&["manage_channels"]);
    let resp = set(
        &deployment,
        json!({ "onDestroyRemoveChildren": true, "destroy": ["root"] }),
    )
    .await;
    assert_eq!(resp["notDestroyed"]["root"]["type"], "forbidden", "{resp}");
    assert!(resp["destroyed"].is_null(), "{resp}");
    assert_eq!(
        surviving(&deployment, &["root", "docs", "f1"]).await,
        ["docs", "f1", "root"]
    );
}

/// L182 and RFC 8620 §5.3, for an onExists "replace" of the root by a
/// new top-level Directory of the same name.
#[tokio::test]
async fn replace_without_manage_space_destroys_nothing() {
    let deployment = Deployment::new(&["manage_channels"]);
    let resp = set(
        &deployment,
        json!({
            "onExists": "replace",
            "onDestroyRemoveChildren": true,
            "create": { "c": { "@type": "Directory", "name": "S1", "parentId": null } }
        }),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], "forbidden", "{resp}");
    assert!(resp["destroyed"].is_null(), "{resp}");
    assert_eq!(
        surviving(&deployment, &["root", "docs", "f1"]).await,
        ["docs", "f1", "root"]
    );
}

/// L182: with `"manage_space"` the destroy of the root "follows
/// {{JMAP-FILENODE}} for its children", so the cascade removes all three.
#[tokio::test]
async fn cascade_with_manage_space_destroys_the_tree() {
    let deployment = Deployment::new(&["manage_space"]);
    let resp = set(
        &deployment,
        json!({ "onDestroyRemoveChildren": true, "destroy": ["root"] }),
    )
    .await;
    assert!(resp["notDestroyed"].is_null(), "{resp}");
    assert!(
        surviving(&deployment, &["root", "docs", "f1"])
            .await
            .is_empty()
    );
}
