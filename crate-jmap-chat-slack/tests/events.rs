//! Live Events API events applied to a synced bridge.
//!
//! Oracles: the example payloads on the Slack event reference pages
//! (`message`, `message_changed`, `message_deleted`, `reaction_added`,
//! `reaction_removed`, `channel_created`, `channel_rename`,
//! `channel_archive`, `member_joined_channel`, `member_left_channel`,
//! under <https://docs.slack.dev/reference/events/>), with their ids
//! renamed to the fixture workspace; draft-atwood-jmap-chat-00 §Message
//! (`deletedAt`: "When set, `body` is empty"), §Reaction, §Chat.

mod common;

use common::{
    ACCOUNT, OWNER, chat_named, chats, message_with_body, messages, ok, on, space, synced_bridge,
    user,
};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_slack::EventOutcome;
use jmap_chat_types::ChatKind;
use serde_json::json;
use std::collections::BTreeMap;
use wiremock::MockServer;

#[tokio::test]
async fn new_message_then_its_duplicate() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    let n = messages(&bridge).await.len();
    let event = json!({
        "type": "message", "channel": "C1", "user": "U1",
        "text": "Hello world", "ts": "1355517523.000005",
    });
    assert_eq!(
        bridge.apply_event(&event).await.unwrap(),
        EventOutcome::Applied
    );
    assert_eq!(
        bridge.apply_event(&event).await.unwrap(),
        EventOutcome::Unchanged
    );
    assert_eq!(messages(&bridge).await.len(), n + 1);
    let m = message_with_body(&bridge, "Hello world").await;
    assert_eq!(m.sent_at.to_string(), "2012-12-14T20:38:43Z");
}

#[tokio::test]
async fn thread_reply_event_links_to_its_parent() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    let event = json!({
        "type": "message", "channel": "C1", "user": "U2", "text": "another reply",
        "ts": "1512200000.000001", "thread_ts": "1512085950.000216",
    });
    bridge.apply_event(&event).await.unwrap();
    let parent = message_with_body(&bridge, "hello").await;
    let reply = message_with_body(&bridge, "another reply").await;
    assert_eq!(reply.thread_root_id.as_ref(), Some(&parent.id));
}

#[tokio::test]
async fn message_changed_edits_and_message_deleted_tombstones() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    // message_changed example: the original ts inside `message`, `edited`.
    let changed = json!({
        "type": "message", "subtype": "message_changed", "hidden": true,
        "channel": "C1", "ts": "1512300000.000001",
        "message": {
            "type": "message", "user": "U2", "text": "second, _edited_",
            "ts": "1512104434.000490",
            "edited": { "user": "U2", "ts": "1512300000.000001" },
        },
    });
    assert_eq!(
        bridge.apply_event(&changed).await.unwrap(),
        EventOutcome::Applied
    );
    let m = message_with_body(&bridge, "edited").await;
    assert_eq!(
        m.edited_at.map(|d| d.to_string()).as_deref(),
        Some("2017-12-03T11:20:00Z")
    );
    assert!(m.mentions.is_empty(), "the edit dropped the mention");

    // An unfurl-style message_changed with no new edit leaves the body.
    let unfurl = json!({
        "type": "message", "subtype": "message_changed", "channel": "C1",
        "message": { "type": "message", "user": "U2", "text": "second, _edited_",
                     "ts": "1512104434.000490",
                     "edited": { "user": "U2", "ts": "1512300000.000001" } },
    });
    assert_eq!(
        bridge.apply_event(&unfurl).await.unwrap(),
        EventOutcome::Unchanged
    );

    let deleted = json!({
        "type": "message", "subtype": "message_deleted", "hidden": true,
        "channel": "C1", "ts": "1512400000.000001", "deleted_ts": "1512104434.000490",
    });
    assert_eq!(
        bridge.apply_event(&deleted).await.unwrap(),
        EventOutcome::Applied
    );
    let gone = messages(&bridge)
        .await
        .into_iter()
        .find(|x| x.id == m.id)
        .unwrap();
    assert!(gone.deleted_at.is_some());
    assert_eq!(gone.deleted_for_all, Some(true));
    assert_eq!(gone.body, "");
    assert_eq!(
        bridge.apply_event(&deleted).await.unwrap(),
        EventOutcome::Unchanged
    );
}

#[tokio::test]
async fn reactions_come_and_go() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    let added = json!({
        "type": "reaction_added", "user": "U1", "reaction": "partyparrot", "item_user": "U2",
        "item": { "type": "message", "channel": "C1", "ts": "1512104434.000490" },
        "event_ts": "1512104500.083113",
    });
    assert_eq!(
        bridge.apply_event(&added).await.unwrap(),
        EventOutcome::Applied
    );
    assert_eq!(
        bridge.apply_event(&added).await.unwrap(),
        EventOutcome::Unchanged
    );
    let m = message_with_body(&bridge, "second").await;
    let r = m.reactions.values().next().unwrap();
    assert_eq!(r.emoji, ":partyparrot:");
    assert_eq!(r.sent_at.to_string(), "2017-12-01T05:01:40.083Z");

    let mut removed = added.clone();
    removed["type"] = json!("reaction_removed");
    assert_eq!(
        bridge.apply_event(&removed).await.unwrap(),
        EventOutcome::Applied
    );
    let m = message_with_body(&bridge, "second").await;
    assert!(m.reactions.is_empty());

    // A reaction on a file is not a Message reaction.
    let file = json!({ "type": "reaction_added", "user": "U1", "reaction": "eyes",
                       "item": { "type": "file", "file": "F123ABC456" } });
    assert_eq!(
        bridge.apply_event(&file).await.unwrap(),
        EventOutcome::Ignored
    );
}

