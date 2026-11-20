//! The account owner's `myRights` and `isSubscribed` on a Mailbox it
//! creates (bd JMAP-4kpz).
//!
//! Oracles:
//! - RFC 8621 §2 (rfc8621.txt:623): `myRights` is "The set of rights
//!   (Access Control Lists (ACLs)) the user has in relation to this
//!   Mailbox".
//! - draft-ietf-jmap-mail-sharing-02 §3.6: "The owner of a mailbox (the
//!   Principal associated with the account containing the mailbox) always
//!   has implicit full rights to the mailbox."
//! - RFC 8621 §2 (rfc8621.txt:696-700): `isSubscribed` "SHOULD default to
//!   ... true for any new Mailboxes created by the user themself."
//!
//! The memory backend's caller is `()`: every caller acts as the account
//! owner.

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_mailbox_get, handle_mailbox_set};
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_types::Id;
use serde_json::{Value, json};

/// Every RFC 8621 §2 MailboxRights property (rfc8621.txt:627-689) and the
/// draft-ietf-jmap-mail-sharing-02 §2 `mayShare` right, true.
fn all_rights() -> Value {
    json!({
        "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
        "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
        "mayRename": true, "mayDelete": true, "maySubmit": true,
        "mayShare": true
    })
}

async fn create(backend: &MemoryBackend, props: Value) -> Value {
    handle_mailbox_set(
        backend,
        &(),
        &BTreeMap::new(),
        json!({"accountId": "acc", "create": {"m": props}}),
        &[],
    )
    .await
    .unwrap()
    .primary
}

async fn get(backend: &MemoryBackend, id: &str) -> Value {
    handle_mailbox_get(backend, &(), json!({"accountId": "acc", "ids": [id]}), &[])
        .await
        .unwrap()
        .primary["list"][0]
        .clone()
}

#[tokio::test]
async fn owner_has_every_right_on_create_echo_and_get() {
    let backend = MemoryBackend::new().with_account(&Id::from("acc"));
    let resp = create(&backend, json!({"name": "Projects"})).await;
    let created = &resp["created"]["m"];
    assert_eq!(created["myRights"], all_rights(), "{resp}");
    let id = created["id"].as_str().unwrap();
    assert_eq!(get(&backend, id).await["myRights"], all_rights());
}

#[tokio::test]
async fn new_mailbox_is_subscribed_by_default() {
    let backend = MemoryBackend::new().with_account(&Id::from("acc"));
    let resp = create(&backend, json!({"name": "Projects"})).await;
    let id = resp["created"]["m"]["id"].as_str().unwrap();
    assert_eq!(get(&backend, id).await["isSubscribed"], true);

    // An explicit value is kept.
    let resp = create(&backend, json!({"name": "Lists", "isSubscribed": false})).await;
    let id = resp["created"]["m"]["id"].as_str().unwrap();
    assert_eq!(get(&backend, id).await["isSubscribed"], false);
}

/// draft-ietf-jmap-metadata-05 §4.1 adds `maySetMetadata` to the Mailbox
/// rights when the capability lists the type; the owner holds it too.
#[tokio::test]
async fn owner_has_may_set_metadata_when_metadata_is_on() {
    let info: DataTypeMetadataInfo = serde_json::from_value(json!({
        "namespaces": [], "supportsVendorNamespaces": true,
        "supportsPrivate": false, "maxDepth": 4
    }))
    .unwrap();
    let backend = MemoryBackend::new()
        .with_metadata_info("Mailbox", info)
        .with_account(&Id::from("acc"));
    let resp = create(&backend, json!({"name": "Projects"})).await;
    let mut want = all_rights();
    want["maySetMetadata"] = json!(true);
    assert_eq!(resp["created"]["m"]["myRights"], want, "{resp}");
}
