//! RFC 8620 §1.2 wire-id validation on `jmap-filenode-server` `/set` handlers
//! (bd JMAP-k9va), mirroring the canonical
//! `crate-jmap-mail-server/tests/integration.rs`
//! `mailbox_set_and_query_reject_malformed_ids`.
//!
//! Oracle: RFC 8620 §1.2, an Id "MUST only contain characters from the
//! 'URL and Filename Safe' base64 alphabet". `"bad id"` contains a space,
//! so a `/set` `update` key or `destroy` element naming it fails that
//! entry with `invalidProperties` naming `id`, written out by hand below.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_filenode_server::handle_filenode_set;
use jmap_types::Id;
use serde_json::{Value, json};

/// Asserts the hand-written RFC 8620 §5.3 SetError for a malformed id.
fn assert_invalid_id(entry: &Value, resp: &Value) {
    assert_eq!(entry["type"], "invalidProperties", "{resp}");
    assert_eq!(entry["properties"], json!(["id"]), "{resp}");
}

#[tokio::test]
async fn filenode_set_rejects_malformed_update_and_destroy_ids() {
    let account_id = Id::from("account1");
    let backend = MemoryBackend::new().with_account(account_id.as_ref());
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
    )
    .await
    .expect("/set must not fail as a method")
    .primary;
    assert_invalid_id(&resp["notUpdated"]["bad id"], &resp);
    assert_invalid_id(&resp["notDestroyed"]["bad id"], &resp);
}
