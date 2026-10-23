//! `ChatBackend::on_outbound_event` for new Messages, retractions, edits,
//! reactions and group Chat changes (bd:JMAP-5mz6.5, bd:JMAP-5mz6.7).
//!
//! Oracles, quoted from the jmap-chat-spec drafts:
//!
//! - draft-atwood-jmap-chat-federation-00.md L585: "When a `Message/set`
//!   update sets `deletedForAll: true` on a message whose `senderId` is
//!   the owner's `accountId` ({{JMAP-CHAT}}), the server MUST send
//!   `Peer/retract` to the server of every other participant in the Chat
//!   before marking the local record." L574-581: the arguments are
//!   `chatId` and "`senderMsgId` (String): The sender-assigned ULID of the
//!   message to retract."
//! - draft-atwood-jmap-chat-00.md L1175: "Otherwise, deletion is local
//!   only: `body` and `attachments` are cleared on this mailbox with no
//!   peer notification."
//! - draft-atwood-jmap-chat-00.md L1143 (§Editing a Message): "Send
//!   `Peer/deliver` carrying an `edit` payload to all recipients".
//! - draft-atwood-jmap-chat-00.md L1020: the server "MUST send
//!   `Peer/groupUpdate` to each initial member".
//! - L1035 (`addMembers`): "On success, the server MUST send
//!   `Peer/groupUpdate` to all current members."
//! - L1038 (`removeMembers`): "The server MUST send `Peer/groupUpdate` to
//!   all remaining members and to the removed members."
//! - L1040: the owner leaving MUST "notify the other members with a
//!   `Peer/groupUpdate` of action `"leave"`"; when the owner is the last
//!   admin and others remain, "MUST notify the other members with a
//!   `Peer/groupUpdate` of action `"close"` in place of `"leave"`".
//! - L1043 (`updateMemberRoles`): "The server MUST send
//!   `Peer/groupUpdate` to all members."
//! - draft-atwood-jmap-chat-federation-00.md L625: `"updateMetadata"` —
//!   "The group's `name`, `description`, or `avatarBlobId` was changed."
//! - draft-atwood-jmap-chat-00.md L1130 (Message/set `create`): "The
//!   server sets `id`, `senderMsgId`, `senderId`, `receivedAt`,
//!   `deliveryState`, and delivery timestamp fields, then enqueues the
//!   message for outbound delivery."
//! - L1158 (add): the server "MUST propagate the reaction to all recipients
//!   via `Peer/deliver` `reactionUpdate` payload, carrying the
//!   `senderReactionId` as the map key."
//! - L1166 (remove): "The server MUST remove the entry and MUST propagate
//!   the removal via `Peer/deliver` `reactionUpdate` payload using the same
//!   `senderReactionId`."
//! - L1168: "Servers MUST reject attempts to add or remove reactions for
//!   other senders with `forbidden`."
//! - draft-atwood-jmap-chat-federation-00.md L374: `senderReactionId` "For
//!   `"remove"`, identifies which reaction to delete." L377: "`action`
//!   (String) — `"add"` or `"remove"`."
//!
//! - draft-atwood-jmap-chat-federation-00.md L462: Peer/receipt `readAt`
//!   "SHOULD be present when the recipient's owner has read the message."
//!   L472 (Sender Behavior): "if `Chat.receiptSharing` is present for the
//!   relevant Chat, that value is used; otherwise the account-level
//!   `PresenceStatus.receiptSharing` applies ... If the effective
//!   preference is `false`, the server MUST NOT invoke `Peer/receipt`".
//! - draft-atwood-jmap-chat-00.md L945: PresenceStatus `receiptSharing`
//!   "Default: `true`." L680: when an update sets `readAt` and "omits
//!   `readDisposition`, the server MUST store `"displayed"`." L660
//!   (`burnOnRead`): "When the recipient's effective `receiptSharing`
//!   preference is `false`, ... the sending server receives no
//!   `Peer/receipt` confirmation", so with sharing on, a burned read
//!   still sends one.
//! - L1086 (§Chat/typing Server Behavior): the server "SHOULD deliver a
//!   `typing` push event ... to all connected clients of each local
//!   participant in the Chat other than the requesting account" and "For
//!   federated Chats, the server SHOULD invoke `Peer/typing`". L1074: an
//!   unknown `chatId` is an `invalidArguments` method error.
//! - draft-atwood-jmap-chat-federation-00.md L706: the server "MAY push
//!   subsequent `Peer/presence` calls to the subscriber whenever the
//!   owner's presence, `statusText`, or `statusEmoji` changes."
//!
//! The owner is referenced by the accountId (draft-atwood-jmap-chat-00
//! §Terminology), so it is never a recipient.

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{
    GroupUpdateAction, OutboundEvent, ReactionAction, handle_chat_set, handle_chat_typing,
    handle_presence_set,
};
use jmap_chat_types::ReadDisposition;
use jmap_server::HandlerOutput;
use jmap_types::{Id, UTCDate};
use serde_json::{Value, json};

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

