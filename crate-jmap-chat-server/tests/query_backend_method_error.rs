//! `/query` handlers that call `query_objects` directly pass a backend
//! `BackendQueryError::Method` through unchanged (bd JMAP-yt1w.24).
//!
//! Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt,
//! Section 5.5 "/query"):
//!
//! > The following additional errors may be returned instead of the
//! > "Foo/query" response:
//! >
//! > "unsupportedFilter": The "filter" is syntactically valid, but the
//! > server cannot process it.
//!
//! The handlers' own `unsupportedFilter` refusals carry no description,
//! so the injected description on the wire proves the backend's error
//! passed through rather than being replaced or redacted to `serverFail`.

mod common;

use common::{ACCOUNT_ID, INJECTED_QUERY_METHOD_ERROR_DESCRIPTION, InjectableBackend};
use jmap_chat_server::message::handle_message_query;
use jmap_chat_server::{handle_contact_query, handle_emoji_query, handle_space_query};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

fn backend_with_query_fault(type_name: &'static str) -> InjectableBackend {
    let backend = InjectableBackend::new();
    backend.inner.register_account(&Id::from(ACCOUNT_ID));
    backend.inject(type_name, "query");
    backend
}

fn wire(err: JmapError) -> Value {
    serde_json::to_value(&err).expect("JmapError must serialize")
}

fn expected() -> Value {
    json!({
        "type": "unsupportedFilter",
        "description": INJECTED_QUERY_METHOD_ERROR_DESCRIPTION,
    })
}

/// draft-atwood-jmap-chat-00 §Message/query (jmap-chat-spec 29e9a41,
/// draft-atwood-jmap-chat-00.md line 1271): "When `chatId` is absent and
/// `hasMention: true` is present, the query spans all Chats of which the
/// owner is a member. Servers that do not support cross-chat mention
/// queries MUST return `unsupportedFilter`". The handler accepts this
/// filter; only the backend knows whether it supports the query.
#[tokio::test]
async fn message_query_passes_backend_unsupported_filter_through() {
    let backend = backend_with_query_fault("Message");
    let args = json!({ "accountId": ACCOUNT_ID, "filter": { "hasMention": true } });
    let err = handle_message_query(&backend, &(), args, &[])
        .await
        .expect_err("backend unsupportedFilter must fail Message/query");
    assert_eq!(wire(err), expected());
}

#[tokio::test]
async fn space_query_passes_backend_method_error_through() {
    let backend = backend_with_query_fault("Space");
    let err = handle_space_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }), &[])
        .await
        .expect_err("backend unsupportedFilter must fail Space/query");
    assert_eq!(wire(err), expected());
}

#[tokio::test]
async fn contact_query_passes_backend_method_error_through() {
    let backend = backend_with_query_fault("ChatContact");
    let err = handle_contact_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
        .await
        .expect_err("backend unsupportedFilter must fail ChatContact/query");
    assert_eq!(wire(err), expected());
}

#[tokio::test]
async fn emoji_query_passes_backend_method_error_through() {
    let backend = backend_with_query_fault("CustomEmoji");
    let err = handle_emoji_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
        .await
        .expect_err("backend unsupportedFilter must fail CustomEmoji/query");
    assert_eq!(wire(err), expected());
}
