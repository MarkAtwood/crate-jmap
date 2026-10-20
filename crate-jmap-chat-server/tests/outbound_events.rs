//! `ChatBackend::on_outbound_event` for retractions, edits and group Chat
//! changes (bd:JMAP-5mz6.5).
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
//!
//! The owner is referenced by the accountId (draft-atwood-jmap-chat-00
//! §Terminology), so it is never a recipient.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_set, handle_message_set, GroupUpdateAction, OutboundEvent};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

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
        handle_chat_set(backend, &(), args).await.expect("Chat/set");
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
