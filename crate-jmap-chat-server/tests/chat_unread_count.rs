//! `Chat.unreadCount` behaviour specific to `MemoryBackend` and this
//! crate's choices (bd:JMAP-y3f2.4, bd:JMAP-0x0q.56). The spec-pinned
//! counts and the ReadPosition lifecycle tests run for every backend in
//! the `testkit` conformance suite (`tests/backend_conformance.rs`).
//!
//! Oracles, from jmap-chat-spec 33702a5:
//!
//! - draft-atwood-jmap-chat-00.md L629 (aa01dd7 L635), §Chat
//!   `unreadCount`: "The number of unread Messages in this Chat, derived
//!   server-side from the owner's ReadPosition record. Candidates are the
//!   Messages whose `id` ULID is lexicographically greater than
//!   `ReadPosition.lastReadMessageId`, or all Messages in the Chat if
//!   `lastReadMessageId` is absent. [...] The value is advisory [...] Servers
//!   SHOULD keep it accurate, but MAY update it some time after the change
//!   that affects it [...] Whenever the server changes the value, the
//!   Chat's state changes as for any other property ({{RFC8620}} Section
//!   5.1)." The exact-count tests below hold for `MemoryBackend`, which
//!   keeps the SHOULD by default.
//! - RFC 8620 §5.1: "If the data changes, this string MUST change", so
//!   a change to the derived `unreadCount` advances the `Chat` state.
//! - RFC 8620 §5.2: `updated` lists "the ids for records that have been
//!   updated since the old state", so `Chat/changes` names the Chat
//!   whose `unreadCount` moved.
//! - jmap-chat-spec aa01dd7 L635, §Chat `unreadCount`: "Which candidates
//!   count is implementation-defined; for example, a server MAY exclude
//!   Messages the owner sent." This server excludes Messages whose
//!   `senderId` is the accountId (the owner's own). The seeded Messages
//!   below come from `OTHER_SENDER` so the L629 counts apply to them
//!   unchanged.

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{
    handle_chat_changes, handle_chat_get, handle_position_set, owner_unread_count,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

// Hand-picked ULIDs (Crockford base32, 26 chars) in ascending order:
// "ULIDs are lexicographically ordered by time" (draft L675).
const M1: &str = "01J0000000000000000000000A";
const M2: &str = "01J0000000000000000000000B";
const M3: &str = "01J0000000000000000000000C";
const M4: &str = "01J0000000000000000000000D";
const M5: &str = "01J0000000000000000000000E";
const OTHER_MSG: &str = "01J0000000000000000000000F";

/// A ChatContact other than the owner; the owner's own Messages carry the
/// accountId as `senderId`.
const OTHER_SENDER: &str = "contact-bob";

const SEEDED_READ_AT: &str = "2000-01-01T00:00:00Z";

fn seed_chat(backend: &MemoryBackend, id: &str, extra: Value) {
    let mut chat = json!({
        "id": id,
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "pinnedMessageIds": [],
    });
    let (Value::Object(chat_map), Value::Object(extra_map)) = (&mut chat, extra) else {
        unreachable!("both are object literals");
    };
    chat_map.extend(extra_map);
    backend.insert_object_for_test("Chat", ACCOUNT_ID, id, chat);
}

fn seed_group(backend: &MemoryBackend, id: &str) {
    seed_chat(
        backend,
        id,
        json!({
            "kind": "group",
            "name": "G",
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
}

fn seed_message(backend: &MemoryBackend, id: &str, chat_id: &str) {
    seed_message_from(backend, OTHER_SENDER, id, chat_id);
}

fn seed_message_from(backend: &MemoryBackend, sender_id: &str, id: &str, chat_id: &str) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "senderMsgId": id,
            "senderId": sender_id,
            "chatId": chat_id,
            "body": "x",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
        }),
    );
}

fn seed_position(backend: &MemoryBackend, id: &str, chat_id: &str) {
    backend.insert_object_for_test(
        "ReadPosition",
        ACCOUNT_ID,
        id,
        json!({ "id": id, "chatId": chat_id, "lastReadAt": SEEDED_READ_AT }),
    );
}

/// Group Chat `g` with M1..M5 and its ReadPosition `rp-g` (no
/// `lastReadMessageId`), plus group Chat `h` with OTHER_MSG.
fn seed_five(backend: &MemoryBackend) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_group(backend, "g");
    seed_group(backend, "h");
    for m in [M1, M2, M3, M4, M5] {
        seed_message(backend, m, "g");
    }
    seed_message(backend, OTHER_MSG, "h");
    seed_position(backend, "rp-g", "g");
}

async fn unread_counts(backend: &MemoryBackend, args: Value) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(backend, &(), args, &[])
        .await
        .expect("Chat/get");
    resp["list"].as_array().expect("list").clone()
}

async fn unread_of(backend: &MemoryBackend, chat_id: &str) -> Value {
    let list = unread_counts(
        backend,
        json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
    )
    .await;
    assert_eq!(list.len(), 1, "{list:?}");
    list[0]["unreadCount"].clone()
}

async fn set_last_read(backend: &MemoryBackend, rp: &str, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_position_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { rp: patch } }),
    )
    .await
    .expect("ReadPosition/set");
    resp
}

async fn chat_state(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [] }),
        &[],
    )
    .await
    .expect("Chat/get");
    resp["state"].as_str().expect("state").to_owned()
}

async fn chats_updated_since(backend: &MemoryBackend, since: &str) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } = handle_chat_changes(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
        &[],
    )
    .await
    .expect("Chat/changes");
    resp["updated"].as_array().expect("updated").clone()
}

