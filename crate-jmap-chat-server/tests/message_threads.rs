//! `replyTo` / `threadRootId` validation and thread counts
//! (bd:JMAP-y3f2.3), and their `Message/changes` reporting
//! (bd:JMAP-y3f2.15). The `replyCount` / `unreadReplyCount` tests that hold
//! for every backend run in the `testkit` conformance suite
//! (`tests/backend_conformance.rs`); the owner-reply tests here pin this
//! server's implementation-defined choice.
//!
//! Oracles, quoted from draft-atwood-jmap-chat-00.md at jmap-chat-spec
//! 91672c9:
//!
//! - L632-633 `replyTo`: "The receiver-assigned `id` of the Message this
//!   replies to. Servers MUST validate that this ID exists in the same
//!   Chat before storing."
//! - L638-639 `replyCount`: "Count of messages in this chat with
//!   `replyTo` equal to this message's `id`. Present only when
//!   `supportsThreads` is `true`."
//! - L641-642 `unreadReplyCount`: "Count of replies to this message
//!   received after the owner's `ReadPosition.lastReadMessageId` in this
//!   Chat. Present only when `supportsThreads` is `true` and this message
//!   is a thread root (`threadRootId` is absent)."
//! - L591: "Message IDs are ULIDs, assigned by the **receiving** mailbox
//!   at storage time. ULIDs are lexicographically ordered by time".
//! - L1510-1514 thread root assignment: "A message with no `replyTo` is a
//!   potential thread root. `threadRootId` MUST be absent." / "A message
//!   replying to a thread root (the referenced message has no
//!   `threadRootId`): set `threadRootId` to the value of `replyTo`." / "A
//!   message replying to a non-root message: set `threadRootId` to the
//!   referenced message's `threadRootId`." / "Servers SHOULD validate them
//!   and MAY correct `threadRootId` if the client supplies an incorrect
//!   value."
//! - L1672 §Input Validation: "When a `Message/set` create or update fails
//!   one of these checks and this document names no other error for that
//!   check, the server MUST reject the entry with an `invalidProperties`
//!   SetError".
//! - L1506: "A thread is the set of Messages sharing a common
//!   `threadRootId`."
//! - jmap-chat-spec aa01dd7 L732, `unreadReplyCount`: "Which of those
//!   replies count is implementation-defined, as for `Chat.unreadCount`";
//!   this server does not count replies whose `senderId` is the accountId
//!   (the owner's own). The fixture Messages come from `OTHER_SENDER`.

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::{
    handle_message_changes, handle_message_get, handle_message_query, handle_message_set,
};
use jmap_chat_server::{ChatLimits, handle_chat_set, handle_position_set};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

// Hand-picked ULIDs (Crockford base32, 26 chars) in ascending order.
const ROOT: &str = "01J0000000000000000000000A";
const A: &str = "01J0000000000000000000000B";
const B: &str = "01J0000000000000000000000C";
const C: &str = "01J0000000000000000000000D";
const OTHER_CHAT_MSG: &str = "01J0000000000000000000000E";
/// A reply to ROOT sent by the owner, sorting after every fixture id.
const OWN_REPLY: &str = "01J0000000000000000000000F";

/// A ChatContact other than the owner; the owner's own Messages carry the
/// accountId as `senderId`.
const OTHER_SENDER: &str = "contact-bob";

async fn create_group(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
        }),
        &[],
    )
    .await
    .expect("Chat/set");
    resp["created"]["g"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("group not created: {resp}"))
        .to_owned()
}

/// Seed one stored Message with a fixed id.
fn seed_message(
    backend: &MemoryBackend,
    id: &str,
    chat_id: &str,
    reply_to: Option<&str>,
    root: Option<&str>,
) {
    seed_message_from(backend, OTHER_SENDER, id, chat_id, reply_to, root);
}

fn seed_message_from(
    backend: &MemoryBackend,
    sender_id: &str,
    id: &str,
    chat_id: &str,
    reply_to: Option<&str>,
    root: Option<&str>,
) {
    let mut msg = json!({
        "id": id,
        "senderMsgId": id,
        "senderId": sender_id,
        "chatId": chat_id,
        "body": "x",
        "bodyType": "text/plain",
        "attachments": [],
        "sentAt": "2024-01-01T00:00:00Z",
        "receivedAt": "2024-01-01T00:00:00Z",
        "deliveryState": "delivered",
    });
    if let Some(r) = reply_to {
        msg["replyTo"] = json!(r);
    }
    if let Some(r) = root {
        msg["threadRootId"] = json!(r);
    }
    backend.insert_object_for_test("Message", ACCOUNT_ID, id, msg);
}

