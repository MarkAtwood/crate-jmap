//! Lifecycle callbacks on Chat destroy, Space destroy and SpaceBan create
//! (bd:JMAP-5mz6.4).
//!
//! Oracles, quoted from the jmap-chat-spec drafts that consume the
//! callbacks:
//!
//! - draft-atwood-jmap-chat-tasks-00.md L167: "When a Chat referenced by
//!   one or more Tasks is destroyed, the affected Tasks' `chatId` fields
//!   MUST be cleared (set to absent) by the server as part of the destroy
//!   operation."
//! - draft-atwood-jmap-vtc-00.md L421: "When a Space or channel Chat
//!   referenced by `spaceId` or `channelId` is destroyed, the server MUST
//!   clear the corresponding field to `null` on all VTCCalls that
//!   reference it."
//! - draft-atwood-jmap-vtc-00.md L1587: "When a user is banned from a
//!   Space (via SpaceBan creation) and has an active VTCParticipant record
//!   in a call bound to that Space (via `spaceId`), the server MUST set
//!   `leftAt` on that VTCParticipant record".
//! - draft-atwood-jmap-chat-filenode-00.md L199: "When a Space is
//!   destroyed via `Space/set` destroy, the server SHOULD also destroy the
//!   associated file tree and all FileNodes within it."
//!
//! Which Chats a Space destroy removes comes from
//! draft-atwood-jmap-chat-00.md L1299 (§Destroying a Space): "Cascades to
//! all channel Chats and their Messages." `removeChannels` (L1282-1283):
//! "Channel Chat ids to remove."

mod common;

use common::{ACCOUNT_ID, MemoryBackend, SPACE_ID};
use jmap_chat_server::memory::LifecycleEvent;
use jmap_chat_server::{handle_ban_set, handle_chat_set, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const OTHER_SPACE_ID: &str = "space2";

/// The owner (member entry id = accountId, draft-atwood-jmap-chat-00
/// §Terminology) holds `"manage_channels"`, which `removeChannels`
/// requires (L1283), `"ban"`, which SpaceBan/set create requires
/// (L1439), and `"manage_space"`, which a Space destroy requires
/// (33702a5 L1383: "The caller MUST hold `"manage_space"`").
fn seed_space(backend: &MemoryBackend, id: &str, channel_ids: &[&str]) {
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "name": "Test",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": channel_ids,
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{
                "id": "r-mod",
                "name": "Moderator",
                "permissions": ["manage_channels", "ban", "manage_space"],
                "position": 1
            }],
            "members": [{ "id": ACCOUNT_ID, "roleIds": ["r-mod"], "joinedAt": "2026-01-01T00:00:00Z" }]
        }),
    );
}

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

/// `SPACE_ID` holds channels `ch1` and `ch2`; `OTHER_SPACE_ID` holds
/// `other`; `dm` is a direct Chat outside any Space.
fn seeded() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_space(&backend, SPACE_ID, &["ch1", "ch2"]);
    seed_space(&backend, OTHER_SPACE_ID, &["other"]);
    for (id, space) in [
        ("ch1", SPACE_ID),
        ("ch2", SPACE_ID),
        ("other", OTHER_SPACE_ID),
    ] {
        seed_chat(
            &backend,
            id,
            json!({ "kind": "channel", "name": id, "spaceId": space }),
        );
    }
    seed_chat(
        &backend,
        "dm",
        json!({ "kind": "direct", "contactId": "c-peer" }),
    );
    backend
}

async fn space_set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_space_set(backend, &(), &std::collections::BTreeMap::new(), args, &[])
            .await
            .expect("Space/set");
    primary
}

fn chat_destroyed(id: &str) -> LifecycleEvent {
    LifecycleEvent::ChatDestroyed(Id::from(id))
}

/// tasks L167 / vtc L421: a destroyed Chat is reported once, by id.
#[tokio::test]
async fn chat_destroy_reports_the_chat_once() {
    let backend = seeded();
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "destroy": ["dm"] }),
        &[],
    )
    .await
    .expect("Chat/set");
    assert_eq!(resp["destroyed"], json!(["dm"]), "{resp}");

    assert_eq!(
        backend.take_lifecycle_events_for_test(),
        vec![chat_destroyed("dm")]
    );
}

/// A destroy that fails (unknown id, RFC 8620 §5.3 `notFound`) destroys
/// nothing, so nothing is reported.
#[tokio::test]
async fn failed_chat_destroy_reports_nothing() {
    let backend = seeded();
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "destroy": ["missing"] }),
        &[],
    )
    .await
    .expect("Chat/set");
    assert_eq!(
        resp["notDestroyed"]["missing"]["type"], "notFound",
        "{resp}"
    );

    assert_eq!(backend.take_lifecycle_events_for_test(), vec![]);
}

/// vtc L421 "channel Chat ... is destroyed": a channel removed through
/// `removeChannels` is reported once, by id; the Space itself is not
/// destroyed.
#[tokio::test]
async fn remove_channels_reports_each_removed_channel_once() {
    let backend = seeded();
    let resp = space_set(
        &backend,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeChannels": ["ch1"] } }
        }),
    )
    .await;
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp}");

    assert_eq!(
        backend.take_lifecycle_events_for_test(),
        vec![chat_destroyed("ch1")]
    );
}

/// filenode L199 / vtc L421: a destroyed Space is reported once, by id,
/// after each of its channel Chats (chat L1299 "Cascades to all channel
/// Chats"), and no Chat of another Space or outside a Space is reported.
#[tokio::test]
async fn space_destroy_reports_its_channels_then_the_space() {
    let backend = seeded();
    let resp = space_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "destroy": [SPACE_ID] }),
    )
    .await;
    assert_eq!(resp["destroyed"], json!([SPACE_ID]), "{resp}");

    let mut events = backend.take_lifecycle_events_for_test();
    assert_eq!(
        events.pop(),
        Some(LifecycleEvent::SpaceDestroyed(Id::from(SPACE_ID)))
    );
    events.sort_by_key(|e| format!("{e:?}"));
    assert_eq!(events, vec![chat_destroyed("ch1"), chat_destroyed("ch2")]);
}

/// A Space destroy that fails destroys nothing, so nothing is reported.
#[tokio::test]
async fn failed_space_destroy_reports_nothing() {
    let backend = seeded();
    let resp = space_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "destroy": ["missing"] }),
    )
    .await;
    assert_eq!(
        resp["notDestroyed"]["missing"]["type"], "notFound",
        "{resp}"
    );

    assert_eq!(backend.take_lifecycle_events_for_test(), vec![]);
}

/// vtc L1587 "banned from a Space (via SpaceBan creation)": each created
/// ban is reported once, carrying the banned `userId` and the `spaceId`.
#[tokio::test]
async fn space_ban_create_reports_the_ban_once() {
    let backend = seeded();
    let HandlerOutput { primary: resp, .. } = handle_ban_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "b1": { "spaceId": SPACE_ID, "userId": "c-peer" } }
        }),
    )
    .await
    .expect("SpaceBan/set");
    let ban_id = resp["created"]["b1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("ban not created: {resp}"))
        .to_owned();

    let events = backend.take_lifecycle_events_for_test();
    let [LifecycleEvent::SpaceBanCreated(ban)] = events.as_slice() else {
        panic!("expected exactly one SpaceBanCreated event: {events:?}");
    };
    assert_eq!(ban.id.as_ref(), ban_id);
    assert_eq!(ban.space_id.as_ref(), SPACE_ID);
    assert_eq!(ban.user_id.as_ref(), "c-peer");
}