async fn message_set(backend: &MemoryBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary: resp, .. } =
        handle_message_set(backend, &(), &std::collections::BTreeMap::new(), args, &[])
            .await
            .expect("Message/set");
    resp
}

/// A Message the owner sends is not counted (implementation-defined,
/// chat-00 aa01dd7 L635), so `h` keeps its one unread Message from OTHER_SENDER; the create still
/// rewrites the Chat's derived count, so `Chat/changes` reports that Chat
/// (RFC 8620 §5.2).
#[tokio::test]
async fn message_create_advances_chat_state() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    let since = chat_state(&backend).await;

    let resp = message_set(
        &backend,
        json!({ "create": { "m": {
            "chatId": "h",
            "body": "hi",
            "bodyType": "text/plain",
            "sentAt": "2026-01-01T00:00:00Z",
        } } }),
    )
    .await;
    assert!(resp["created"].get("m").is_some(), "{resp}");

    assert_eq!(unread_of(&backend, "h").await, 1);
    assert_eq!(
        chats_updated_since(&backend, &since).await,
        vec![json!("h")]
    );
}

/// L629: the server "MAY update it some time after the change that
/// affects it", and "Whenever the server changes the value, the Chat's
/// state changes". A backend that keeps the count itself
/// (`maintains_unread_count`) gets its stored value from `Chat/get`, and
/// a ReadPosition update that leaves that value alone leaves the Chat
/// state alone too (RFC 8620 §5.1).
#[tokio::test]
async fn backend_maintained_unread_count_is_not_recomputed() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    seed_chat(
        &backend,
        "g",
        json!({
            "kind": "group",
            "name": "G",
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
            "unreadCount": 7,
        }),
    );
    backend.set_maintains_unread_count_for_test(true);
    assert_eq!(unread_of(&backend, "g").await, 7, "stored value, not 5");
    let since = chat_state(&backend).await;

    let resp = set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M3 })).await;
    assert!(resp["updated"].get("rp-g").is_some(), "{resp}");
    message_set(&backend, json!({ "destroy": [M5] })).await;

    assert_eq!(unread_of(&backend, "g").await, 7);
    assert_eq!(chat_state(&backend).await, since);
    assert_eq!(
        chats_updated_since(&backend, &since).await,
        Vec::<Value>::new()
    );
}

// Mixed-sender Chat `x` (implementation-defined, chat-00 aa01dd7 L635):
// Messages whose `senderId` is the accountId are the owner's and are
// never counted. In id order:
// X1 other, X2 owner, X3 other, X4 owner, X5 other, X6 owner, X7 other.
const X1: &str = "01J00000000000000000000011";
const X2: &str = "01J00000000000000000000012";
const X3: &str = "01J00000000000000000000013";
const X4: &str = "01J00000000000000000000014";
const X5: &str = "01J00000000000000000000015";
const X6: &str = "01J00000000000000000000016";
const X7: &str = "01J00000000000000000000017";

fn seed_mixed(backend: &MemoryBackend) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_group(backend, "x");
    for (id, sender) in [
        (X1, OTHER_SENDER),
        (X2, ACCOUNT_ID),
        (X3, OTHER_SENDER),
        (X4, ACCOUNT_ID),
        (X5, OTHER_SENDER),
        (X6, ACCOUNT_ID),
        (X7, OTHER_SENDER),
    ] {
        seed_message_from(backend, sender, id, "x");
    }
    seed_position(backend, "rp-x", "x");
}

/// Hand count with no `lastReadMessageId`: the 4 Messages from
/// OTHER_SENDER (X1, X3, X5, X7) count; the owner's 3 do not.
#[tokio::test]
async fn owner_messages_not_counted_without_last_read() {
    let backend = MemoryBackend::new();
    seed_mixed(&backend);
    assert_eq!(unread_of(&backend, "x").await, 4);
}

/// Hand count after `lastReadMessageId` X2: X3..X7 sort after it, 3 from
/// OTHER_SENDER (X3, X5, X7) and 2 from the owner (X4, X6), so 3.
#[tokio::test]
async fn owner_messages_not_counted_after_last_read() {
    let backend = MemoryBackend::new();
    seed_mixed(&backend);
    set_last_read(&backend, "rp-x", json!({ "lastReadMessageId": X2 })).await;
    assert_eq!(unread_of(&backend, "x").await, 3);
}

/// The count the kit writes to the stored Chat on a ReadPosition update
/// follows the same rule: once the backend reports its stored value
/// (`maintains_unread_count`), `Chat/get` still returns 3 (X3, X5, X7).
#[tokio::test]
async fn stored_unread_count_excludes_owner_messages() {
    let backend = MemoryBackend::new();
    seed_mixed(&backend);
    set_last_read(&backend, "rp-x", json!({ "lastReadMessageId": X2 })).await;
    backend.set_maintains_unread_count_for_test(true);
    assert_eq!(unread_of(&backend, "x").await, 3);
}

/// The push badge total (draft-atwood-jmap-chat-push-00 §ChatMessagePush
/// `unreadCount`) sums the same per-Chat count: `x` is the only Chat and
/// has 3 (X3, X5, X7).
#[tokio::test]
async fn push_badge_excludes_owner_messages() {
    let backend = MemoryBackend::new();
    seed_mixed(&backend);
    set_last_read(&backend, "rp-x", json!({ "lastReadMessageId": X2 })).await;
    let total = owner_unread_count(&backend, &(), &Id::from(ACCOUNT_ID))
        .await
        .expect("owner_unread_count");
    assert_eq!(total, 3);
}
