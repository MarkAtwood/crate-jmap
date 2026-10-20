//! `ChatContact/set` create with the owner's identity (bd:JMAP-405z.3).
//!
//! Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec cefc217)
//! §ChatContact/set. The owner's identity is the `ownerUserId` Session
//! field; `MemoryBackend::set_owner_user_id_for_test` configures it.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::handle_contact_set;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::json;

/// draft-atwood-jmap-chat-00 §ChatContact/set: "If `identity` equals the
/// owner's identity (the `ownerUserId` Session field), the server MUST
/// reject the create with an `invalidProperties` SetError naming
/// `identity`: the owner has no ChatContact, and clients refer to the
/// owner by the accountId."
#[tokio::test]
async fn contact_create_owner_identity_invalid_properties() {
    let backend = MemoryBackend::new();
    let account_id = Id::from(ACCOUNT_ID);
    backend.register_account(&account_id);
    backend.set_owner_user_id_for_test(&account_id, "owner@example.com");

    let HandlerOutput { primary: resp, .. } = handle_contact_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "k1": { "identity": "owner@example.com" } }
        }),
    )
    .await
    .expect("ChatContact/set");

    assert!(
        resp["created"]["k1"].is_null(),
        "owner contact created: {resp}"
    );
    assert_eq!(resp["notCreated"]["k1"]["type"], json!("invalidProperties"));
    assert_eq!(resp["notCreated"]["k1"]["properties"], json!(["identity"]));
}