/// Message `id` in Chat `chat`, sent by `sender`, with a `senderMsgId`
/// distinct from `id` so the event is seen to carry the right one.
fn seed_message(backend: &MemoryBackend, id: &str, sender: &str) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "senderMsgId": format!("smid-{id}"),
            "senderId": sender,
            "chatId": "dm",
            "body": "hello",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
        }),
    );
}

/// Message `m1` in Chat `dm`, sent by `bob`, holding `reactions`.
fn seed_reacted_message(backend: &MemoryBackend, reactions: Value) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smid-m1",
            "senderId": "bob",
            "chatId": "dm",
            "body": "hello",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
            "reactions": reactions,
        }),
    );
}

/// The `ReactionUpdate` the hand-written expectations name: `m1` in `dm`,
/// `senderMsgId` `smid-m1`.
fn reaction_update(sender_reaction_id: &str, action: ReactionAction) -> OutboundEvent {
    OutboundEvent::ReactionUpdate {
        chat_id: Id::from("dm"),
        message_id: Id::from("m1"),
        sender_msg_id: Id::from("smid-m1"),
        sender_reaction_id: sender_reaction_id.to_owned(),
        action,
    }
}

/// Group `g1`: the owner and `members` (id, role).
fn seed_group(backend: &MemoryBackend, owner_role: &str, members: &[(&str, &str)]) {
    let mut list =
        vec![json!({ "id": ACCOUNT_ID, "role": owner_role, "joinedAt": "2026-01-01T00:00:00Z" })];
    list.extend(
        members.iter().map(
            |(id, role)| json!({ "id": id, "role": role, "joinedAt": "2026-01-01T00:00:00Z" }),
        ),
    );
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "g1",
        json!({
            "id": "g1",
            "kind": "group",
            "name": "Team",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": list,
        }),
    );
}

async fn message_update(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_message_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
    )
    .await
    .expect("Message/set");
    primary
}

async fn chat_set(backend: &MemoryBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_chat_set(backend, &(), &std::collections::BTreeMap::new(), args)
            .await
            .expect("Chat/set");
    primary
}

/// A GroupUpdate event as (chatId, action name, sorted payload ids,
/// sorted recipients), comparable to hand-written expectations.
fn group_update(event: &OutboundEvent) -> (String, &'static str, Vec<String>, Vec<String>) {
    let OutboundEvent::GroupUpdate {
        chat_id,
        action,
        recipients,
    } = event
    else {
        panic!("expected GroupUpdate, got {event:?}");
    };
    let (name, payload): (&str, Vec<String>) = match action {
        GroupUpdateAction::Create => ("create", vec![]),
        GroupUpdateAction::AddMembers { added } => (
            "addMembers",
            added
                .iter()
                .map(|m| format!("{}:{}", m.id, m.role))
                .collect(),
        ),
        GroupUpdateAction::RemoveMembers { removed_ids } => (
            "removeMembers",
            removed_ids.iter().map(|id| id.to_string()).collect(),
        ),
        GroupUpdateAction::UpdateRoles { updated } => (
            "updateRoles",
            updated
                .iter()
                .map(|m| format!("{}:{}", m.id, m.role))
                .collect(),
        ),
        GroupUpdateAction::UpdateMetadata => ("updateMetadata", vec![]),
        GroupUpdateAction::Leave => ("leave", vec![]),
        GroupUpdateAction::Close => ("close", vec![]),
        other => panic!("unexpected action {other:?}"),
    };
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    let recipients = recipients.iter().map(|id| id.to_string()).collect();
    (
        chat_id.to_string(),
        name,
        sorted(payload),
        sorted(recipients),
    )
}

