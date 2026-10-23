//! `ChatMessagePush.unreadCount` badge total, `owner_unread_count`
//! (bd:JMAP-3rpv).
//!
//! Oracle: draft-atwood-jmap-chat-push-00 at jmap-chat-spec commit
//! 6c38e2f, §ChatMessagePush lines 149-150: "The account owner's total
//! unread message count across all non-muted Chats at the time this push
//! was generated", and draft-atwood-jmap-chat-00 §Chat `unreadCount`,
//! `muted` and `muteUntil` ("Muting expires at this time"). Fixtures are
//! hand-written JSON seeded into the reference `MemoryBackend`; each
//! expected total is the hand-summed `unreadCount` of the non-muted Chats.

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::owner_unread_count;
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acct-owner";
const FUTURE: &str = "2099-01-01T00:00:00Z";
const PAST: &str = "2000-01-01T00:00:00Z";

fn backend(maintains_unread_count: bool) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT));
    backend.set_maintains_unread_count_for_test(maintains_unread_count);
    backend
}

/// Store a group Chat `id` with stored `unreadCount` and the mute fields
/// merged in from `fields`.
fn chat(backend: &MemoryBackend, id: &str, unread: u64, fields: Value) {
    let mut chat = json!({
        "id": id,
        "kind": "group",
        "createdAt": "2026-10-01T00:00:00Z",
        "unreadCount": unread,
        "name": id,
    });
    for (k, v) in fields.as_object().expect("fields object") {
        chat[k] = v.clone();
    }
    backend.insert_object_for_test("Chat", ACCOUNT, id, chat);
}

fn message(backend: &MemoryBackend, id: &str, chat_id: &str) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT,
        id,
        json!({
            "id": id,
            "senderMsgId": format!("s{id}"),
            "senderId": "contact-bob",
            "chatId": chat_id,
            "body": "hi",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [],
            "sentAt": "2026-10-19T12:00:00Z",
            "receivedAt": "2026-10-19T12:00:01Z",
            "deliveryState": "received",
        }),
    );
}

async fn total(backend: &MemoryBackend) -> u64 {
    owner_unread_count(backend, &(), &Id::from(ACCOUNT))
        .await
        .expect("owner_unread_count")
}

/// "across all non-muted Chats": a Chat with `muted: true`, or with a
/// `muteUntil` still in the future, is left out; a Chat whose `muteUntil`
/// has passed counts again ("Muting expires at this time") even though
/// `muted` is still `true`. Backend-maintained counts are used as stored.
#[tokio::test]
async fn sums_stored_counts_of_non_muted_chats() {
    let b = backend(true);
    chat(&b, "c1", 3, json!({}));
    chat(&b, "c2", 5, json!({ "muted": true }));
    chat(&b, "c3", 7, json!({ "muted": true, "muteUntil": PAST }));
    chat(&b, "c4", 11, json!({ "muted": false, "muteUntil": FUTURE }));
    assert_eq!(total(&b).await, 3 + 7);
}

/// draft-atwood-jmap-chat-00 §Chat `unreadCount`: "The number of Messages
/// in this Chat whose `id` ULID is lexicographically greater than
/// `ReadPosition.lastReadMessageId` ... If `lastReadMessageId` is absent,
/// all messages are counted as unread." Without a backend-maintained
/// count the stored value (here a stale 0) is not trusted.
#[tokio::test]
async fn derives_counts_when_backend_does_not_maintain_them() {
    let b = backend(false);
    chat(&b, "c1", 0, json!({}));
    chat(&b, "c2", 0, json!({ "muted": true }));
    chat(&b, "c3", 0, json!({}));
    message(&b, "01J0000000000000000000000A", "c1");
    message(&b, "01J0000000000000000000000B", "c1");
    message(&b, "01J0000000000000000000000C", "c1");
    message(&b, "01J0000000000000000000000D", "c2");
    message(&b, "01J0000000000000000000000E", "c3");
    b.insert_object_for_test(
        "ReadPosition",
        ACCOUNT,
        "rp1",
        json!({
            "id": "rp1",
            "chatId": "c1",
            "lastReadMessageId": "01J0000000000000000000000A",
        }),
    );
    // c1: B and C sort after A; c2 is muted; c3 has no ReadPosition.
    assert_eq!(total(&b).await, 2 + 1);
}

/// An account with no Chats has nothing unread.
#[tokio::test]
async fn no_chats_is_zero() {
    assert_eq!(total(&backend(true)).await, 0);
}
