//! `sweep_expired` applies the draft's time-driven expiry rules against an
//! injected clock (bd:JMAP-y3f2.6).
//!
//! Oracles, from jmap-chat-spec 91672c9, draft-atwood-jmap-chat-00.md:
//!
//! - L559, §Chat `muteUntil`: "Muting expires at this time. Servers SHOULD
//!   clear `muted` and `muteUntil` automatically when the time passes."
//! - L581, §Chat `messageExpirySeconds`: "When set and non-zero, messages
//!   in this chat older than this many seconds are deleted by this
//!   mailbox."
//! - L654, §Message `receivedAt`: "Authoritative for ordering and expiry
//!   calculations."
//! - L657, §Message `senderExpiresAt`: "servers MUST permanently delete
//!   this message — removing the row entirely, not leaving a tombstone —
//!   at or before this time. A hard-deleted message appears in the
//!   `destroyed` list of subsequent `Message/changes` responses, not
//!   `updated`."
//! - L660, §Message `burnOnRead`: "the receiving server MUST permanently
//!   hard-delete this message (row removal, not tombstone) immediately
//!   after setting `readAt`."
//! - L663, §Message `expiresAfterReadSeconds`: "the receiving server MUST
//!   permanently hard-delete this message (row removal, not tombstone)
//!   this many seconds after setting `readAt`."
//! - L660 and L663, both rules: "Applies only to the receiving mailbox;
//!   the sender's own copy is not affected."
//! - L941-942, §PresenceStatus `expiresAt`: "If set, the server SHOULD
//!   clear `statusText` and `statusEmoji` (setting them to null) at this
//!   time, and SHOULD reset `presence` to `"online"`."
//!
//! - L547-548, §Chat `unreadCount`: "Count of Messages in this Chat whose
//!   `id` ULID is lexicographically greater than
//!   `ReadPosition.lastReadMessageId`. [...] If `lastReadMessageId` is
//!   absent, all messages are counted as unread." An expired Message
//!   lowers that count, and RFC 8620 §5.1 ("If the data changes, this
//!   string MUST change") then requires the `Chat` state to move
//!   (bd:JMAP-y3f2.18).
//!
//! Clock values are hand-computed: 2024-01-01T00:00:00Z is Unix time
//! 1_704_067_200.

mod common;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::{handle_message_changes, handle_message_get};
use jmap_chat_server::{handle_chat_changes, handle_chat_get, handle_presence_get, sweep_expired};
use jmap_types::Id;
use serde_json::{Value, json};

/// 2024-01-01T00:00:00Z.
const T0: u64 = 1_704_067_200;
const T0_STR: &str = "2024-01-01T00:00:00Z";

fn at(offset_secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(T0 + offset_secs)
}

fn backend() -> MemoryBackend {
    let b = MemoryBackend::new();
    b.register_account(&Id::from(ACCOUNT_ID));
    b
}

fn seed_chat(b: &MemoryBackend, id: &str, extra: Value) {
    let mut chat = json!({
        "id": id,
        "kind": "direct",
        "contactId": "contact1",
        "createdAt": T0_STR,
        "unreadCount": 0,
        "pinnedMessageIds": [],
    });
    chat.as_object_mut()
        .expect("object literal")
        .extend(extra.as_object().expect("object literal").clone());
    b.insert_object_for_test("Chat", ACCOUNT_ID, id, chat);
}

fn seed_message(b: &MemoryBackend, id: &str, chat_id: &str, extra: Value) {
    let mut msg = json!({
        "id": id,
        "senderMsgId": id,
        "senderId": "other",
        "chatId": chat_id,
        "body": "x",
        "bodyType": "text/plain",
        "attachments": [],
        "sentAt": T0_STR,
        "receivedAt": T0_STR,
        "deliveryState": "delivered",
    });
    msg.as_object_mut()
        .expect("object literal")
        .extend(extra.as_object().expect("object literal").clone());
    b.insert_object_for_test("Message", ACCOUNT_ID, id, msg);
}

async fn sweep(b: &MemoryBackend, now: SystemTime) {
    sweep_expired(b, &(), &Id::from(ACCOUNT_ID), now)
        .await
        .expect("sweep");
}