#[tokio::test]
async fn channel_lifecycle() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    let created = json!({ "type": "channel_created",
        "channel": { "id": "C024BE91L", "name": "fun", "created": 1360782804, "creator": "U1" } });
    assert_eq!(
        bridge.apply_event(&created).await.unwrap(),
        EventOutcome::Applied
    );
    let fun = chat_named(&bridge, "fun").await;
    assert_eq!(fun.kind, ChatKind::Channel);
    assert!(
        space(&bridge)
            .await
            .uncategorized_channel_ids
            .contains(&fun.id)
    );

    let renamed = json!({ "type": "channel_rename",
        "channel": { "id": "C024BE91L", "name": "new_name", "created": 1360782804 } });
    assert_eq!(
        bridge.apply_event(&renamed).await.unwrap(),
        EventOutcome::Applied
    );
    assert_eq!(chat_named(&bridge, "new_name").await.id, fun.id);

    // Archived: Message/set create is refused with forbidden.
    let archived = json!({ "type": "channel_archive", "channel": "C024BE91L", "user": "U1" });
    assert_eq!(
        bridge.apply_event(&archived).await.unwrap(),
        EventOutcome::Applied
    );
    let out = handle_message_set(
        &bridge,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT, "create": { "m": {
            "chatId": fun.id, "body": "hi", "bodyType": "text/plain",
            "sentAt": "2026-01-01T00:00:00Z" } } }),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        out.primary["notCreated"]["m"]["type"], "forbidden",
        "{}",
        out.primary
    );
    let unarchived = json!({ "type": "channel_unarchive", "channel": "C024BE91L", "user": "U1" });
    assert_eq!(
        bridge.apply_event(&unarchived).await.unwrap(),
        EventOutcome::Applied
    );

    let deleted = json!({ "type": "channel_deleted", "channel": "C024BE91L" });
    assert_eq!(
        bridge.apply_event(&deleted).await.unwrap(),
        EventOutcome::Applied
    );
    assert!(chats(&bridge).await.iter().all(|c| c.id != fun.id));
    assert!(
        !space(&bridge)
            .await
            .uncategorized_channel_ids
            .contains(&fun.id)
    );
}

#[tokio::test]
async fn members_join_and_leave() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    on(
        &server,
        "users.info",
        ok(json!({ "user": user("U3", "carol", "Carol") })),
    )
    .await;
    let joined = json!({ "type": "member_joined_channel", "user": "U3", "channel": "G1",
                         "channel_type": "G", "team": "T12345678", "inviter": "U1" });
    assert_eq!(
        bridge.apply_event(&joined).await.unwrap(),
        EventOutcome::Applied
    );
    let group = chats(&bridge)
        .await
        .into_iter()
        .find(|c| c.kind == ChatKind::Group)
        .unwrap();
    assert_eq!(group.members.as_ref().map(Vec::len), Some(4));
    assert_eq!(
        space(&bridge).await.member_count,
        4,
        "carol joined the Space too"
    );

    let left = json!({ "type": "member_left_channel", "user": "U3", "channel": "G1",
                       "channel_type": "G", "team": "T12345678" });
    assert_eq!(
        bridge.apply_event(&left).await.unwrap(),
        EventOutcome::Applied
    );
    let group = chats(&bridge)
        .await
        .into_iter()
        .find(|c| c.kind == ChatKind::Group)
        .unwrap();
    assert_eq!(group.members.as_ref().map(Vec::len), Some(3));

    // The owner leaving #general removes it and its Messages.
    let general = chat_named(&bridge, "general").await;
    let owner_left = json!({ "type": "member_left_channel", "user": OWNER, "channel": "C1",
                             "channel_type": "C", "team": "T12345678" });
    assert_eq!(
        bridge.apply_event(&owner_left).await.unwrap(),
        EventOutcome::Applied
    );
    assert!(chats(&bridge).await.iter().all(|c| c.id != general.id));
    assert!(
        messages(&bridge)
            .await
            .iter()
            .all(|m| m.chat_id != general.id)
    );
}

#[tokio::test]
async fn the_owner_joining_imports_the_conversation() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    on(
        &server,
        "conversations.info",
        ok(json!({ "channel": { "id": "C9", "name": "random", "is_channel": true, "created": 1449252889 } })),
    )
    .await;
    common::on_with(
        &server,
        "conversations.history",
        "channel=C9",
        ok(json!({ "messages": [{ "type": "message", "user": "U1", "text": "welcome", "ts": "1512500000.000001" }] })),
    )
    .await;
    let joined = json!({ "type": "member_joined_channel", "user": OWNER, "channel": "C9",
                         "channel_type": "C", "team": "T12345678" });
    assert_eq!(
        bridge.apply_event(&joined).await.unwrap(),
        EventOutcome::Applied
    );
    let random = chat_named(&bridge, "random").await;
    let welcome = message_with_body(&bridge, "welcome").await;
    assert_eq!(welcome.chat_id, random.id);
}

#[tokio::test]
async fn unmapped_events_are_ignored() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    for event in [
        json!({ "type": "app_home_opened", "user": "U1", "channel": "D1" }),
        json!({ "type": "message", "subtype": "channel_topic", "channel": "C1",
                "user": "U1", "text": "set the topic", "ts": "1512600000.000001" }),
        json!({ "type": "message", "subtype": "message_deleted", "channel": "C1",
                "deleted_ts": "1.000001" }),
    ] {
        assert_eq!(
            bridge.apply_event(&event).await.unwrap(),
            EventOutcome::Ignored,
            "{event}"
        );
    }
}
