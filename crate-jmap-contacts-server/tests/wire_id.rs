//! RFC 8620 §1.2 wire-id validation on `jmap-contacts-server` `/set` handlers
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
use jmap_contacts_server::{handle_address_book_set, handle_contact_card_copy};
use jmap_types::Id;
use serde_json::{json, Value};

/// Asserts the hand-written RFC 8620 §5.3 SetError for a malformed id.
fn assert_invalid_id(entry: &Value, resp: &Value) {
    assert_eq!(entry["type"], "invalidProperties", "{resp}");
    assert_eq!(entry["properties"], json!(["id"]), "{resp}");
}

#[tokio::test]
async fn address_book_set_rejects_malformed_update_and_destroy_ids() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    let args_with_account = |mut args: Value| {
        args["accountId"] = json!(account_id.as_ref());
        args
    };
    let args = json!({
        "update": { "bad id": { "name": "x" } },
        "destroy": ["bad id"]
    });
    let resp = handle_address_book_set(&backend, &(), args_with_account(args))
        .await
        .expect("/set must not fail as a method")
        .primary;
    assert_invalid_id(&resp["notUpdated"]["bad id"], &resp);
    assert_invalid_id(&resp["notDestroyed"]["bad id"], &resp);
}

/// RFC 8620 §5.4 `fromAccountId` is an Id argument; a malformed one is
/// the method-level `invalidArguments` error (RFC 8620 §3.6.2).
#[tokio::test]
async fn contact_card_copy_rejects_malformed_from_account_id() {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    let args = json!({
        "accountId": account_id.as_ref(),
        "fromAccountId": "bad id",
        "create": {}
    });
    let err = handle_contact_card_copy(&backend, &(), args, "c0")
        .await
        .expect_err("a malformed fromAccountId must fail the method");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}
