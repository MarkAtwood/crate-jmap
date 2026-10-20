//! Per-recipient `ChatPresenceEvent` projection, `project_presence`
//! (bd:JMAP-yuut.3).
//!
//! Oracle: draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent (wss:269,
//! wss:293) and draft-atwood-jmap-chat-00 §Presence Events (chat:1557),
//! at jmap-chat-spec commit 91672c9. Each test quotes the sentence it
//! enforces. Input frames are the wss:281-289 example; expected frames
//! are hand-written JSON derived only from the draft text.

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::project_presence;
use jmap_chat_types::ChatPresenceEvent;
use jmap_types::Id;
use serde_json::{json, Value};

const RECIPIENT: &str = "acct-owner";
const OTHER_ACCOUNT: &str = "acct-other";
const CONTACT: &str = "c8xq2m";

/// The contact's ChatContact as held on `account`'s contact list.
fn contact(blocked: bool) -> Value {
    json!({
        "id": CONTACT,
        "identity": "bob@example.net",
        "login": "bob",
        "displayName": "Bob",
        "blocked": blocked,
        "firstSeenAt": "2026-10-01T00:00:00Z",
        "lastSeenAt": "2026-10-19T00:00:00Z",
    })
}

/// A backend whose `account` lists the contact with `blocked`.
fn backend_with(account: &str, blocked: bool) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(RECIPIENT));
    backend.register_account(&Id::from(OTHER_ACCOUNT));
    backend.insert_object_for_test("ChatContact", account, CONTACT, contact(blocked));
    backend
}

/// The wss:286-296 example frame (spec 6bcd9d8), addressed to the
/// recipient's account, with `presence` set.
fn frame(presence: &str) -> Value {
    json!({
        "@type": "ChatPresenceEvent",
        "accountId": RECIPIENT,
        "contactId": CONTACT,
        "presence": presence,
        "lastActiveAt": "2026-04-26T12:00:00Z",
        "statusText": "In a meeting",
        "statusEmoji": null,
    })
}

fn event(presence: &str) -> ChatPresenceEvent {
    serde_json::from_value(frame(presence)).expect("valid ChatPresenceEvent")
}

async fn project(backend: &MemoryBackend, event: &ChatPresenceEvent) -> Option<Value> {
    project_presence(backend, &(), &Id::from(RECIPIENT), event)
        .await
        .expect("project_presence")
        .map(|e| {
            let mut v = serde_json::to_value(e).expect("serialize");
            v["@type"] = json!("ChatPresenceEvent");
            v
        })
}

/// draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent (wss:269): "When the
/// owner's `PresenceStatus.presence` is `"invisible"`, the server MUST
/// deliver `"offline"` in outbound `ChatPresenceEvent` frames. The
/// `"invisible"` value MUST NOT appear in events delivered to other
/// users". Only `presence` changes; the other fields are the example's.
#[tokio::test]
async fn invisible_is_delivered_as_offline() {
    let backend = backend_with(RECIPIENT, false);
    let expected = json!({
        "@type": "ChatPresenceEvent",
        "accountId": "acct-owner",
        "contactId": "c8xq2m",
        "presence": "offline",
        "lastActiveAt": "2026-04-26T12:00:00Z",
        "statusText": "In a meeting",
        "statusEmoji": null,
    });
    assert_eq!(project(&backend, &event("invisible")).await, Some(expected));
}

/// draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent (wss:267): `presence`
/// is "The updated presence state: `"online"`, `"away"`, `"busy"`,
/// `"invisible"`, or `"offline"`." Only `"invisible"` is rewritten
/// (wss:269); the other states reach an unblocked recipient as sent.
#[tokio::test]
async fn visible_states_are_delivered_unchanged() {
    let backend = backend_with(RECIPIENT, false);
    for state in ["online", "away", "busy", "offline"] {
        assert_eq!(
            project(&backend, &event(state)).await,
            Some(frame(state)),
            "{state}"
        );
    }
}

/// draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent (wss:293): "Servers
/// MUST NOT deliver `ChatPresenceEvent` for a contact whose `blocked`
/// field is `true` on the owner's ChatContact record for that contact."
/// draft-atwood-jmap-chat-00 §Presence Events (chat:1557): "the server
/// MUST silently drop it if the identified ChatContact (`contactId`) has
/// `blocked: true` on the receiving owner's contact list."
#[tokio::test]
async fn blocked_contact_is_dropped() {
    let backend = backend_with(RECIPIENT, true);
    assert_eq!(project(&backend, &event("online")).await, None);
    assert_eq!(project(&backend, &event("invisible")).await, None);
}

/// chat:1557 reads `blocked` "on the receiving owner's contact list". A
/// block on another account's list does not drop the recipient's event.
#[tokio::test]
async fn block_on_another_accounts_list_does_not_drop() {
    let backend = backend_with(OTHER_ACCOUNT, true);
    assert_eq!(project(&backend, &event("away")).await, Some(frame("away")));
}

/// draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent (wss:266-267, spec
/// 6bcd9d8): "`accountId` (String): The id of the account that contains
/// the ChatContact." An event about another account's contact of the same
/// id is not the recipient's to receive.
#[tokio::test]
async fn event_for_another_account_is_dropped() {
    let backend = backend_with(RECIPIENT, false);
    let mut e = event("online");
    e.account_id = Id::from(OTHER_ACCOUNT);
    assert_eq!(project(&backend, &e).await, None);
}
