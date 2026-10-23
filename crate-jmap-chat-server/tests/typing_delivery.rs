//! Per-recipient `ChatTypingEvent` delivery, `should_deliver_typing`
//! (bd:JMAP-yuut.2, bd:JMAP-x2gd.16).
//!
//! Oracle: draft-atwood-jmap-chat-wss-00 §ChatTypingEvent (wss:250-254)
//! and draft-atwood-jmap-chat-00 `receiveTypingIndicators` (chat:562) and
//! §Chat/typing Server Behavior (chat:1086), at jmap-chat-spec commit
//! 91672c9. Each test quotes the sentence it enforces. Fixtures are
//! draft-shaped JSON seeded into the reference `MemoryBackend`; expected
//! results come only from the draft text.

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::{
    handle_chat_set, handle_space_get, handle_space_set, should_deliver_typing,
};
use jmap_chat_types::ChatTypingEvent;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const RECIPIENT: &str = "acct-owner";
const OTHER_ACCOUNT: &str = "acct-other";
const SENDER: &str = "contact-bob";
const CHAT: &str = "chat-1";

fn recipient() -> Id {
    Id::from(RECIPIENT)
}

/// A Chat `CHAT` of `kind`, with `fields` merged in.
fn chat(kind: &str, fields: Value) -> Value {
    let mut chat = json!({
        "id": CHAT,
        "kind": kind,
        "createdAt": "2026-10-01T00:00:00Z",
        "unreadCount": 0,
    });
    for (k, v) in fields.as_object().expect("fields object") {
        chat[k] = v.clone();
    }
    chat
}

/// A group Chat whose `members` are `member_ids`, with `fields` merged in.
fn group(member_ids: &[&str], fields: Value) -> Value {
    let members: Vec<Value> = member_ids
        .iter()
        .map(|id| json!({ "id": id, "role": "member", "joinedAt": "2026-10-01T00:00:00Z" }))
        .collect();
    let mut fields = fields;
    fields["name"] = json!("Team");
    fields["members"] = Value::Array(members);
    chat("group", fields)
}

/// The sender's ChatContact as held on `account`'s contact list.
fn sender_contact(blocked: bool) -> Value {
    json!({
        "id": SENDER,
        "identity": "bob@example.net",
        "login": "bob",
        "displayName": "Bob",
        "blocked": blocked,
        "firstSeenAt": "2026-10-01T00:00:00Z",
        "lastSeenAt": "2026-10-19T00:00:00Z",
    })
}

/// A backend holding `chat` (if any) in the recipient's account and the
/// sender's ChatContact on the recipient's contact list.
fn backend_with(chat: Option<Value>, sender_blocked: bool) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&recipient());
    if let Some(chat) = chat {
        backend.insert_object_for_test("Chat", RECIPIENT, CHAT, chat);
    }
    backend.insert_object_for_test(
        "ChatContact",
        RECIPIENT,
        SENDER,
        sender_contact(sender_blocked),
    );
    backend
}

/// The wss:241-249 example frame (spec 6bcd9d8), addressed to the
/// recipient's account, with `chatId` and `senderId` set.
fn event(chat_id: &str, sender_id: &str) -> ChatTypingEvent {
    serde_json::from_value(json!({
        "@type": "ChatTypingEvent",
        "accountId": RECIPIENT,
        "chatId": chat_id,
        "senderId": sender_id,
        "typing": true,
    }))
    .expect("valid ChatTypingEvent")
}

async fn decide(backend: &MemoryBackend, event: &ChatTypingEvent) -> bool {
    should_deliver_typing(backend, &(), &recipient(), event)
        .await
        .expect("should_deliver_typing")
}

/// draft-atwood-jmap-chat-00 §Chat/typing (chat:1086): "the server SHOULD
/// deliver a `typing` push event ... to all connected clients of each
/// local participant in the Chat other than the requesting account,
/// subject to the `receiveTypingIndicators` and blocked-sender checks
/// above." A member recipient with the default preference and an
/// unblocked sender receives it.
#[tokio::test]
async fn member_with_defaults_receives_typing() {
    let backend = backend_with(Some(group(&[RECIPIENT, SENDER], json!({}))), false);
    assert!(decide(&backend, &event(CHAT, SENDER)).await);
}

/// draft-atwood-jmap-chat-00 §Chat/typing (chat:1086): delivery goes to
/// "each local participant in the Chat other than the requesting
/// account". The sender's own account is not a recipient.
#[tokio::test]
async fn sender_is_not_a_recipient() {
    let backend = backend_with(Some(group(&[RECIPIENT, SENDER], json!({}))), false);
    assert!(!decide(&backend, &event(CHAT, RECIPIENT)).await);
}

/// draft-atwood-jmap-chat-wss-00 §ChatTypingEvent (wss:255, spec
/// 33702a5): "Servers SHOULD verify at delivery time that the owner
/// remains a current member of the identified Chat. If the owner is no
/// longer a member, the event SHOULD be dropped silently." A group whose `members` no longer lists the
/// owner's accountId drops it.
#[tokio::test]
async fn non_member_recipient_is_dropped() {
    let backend = backend_with(Some(group(&[SENDER], json!({}))), false);
    assert!(!decide(&backend, &event(CHAT, SENDER)).await);
}

