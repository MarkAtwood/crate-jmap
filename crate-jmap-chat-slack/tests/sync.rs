//! Initial sync: users.list, conversations.list, conversations.history and
//! conversations.replies into the JMAP account.
//!
//! Oracles: the Slack reference pages cited in `common` for the fixtures;
//! draft-atwood-jmap-chat-00 §Space (`members`, `uncategorizedChannelIds`,
//! `memberCount`), §Chat (`kind`, `contactId`, `members`, `spaceId`),
//! §Message (`senderId` "The accountId for owner-composed messages";
//! `reactions` keyed by `senderReactionId`), §Thread Model ("A message
//! replying to a thread root ...: set `threadRootId` to the value of
//! `replyTo`").

mod common;

use common::{
    ACCOUNT, OWNER, chat_named, chats, client, dummy_token, messages, mount_workspace, space,
};
use jmap_chat_server::ChatSettings as _;
use jmap_chat_slack::{BridgeConfig, BridgeError, SlackBridge};
use jmap_chat_types::{BodyType, ChatContact, ChatKind};
use jmap_server::JmapReads;
use jmap_types::Id;
use serde_json::Value;
use wiremock::MockServer;

#[tokio::test]
async fn initial_sync_maps_the_workspace() {
    let server = MockServer::start().await;
    mount_workspace(&server).await;
    let bridge = SlackBridge::connect(
        client(&server, &dummy_token("xoxp-")),
        BridgeConfig::new(Id::from(ACCOUNT)),
    )
    .await
    .unwrap();
    let report = bridge.initial_sync().await.unwrap();
    assert_eq!(
        report.contacts, 2,
        "alice and bob; the owner is the account"
    );
    assert_eq!(report.conversations, 3);
    // first, hello, reply, second (C1) and hi grace (D1); channel_join skipped.
    assert_eq!(report.messages, 5);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].channel, "G1");
    assert_eq!(report.skipped[0].error, "not_in_channel");

    let account = Id::from(ACCOUNT);
    let (contacts, _) = bridge
        .memory()
        .get_objects::<ChatContact>(&(), &account, None, None)
        .await
        .unwrap();
    let alice = contacts.iter().find(|c| c.login == "Alice").unwrap();
    assert_eq!(alice.identity, "slack:T12345678:U1");

    // Space: the workspace, owner plus both users, #general listed.
    let space = space(&bridge).await;
    assert_eq!(space.name, "Subarachnoid Workspace");
    assert_eq!(space.member_count, 3);
    assert!(space.members.iter().any(|m| m.id == account));
    let general = chat_named(&bridge, "general").await;
    assert_eq!(general.kind, ChatKind::Channel);
    assert_eq!(general.space_id.as_ref(), Some(&space.id));
    assert_eq!(general.topic.as_deref(), Some("Company-wide announcements"));
    assert_eq!(space.uncategorized_channel_ids, vec![general.id.clone()]);

    let all = chats(&bridge).await;
    let dm = all.iter().find(|c| c.kind == ChatKind::Direct).unwrap();
    assert_eq!(dm.contact_id.as_ref(), Some(&alice.id));
    let group = all.iter().find(|c| c.kind == ChatKind::Group).unwrap();
    assert_eq!(group.members.as_ref().map(Vec::len), Some(3));
    assert_eq!(group.name.as_deref(), Some("Alice, Bob"));

    let msgs = messages(&bridge).await;
    let in_general: Vec<_> = msgs.iter().filter(|m| m.chat_id == general.id).collect();
    let bodies: Vec<&str> = in_general.iter().map(|m| m.body.as_str()).collect();
    // ULIDs minted oldest first follow Slack's order.
    assert_eq!(bodies[0], "first");
    assert!(bodies[1].contains("hello"), "{bodies:?}");
    assert_eq!(bodies[2], "reply");
    assert!(bodies[3].contains("second"), "{bodies:?}");

    let first = in_general[0];
    assert_eq!(first.sender_id, account, "the owner's message");
    let hello = in_general[1];
    assert_eq!(hello.body_type, BodyType::Rich);
    let spans: Value = serde_json::from_str(&hello.body).unwrap();
    assert_eq!(spans["spans"][0]["type"], "bold");
    assert_eq!(hello.sent_at.to_string(), "2017-11-30T23:52:30Z");
    assert_eq!(hello.reactions.len(), 2);
    let senders: Vec<&Id> = hello.reactions.values().map(|r| &r.sender_id).collect();
    assert!(senders.contains(&&account));
    assert!(hello.reactions.values().all(|r| r.emoji == "\u{1F44D}"));

    let reply = in_general[2];
    assert_eq!(reply.thread_root_id.as_ref(), Some(&hello.id));
    assert_eq!(reply.reply_to.as_ref(), Some(&hello.id));

    let second = in_general[3];
    assert_eq!(second.mentions.len(), 1);
    assert_eq!(second.mentions[0].id, alice.id);

    // The owner's identity is the ownerUserId.
    assert_eq!(
        bridge
            .owner_user_id(&(), &account)
            .await
            .unwrap()
            .as_deref(),
        Some(format!("slack:T12345678:{OWNER}").as_str())
    );
}

#[tokio::test]
async fn a_second_sync_reconciles_instead_of_duplicating() {
    let server = MockServer::start().await;
    let bridge = common::synced_bridge(&server).await;
    let before = messages(&bridge).await;
    let report = bridge.initial_sync().await.unwrap();
    assert_eq!(report.contacts, 0);
    assert_eq!(report.conversations, 0);
    assert_eq!(report.messages, 0);
    let after = messages(&bridge).await;
    assert_eq!(before.len(), after.len());
    assert_eq!(chats(&bridge).await.len(), 3);
}

#[tokio::test]
async fn an_app_level_token_cannot_run_the_bridge() {
    let server = MockServer::start().await;
    let err = SlackBridge::connect(
        client(&server, &dummy_token("xapp-")),
        BridgeConfig::new(Id::from(ACCOUNT)),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, BridgeError::AppToken), "{err:?}");
}

#[tokio::test]
async fn a_refused_token_fails_connect() {
    let server = MockServer::start().await;
    common::on(&server, "auth.test", common::slack_error("invalid_auth")).await;
    let err = SlackBridge::connect(
        client(&server, &dummy_token("xoxb-")),
        BridgeConfig::new(Id::from(ACCOUNT)),
    )
    .await
    .unwrap_err();
    let BridgeError::Slack(e) = err else {
        panic!("{err:?}");
    };
    assert_eq!(e.code(), Some("invalid_auth"));
}