/// Fixture thread in one Chat:
///
/// ```text
/// ROOT
/// ├── A  (replyTo ROOT, threadRootId ROOT)
/// │   └── B  (replyTo A, threadRootId ROOT)
/// └── C  (replyTo ROOT, threadRootId ROOT)
/// ```
///
/// plus OTHER_CHAT_MSG in a second Chat, and a ReadPosition for the
/// first Chat with `lastReadMessageId` = A.
async fn fixture(backend: &MemoryBackend) -> (String, String) {
    let chat = create_group(backend).await;
    let other = create_group(backend).await;
    seed_message(backend, ROOT, &chat, None, None);
    seed_message(backend, A, &chat, Some(ROOT), Some(ROOT));
    seed_message(backend, B, &chat, Some(A), Some(ROOT));
    seed_message(backend, C, &chat, Some(ROOT), Some(ROOT));
    seed_message(backend, OTHER_CHAT_MSG, &other, None, None);
    backend.insert_object_for_test(
        "ReadPosition",
        ACCOUNT_ID,
        "rp1",
        json!({
            "id": "rp1",
            "chatId": chat,
            "lastReadMessageId": A,
            "lastReadAt": "2024-01-01T00:00:00Z",
        }),
    );
    (chat, other)
}

fn threads_on(backend: &MemoryBackend) {
    backend.set_limits_for_test(Some(ChatLimits::default().with_supports_threads(true)));
}

async fn create(backend: &MemoryBackend, obj: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "create": { "m": obj } }),
        &[],
    )
    .await
    .expect("Message/set");
    resp
}

fn reply(chat: &str, reply_to: &str, root: Option<&str>) -> Value {
    let mut obj = json!({
        "chatId": chat,
        "body": "re",
        "bodyType": "text/plain",
        "sentAt": "2024-01-01T00:00:00Z",
        "replyTo": reply_to,
    });
    if let Some(r) = root {
        obj["threadRootId"] = json!(r);
    }
    obj
}

async fn get(backend: &MemoryBackend, ids: &[&str]) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ids }),
        &[],
    )
    .await
    .expect("Message/get");
    resp["list"].as_array().expect("list").clone()
}

/// L632-633: a `replyTo` naming a Message of another Chat does not exist
/// "in the same Chat", so the create is rejected (L1672:
/// `invalidProperties` naming `replyTo`).
#[tokio::test]
async fn cross_chat_reply_to_rejected() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    let resp = create(&backend, reply(&chat, OTHER_CHAT_MSG, None)).await;
    assert_eq!(
        resp["notCreated"]["m"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["m"]["properties"],
        json!(["replyTo"]),
        "{resp}"
    );
}

/// L632-633: a `replyTo` naming no Message at all is rejected the same way.
#[tokio::test]
async fn unknown_reply_to_rejected() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    let resp = create(&backend, reply(&chat, "01J00000000000000000000ZZZ", None)).await;
    assert_eq!(
        resp["notCreated"]["m"]["properties"],
        json!(["replyTo"]),
        "{resp}"
    );
}

/// L1510-1514: each `threadRootId` that breaks an assignment rule is
/// rejected with `invalidProperties` naming `threadRootId`.
#[tokio::test]
async fn invalid_thread_root_id_rejected() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    let no_reply = json!({
        "chatId": chat,
        "body": "x",
        "bodyType": "text/plain",
        "sentAt": "2024-01-01T00:00:00Z",
        "threadRootId": ROOT,
    });
    let cases = [
        // "A message with no `replyTo` ... `threadRootId` MUST be absent."
        no_reply,
        // Reply to the root ROOT: threadRootId must be ROOT, not A.
        reply(&chat, ROOT, Some(A)),
        // Reply to non-root A: threadRootId must be A's root, ROOT, not A.
        reply(&chat, A, Some(A)),
    ];
    for obj in cases {
        let resp = create(&backend, obj.clone()).await;
        assert_eq!(
            resp["notCreated"]["m"]["type"], "invalidProperties",
            "{obj} -> {resp}"
        );
        assert_eq!(
            resp["notCreated"]["m"]["properties"],
            json!(["threadRootId"]),
            "{obj} -> {resp}"
        );
    }
}

/// L1511-1512: a correct `threadRootId` is accepted, and an omitted one is
/// set by the server ("MAY correct") to the value the rules give.
#[tokio::test]
async fn thread_root_id_accepted_or_filled_in() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    let cases = [
        (reply(&chat, ROOT, Some(ROOT)), ROOT),
        (reply(&chat, B, Some(ROOT)), ROOT),
        (reply(&chat, ROOT, None), ROOT),
        (reply(&chat, B, None), ROOT),
    ];
    for (obj, root) in cases {
        let resp = create(&backend, obj.clone()).await;
        assert_eq!(
            resp["created"]["m"]["threadRootId"], root,
            "{obj} -> {resp}"
        );
    }
}

