//! jmap-chat-compliance-tests §7.9: a direct chat with a federated user,
//! created by identity (bd:JMAP-ssda, audit JMAP-x3lu).
//!
//! Oracles, both at jmap-chat-spec 120db2c:
//! - jmap-chat-compliance-tests.md L467-471, §7.9, Severity MUST, quoted
//!   on the test.
//! - draft-atwood-jmap-chat-00.md L1089, §ChatContact/set: "If a
//!   ChatContact with that `identity` already exists, the server MUST
//!   reject the create with a SetError of type `alreadyExists`, including
//!   the existing ChatContact's id in the `existingId` field ({{RFC8620}}
//!   Section 5.3). If `identity` equals the owner's identity (the
//!   `ownerUserId` Session field), the server MUST reject the create with
//!   an `invalidProperties` SetError naming `identity`".

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::{
    handle_chat_set, handle_contact_get, handle_contact_query, handle_contact_set,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const OWNER: &str = "owner@home.example";
const FEDERATED: &str = "dave@far.example";

async fn contact_set(backend: &MemoryBackend<()>, create: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_contact_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": create }),
    )
    .await
    .expect("ChatContact/set");
    primary
}

async fn contact_ids_with_identity(backend: &MemoryBackend<()>, identity: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_contact_query(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "identity": identity } }),
    )
    .await
    .expect("ChatContact/query");
    primary["ids"].clone()
}

/// jmap-chat-compliance-tests.md L471: "For an identity of a user with no
/// local account and no ChatContact (federated user), call
/// `ChatContact/set` create with `{"identity": "<identity>"}`. Verify the
/// server creates the ChatContact, assigns its `id`, and returns it with
/// that `identity`. Call it again with the same `identity`. Verify the
/// error is `alreadyExists` with `existingId` equal to that `id`. Call
/// `ChatContact/query` with filter `{"identity": "<identity>"}`. Verify it
/// returns exactly that `id`. Then call `Chat/set` create with `{"kind":
/// "direct", "contactId": "<that id>"}`. Verify success. Call
/// `ChatContact/set` create with `{"identity": "<ownerUserId>"}`, the
/// owner's identity from the Session. Verify the error is
/// `invalidProperties` naming `identity`, and that no ChatContact with
/// that `identity` exists afterwards."
#[tokio::test]
async fn direct_chat_with_a_federated_user_by_identity() {
    let backend = MemoryBackend::<()>::default();
    let account_id = Id::from(ACCOUNT_ID);
    backend.register_account(&account_id);
    backend.set_owner_user_id_for_test(&account_id, OWNER);

    let resp = contact_set(&backend, json!({ "k1": { "identity": FEDERATED } })).await;
    let created = &resp["created"]["k1"];
    let id = created["id"].as_str().expect("assigned id").to_owned();
    let HandlerOutput { primary, .. } = handle_contact_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("ChatContact/get");
    assert_eq!(primary["list"][0]["identity"], FEDERATED, "{primary}");

    let resp = contact_set(&backend, json!({ "k2": { "identity": FEDERATED } })).await;
    assert!(resp["created"]["k2"].is_null(), "duplicate created: {resp}");
    assert_eq!(resp["notCreated"]["k2"]["type"], "alreadyExists", "{resp}");
    assert_eq!(resp["notCreated"]["k2"]["existingId"], json!(id), "{resp}");

    assert_eq!(
        contact_ids_with_identity(&backend, FEDERATED).await,
        json!([id])
    );

    let HandlerOutput { primary, .. } = handle_chat_set(
        &backend,
        &(),
        &BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "d": { "kind": "direct", "contactId": id } },
        }),
        &[],
    )
    .await
    .expect("Chat/set");
    assert!(
        primary["created"]["d"]["id"].is_string(),
        "direct chat: {primary}"
    );

    let resp = contact_set(&backend, json!({ "k3": { "identity": OWNER } })).await;
    assert_eq!(resp["notCreated"]["k3"]["type"], "invalidProperties");
    assert_eq!(resp["notCreated"]["k3"]["properties"], json!(["identity"]));
    assert_eq!(contact_ids_with_identity(&backend, OWNER).await, json!([]));
}