fn expect(
    chat_id: &str,
    action: &'static str,
    payload: &[&str],
    recipients: &[&str],
) -> (String, &'static str, Vec<String>, Vec<String>) {
    let owned = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect();
    (
        chat_id.to_owned(),
        action,
        owned(payload),
        owned(recipients),
    )
}

/// fed L585, L574-581: one Peer/retract carrying chatId and senderMsgId.
#[tokio::test]
async fn delete_for_all_reports_one_retract() {
    let backend = backend();
    seed_message(&backend, "m1", ACCOUNT_ID);
    let resp = message_update(
        &backend,
        "m1",
        json!({ "deletedAt": "2026-01-02T00:00:00Z", "deletedForAll": true }),
    )
    .await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![OutboundEvent::Retract {
            chat_id: Id::from("dm"),
            message_id: Id::from("m1"),
            sender_msg_id: Id::from("smid-m1"),
        }]
    );
}

/// chat-00 L1175: a local delete has "no peer notification".
#[tokio::test]
async fn local_delete_reports_nothing() {
    let backend = backend();
    seed_message(&backend, "m1", ACCOUNT_ID);
    let resp = message_update(
        &backend,
        "m1",
        json!({ "deletedAt": "2026-01-02T00:00:00Z" }),
    )
    .await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1174: `deletedForAll` on another sender's message is
/// `forbidden`, so there is nothing to retract.
#[tokio::test]
async fn rejected_retract_reports_nothing() {
    let backend = backend();
    seed_message(&backend, "m1", "c-peer");
    let resp = message_update(&backend, "m1", json!({ "deletedForAll": true })).await;
    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden", "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1143: an edit sends one Peer/deliver `edit`, identified by
/// senderMsgId (fed L361).
#[tokio::test]
async fn edit_reports_one_edit() {
    let backend = backend();
    seed_message(&backend, "m1", ACCOUNT_ID);
    let resp = message_update(&backend, "m1", json!({ "body": "hello, edited" })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![OutboundEvent::Edit {
            chat_id: Id::from("dm"),
            message_id: Id::from("m1"),
            sender_msg_id: Id::from("smid-m1"),
        }]
    );
}

/// chat-00 L1136: an edit is "on a message where `senderId` is the
/// accountId and `deletedAt` is absent. The server MUST reject an edit of
/// any other message with a `forbidden` SetError." A rejected edit sends
/// no Peer/deliver `edit`.
#[tokio::test]
async fn edit_of_peer_message_reports_nothing() {
    let backend = backend();
    seed_message(&backend, "m1", "c-peer");
    let resp = message_update(&backend, "m1", json!({ "body": "hello, edited" })).await;
    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden", "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1136: the owner's own Message with `deletedAt` set is not
/// editable ("`deletedAt` is absent"), so the edit is `forbidden` and no
/// Peer/deliver `edit` is sent.
#[tokio::test]
async fn edit_of_deleted_message_reports_nothing() {
    let backend = backend();
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smid-m1",
            "senderId": ACCOUNT_ID,
            "chatId": "dm",
            "body": "",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
            "deletedAt": "2026-01-02T00:00:00Z",
        }),
    );
    let resp = message_update(&backend, "m1", json!({ "body": "back again" })).await;
    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden", "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1020: "MUST send `Peer/groupUpdate` to each initial member".
#[tokio::test]
async fn group_create_reports_create_to_initial_members() {
    let backend = backend();
    let resp = chat_set(
        &backend,
        json!({ "create": { "g": { "kind": "group", "name": "Team", "memberIds": ["bob", "carol"] } } }),
    )
    .await;
    let chat_id = resp["created"]["g"]["id"].as_str().expect("created");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        group_update(&events[0]),
        expect(chat_id, "create", &[], &["bob", "carol"])
    );
}

