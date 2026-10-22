//! RFC 8620 §5.3 `ifInState` on every chat-client `/set` method.
//!
//! Oracle: RFC 8620 §5.3, "ifInState: `String|null` This is a state
//! string as returned by the `Foo/get` method ... If supplied, the string
//! must match the current state; otherwise, the method will be aborted
//! and a `stateMismatch` error returned. If null, any changes will be
//! applied to the current state." Each call is checked on the recorded
//! request body only; the mock's empty response is irrelevant here.

#[path = "helpers.rs"]
mod helpers;

use jmap_chat_client::methods::{
    ChatContactPatch, ChatCreateInput, ChatPatch, CustomEmojiCreateInput, CustomEmojiPatch,
    MessageCreateInput, MessagePatch, PresenceStatusPatch, PushSubscriptionCreateInput,
    PushSubscriptionPatch, SpaceBanCreateInput, SpaceBanPatch, SpaceCreateInput,
    SpaceInviteCreateInput, SpacePatch,
};
use jmap_chat_client::BodyType;
use jmap_types::{Id, State, UTCDate};
use serde_json::{json, Value};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn serve() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "sessionState": "s1", "methodResponses": [] })),
        )
        .mount(&server)
        .await;
    server
}

/// `(method name, ifInState argument)` of each request sent.
async fn sent(server: &MockServer) -> Vec<(String, Option<Value>)> {
    let reqs = server.received_requests().await.expect("recorded requests");
    reqs.iter()
        .map(|r| {
            let body: Value = serde_json::from_slice(&r.body).expect("JSON body");
            let call = &body["methodCalls"][0];
            let name = call[0].as_str().expect("method name").to_owned();
            (name, call[1].get("ifInState").cloned())
        })
        .collect()
}

async fn send_all(server: &MockServer, s: Option<&State>) {
    let sc = helpers::make_client(server);
    let id = Id::from("x");
    let ids = [Id::from("x")];
    let at = UTCDate::from("2026-01-01T00:00:00Z");

    let _ = sc
        .chat_create(
            &ChatCreateInput::Direct {
                client_id: Some("c1"),
                contact_id: &id,
            },
            s,
        )
        .await;
    let chat_patch = ChatPatch {
        muted: Some(true),
        ..Default::default()
    };
    let _ = sc.chat_update(&id, &chat_patch, s).await;
    let _ = sc.chat_destroy(&ids, s).await;

    let message = MessageCreateInput {
        client_id: Some("m1"),
        chat_id: &id,
        body: "hi",
        body_type: BodyType::Plain,
        sent_at: &at,
        reply_to: None,
        attachments: None,
        mentions: None,
        broadcast_mentions: None,
        actions: None,
        thread_root_id: None,
        topic_id: None,
        subject: None,
        sender_expires_at: None,
        burn_on_read: None,
        expires_after_read_seconds: None,
    };
    let _ = sc.message_create(&message, s).await;
    let message_patch = MessagePatch {
        body: Some("edited"),
        ..Default::default()
    };
    let _ = sc.message_update(&id, &message_patch, s).await;
    let _ = sc.message_destroy(&ids, s).await;

    let _ = sc
        .space_create(
            &SpaceCreateInput {
                client_id: Some("s1"),
                name: "Space",
                description: None,
                icon_blob_id: None,
            },
            s,
        )
        .await;
    let space_patch = SpacePatch {
        name: Some("Renamed"),
        ..Default::default()
    };
    let _ = sc.space_update(&id, &space_patch, s).await;
    let _ = sc.space_destroy(&ids, s).await;

    let _ = sc
        .space_ban_create(
            &SpaceBanCreateInput {
                client_id: Some("b1"),
                space_id: &id,
                user_id: &id,
                reason: None,
                expires_at: None,
            },
            s,
        )
        .await;
    let ban_patch = SpaceBanPatch {
        reason: "spam".into(),
        ..Default::default()
    };
    let _ = sc.space_ban_update(&id, &ban_patch, s).await;
    let _ = sc.space_ban_destroy(&ids, s).await;

    let _ = sc
        .space_invite_create(
            &SpaceInviteCreateInput {
                client_id: Some("i1"),
                space_id: &id,
                default_channel_id: None,
                expires_at: None,
                max_uses: None,
            },
            s,
        )
        .await;
    let _ = sc.space_invite_destroy(&ids, s).await;

    let _ = sc.chat_contact_create("did:example:bob", s).await;
    let contact_patch = ChatContactPatch {
        blocked: Some(true),
        ..Default::default()
    };
    let _ = sc.chat_contact_update(&id, &contact_patch, s).await;

    let _ = sc
        .custom_emoji_create(
            &CustomEmojiCreateInput {
                client_id: Some("e1"),
                name: "party",
                blob_id: &id,
                space_id: None,
            },
            s,
        )
        .await;
    let emoji_patch = CustomEmojiPatch {
        name: Some("parrot"),
        ..Default::default()
    };
    let _ = sc.custom_emoji_update(&id, &emoji_patch, s).await;
    let _ = sc.custom_emoji_destroy(&ids, s).await;

    let _ = sc.read_position_update(&id, &id, s).await;
    let presence_patch = PresenceStatusPatch {
        status_text: "away".into(),
        ..Default::default()
    };
    let _ = sc.presence_status_update(&id, &presence_patch, s).await;

    let _ = sc
        .push_subscription_create(
            &PushSubscriptionCreateInput {
                client_id: Some("p1"),
                device_client_id: "device-abc",
                url: "https://push.example.com/endpoint",
                expires: None,
                types: None,
                chat_push: None,
            },
            s,
        )
        .await;
    let push_patch = PushSubscriptionPatch {
        verification_code: Some("code"),
        ..Default::default()
    };
    let _ = sc.push_subscription_update(&id, &push_patch, s).await;
    let _ = sc.push_subscription_destroy(&ids, s).await;
}

const METHODS: [&str; 24] = [
    "Chat/set",
    "Chat/set",
    "Chat/set",
    "Message/set",
    "Message/set",
    "Message/set",
    "Space/set",
    "Space/set",
    "Space/set",
    "SpaceBan/set",
    "SpaceBan/set",
    "SpaceBan/set",
    "SpaceInvite/set",
    "SpaceInvite/set",
    "ChatContact/set",
    "ChatContact/set",
    "CustomEmoji/set",
    "CustomEmoji/set",
    "CustomEmoji/set",
    "ReadPosition/set",
    "PresenceStatus/set",
    "PushSubscription/set",
    "PushSubscription/set",
    "PushSubscription/set",
];

#[tokio::test]
async fn some_if_in_state_is_sent() {
    let server = serve().await;
    send_all(&server, Some(&State::from("s-guard"))).await;
    let want: Vec<_> = METHODS
        .iter()
        .map(|m| (m.to_string(), Some(json!("s-guard"))))
        .collect();
    assert_eq!(sent(&server).await, want);
}

#[tokio::test]
async fn none_if_in_state_is_omitted() {
    let server = serve().await;
    send_all(&server, None).await;
    let want: Vec<_> = METHODS.iter().map(|m| (m.to_string(), None)).collect();
    assert_eq!(sent(&server).await, want);
}
