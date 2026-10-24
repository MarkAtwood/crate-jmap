//! RFC 8620 §1.2 wire-id validation on `jmap-filenode-server` `/set` handlers
//! (bd JMAP-k9va), mirroring the canonical
//! `crate-jmap-mail-server/tests/integration.rs`
//! `mailbox_set_and_query_reject_malformed_ids`.
//!
//! Oracle: RFC 8620 §1.2, an Id "MUST only contain characters from the
//! 'URL and Filename Safe' base64 alphabet". `"bad id"` contains a space,
//! so a `/set` `update` key or `destroy` element naming it fails that
//! entry with `invalidProperties` naming `id`, written out by hand below.
//! The same check covers Id-typed properties of a create body and the
//! `ids` argument of `/get`.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_filenode_server::{handle_filenode_get, handle_filenode_set};
use jmap_types::{Id, MethodErrorType};
use serde_json::{Value, json};

/// Asserts the hand-written RFC 8620 §5.3 SetError for a malformed id.
fn assert_invalid_id(entry: &Value, resp: &Value) {
    assert_eq!(entry["type"], "invalidProperties", "{resp}");
    assert_eq!(entry["properties"], json!(["id"]), "{resp}");
}

#[tokio::test]
async fn filenode_set_rejects_malformed_update_and_destroy_ids() {
    let account_id = Id::from("account1");
    let backend = MemoryBackend::new().with_account(&account_id);
    let args_with_account = |mut args: Value| {
        args["accountId"] = json!(account_id.as_ref());
        args
    };
    let args = json!({
        "update": { "bad id": { "name": "x" } },
        "destroy": ["bad id"]
    });
    let resp = handle_filenode_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args_with_account(args),
        &[],
    )
    .await
    .expect("/set must not fail as a method")
    .primary;
    assert_invalid_id(&resp["notUpdated"]["bad id"], &resp);
    assert_invalid_id(&resp["notDestroyed"]["bad id"], &resp);
}

/// draft-ietf-jmap-filenode-16 `parentId` is "Id|null"; a create naming
/// `"bad id"` there fails with `invalidProperties` and creates nothing.
#[tokio::test]
async fn filenode_set_create_rejects_malformed_parent_id() {
    let backend = MemoryBackend::new().with_account(&Id::from("account1"));
    let args = json!({
        "accountId": "account1",
        "create": { "c": { "@type": "Directory", "name": "d", "parentId": "bad id" } }
    });
    let resp = handle_filenode_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("/set must not fail as a method")
        .primary;
    assert_eq!(
        resp["notCreated"]["c"]["type"], "invalidProperties",
        "{resp}"
    );
    assert!(resp["created"].get("c").is_none(), "{resp}");
}

/// RFC 8620 §5.1 types `ids` "Id[]|null" and `notFound` "Id[]"; §3.9: "In
/// case of invalid arguments (unknown/insufficient/wrong type for data,
/// etc.), the method MUST return an 'invalidArguments' error". `"bad id"`
/// is not an Id, so the call fails instead of listing it in `notFound`.
#[tokio::test]
async fn filenode_get_rejects_malformed_ids() {
    let backend = MemoryBackend::new().with_account(&Id::from("account1"));
    let err = handle_filenode_get(
        &backend,
        &(),
        json!({ "accountId": "account1", "ids": ["bad id"] }),
        &[],
    )
    .await
    .expect_err("a malformed id in ids must be rejected");
    assert_eq!(err.error_type, MethodErrorType::InvalidArguments);
}

/// draft-ietf-jmap-filenode-16 types `shareWith` "Id[FilesRights]|null",
/// so each key is an Id. A `"bad id"` key on create, or as the id segment
/// of a `shareWith/<id>/<right>` patch path, is `invalidProperties` naming
/// `shareWith`. A well-formed segment (`p1`) passes the check and reaches
/// the backend, which reports RFC 8620 §5.3 `notFound` for the missing
/// node.
#[tokio::test]
async fn filenode_set_rejects_malformed_share_with_keys() {
    let backend = MemoryBackend::new().with_account(&Id::from("account1"));
    let args = json!({
        "accountId": "account1",
        "create": { "c1": {
            "@type": "Directory",
            "name": "d",
            "shareWith": { "bad id": { "mayRead": true } }
        } },
        "update": {
            "n1": { "shareWith/bad id/mayRead": true },
            "n2": { "shareWith": { "bad id": { "mayRead": true } } },
            "n3": { "shareWith/p1/mayRead": true }
        }
    });
    let resp = handle_filenode_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("/set must not fail as a method")
        .primary;
    for entry in [
        &resp["notCreated"]["c1"],
        &resp["notUpdated"]["n1"],
        &resp["notUpdated"]["n2"],
    ] {
        assert_eq!(entry["type"], "invalidProperties", "{resp}");
        assert_eq!(entry["properties"], json!(["shareWith"]), "{resp}");
    }
    assert_eq!(resp["notUpdated"]["n3"]["type"], "notFound", "{resp}");
}
