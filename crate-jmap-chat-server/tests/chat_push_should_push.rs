//! `ChatMessagePush` delivery eligibility, `should_push` (bd:JMAP-nbk3.7).
//!
//! Oracle: draft-atwood-jmap-chat-push-00 at jmap-chat-spec commit cefc217,
//! sections "PushSubscription Extension", "ChatPushConfig", "End-to-End
//! Encrypted Deployments" and "Delivery", plus draft-atwood-jmap-chat-00
//! §Terminology (the owner's member entry and `senderId` are the
//! accountId). Each test quotes the sentence it enforces. Fixtures are
//! draft-shaped JSON seeded into the reference `MemoryBackend`; expected
//! results come only from the draft text.
//!
//! The "stored before the `PushSubscription` was created" case (push:213)
//! has no test: per plan decision D6 the consumer evaluates only the
//! subscription set snapshotted with the message write, and `should_push`
//! documents that rule.

use std::collections::HashMap;

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::{OwnerFlags, should_push};
use jmap_chat_types::{ChatPushCapability, ChatPushConfig, Message};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acct-owner";
const SENDER: &str = "contact-bob";
const CHAT: &str = "chat-1";
const OTHER_CHAT: &str = "chat-2";
const ENCRYPTED_BODY_TYPE: &str = "message/mls";
const FUTURE: &str = "2099-01-01T00:00:00Z";
const PAST: &str = "2000-01-01T00:00:00Z";

const NOBODY: OwnerFlags = OwnerFlags::new(false, false);

fn account() -> Id {
    Id::from(ACCOUNT)
}

/// `ACCOUNT` advertises `urn:ietf:params:jmap:chat:push`.
fn caps() -> HashMap<Id, ChatPushCapability> {
    let cap: ChatPushCapability = serde_json::from_value(json!({
        "maxSnippetBytes": 256,
        "supportedUrgencyValues": ["normal", "high"],
    }))
    .expect("valid ChatPushCapability");
    HashMap::from([(account(), cap)])
}

fn config(v: Value) -> ChatPushConfig {
    serde_json::from_value(v).expect("valid ChatPushConfig")
}

/// A config that matches every Chat: `kinds` and `chatIds` absent.
fn all_chats() -> ChatPushConfig {
    config(json!({}))
}

/// The group Chat `CHAT`, with `members` and the owner-side mute fields
/// merged in from `fields`.
fn group_chat(members: Value, fields: Value) -> Value {
    let mut chat = json!({
        "id": CHAT,
        "kind": "group",
        "createdAt": "2026-10-01T00:00:00Z",
        "unreadCount": 0,
        "name": "Team",
        "members": members,
    });
    for (k, v) in fields.as_object().expect("fields object") {
        chat[k] = v.clone();
    }
    chat
}

/// The owner (member entry = accountId) and the sender.
fn owner_and_sender() -> Value {
    json!([
        { "id": ACCOUNT, "role": "admin", "joinedAt": "2026-10-01T00:00:00Z" },
        { "id": SENDER, "role": "member", "joinedAt": "2026-10-01T00:00:00Z" },
    ])
}

/// A backend holding `chat` (if any) and the sender's ChatContact on the
/// owner's contact list.
fn backend_with(chat: Option<Value>, sender_blocked: bool) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&account());
    if let Some(chat) = chat {
        backend.insert_object_for_test("Chat", ACCOUNT, CHAT, chat);
    }
    backend.insert_object_for_test(
        "ChatContact",
        ACCOUNT,
        SENDER,
        json!({
            "id": SENDER,
            "identity": "bob@example.net",
            "login": "bob",
            "displayName": "Bob",
            "blocked": sender_blocked,
            "firstSeenAt": "2026-10-01T00:00:00Z",
            "lastSeenAt": "2026-10-19T00:00:00Z",
        }),
    );
    backend
}

/// An unmuted group Chat with the owner and sender as members.
fn plain_backend() -> MemoryBackend {
    backend_with(Some(group_chat(owner_and_sender(), json!({}))), false)
}

/// The group Chat muted with `fields`, the owner and sender as members.
fn muted_backend(fields: Value) -> MemoryBackend {
    backend_with(Some(group_chat(owner_and_sender(), fields)), false)
}

fn message(sender: &str, body_type: &str, mentions: Value, broadcast: Value) -> Message {
    serde_json::from_value(json!({
        "id": "msg-1",
        "senderMsgId": "smsg-1",
        "senderId": sender,
        "chatId": CHAT,
        "body": "hello",
        "bodyType": body_type,
        "attachments": [],
        "mentions": mentions,
        "broadcastMentions": broadcast,
        "sentAt": "2026-10-19T12:00:00Z",
        "receivedAt": "2026-10-19T12:00:01Z",
        "deliveryState": "received",
    }))
    .expect("valid Message")
}