/// L1198 "`threadRootId` ... Valid only when `supportsThreads` is `true`"
/// and L1506: with threads on, the filter returns the Messages whose
/// `threadRootId` is ROOT, which are A, B and C.
#[tokio::test]
async fn thread_root_id_filter_with_supports_threads() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    threads_on(&backend);
    let HandlerOutput { primary: resp, .. } = handle_message_query(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": { "chatId": chat, "threadRootId": ROOT },
        }),
        &[],
    )
    .await
    .expect("Message/query");
    let mut ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().expect("id"))
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, [A, B, C], "{resp}");
}

// ---------------------------------------------------------------------------
// Count changes advance Message state (bd:JMAP-y3f2.15)
//
// RFC 8620 §5.1: "If the data changes, this string MUST change." §5.2
// `updated`: "An array of Ids for records that have been updated since
// the old state." `replyCount` and `unreadReplyCount` are Message data
// (L638-642), so a write that changes them must list the Message.
// ---------------------------------------------------------------------------

async fn message_state(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [] }),
        &[],
    )
    .await
    .expect("Message/get");
    resp["state"].as_str().expect("state").to_owned()
}

async fn message_changes(backend: &MemoryBackend, since: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_changes(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
        &[],
    )
    .await
    .expect("Message/changes");
    resp
}

/// Fixture plus OWN_REPLY to ROOT from the owner. Hand count for ROOT:
/// replies A, C and OWN_REPLY -> replyCount 3; after lastReadMessageId A
/// come C and OWN_REPLY, and the owner's reply is not counted
/// (implementation-defined, chat-00 aa01dd7 L732) -> unreadReplyCount 1.
#[tokio::test]
async fn unread_reply_count_excludes_owner_replies() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    seed_message_from(
        &backend,
        ACCOUNT_ID,
        OWN_REPLY,
        &chat,
        Some(ROOT),
        Some(ROOT),
    );
    threads_on(&backend);
    let list = get(&backend, &[ROOT]).await;
    assert_eq!(list[0]["replyCount"], 3, "{list:?}");
    assert_eq!(list[0]["unreadReplyCount"], 1, "{list:?}");
}

/// Same, with no `lastReadMessageId`: of ROOT's replies A, C and
/// OWN_REPLY, only the two from OTHER_SENDER count -> 2.
#[tokio::test]
async fn unread_reply_count_excludes_owner_replies_without_read_position() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    seed_message_from(
        &backend,
        ACCOUNT_ID,
        OWN_REPLY,
        &chat,
        Some(ROOT),
        Some(ROOT),
    );
    backend.insert_object_for_test(
        "ReadPosition",
        ACCOUNT_ID,
        "rp1",
        json!({ "id": "rp1", "chatId": chat }),
    );
    threads_on(&backend);
    let list = get(&backend, &[ROOT]).await;
    assert_eq!(list[0]["unreadReplyCount"], 2, "{list:?}");
}

/// Moving `lastReadMessageId` from C to OWN_REPLY crosses only the
/// owner's reply, which is never counted, so ROOT's `unreadReplyCount`
/// stays 0 and `Message/changes` lists nothing as updated (RFC 8620
/// §5.2 `updated`: records "updated since the old state").
#[tokio::test]
async fn read_past_owner_reply_leaves_root_unchanged() {
    let backend = common::account_backend();
    let (chat, _) = fixture(&backend).await;
    seed_message_from(
        &backend,
        ACCOUNT_ID,
        OWN_REPLY,
        &chat,
        Some(ROOT),
        Some(ROOT),
    );
    threads_on(&backend);
    for last_read in [C, OWN_REPLY] {
        let since = message_state(&backend).await;
        let HandlerOutput { primary: resp, .. } = handle_position_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": { "rp1": { "lastReadMessageId": last_read } },
            }),
        )
        .await
        .expect("ReadPosition/set");
        assert!(resp["updated"].get("rp1").is_some(), "{resp}");
        let changes = message_changes(&backend, &since).await;
        let expected = if last_read == C {
            json!([ROOT])
        } else {
            json!([])
        };
        assert_eq!(changes["updated"], expected, "{last_read}: {changes}");
    }
    let list = get(&backend, &[ROOT]).await;
    assert_eq!(list[0]["unreadReplyCount"], 0, "{list:?}");
}
