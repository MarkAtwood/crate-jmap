//! `ChatContact/set` create rejects a client-supplied `did`
//! (bd:JMAP-0x0q.12).
//!
//! Oracle: draft-atwood-jmap-chat-did-00 (jmap-chat-spec 6bcd9d8)
//! §Optional `did` field, L201: "`did` (String, optional): A
//! DID-referencing property ... It is server-set (`ChatContact/set` create
//! accepts only `identity`); how the server learns the value is
//! deployment-defined." RFC 8620 §5.3 `invalidProperties`: "The record
//! given is invalid in some way", with "a property called `properties`
//! with the list of invalid properties".
//! `did:web:w3c-ccg.github.io:user:alice` is the did:web method
//! specification's example.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_contact_get, handle_contact_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

async fn create(backend: &MemoryBackend, obj: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_contact_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "k1": obj } }),
    )
    .await
    .expect("ChatContact/set");
    resp
}

async fn get_all(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_contact_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
    )
    .await
    .expect("ChatContact/get");
    resp["list"].clone()
}

/// "create accepts only `identity`" (L201): a create carrying `did`, be it
/// a valid DID, a non-DID string, a non-string or `null`, is rejected
/// with `invalidProperties` naming `did`, and nothing is recorded.
#[tokio::test]
async fn create_with_did_rejected() {
    for did in [
        json!("did:web:w3c-ccg.github.io:user:alice"),
        json!("alice@example.com"),
        json!(42),
        json!(null),
    ] {
        let backend = backend();
        let resp = create(
            &backend,
            json!({ "identity": "alice@example.com", "did": did }),
        )
        .await;

        assert!(resp["created"]["k1"].is_null(), "{did}: {resp}");
        assert_eq!(
            resp["notCreated"]["k1"],
            json!({ "type": "invalidProperties", "properties": ["did"] }),
            "{did}"
        );
        assert_eq!(get_all(&backend).await, json!([]), "{did}");
    }
}

/// The same create without `did` succeeds, and no DID is bound: "When
/// absent, no DID is bound to this ChatContact" (L201).
#[tokio::test]
async fn create_without_did_binds_none() {
    let backend = backend();
    let resp = create(&backend, json!({ "identity": "alice@example.com" })).await;

    assert!(resp["notCreated"].is_null(), "{resp}");
    assert_eq!(
        resp["created"]["k1"]["identity"],
        json!("alice@example.com")
    );
    let list = get_all(&backend).await;
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");
    assert!(list[0].get("did").is_none(), "{list}");
}