/// A direct Chat is not a group: no Peer/groupUpdate.
#[tokio::test]
async fn direct_create_reports_nothing() {
    let backend = backend();
    let resp = chat_set(
        &backend,
        json!({ "create": { "d": { "kind": "direct", "contactId": "bob" } } }),
    )
    .await;
    assert!(resp["created"].get("d").is_some(), "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1035: addMembers notifies "all current members", the added
/// one included; default role "member".
#[tokio::test]
async fn add_members_reports_to_all_current_members() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "member"), ("carol", "member")]);
    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": { "addMembers": [{ "id": "dave" }] } } }),
    )
    .await;
    assert!(resp["updated"].get("g1").is_some(), "{resp}");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        group_update(&events[0]),
        expect(
            "g1",
            "addMembers",
            &["dave:member"],
            &["bob", "carol", "dave"]
        )
    );
}

/// chat-00 L1038: removeMembers notifies the remaining and the removed.
#[tokio::test]
async fn remove_members_reports_to_remaining_and_removed() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "member"), ("carol", "member")]);
    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": { "removeMembers": ["bob"] } } }),
    )
    .await;
    assert!(resp["updated"].get("g1").is_some(), "{resp}");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        group_update(&events[0]),
        expect("g1", "removeMembers", &["bob"], &["bob", "carol"])
    );
}

/// chat-00 L1043: updateMemberRoles notifies all members.
#[tokio::test]
async fn update_roles_reports_to_all_members() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "member"), ("carol", "member")]);
    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": { "updateMemberRoles": [{ "id": "bob", "role": "admin" }] } } }),
    )
    .await;
    assert!(resp["updated"].get("g1").is_some(), "{resp}");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        group_update(&events[0]),
        expect("g1", "updateRoles", &["bob:admin"], &["bob", "carol"])
    );
}

/// chat-00 L1040: the owner leaving, another admin remaining, is
/// `"leave"` to the other members.
#[tokio::test]
async fn owner_leave_reports_leave() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "admin"), ("carol", "member")]);
    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": { "removeMembers": [ACCOUNT_ID] } } }),
    )
    .await;
    assert!(resp["updated"].get("g1").is_some(), "{resp}");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        group_update(&events[0]),
        expect("g1", "leave", &[], &["bob", "carol"])
    );
}

/// chat-00 L1040: the last admin leaving while others remain is
/// `"close"` "in place of `"leave"`".
#[tokio::test]
async fn last_admin_leave_reports_close() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "member"), ("carol", "member")]);
    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": { "removeMembers": [ACCOUNT_ID] } } }),
    )
    .await;
    assert!(resp["updated"].get("g1").is_some(), "{resp}");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        group_update(&events[0]),
        expect("g1", "close", &[], &["bob", "carol"])
    );
}

/// fed L625: a name change is `"updateMetadata"`.
#[tokio::test]
async fn name_change_reports_update_metadata() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "member")]);
    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": { "name": "Renamed" } } }),
    )
    .await;
    assert!(resp["updated"].get("g1").is_some(), "{resp}");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        group_update(&events[0]),
        expect("g1", "updateMetadata", &[], &["bob"])
    );
}

