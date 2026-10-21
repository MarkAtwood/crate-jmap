//! The reference `MemoryBackend` validates `chatPush` on push subscription
//! create and update and strips it on list (bd:JMAP-nbk3.5).
//!
//! Oracle: draft-atwood-jmap-chat-push-00 at jmap-chat-spec commit cefc217,
//! section "PushSubscription Extension" / "ChatPushConfig", plus the
//! approved JMAP-nbk3 plan decision D4 (bad urgency is `invalidProperties`).
//! Each test quotes the sentence it enforces.

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_types::ChatPushCapability;
use jmap_server::{BackendSetError, PushSubscriptionBackend, SetErrorType};
use jmap_types::{Id, PushSubscription};
use serde_json::{json, Value};

const ACCOUNT: &str = "acct-push";

/// A backend whose `ACCOUNT` advertises `urn:ietf:params:jmap:chat:push`
/// with the minimum urgency set the draft requires.
fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    let cap: ChatPushCapability = serde_json::from_value(json!({
        "maxSnippetBytes": 256,
        "supportedUrgencyValues": ["normal", "high"],
    }))
    .expect("valid ChatPushCapability");
    backend.set_chat_push_capability_for_test(&Id::from(ACCOUNT), Some(cap));
    backend
}

fn subscription(chat_push: Value) -> PushSubscription {
    serde_json::from_value(json!({
        "deviceClientId": "device-1",
        "url": "https://push.example.com/endpoint/1",
        "types": null,
        "chatPush": chat_push,
    }))
    .expect("valid PushSubscription")
}

fn expect_invalid_urgency<T: std::fmt::Debug>(
    result: Result<T, BackendSetError<impl std::fmt::Debug>>,
) {
    match result {
        Err(BackendSetError::SetError(se)) => {
            assert_eq!(se.error_type, SetErrorType::InvalidProperties);
            assert_eq!(
                se.properties,
                Some(vec![format!("chatPush/{ACCOUNT}/urgency")])
            );
        }
        other => panic!("expected invalidProperties, got {other:?}"),
    }
}

/// draft-atwood-jmap-chat-push-00 §"ChatPushConfig" (push:124): "If the
/// client supplies a value not in `supportedUrgencyValues`, the server MUST
/// return an `invalidProperties` SetError on `PushSubscription/set` whose
/// `properties` names the offending path (e.g., `chatPush/<id>/urgency`)
/// and MUST NOT store the configuration."
#[tokio::test]
async fn create_with_unsupported_urgency_stores_nothing() {
    let backend = backend();
    let result = backend
        .create_push_subscription(&(), subscription(json!({ ACCOUNT: { "urgency": "low" } })))
        .await;
    expect_invalid_urgency(result);
    let listed = backend
        .list_push_subscriptions(&())
        .await
        .expect("list succeeds");
    assert!(listed.is_empty(), "rejected subscription was stored");
}

/// draft-atwood-jmap-chat-push-00 §"ChatPushConfig" (push:124), as above,
/// on update: the previously stored configuration is kept.
#[tokio::test]
async fn update_with_unsupported_urgency_keeps_stored_config() {
    let backend = backend();
    let stored = backend
        .create_push_subscription(&(), subscription(json!({ ACCOUNT: { "urgency": "high" } })))
        .await
        .expect("valid create succeeds");
    let mut changed = stored.subscription.clone();
    changed.extra.insert(
        "chatPush".to_owned(),
        json!({ ACCOUNT: { "urgency": "low" } }),
    );
    expect_invalid_urgency(backend.update_push_subscription(&(), changed).await);
    let listed = backend
        .list_push_subscriptions(&())
        .await
        .expect("list succeeds");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].subscription.extra.get("chatPush"),
        Some(&json!({ ACCOUNT: { "urgency": "high" } }))
    );
}

/// draft-atwood-jmap-chat-push-00 §"PushSubscription Extension" (push:110):
/// "If a `PushSubscription/set` create or update includes a `chatPush`
/// entry for an account whose `accountCapabilities` does not include
/// `urn:ietf:params:jmap:chat:push`, the server MUST return an
/// `invalidProperties` SetError whose `properties` names the offending
/// entry (e.g., `chatPush/<id>`) and MUST NOT store the subscription
/// change."
/// The draft states no rule for an entry stored while the account
/// advertised the capability; this server stops returning it after the
/// capability is withdrawn, because `should_push` then treats it as absent.
#[tokio::test]
async fn list_strips_entry_after_capability_is_withdrawn() {
    let backend = backend();
    backend
        .create_push_subscription(&(), subscription(json!({ ACCOUNT: { "urgency": "high" } })))
        .await
        .expect("valid create succeeds");
    backend.set_chat_push_capability_for_test(&Id::from(ACCOUNT), None);
    let listed = backend
        .list_push_subscriptions(&())
        .await
        .expect("list succeeds");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].subscription.extra.get("chatPush"),
        Some(&json!({}))
    );
}
