//! JMAP writes through the jmap-chat-server handlers reach Slack, and
//! Slack's refusals surface as SetErrors.
//!
//! Oracles: the chat.postMessage, chat.update, chat.delete, reactions.add
//! and reactions.remove reference pages (arguments `channel`, `text`,
//! `thread_ts`, `mrkdwn`, `ts`, `timestamp`, `name`; error codes);
//! draft-atwood-jmap-chat-00 §Message/set (creating, editing, "Adding and
//! Removing Reactions", "Deleting a Message"); RFC 8620 §5.3 SetError types.

mod common;

use std::collections::BTreeMap;

use common::{
    ACCOUNT, calls, chat_named, form, message_with_body, messages, ok, on, on_with, slack_error,
    synced_bridge,
};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{handle_chat_set, handle_space_set};
use jmap_chat_slack::{EventOutcome, SlackBridge};
use jmap_chat_types::DeliveryState;
use serde_json::{Value, json};
use wiremock::{MockServer, ResponseTemplate};

async fn message_set(bridge: &SlackBridge, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_message_set(bridge, &(), &BTreeMap::new(), args, &[])
        .await
        .unwrap()
        .primary
}

fn create(chat_id: &str, extra: Value) -> Value {
    let mut m = json!({ "chatId": chat_id, "body": "hi", "bodyType": "text/plain",
                        "sentAt": "2026-01-01T00:00:00Z" });
    if let (Some(m), Some(extra)) = (m.as_object_mut(), extra.as_object()) {
        m.extend(extra.clone());
    }
    json!({ "create": { "m": m } })
}

fn posted(ts: &str) -> ResponseTemplate {
    ok(json!({ "channel": "C1", "ts": ts, "message": { "type": "message", "ts": ts } }))
}

#[tokio::test]
async fn a_plain_create_posts_and_its_echo_is_unchanged() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    on(&server, "chat.postMessage", posted("1600000000.000100")).await;
    let general = chat_named(&bridge, "general").await;
    let resp = message_set(
        &bridge,
        create(general.id.as_ref(), json!({ "body": "1 < 2 *literally*" })),
    )
    .await;
    let created = &resp["created"]["m"];
    assert!(created["id"].is_string(), "{resp}");
    let reqs = calls(&server, "chat.postMessage").await;
    assert_eq!(reqs.len(), 1);
    assert_eq!(form(&reqs[0], "channel").as_deref(), Some("C1"));
    assert_eq!(
        form(&reqs[0], "text").as_deref(),
        Some("1 &lt; 2 *literally*")
    );
    assert_eq!(form(&reqs[0], "mrkdwn").as_deref(), Some("false"));
    assert_eq!(form(&reqs[0], "thread_ts"), None);

    let m = message_with_body(&bridge, "literally").await;
    assert_eq!(m.delivery_state, DeliveryState::Delivered);
    assert_eq!(m.sender_id.as_ref(), ACCOUNT);
    assert!(
        bridge.memory().drain_outbound_events().len() == 1,
        "the NewMessage event is recorded"
    );

    // The Events API echo of our own post changes nothing.
    let echo = json!({ "type": "message", "channel": "C1", "user": common::OWNER,
                       "text": "1 &lt; 2 *literally*", "ts": "1600000000.000100" });
    assert_eq!(
        bridge.apply_event(&echo).await.unwrap(),
        EventOutcome::Unchanged
    );
}

#[tokio::test]
async fn rich_bodies_mentions_and_threads() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    on(&server, "chat.postMessage", posted("1600000000.000200")).await;
    let general = chat_named(&bridge, "general").await;
    let root = message_with_body(&bridge, "hello").await;
    let alice = messages(&bridge)
        .await
        .into_iter()
        .find(|m| !m.mentions.is_empty())
        .unwrap()
        .mentions[0]
        .id
        .clone();
    let body = json!({ "spans": [
        { "type": "mention", "text": "@Alice", "userId": alice },
        { "type": "text", "text": " look " },
        { "type": "bold", "text": "here" },
    ]})
    .to_string();
    let resp = message_set(
        &bridge,
        create(
            general.id.as_ref(),
            json!({ "body": body, "bodyType": "application/jmap-chat-rich",
                    "mentions": [{ "id": alice }], "replyTo": root.id, "threadRootId": root.id }),
        ),
    )
    .await;
    assert!(resp["created"]["m"].is_object(), "{resp}");
    let reqs = calls(&server, "chat.postMessage").await;
    assert_eq!(form(&reqs[0], "text").as_deref(), Some("<@U1> look *here*"));
    assert_eq!(form(&reqs[0], "mrkdwn").as_deref(), Some("true"));
    assert_eq!(
        form(&reqs[0], "thread_ts").as_deref(),
        Some("1512085950.000216")
    );
}

