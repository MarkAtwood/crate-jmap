//! `Message/set` create stores the Message and enqueues its outbound
//! delivery as one unit through `ChatBackend::create_message`
//! (bd:JMAP-z4z0.5).
//!
//! Oracles:
//!
//! - draft-atwood-jmap-chat-00 at jmap-chat-spec b1cd625 L1222
//!   (§Message/set): "The server sets `id`, `senderMsgId`, `senderId`,
//!   `receivedAt`, `deliveryState`, and delivery timestamp fields, then
//!   enqueues the message for outbound delivery."
//! - draft-atwood-jmap-chat-00 §Message `senderMsgId`: "Equals `id` for
//!   messages composed by the owner."
//! - `ChatBackend::create_object` contract: "A call that returns `Err`
//!   MUST leave the stored data, the state and the change log unchanged."
//!   `create_message` extends it: "On `Err` nothing is stored and nothing
//!   is enqueued."
//! - RFC 8620 §5.3: a backend failure is reported per entry in
//!   `notCreated`; an unexpected one is `serverFail`.

mod common;

use common::{ACCOUNT_ID, InjectableBackend, TrackingBackend};
use jmap_chat_server::message::{handle_message_changes, handle_message_get, handle_message_set};
use jmap_chat_server::{ChatBackend, OutboundEvent};
use jmap_server::HandlerOutput;
use jmap_types::{Id, State};
use serde_json::{Value, json};

async fn state<B: ChatBackend<CallerCtx = ()>>(backend: &B) -> State {
    backend
        .get_state::<jmap_chat_types::Message>(&(), &Id::from(ACCOUNT_ID))
        .await
        .expect("Message state")
}

async fn create<B: ChatBackend<CallerCtx = ()>>(backend: &B) -> Value {
    let HandlerOutput { primary, .. } = handle_message_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "c": {
                "chatId": "g1",
                "body": "hi",
                "bodyType": "text/plain",
                "sentAt": "2026-01-02T00:00:00Z",
            } },
        }),
        &[],
    )
    .await
    .expect("Message/set");
    primary
}

async fn stored_ids<B: ChatBackend<CallerCtx = ()>>(backend: &B) -> Vec<Value> {
    let HandlerOutput { primary, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null, "properties": ["id"] }),
        &[],
    )
    .await
    .expect("Message/get");
    primary["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|m| m["id"].clone())
        .collect()
}

/// Creates one Message and checks the stored Message, the state change
/// and the single `NewMessage` event against the spec.
async fn check_create<B: ChatBackend<CallerCtx = ()>>(
    backend: &B,
    take_events: impl FnOnce() -> Vec<OutboundEvent>,
) {
    let before = state(backend).await;
    let resp = create(backend).await;
    let created = &resp["created"]["c"];
    let id = created["id"].as_str().expect("created id").to_owned();
    assert_eq!(created["senderMsgId"], json!(id), "{resp}");
    assert_eq!(resp["notCreated"], Value::Null, "{resp}");
    assert_eq!(stored_ids(backend).await, vec![json!(id)]);

    let HandlerOutput {
        primary: changes, ..
    } = handle_message_changes(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": before }),
        &[],
    )
    .await
    .expect("Message/changes");
    assert_eq!(changes["created"], json!([id]), "{changes}");
    assert_eq!(changes["updated"], json!([]), "{changes}");
    assert_eq!(changes["destroyed"], json!([]), "{changes}");

    assert_eq!(
        take_events(),
        vec![OutboundEvent::NewMessage {
            chat_id: Id::from("g1"),
            message_id: Id::from(id.as_str()),
            sender_msg_id: Id::from(id.as_str()),
        }]
    );
}

#[tokio::test]
async fn failed_create_message_stores_and_enqueues_nothing() {
    let backend = InjectableBackend::new();
    common::seed_group_chat(&backend.inner, "g1");
    let before = state(&backend).await;
    backend.inject("Message", "create_message");

    let resp = create(&backend).await;

    assert_eq!(resp["notCreated"]["c"]["type"], "serverFail", "{resp}");
    assert_eq!(resp["created"], Value::Null, "{resp}");
    assert_eq!(stored_ids(&backend).await, Vec::<Value>::new());
    assert_eq!(state(&backend).await, before);
    assert_eq!(backend.inner.take_outbound_events_for_test(), vec![]);
}

#[tokio::test]
async fn default_create_message_stores_and_reports_one_new_message() {
    let backend = TrackingBackend::new();
    common::seed_group_chat(backend.inner(), "g1");
    check_create(&backend, || backend.inner().take_outbound_events_for_test()).await;
}

#[tokio::test]
async fn memory_create_message_stores_and_reports_one_new_message() {
    let backend = common::account_backend();
    common::seed_group_chat(&backend, "g1");
    check_create(&backend, || backend.take_outbound_events_for_test()).await;
}