async fn message_exists(b: &MemoryBackend, id: &str) -> bool {
    let resp = handle_message_get(b, &(), json!({ "accountId": ACCOUNT_ID, "ids": [id] }), &[])
        .await
        .expect("Message/get")
        .primary;
    resp["list"].as_array().expect("list").len() == 1
}

async fn chat(b: &MemoryBackend, id: &str) -> Value {
    let resp = handle_chat_get(b, &(), json!({ "accountId": ACCOUNT_ID, "ids": [id] }), &[])
        .await
        .expect("Chat/get")
        .primary;
    resp["list"][0].clone()
}

/// L657: deletion "at or before this time", and the id lands in
/// `destroyed`, not `updated`.
#[tokio::test]
async fn sender_expires_at_hard_deletes_at_deadline() {
    let b = backend();
    seed_chat(&b, "c1", json!({}));
    seed_message(
        &b,
        "m1",
        "c1",
        json!({ "senderExpiresAt": "2024-01-01T00:00:10Z" }),
    );
    let state = handle_message_get(&b, &(), json!({ "accountId": ACCOUNT_ID, "ids": [] }), &[])
        .await
        .expect("Message/get")
        .primary["state"]
        .clone();

    sweep(&b, at(9)).await;
    assert!(
        message_exists(&b, "m1").await,
        "deadline not reached at T+9"
    );

    sweep(&b, at(10)).await;
    assert!(!message_exists(&b, "m1").await, "deleted at the deadline");
    let changes = handle_message_changes(
        &b,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": state }),
        &[],
    )
    .await
    .expect("Message/changes")
    .primary;
    assert_eq!(changes["destroyed"], json!(["m1"]));
    assert_eq!(changes["updated"], json!([]));
}

/// L581 + L654: "older than this many seconds", measured from
/// `receivedAt`; a zero policy deletes nothing ("When set and non-zero").
#[tokio::test]
async fn message_expiry_seconds_deletes_messages_older_than_policy() {
    let b = backend();
    seed_chat(&b, "c1", json!({ "messageExpirySeconds": 60 }));
    seed_chat(&b, "c0", json!({ "messageExpirySeconds": 0 }));
    seed_message(&b, "m1", "c1", json!({}));
    seed_message(&b, "m0", "c0", json!({}));

    sweep(&b, at(60)).await;
    assert!(
        message_exists(&b, "m1").await,
        "exactly 60s old is not older than 60s"
    );

    sweep(&b, at(61)).await;
    assert!(!message_exists(&b, "m1").await, "61s old is older than 60s");
    assert!(
        message_exists(&b, "m0").await,
        "zero policy deletes nothing"
    );
}

/// L663: deletion "this many seconds after setting `readAt`"; an unread
/// message has no such deadline.
#[tokio::test]
async fn expires_after_read_seconds_counts_from_read_at() {
    let b = backend();
    seed_chat(&b, "c1", json!({}));
    seed_message(
        &b,
        "read",
        "c1",
        json!({ "expiresAfterReadSeconds": 30, "readAt": "2024-01-01T00:01:40Z" }),
    );
    seed_message(&b, "unread", "c1", json!({ "expiresAfterReadSeconds": 30 }));

    sweep(&b, at(129)).await;
    assert!(message_exists(&b, "read").await, "29s after readAt");

    sweep(&b, at(130)).await;
    assert!(!message_exists(&b, "read").await, "30s after readAt");
    assert!(
        message_exists(&b, "unread").await,
        "never read, never expires"
    );
}

/// L660: a read `burnOnRead` message must not survive; the sweep deletes
/// one stored with `readAt` set.
#[tokio::test]
async fn read_burn_on_read_message_is_deleted() {
    let b = backend();
    seed_chat(&b, "c1", json!({}));
    seed_message(
        &b,
        "burn",
        "c1",
        json!({ "burnOnRead": true, "readAt": T0_STR }),
    );
    seed_message(&b, "unread", "c1", json!({ "burnOnRead": true }));

    sweep(&b, at(0)).await;
    assert!(!message_exists(&b, "burn").await);
    assert!(message_exists(&b, "unread").await, "not yet read");
}