/// chat-00 L1032 ("all require admin role"): a rejected member change
/// changes nothing, so nothing is reported.
#[tokio::test]
async fn rejected_member_change_reports_nothing() {
    let backend = backend();
    seed_group(&backend, "member", &[("bob", "admin")]);
    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": { "addMembers": [{ "id": "dave" }] } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["g1"]["type"], "forbidden", "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1130: a created Message is enqueued for outbound delivery,
/// once, naming the Chat and the server-assigned id and senderMsgId.
#[tokio::test]
async fn create_reports_one_new_message() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "member")]);
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
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
    )
    .await
    .expect("Message/set");
    let created = &resp["created"]["c"];
    let id = created["id"].as_str().expect("created id");
    let sender_msg_id = created["senderMsgId"].as_str().expect("senderMsgId");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![OutboundEvent::NewMessage {
            chat_id: Id::from("g1"),
            message_id: Id::from(id),
            sender_msg_id: Id::from(sender_msg_id),
        }]
    );
}

/// chat-00 L1130: only a Message the server stores is enqueued. L521: in a
/// closed group "the server MUST reject with a `forbidden` SetError every
/// `Message/set` create in this Chat", so nothing is reported.
#[tokio::test]
async fn rejected_create_reports_nothing() {
    let backend = backend();
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "g1",
        json!({
            "id": "g1",
            "kind": "group",
            "name": "Team",
            "createdAt": "2026-01-01T00:00:00Z",
            "closedAt": "2026-01-01T06:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": [
                { "id": ACCOUNT_ID, "role": "member", "joinedAt": "2026-01-01T00:00:00Z" },
            ],
        }),
    );
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
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
    )
    .await
    .expect("Message/set");
    assert_eq!(resp["notCreated"]["c"]["type"], "forbidden", "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1158, fed L377: adding a reaction reports one `"add"` keyed by
/// the `senderReactionId`.
#[tokio::test]
async fn reaction_add_reports_one_add() {
    let backend = backend();
    seed_reacted_message(&backend, json!({}));
    let resp = message_update(
        &backend,
        "m1",
        json!({ "reactions/r1": { "emoji": "+1", "sentAt": "2026-01-02T00:00:00Z" } }),
    )
    .await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![reaction_update("r1", ReactionAction::Add)]
    );
}