#[tokio::test]
async fn slack_refusals_surface_as_set_errors() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    let general = chat_named(&bridge, "general").await;
    on_with(
        &server,
        "chat.postMessage",
        "text=no",
        slack_error("not_in_channel"),
    )
    .await;
    on_with(
        &server,
        "chat.postMessage",
        "text=long",
        slack_error("msg_too_long"),
    )
    .await;
    on_with(
        &server,
        "chat.postMessage",
        "text=slow",
        ResponseTemplate::new(429).insert_header("Retry-After", "120"),
    )
    .await;
    on_with(
        &server,
        "chat.postMessage",
        "text=odd",
        slack_error("brand_new_code"),
    )
    .await;
    let before = messages(&bridge).await.len();
    for (text, expected) in [
        ("no", "forbidden"),
        ("long", "tooLarge"),
        ("slow", "rateLimit"),
        ("odd", "serverFail"),
    ] {
        let resp = message_set(
            &bridge,
            create(general.id.as_ref(), json!({ "body": text })),
        )
        .await;
        assert_eq!(resp["notCreated"]["m"]["type"], expected, "{text}: {resp}");
    }
    assert_eq!(messages(&bridge).await.len(), before, "nothing stored");
}

#[tokio::test]
async fn unrepresentable_creates_are_invalid_properties() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    let general = chat_named(&bridge, "general").await;
    let resp = message_set(
        &bridge,
        create(general.id.as_ref(), json!({ "bodyType": "text/markdown" })),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["m"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notCreated"]["m"]["properties"], json!(["bodyType"]));
    let resp = message_set(
        &bridge,
        create(
            general.id.as_ref(),
            json!({ "topicId": "t1", "subject": "Topic" }),
        ),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["m"]["properties"],
        json!(["topicId"]),
        "{resp}"
    );
    assert!(calls(&server, "chat.postMessage").await.is_empty());
}

#[tokio::test]
async fn edits_reactions_and_deletes() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    on(&server, "chat.postMessage", posted("1600000000.000300")).await;
    on(&server, "chat.update", posted("1600000000.000300")).await;
    on(
        &server,
        "chat.delete",
        ok(json!({ "channel": "C1", "ts": "1600000000.000300" })),
    )
    .await;
    on(&server, "reactions.add", ok(json!({}))).await;
    on(&server, "reactions.remove", ok(json!({}))).await;
    let general = chat_named(&bridge, "general").await;
    let resp = message_set(
        &bridge,
        create(general.id.as_ref(), json!({ "body": "draft" })),
    )
    .await;
    let id = resp["created"]["m"]["id"].as_str().unwrap().to_owned();

    // Edit: chat.update with the new text.
    let resp = message_set(&bridge, json!({ "update": { &id: { "body": "final" } } })).await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    let reqs = calls(&server, "chat.update").await;
    assert_eq!(form(&reqs[0], "ts").as_deref(), Some("1600000000.000300"));
    assert_eq!(form(&reqs[0], "text").as_deref(), Some("final"));
    // Its echo (edited, same text) leaves the body alone.
    let echo = json!({ "type": "message", "subtype": "message_changed", "channel": "C1",
        "message": { "type": "message", "user": common::OWNER, "text": "final",
                     "ts": "1600000000.000300",
                     "edited": { "user": common::OWNER, "ts": "1600000001.000000" } } });
    assert_eq!(
        bridge.apply_event(&echo).await.unwrap(),
        EventOutcome::Unchanged
    );

    // Reaction add: reactions.add with the Slack name of the emoji.
    let rid = "01JREACTION00000000000000A";
    let resp = message_set(
        &bridge,
        json!({ "update": { &id: { format!("reactions/{rid}"): { "emoji": "\u{1F44D}",
                                   "sentAt": "2026-01-01T00:00:00Z" } } } }),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    let reqs = calls(&server, "reactions.add").await;
    assert_eq!(form(&reqs[0], "name").as_deref(), Some("+1"));
    assert_eq!(
        form(&reqs[0], "timestamp").as_deref(),
        Some("1600000000.000300")
    );
    // Its echo is already known.
    let echo = json!({ "type": "reaction_added", "user": common::OWNER, "reaction": "+1",
        "item": { "type": "message", "channel": "C1", "ts": "1600000000.000300" } });
    assert_eq!(
        bridge.apply_event(&echo).await.unwrap(),
        EventOutcome::Unchanged
    );
    // An emoji with no Slack name is refused before any call.
    let resp = message_set(
        &bridge,
        json!({ "update": { &id: { "reactions/01JREACTION00000000000000B": { "emoji": "\u{1F9A9}" } } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id]["type"], "invalidProperties",
        "{resp}"
    );

    let resp = message_set(
        &bridge,
        json!({ "update": { &id: { format!("reactions/{rid}"): null } } }),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    assert_eq!(
        form(&calls(&server, "reactions.remove").await[0], "name").as_deref(),
        Some("+1")
    );

    // Delete for all: chat.delete, then a tombstone.
    let resp = message_set(
        &bridge,
        json!({ "update": { &id: { "deletedAt": "2026-01-02T00:00:00Z", "deletedForAll": true } } }),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    assert_eq!(calls(&server, "chat.delete").await.len(), 1);
    let echo = json!({ "type": "message", "subtype": "message_deleted", "channel": "C1",
                       "deleted_ts": "1600000000.000300" });
    assert_eq!(
        bridge.apply_event(&echo).await.unwrap(),
        EventOutcome::Unchanged
    );
}

#[tokio::test]
async fn destroy_deletes_in_slack() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    on(&server, "chat.delete", ok(json!({}))).await;
    let first = message_with_body(&bridge, "first").await;
    let resp = message_set(&bridge, json!({ "destroy": [first.id] })).await;
    assert_eq!(resp["destroyed"], json!([first.id]), "{resp}");
    let reqs = calls(&server, "chat.delete").await;
    assert_eq!(form(&reqs[0], "channel").as_deref(), Some("C1"));
    assert_eq!(form(&reqs[0], "ts").as_deref(), Some("1512000000.000001"));
    assert!(messages(&bridge).await.iter().all(|m| m.id != first.id));
}

#[tokio::test]
async fn a_refused_delete_keeps_the_message() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    on(&server, "chat.delete", slack_error("cant_delete_message")).await;
    let hello = message_with_body(&bridge, "hello").await;
    let resp = message_set(&bridge, json!({ "destroy": [hello.id] })).await;
    assert_eq!(
        resp["notDestroyed"][hello.id.as_ref()]["type"],
        "forbidden",
        "{resp}"
    );
    assert!(messages(&bridge).await.iter().any(|m| m.id == hello.id));
}

#[tokio::test]
async fn chat_and_space_changes_that_slack_owns_are_refused() {
    let server = MockServer::start().await;
    let bridge = synced_bridge(&server).await;
    let general = chat_named(&bridge, "general").await;
    let resp = handle_chat_set(
        &bridge,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT, "update": {
            general.id.as_ref(): { "pinnedMessageIds": [] } } }),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(
        resp["notUpdated"][general.id.as_ref()]["type"],
        "forbidden",
        "{resp}"
    );
    // A local preference is fine.
    let resp = handle_chat_set(
        &bridge,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT, "update": { general.id.as_ref(): { "muted": true } } }),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert!(resp["updated"].get(general.id.as_ref()).is_some(), "{resp}");
    // Creating a Chat is not bridged.
    let resp = handle_chat_set(
        &bridge,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT, "create": { "g": { "kind": "group", "name": "G", "memberIds": [] } } }),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(resp["notCreated"]["g"]["type"], "forbidden", "{resp}");

    let space_id = bridge.space_id().to_string();
    let resp = handle_space_set(
        &bridge,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT, "update": { &space_id: { "name": "Renamed" } } }),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(resp["notUpdated"][&space_id]["type"], "forbidden", "{resp}");
}
