//! Error types the draft names for Chat/set, Message/set and
//! Message/query rejections (bd:JMAP-yt1w.1).

mod common;

use common::{make_chat_group, make_space, MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_set, handle_message_query, handle_message_set};
use serde_json::json;

/// Oracle: draft-atwood-jmap-chat-00 §Chat/set — "Creating a Chat with
/// `kind: "channel"` via `Chat/set` is not supported; the server MUST
/// return a SetError of type `forbidden`."
#[tokio::test]
async fn chat_set_create_channel_is_forbidden() {
    let backend = MemoryBackend::new();
    let space_id = make_space(&backend, "S").await;
    let (resp, _) = handle_chat_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "with": { "kind": "channel", "name": "general", "spaceId": space_id },
                "without": { "kind": "channel", "name": "general" }
            }
        }),
    )
    .await
    .expect("handle_chat_set");

    assert_eq!(resp["created"], json!(null), "{resp}");
    for key in ["with", "without"] {
        assert_eq!(
            resp["notCreated"][key]["type"], "forbidden",
            "{key}: {resp}"
        );
    }
}

/// Oracle: draft-atwood-jmap-chat-00 §Account-Level Capability Object,
/// `maxBodyBytes` — "Servers MUST reject messages exceeding this limit
/// with `invalidArguments`." The reference server's limit is 100,000
/// bytes.
#[tokio::test]
async fn message_set_create_over_long_body_is_invalid_arguments() {
    let backend = MemoryBackend::new();
    let chat_id = make_chat_group(&backend, "G").await;
    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m0": {
                "chatId": chat_id,
                "body": "a".repeat(100_001),
                "sentAt": "2024-01-01T00:00:00Z"
            } }
        }),
    )
    .await
    .expect("handle_message_set");

    assert_eq!(resp["created"], json!(null), "{resp}");
    assert_eq!(
        resp["notCreated"]["m0"]["type"], "invalidArguments",
        "{resp}"
    );
}

/// Oracle: draft-atwood-jmap-chat-00 §Message/query — `text`: "Servers
/// that do not support full-text search MUST return
/// `unsupportedFilter`."; `threadRootId`: "Valid only when
/// `supportsThreads` is `true`; otherwise servers MUST return
/// `unsupportedFilter`." The reference server supports neither.
#[tokio::test]
async fn message_query_text_and_thread_root_filters_are_unsupported() {
    let backend = MemoryBackend::new();
    let chat_id = make_chat_group(&backend, "G").await;
    for (key, value) in [("text", "hello"), ("threadRootId", "m1")] {
        let err = handle_message_query(
            &backend,
            &(),
            json!({
                "accountId": ACCOUNT_ID,
                "filter": { "chatId": chat_id, key: value }
            }),
        )
        .await
        .expect_err(key);
        assert_eq!(err.error_type.as_str(), "unsupportedFilter", "{key}");
    }
}