/// chat-00 L1166, fed L374/L377: removing a reaction reports one
/// `"remove"` with the same `senderReactionId`, carrying the removed
/// reaction since the stored Message no longer holds it.
#[tokio::test]
async fn reaction_remove_reports_one_remove() {
    let backend = backend();
    let stored = json!({
        "emoji": "+1",
        "senderId": ACCOUNT_ID,
        "sentAt": "2026-01-01T12:00:00Z",
    });
    seed_reacted_message(&backend, json!({ "r1": stored }));
    let resp = message_update(&backend, "m1", json!({ "reactions/r1": null })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    let events = backend.take_outbound_events_for_test();
    assert_eq!(events.len(), 1, "{events:?}");
    let OutboundEvent::ReactionUpdate {
        chat_id,
        message_id,
        sender_msg_id,
        sender_reaction_id,
        action: ReactionAction::Remove { removed },
    } = &events[0]
    else {
        panic!("expected a ReactionUpdate remove, got {events:?}");
    };
    assert_eq!(
        (
            chat_id.as_ref(),
            message_id.as_ref(),
            sender_msg_id.as_ref()
        ),
        ("dm", "m1", "smid-m1")
    );
    assert_eq!(sender_reaction_id, "r1");
    assert_eq!(serde_json::to_value(removed).expect("Reaction"), stored);
}

/// chat-00 L1166: a removal propagates "the removal"; a null on a key with
/// no stored reaction removes nothing, so nothing is reported.
#[tokio::test]
async fn removing_absent_reaction_reports_nothing() {
    let backend = backend();
    seed_reacted_message(&backend, json!({}));
    message_update(&backend, "m1", json!({ "reactions/r1": null })).await;
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L1168: a removal of another sender's reaction is rejected with
/// `forbidden`, so nothing is reported.
#[tokio::test]
async fn rejected_reaction_reports_nothing() {
    let backend = backend();
    seed_reacted_message(
        &backend,
        json!({ "r1": { "emoji": "+1", "senderId": "bob", "sentAt": "2026-01-01T12:00:00Z" } }),
    );
    let resp = message_update(&backend, "m1", json!({ "reactions/r1": null })).await;
    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden", "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// PresenceStatus `ps1` with account-level `receiptSharing`.
fn seed_presence(backend: &MemoryBackend, receipt_sharing: bool) {
    backend.insert_object_for_test(
        "PresenceStatus",
        ACCOUNT_ID,
        "ps1",
        json!({
            "id": "ps1",
            "presence": "online",
            "receiptSharing": receipt_sharing,
            "updatedAt": "2026-01-01T00:00:00Z",
        }),
    );
}

/// Direct Chat `dm` with the per-Chat `receiptSharing` override.
fn seed_dm_receipt_sharing(backend: &MemoryBackend, receipt_sharing: bool) {
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "dm",
        json!({
            "id": "dm",
            "kind": "direct",
            "contactId": "bob",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "receiptSharing": receipt_sharing,
        }),
    );
}

/// The `Receipt` for `m1` in `dm` (`senderMsgId` `smid-m1`), read at
/// 2026-01-02T00:00:00Z.
fn receipt(read_disposition: Option<ReadDisposition>) -> OutboundEvent {
    OutboundEvent::Receipt {
        chat_id: Id::from("dm"),
        message_id: Id::from("m1"),
        sender_msg_id: Id::from("smid-m1"),
        read_at: UTCDate::from("2026-01-02T00:00:00Z"),
        read_disposition,
    }
}

/// fed L462, L472 with no override and chat-00 L945 default `true`;
/// chat-00 L680 stores `"displayed"` when `readDisposition` is omitted.
#[tokio::test]
async fn read_reports_receipt_with_default_disposition() {
    let backend = backend();
    seed_message(&backend, "m1", "bob");
    let resp = message_update(&backend, "m1", json!({ "readAt": "2026-01-02T00:00:00Z" })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![receipt(Some(ReadDisposition::Displayed))]
    );
}

/// fed L472: the account-level preference applies when no Chat override.
#[tokio::test]
async fn read_reports_receipt_with_given_disposition() {
    let backend = backend();
    seed_presence(&backend, true);
    seed_message(&backend, "m1", "bob");
    let resp = message_update(
        &backend,
        "m1",
        json!({ "readAt": "2026-01-02T00:00:00Z", "readDisposition": "processed" }),
    )
    .await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![receipt(Some(ReadDisposition::Processed))]
    );
}

/// fed L472: account-level `receiptSharing: false` and no Chat override
/// means the server "MUST NOT invoke `Peer/receipt`".
#[tokio::test]
async fn account_receipt_sharing_off_suppresses_receipt() {
    let backend = backend();
    seed_presence(&backend, false);
    seed_message(&backend, "m1", "bob");
    let resp = message_update(&backend, "m1", json!({ "readAt": "2026-01-02T00:00:00Z" })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// fed L472: a present `Chat.receiptSharing` "is used" over the account.
#[tokio::test]
async fn chat_receipt_sharing_on_overrides_account_off() {
    let backend = backend();
    seed_presence(&backend, false);
    seed_dm_receipt_sharing(&backend, true);
    seed_message(&backend, "m1", "bob");
    let resp = message_update(&backend, "m1", json!({ "readAt": "2026-01-02T00:00:00Z" })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![receipt(Some(ReadDisposition::Displayed))]
    );
}

/// fed L472: a `Chat.receiptSharing: false` override suppresses the receipt
/// even though the account shares.
#[tokio::test]
async fn chat_receipt_sharing_off_overrides_account_on() {
    let backend = backend();
    seed_presence(&backend, true);
    seed_dm_receipt_sharing(&backend, false);
    seed_message(&backend, "m1", "bob");
    let resp = message_update(&backend, "m1", json!({ "readAt": "2026-01-02T00:00:00Z" })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// fed L462: a receipt reports the recipient reading another's message;
/// the owner's own message has no remote sender to tell.
#[tokio::test]
async fn reading_own_message_reports_nothing() {
    let backend = backend();
    seed_message(&backend, "m1", ACCOUNT_ID);
    let resp = message_update(&backend, "m1", json!({ "readAt": "2026-01-02T00:00:00Z" })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// fed L462: `readAt: null` clears the read state; nothing was read.
#[tokio::test]
async fn clearing_read_at_reports_nothing() {
    let backend = backend();
    seed_message(&backend, "m1", "bob");
    let resp = message_update(&backend, "m1", json!({ "readAt": null })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

/// chat-00 L660: the burn happens, and with sharing on the sender still
/// gets its `Peer/receipt`.
#[tokio::test]
async fn burn_on_read_still_reports_receipt() {
    let backend = backend();
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smid-m1",
            "senderId": "bob",
            "chatId": "dm",
            "body": "hello",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
            "burnOnRead": true,
        }),
    );
    let resp = message_update(&backend, "m1", json!({ "readAt": "2026-01-02T00:00:00Z" })).await;
    assert!(resp["updated"].get("m1").is_some(), "{resp}");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![receipt(Some(ReadDisposition::Displayed))]
    );
}

async fn chat_typing(backend: &MemoryBackend, chat_id: &str, typing: bool) -> Result<(), String> {
    handle_chat_typing(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "chatId": chat_id, "typing": typing }),
    )
    .await
    .map(|_| ())
    .map_err(|e| format!("{e:?}"))
}

/// chat-00 L1086: a valid `Chat/typing` leads to local `typing` push and
/// `Peer/typing`, both carrying the Chat and the flag.
#[tokio::test]
async fn chat_typing_reports_typing() {
    let backend = backend();
    seed_group(&backend, "admin", &[("bob", "member")]);
    chat_typing(&backend, "g1", true)
        .await
        .expect("Chat/typing");
    chat_typing(&backend, "g1", false)
        .await
        .expect("Chat/typing");
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![
            OutboundEvent::Typing {
                chat_id: Id::from("g1"),
                typing: true,
            },
            OutboundEvent::Typing {
                chat_id: Id::from("g1"),
                typing: false,
            },
        ]
    );
}

/// chat-00 L1074: an unknown `chatId` is rejected, so nothing is sent.
#[tokio::test]
async fn rejected_chat_typing_reports_nothing() {
    let backend = backend();
    assert!(chat_typing(&backend, "nope", true).await.is_err());
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}

async fn presence_update(backend: &MemoryBackend, patch: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_presence_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { "ps1": patch } }),
    )
    .await
    .expect("PresenceStatus/set");
    primary
}

/// fed L706: a change to presence, `statusText` or `statusEmoji` may be
/// pushed as `Peer/presence`.
#[tokio::test]
async fn presence_change_reports_presence() {
    let backend = backend();
    seed_presence(&backend, true);
    for patch in [
        json!({ "presence": "busy" }),
        json!({ "statusText": "In a meeting" }),
        json!({ "statusEmoji": null }),
    ] {
        let resp = presence_update(&backend, patch).await;
        assert!(resp["updated"].get("ps1").is_some(), "{resp}");
    }
    assert_eq!(
        backend.take_outbound_events_for_test(),
        vec![
            OutboundEvent::Presence,
            OutboundEvent::Presence,
            OutboundEvent::Presence
        ]
    );
}

/// fed L706 names only presence, `statusText` and `statusEmoji`;
/// `receiptSharing` is not carried by `Peer/presence`.
#[tokio::test]
async fn receipt_sharing_change_reports_no_presence() {
    let backend = backend();
    seed_presence(&backend, true);
    let resp = presence_update(&backend, json!({ "receiptSharing": false })).await;
    assert!(resp["updated"].get("ps1").is_some(), "{resp}");
    assert_eq!(backend.take_outbound_events_for_test(), vec![]);
}