/// draft-atwood-jmap-chat-wss-00 §ChatTypingEvent (wss:229-230, spec
/// 6bcd9d8): "`accountId` (String): The id of the account that contains
/// the Chat." An event for a Chat in another account is not about the
/// recipient's Chat of the same id, so it is dropped.
#[tokio::test]
async fn event_for_another_account_is_dropped() {
    let backend = backend_with(Some(group(&[RECIPIENT, SENDER], json!({}))), false);
    let mut e = event(CHAT, SENDER);
    e.account_id = Id::from(OTHER_ACCOUNT);
    assert!(!decide(&backend, &e).await);
}

/// draft-atwood-jmap-chat-wss-00 §ChatTypingEvent (wss:255, spec
/// 33702a5): "If the owner is no longer a member, the event SHOULD be
/// dropped silently." A
/// Chat the recipient's account does not hold has no membership to keep.
#[tokio::test]
async fn chat_missing_from_recipient_account_is_dropped() {
    let backend = backend_with(None, false);
    assert!(!decide(&backend, &event(CHAT, SENDER)).await);
}

/// draft-atwood-jmap-chat-wss-00 §ChatTypingEvent (wss:250): "For direct
/// and group Chats, servers MUST check each recipient's
/// `receiveTypingIndicators` field ... If `receiveTypingIndicators` is
/// `false`, the server MUST silently drop the `ChatTypingEvent`".
#[tokio::test]
async fn receive_typing_indicators_false_drops_group_and_direct() {
    let off = json!({ "receiveTypingIndicators": false });
    let group_backend = backend_with(Some(group(&[RECIPIENT, SENDER], off.clone())), false);
    assert!(!decide(&group_backend, &event(CHAT, SENDER)).await);

    let mut direct_fields = off;
    direct_fields["contactId"] = json!(SENDER);
    let direct_backend = backend_with(Some(chat("direct", direct_fields)), false);
    assert!(!decide(&direct_backend, &event(CHAT, SENDER)).await);
}

/// draft-atwood-jmap-chat-00 `receiveTypingIndicators` (chat:562): it
/// "Applies to direct and group chats only; for channel chats, servers
/// MUST store the value if set via `Chat/set` but MUST NOT use it to
/// suppress typing events." wss:250: "This check does not apply to
/// channel Chats." The channel and its Space come from the handlers, so
/// the owner is a Space member; the value is stored through `Chat/set`.
#[tokio::test]
async fn receive_typing_indicators_false_does_not_drop_channel() {
    let backend = backend_with(None, false);
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": RECIPIENT, "create": { "s": { "name": "S" } } }),
    )
    .await
    .expect("Space/set create");
    let space_id = resp["created"]["s"]["id"]
        .as_str()
        .expect("space id")
        .to_owned();
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": RECIPIENT,
            "update": { &space_id: { "addChannels": [{ "name": "general" }] } }
        }),
    )
    .await
    .expect("Space/set addChannels");
    assert!(resp["notUpdated"].is_null(), "addChannels failed: {resp}");
    let HandlerOutput { primary: resp, .. } = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": RECIPIENT, "ids": [&space_id] }),
    )
    .await
    .expect("Space/get");
    let channel_id = resp["list"][0]["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        &backend,
        &(),
        json!({
            "accountId": RECIPIENT,
            "update": { &channel_id: { "receiveTypingIndicators": false } }
        }),
    )
    .await
    .expect("Chat/set update");
    assert!(
        resp["updated"].get(&channel_id).is_some(),
        "update failed: {resp}"
    );

    assert!(decide(&backend, &event(&channel_id, SENDER)).await);
}

/// draft-atwood-jmap-chat-wss-00 §ChatTypingEvent (wss:254): "Servers
/// MUST NOT deliver a `ChatTypingEvent` whose `senderId` corresponds to a
/// ChatContact that is `blocked` on the recipient's ChatContact record."
#[tokio::test]
async fn blocked_sender_is_dropped() {
    let backend = backend_with(Some(group(&[RECIPIENT, SENDER], json!({}))), true);
    assert!(!decide(&backend, &event(CHAT, SENDER)).await);
}

/// draft-atwood-jmap-chat-wss-00 §ChatTypingEvent (wss:254): the block is
/// read from "the recipient's ChatContact record" (bd:JMAP-x2gd.16). The
/// same ChatContact blocked on another account's list does not suppress
/// delivery to this recipient, who has not blocked it.
#[tokio::test]
async fn block_on_another_accounts_list_does_not_drop() {
    let backend = backend_with(Some(group(&[RECIPIENT, SENDER], json!({}))), false);
    backend.register_account(&Id::from(OTHER_ACCOUNT));
    backend.insert_object_for_test("ChatContact", OTHER_ACCOUNT, SENDER, sender_contact(true));
    assert!(decide(&backend, &event(CHAT, SENDER)).await);
}
