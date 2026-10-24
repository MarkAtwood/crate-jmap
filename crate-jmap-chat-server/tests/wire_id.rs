//! RFC 8620 §1.2 wire-id validation on `jmap-chat-server` `/set` handlers
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

use common::{ACCOUNT_ID, MemoryBackend, make_space};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{handle_chat_set, handle_emoji_set, handle_invite_set, handle_space_set};
use jmap_types::Id;
use serde_json::{Value, json};

/// Asserts the hand-written RFC 8620 §5.3 SetError for a malformed id.
fn assert_invalid_id(entry: &Value, resp: &Value) {
    assert_invalid_property(entry, resp, "id");
}

/// Asserts an `invalidProperties` SetError naming exactly `property`.
fn assert_invalid_property(entry: &Value, resp: &Value, property: &str) {
    assert_eq!(entry["type"], "invalidProperties", "{resp}");
    assert_eq!(entry["properties"], json!([property]), "{resp}");
}

/// Runs one `/set` create `c` through `handler` and returns its
/// `notCreated` entry with the full response. A trailing `using` is
/// passed to handlers that take the request's capability list.
macro_rules! create_entry {
    ($handler:ident, $backend:expr, $obj:expr $(, $using:expr)?) => {{
        let resp = $handler(
            $backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "create": { "c": $obj } }),
            $($using)?
        )
        .await
        .expect("/set must not fail as a method")
        .primary;
        (resp["notCreated"]["c"].clone(), resp)
    }};
}

async fn create_group(backend: &MemoryBackend) -> String {
    let resp = handle_chat_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
        }),
        &[],
    )
    .await
    .expect("Chat/set")
    .primary;
    resp["created"]["g"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("group not created: {resp}"))
        .to_owned()
}

#[tokio::test]
async fn space_set_rejects_malformed_update_and_destroy_ids() {
    let backend = common::account_backend();
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
    let resp = handle_space_set(
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

#[tokio::test]
async fn chat_set_rejects_malformed_destroy_id() {
    let backend = common::account_backend();
    let resp = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "destroy": ["bad id"] }),
        &[],
    )
    .await
    .expect("/set must not fail as a method")
    .primary;
    assert_invalid_id(&resp["notDestroyed"]["bad id"], &resp);
}

/// §Chat/set create: `memberIds` are "ChatContact.ids", `contactId` and
/// `avatarBlobId` are Ids; a malformed one fails the create naming it.
#[tokio::test]
async fn chat_set_create_rejects_malformed_id_properties() {
    let backend = common::account_backend();
    let (entry, resp) = create_entry!(
        handle_chat_set,
        &backend,
        json!({ "kind": "group", "name": "G", "memberIds": ["bad id"] }),
        &[]
    );
    assert_invalid_property(&entry, &resp, "memberIds");
    let (entry, resp) = create_entry!(
        handle_chat_set,
        &backend,
        json!({ "kind": "direct", "contactId": "bad id" }),
        &[]
    );
    assert_invalid_property(&entry, &resp, "contactId");
    let (entry, resp) = create_entry!(
        handle_chat_set,
        &backend,
        json!({ "kind": "group", "name": "G", "memberIds": [], "avatarBlobId": "bad id" }),
        &[]
    );
    assert_invalid_property(&entry, &resp, "avatarBlobId");
}

#[tokio::test]
async fn chat_set_update_rejects_malformed_pinned_message_id() {
    let backend = common::account_backend();
    let chat_id = create_group(&backend).await;
    let resp = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { &chat_id: { "pinnedMessageIds": ["bad id"] } }
        }),
        &[],
    )
    .await
    .expect("/set must not fail as a method")
    .primary;
    assert_invalid_property(&resp["notUpdated"][&chat_id], &resp, "pinnedMessageIds");
}

#[tokio::test]
async fn message_set_create_rejects_malformed_reply_to_and_thread_root() {
    let backend = common::account_backend();
    let chat_id = create_group(&backend).await;
    for property in ["replyTo", "threadRootId"] {
        let mut obj = json!({
            "chatId": chat_id,
            "body": "hi",
            "bodyType": "text/plain",
            "sentAt": "2024-01-01T00:00:00Z",
        });
        obj[property] = json!("bad id");
        let (entry, resp) = create_entry!(handle_message_set, &backend, obj, &[]);
        assert_invalid_property(&entry, &resp, property);
    }
}

#[tokio::test]
async fn invite_set_create_rejects_malformed_default_channel_id() {
    let backend = common::account_backend();
    let space_id = make_space(&backend, "S").await;
    let (entry, resp) = create_entry!(
        handle_invite_set,
        &backend,
        json!({ "spaceId": space_id, "defaultChannelId": "bad id" })
    );
    assert_invalid_property(&entry, &resp, "defaultChannelId");
}

#[tokio::test]
async fn emoji_set_create_rejects_malformed_space_id() {
    let backend = common::account_backend();
    let (entry, resp) = create_entry!(
        handle_emoji_set,
        &backend,
        json!({ "name": "party", "blobId": "b", "spaceId": "bad id" })
    );
    assert_invalid_property(&entry, &resp, "spaceId");
}