/// A plain inbound message from `SENDER` with no mentions.
fn inbound() -> Message {
    message(SENDER, "text/plain", json!([]), json!([]))
}

async fn decide(
    backend: &MemoryBackend,
    caps: &HashMap<Id, ChatPushCapability>,
    cfg: &ChatPushConfig,
    msg: &Message,
    owner: OwnerFlags,
) -> bool {
    should_push(backend, &(), &account(), cfg, caps, msg, owner)
        .await
        .expect("should_push")
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:209): "When a new
/// inbound message is stored that matches a `ChatPushConfig`, the server
/// MUST construct a `ChatMessagePush` payload and deliver it to the
/// registered push endpoint for that `PushSubscription`." A message from
/// another participant, in an unmuted Chat the owner belongs to, from an
/// unblocked sender, is pushed.
#[tokio::test]
async fn inbound_message_in_matching_chat_is_pushed() {
    let backend = plain_backend();
    assert!(decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §ChatPushConfig `chatIds` (push:118):
/// "If both `kinds` and `chatIds` are present, a Chat matches only if its
/// id appears in `chatIds` and its kind appears in `kinds`. A Chat whose id
/// is in `chatIds` but whose kind is not in `kinds` receives no push".
#[tokio::test]
async fn chat_in_chat_ids_but_kind_not_in_kinds_is_not_pushed() {
    let backend = plain_backend();
    let cfg = config(json!({ "kinds": ["direct"], "chatIds": [CHAT] }));
    assert!(!decide(&backend, &caps(), &cfg, &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §ChatPushConfig `chatIds` (push:118):
/// "a Chat matches only if its id appears in `chatIds` and its kind
/// appears in `kinds`." Both match here, so the message is pushed; with
/// `chatIds` naming only another Chat it is not.
#[tokio::test]
async fn chat_ids_and_kinds_both_required_to_match() {
    let backend = plain_backend();
    let both = config(json!({ "kinds": ["group"], "chatIds": [CHAT] }));
    assert!(decide(&backend, &caps(), &both, &inbound(), NOBODY).await);
    let other = config(json!({ "kinds": ["group"], "chatIds": [OTHER_CHAT] }));
    assert!(!decide(&backend, &caps(), &other, &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §ChatPushConfig `kinds` (push:115):
/// "The Chat kinds for which push is enabled." A group Chat under
/// `kinds: ["channel"]` is not pushed.
#[tokio::test]
async fn kind_not_in_kinds_is_not_pushed() {
    let backend = plain_backend();
    let cfg = config(json!({ "kinds": ["channel"] }));
    assert!(!decide(&backend, &caps(), &cfg, &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:214): "The server MUST
/// NOT deliver a `ChatMessagePush` in any of the following cases: ... The
/// sender is the account owner (push is for inbound messages from other
/// participants only)." Per §ChatMessageEntry `senderId` (push:170), an
/// owner-composed message carries the accountId as its sender.
#[tokio::test]
async fn message_sent_by_owner_is_not_pushed() {
    let backend = plain_backend();
    let own = message(ACCOUNT, "text/plain", json!([]), json!([]));
    assert!(!decide(&backend, &caps(), &all_chats(), &own, NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:215): "At the time of
/// delivery, the account is no longer a member of the Chat containing the
/// message." The group's `members` (draft-atwood-jmap-chat-00 §Chat: "Full
/// membership list") no longer has the owner's entry, the accountId.
#[tokio::test]
async fn owner_no_longer_group_member_is_not_pushed() {
    let members = json!([
        { "id": SENDER, "role": "admin", "joinedAt": "2026-10-01T00:00:00Z" },
    ]);
    let backend = backend_with(Some(group_chat(members, json!({}))), false);
    assert!(!decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:215): "At the time of
/// delivery, the account is no longer a member of the Chat containing the
/// message." The account no longer holds the Chat at all.
#[tokio::test]
async fn chat_gone_from_account_is_not_pushed() {
    let backend = backend_with(None, false);
    assert!(!decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:216): "The Chat is muted
/// (`Chat.muted: true` or within an active `muteUntil` period)".
#[tokio::test]
async fn muted_chat_is_not_pushed() {
    let backend = muted_backend(json!({ "muted": true }));
    assert!(!decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:216): "The Chat is muted
/// (`Chat.muted: true` or within an active `muteUntil` period)". A
/// `muteUntil` in the future is an active period.
#[tokio::test]
async fn active_mute_until_is_not_pushed() {
    let backend = muted_backend(json!({ "muted": false, "muteUntil": FUTURE }));
    assert!(!decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:216): "within an active
/// `muteUntil` period". A `muteUntil` in the past is not active
/// (draft-atwood-jmap-chat-00 §Chat `muteUntil`: "Muting expires at this
/// time."), so the message is pushed.
#[tokio::test]
async fn expired_mute_until_is_pushed() {
    let backend = muted_backend(json!({ "muted": false, "muteUntil": PAST }));
    assert!(decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-00 §Chat `muteUntil`: "Muting expires at this
/// time. Servers SHOULD clear `muted` and `muteUntil` automatically when
/// the time passes." A timed mute is `muted: true` plus `muteUntil`; once
/// `muteUntil` has passed the mute is over even if the server has not yet
/// cleared `muted`, so the message is pushed.
#[tokio::test]
async fn expired_timed_mute_with_muted_true_is_pushed() {
    let backend = muted_backend(json!({ "muted": true, "muteUntil": PAST }));
    assert!(decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:216): "The Chat is muted
/// ..., UNLESS the message contains a broadcast mention whose resolved
/// recipient set includes the account owner". `"everyone"` targets every
/// Chat member (§ChatMessageEntry `mentionScopes`, push:182).
#[tokio::test]
async fn broadcast_mention_targeting_owner_bypasses_mute() {
    let backend = muted_backend(json!({ "muted": true, "muteUntil": FUTURE }));
    let msg = message(
        SENDER,
        "text/plain",
        json!([]),
        json!([{ "scope": "everyone" }]),
    );
    assert!(decide(&backend, &caps(), &all_chats(), &msg, NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:216): "UNLESS the
/// message contains a broadcast mention whose resolved recipient set
/// includes the account owner". §ChatMessageEntry (push:182): "`"here"` is
/// included only when the owner satisfies the deployment-defined "active"
/// predicate at delivery time". An inactive owner is not targeted by
/// `"here"`, so the mute holds.
#[tokio::test]
async fn broadcast_mention_not_targeting_owner_does_not_bypass_mute() {
    let backend = muted_backend(json!({ "muted": true }));
    let msg = message(
        SENDER,
        "text/plain",
        json!([]),
        json!([{ "scope": "here" }]),
    );
    assert!(!decide(&backend, &caps(), &all_chats(), &msg, NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:219): "For clarity: a
/// direct `@user` mention (`hasMention: true` with empty `mentionScopes`)
/// in a muted Chat does NOT bypass the mute suppression."
#[tokio::test]
async fn direct_mention_does_not_bypass_mute() {
    let backend = muted_backend(json!({ "muted": true }));
    let msg = message(SENDER, "text/plain", json!([{ "id": ACCOUNT }]), json!([]));
    assert!(!decide(&backend, &caps(), &all_chats(), &msg, NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §End-to-End Encrypted Deployments
/// (push:199): "The server MUST omit both fields from the entry when
/// `encrypted` is `true`. Such a message never triggers `mentionUrgency`
/// and never bypasses mute suppression".
#[tokio::test]
async fn encrypted_message_never_bypasses_mute() {
    let backend = muted_backend(json!({ "muted": true }));
    let msg = message(
        SENDER,
        ENCRYPTED_BODY_TYPE,
        json!([]),
        json!([{ "scope": "everyone" }]),
    );
    assert!(!decide(&backend, &caps(), &all_chats(), &msg, NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §Delivery (push:217): "The message
/// sender's ChatContact record has `blocked: true` on the account owner's
/// contact list."
#[tokio::test]
async fn blocked_sender_is_not_pushed() {
    let backend = backend_with(Some(group_chat(owner_and_sender(), json!({}))), true);
    assert!(!decide(&backend, &caps(), &all_chats(), &inbound(), NOBODY).await);
}

/// draft-atwood-jmap-chat-push-00 §PushSubscription Extension (push:108,
/// push:110): "When `chatPush` is absent or null for an account, the server
/// delivers standard `StateChange` notifications for that account without
/// inline message content." and "If a `PushSubscription/set` create or update includes a `chatPush`
/// entry for an account whose `accountCapabilities` does not include
/// `urn:ietf:params:jmap:chat:push`, the server MUST return an
/// `invalidProperties` SetError whose `properties` names the offending
/// entry (e.g., `chatPush/<id>`) and MUST NOT store the subscription
/// change."
/// A config for an account that does not advertise the capability counts
/// as absent: no `ChatMessagePush`.
#[tokio::test]
async fn config_for_account_without_capability_is_not_pushed() {
    let backend = plain_backend();
    let no_caps = HashMap::new();
    assert!(!decide(&backend, &no_caps, &all_chats(), &inbound(), NOBODY).await);
}
