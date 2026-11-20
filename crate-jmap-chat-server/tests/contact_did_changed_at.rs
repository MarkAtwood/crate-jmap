//! `ChatContact/set` rejects a client-supplied `didChangedAt`.
//!
//! Oracle: draft-atwood-jmap-chat-did-00 (jmap-chat-spec 1018d37) §DID
//! changes: "`didChangedAt` (UTCDate, optional): Server-set." and
//! §ChatContact/set of draft-atwood-jmap-chat-00: "create accepts only
//! `identity`" and "update supports: `blocked`, `displayName`". RFC 8620
//! §5.3: a server-set property in a create or update is
//! `invalidProperties`, with "a property called `properties` with the list
//! of invalid properties".

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::{handle_contact_get, handle_contact_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const WHEN: &str = "2026-11-17T12:00:00Z";

fn backend() -> MemoryBackend<()> {
    let backend = MemoryBackend::<()>::default();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

async fn set(backend: &MemoryBackend<()>, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary: resp, .. } = handle_contact_set(backend, &(), args)
        .await
        .expect("ChatContact/set");
    resp
}

async fn get_all(backend: &MemoryBackend<()>) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_contact_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
    )
    .await
    .expect("ChatContact/get");
    resp["list"].clone()
}

/// A create carrying `didChangedAt` is refused, and nothing is recorded.
#[tokio::test]
async fn create_with_did_changed_at_rejected() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "create": { "k1": { "identity": "alice@example.com", "didChangedAt": WHEN } } }),
    )
    .await;
    assert!(resp["created"]["k1"].is_null(), "{resp}");
    assert_eq!(
        resp["notCreated"]["k1"],
        json!({ "type": "invalidProperties", "properties": ["didChangedAt"] })
    );
    assert_eq!(get_all(&backend).await, json!([]));
}

/// An update setting `didChangedAt` is refused and the record keeps none;
/// an update of `blocked` beside it shows the refusal is about the field.
#[tokio::test]
async fn update_of_did_changed_at_rejected() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "create": { "k1": { "identity": "alice@example.com" } } }),
    )
    .await;
    let id = resp["created"]["k1"]["id"].as_str().unwrap().to_owned();

    let resp = set(
        &backend,
        json!({ "update": { &id: { "didChangedAt": WHEN } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][&id]["properties"],
        json!(["didChangedAt"]),
        "{resp}"
    );
    let list = get_all(&backend).await;
    assert!(list[0].get("didChangedAt").is_none(), "{list}");

    let resp = set(&backend, json!({ "update": { &id: { "blocked": true } } })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
}
