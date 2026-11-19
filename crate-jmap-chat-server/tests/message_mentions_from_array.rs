//! jmap-chat-compliance-tests §4.5: `Message/query` `hasMention` takes
//! mentions only from the `mentions` array, never from body text
//! (bd:JMAP-ssda, audit JMAP-x3lu).
//!
//! Oracles, both at jmap-chat-spec 120db2c:
//! - jmap-chat-compliance-tests.md L225-229, §4.5, Severity MUST, quoted
//!   on the test.
//! - draft-atwood-jmap-chat-00.md L442, §Mention: "The `mentions` array
//!   is authoritative for every `bodyType`: servers MUST take mentions
//!   only from it, for mention-based push, notification, and query
//!   behavior, and MUST NOT derive mentions from `body`."
//!
//! The messages are sent with `Message/set` create in a group chat.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::handle_chat_set;
use jmap_chat_server::message::{handle_message_query, handle_message_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const OWNER_LOGIN: &str = "owner@home.example";

async fn create_group(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary, .. } = handle_chat_set(
        backend,
        &(),
        &BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
        }),
        &[],
    )
    .await
    .expect("Chat/set");
    primary["created"]["g"]["id"]
        .as_str()
        .expect("created group id")
        .to_owned()
}

async fn send(backend: &MemoryBackend, chat_id: &str, mentions: Value) -> String {
    let HandlerOutput { primary, .. } = handle_message_set(
        backend,
        &(),
        &BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": {
                "chatId": chat_id,
                "body": format!("ping @{OWNER_LOGIN}, see @owner"),
                "bodyType": "text/plain",
                "sentAt": "2024-01-01T00:00:00Z",
                "mentions": mentions,
            } }
        }),
        &[],
    )
    .await
    .expect("Message/set");
    primary["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("message created: {primary}"))
        .to_owned()
}

async fn has_mention_ids(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary, .. } = handle_message_query(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "hasMention": true } }),
        &[],
    )
    .await
    .expect("Message/query");
    primary["ids"].clone()
}

/// jmap-chat-compliance-tests.md L229: "Send a `text/plain` message whose
/// body contains `@` followed by the owner's login, with an empty
/// `mentions` array. Verify `Message/query` with `hasMention: true` does
/// not match it. Send it again with `mentions: [{"id": "<accountId>"}]`.
/// Verify it matches."
#[tokio::test]
async fn body_at_login_is_not_a_mention_but_mentions_array_is() {
    let backend = common::account_backend();
    backend.set_owner_user_id_for_test(&Id::from(ACCOUNT_ID), OWNER_LOGIN);
    let chat_id = create_group(&backend).await;

    send(&backend, &chat_id, json!([])).await;
    assert_eq!(has_mention_ids(&backend).await, json!([]));

    let mentioned = send(&backend, &chat_id, json!([{ "id": ACCOUNT_ID }])).await;
    assert_eq!(has_mention_ids(&backend).await, json!([mentioned]));
}