/// L660 + L663: "the sender's own copy is not affected", so an own-sent
/// message (`senderId` is the account) survives with `readAt` set.
#[tokio::test]
async fn read_rules_spare_the_senders_own_copy() {
    let b = backend();
    seed_chat(&b, "c1", json!({}));
    seed_message(
        &b,
        "own-timed",
        "c1",
        json!({
            "senderId": ACCOUNT_ID,
            "expiresAfterReadSeconds": 30,
            "readAt": T0_STR,
        }),
    );
    seed_message(
        &b,
        "own-burn",
        "c1",
        json!({ "senderId": ACCOUNT_ID, "burnOnRead": true, "readAt": T0_STR }),
    );

    sweep(&b, at(3600)).await;
    assert!(message_exists(&b, "own-timed").await, "own copy kept");
    assert!(message_exists(&b, "own-burn").await, "own copy kept");
}

/// L559: `muted` and `muteUntil` are cleared once the time passes.
#[tokio::test]
async fn mute_until_clears_muted_and_mute_until() {
    let b = backend();
    seed_chat(
        &b,
        "c1",
        json!({ "muted": true, "muteUntil": "2024-01-01T00:00:05Z" }),
    );

    sweep(&b, at(4)).await;
    let before = chat(&b, "c1").await;
    assert_eq!(before["muted"], json!(true));
    assert_eq!(before["muteUntil"], json!("2024-01-01T00:00:05Z"));

    sweep(&b, at(5)).await;
    let after = chat(&b, "c1").await;
    assert_eq!(after["muted"], json!(false));
    assert!(
        after.get("muteUntil").is_none_or(Value::is_null),
        "muteUntil cleared: {after}"
    );
}

/// L941-942: at `expiresAt` the status text and emoji are nulled and
/// presence resets to "online".
#[tokio::test]
async fn presence_expires_at_resets_status() {
    let b = backend();
    b.insert_object_for_test(
        "PresenceStatus",
        ACCOUNT_ID,
        "p1",
        json!({
            "id": "p1",
            "presence": "busy",
            "receiptSharing": true,
            "updatedAt": T0_STR,
            "statusText": "In a meeting",
            "statusEmoji": ":calendar:",
            "expiresAt": "2024-01-01T01:00:00Z",
        }),
    );
    let get = || async {
        handle_presence_get(&b, &(), json!({ "accountId": ACCOUNT_ID, "ids": ["p1"] }))
            .await
            .expect("PresenceStatus/get")
            .primary["list"][0]
            .clone()
    };

    sweep(&b, at(3599)).await;
    let before = get().await;
    assert_eq!(before["presence"], json!("busy"));
    assert_eq!(before["statusText"], json!("In a meeting"));

    sweep(&b, at(3600)).await;
    let after = get().await;
    assert_eq!(after["presence"], json!("online"));
    assert!(
        after.get("statusText").is_none_or(Value::is_null),
        "{after}"
    );
    assert!(
        after.get("statusEmoji").is_none_or(Value::is_null),
        "{after}"
    );
}

async fn chat_state(b: &MemoryBackend) -> Value {
    handle_chat_get(b, &(), json!({ "accountId": ACCOUNT_ID, "ids": [] }), &[])
        .await
        .expect("Chat/get")
        .primary["state"]
        .clone()
}

/// L547-548 with RFC 8620 §5.1/§5.2: expiring an unread Message lowers
/// its Chat's `unreadCount`, so `Chat/changes` lists exactly that Chat;
/// a re-run at the same time changes nothing.
#[tokio::test]
async fn expiry_records_chat_unread_count_change() {
    let b = backend();
    seed_chat(&b, "c1", json!({}));
    seed_chat(&b, "c2", json!({}));
    seed_message(
        &b,
        "m1",
        "c1",
        json!({ "senderExpiresAt": "2024-01-01T00:00:10Z" }),
    );
    seed_message(&b, "m2", "c2", json!({}));
    let since = chat_state(&b).await;

    sweep(&b, at(10)).await;

    assert_eq!(chat(&b, "c1").await["unreadCount"], json!(0));
    assert_eq!(chat(&b, "c2").await["unreadCount"], json!(1));
    let changes = handle_chat_changes(
        &b,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
        &[],
    )
    .await
    .expect("Chat/changes")
    .primary;
    assert_eq!(changes["updated"], json!(["c1"]), "{changes}");

    let after = chat_state(&b).await;
    sweep(&b, at(10)).await;
    assert_eq!(chat_state(&b).await, after, "re-run changes no state");
}
