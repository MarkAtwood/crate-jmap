//! The reference `MemoryBackend` applies every `/query` filter clause the
//! draft defines, FilterOperators included, and `/queryChanges` applies
//! the same filter (bd:JMAP-vwr2).
//!
//! Oracles: draft-atwood-jmap-chat-00 (jmap-chat-spec 33702a5):
//! - L1064 §ChatContact/query: "Filter properties: `blocked` (Boolean),
//!   `presence` (String), `identity` (String; ...)."
//! - L1139 §Chat/query: "Filter properties: `kind` (String), `muted`
//!   (Boolean)."
//! - L1279 §Message/query `text`: "Servers that do not support full-text
//!   search MUST return `unsupportedFilter`."
//! - L1285 `topicId`: "Return only messages whose `topicId` equals this
//!   value."
//! - L1288 `hasAttachment`: "Filter to messages with or without
//!   attachments."
//! - L1291 `hasMention`: "Filter to messages that mention the owner: the
//!   accountId appears in `mentions`, OR ..."
//! - L1389 §Space/query: "Filter properties: `name` (String, substring
//!   match), `isPublic` (Boolean)."
//! - L1482 §CustomEmoji/query: "Filter properties: `spaceId` (String,
//!   optional — if absent returns all emoji accessible to the account
//!   including server-global)."
//! - RFC 8620 §5.5 FilterOperator: "AND: All of the conditions must match
//!   for the filter to match. OR: At least one of the conditions must
//!   match for the filter to match. NOT: None of the conditions must match
//!   for the filter to match."
//! - RFC 8620 §5.6: `/queryChanges` takes the "filter" "as given in the
//!   query", and `added` lists every id "added to the results since the
//!   old state".
//!
//! Objects are seeded as hand-written JSON.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{
    handle_chat_query, handle_contact_query, handle_emoji_query, handle_emoji_query_changes,
    handle_emoji_set, handle_message_query, handle_message_query_changes, handle_space_query,
};
use jmap_server::HandlerOutput;
use jmap_types::{Id, JmapError};
use serde_json::{json, Value};

fn sorted_ids(resp: &Value) -> Vec<String> {
    let mut ids: Vec<String> = resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|id| id.as_str().expect("id").to_owned())
        .collect();
    ids.sort();
    ids
}

fn seeded(type_name: &'static str, objects: &[(&str, Value)]) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    for (id, obj) in objects {
        let mut obj = obj.clone();
        obj["id"] = json!(id);
        backend.insert_object_for_test(type_name, ACCOUNT_ID, id, obj);
    }
    backend
}

fn chat_backend() -> MemoryBackend {
    seeded(
        "Chat",
        &[
            ("g-muted", json!({ "kind": "group", "muted": true })),
            ("g-loud", json!({ "kind": "group", "muted": false })),
            ("d-loud", json!({ "kind": "direct", "muted": false })),
        ],
    )
}

async fn chat_query(backend: &MemoryBackend, filter: Value) -> Vec<String> {
    let HandlerOutput { primary: resp, .. } = handle_chat_query(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": filter }),
    )
    .await
    .expect("Chat/query");
    sorted_ids(&resp)
}

/// L1139: `kind` and `muted` each restrict the result; together both must
/// hold.
#[tokio::test]
async fn chat_query_filters_kind_and_muted() {
    let backend = chat_backend();
    assert_eq!(
        chat_query(&backend, json!({ "kind": "group" })).await,
        ["g-loud", "g-muted"]
    );
    assert_eq!(
        chat_query(&backend, json!({ "muted": true })).await,
        ["g-muted"]
    );
    assert_eq!(
        chat_query(&backend, json!({ "kind": "group", "muted": false })).await,
        ["g-loud"]
    );
}

/// RFC 8620 §5.5 OR and NOT over §Chat/query conditions.
#[tokio::test]
async fn chat_query_applies_filter_operators() {
    let backend = chat_backend();
    let or = json!({ "operator": "OR", "conditions": [
        { "kind": "direct" }, { "muted": true },
    ] });
    assert_eq!(chat_query(&backend, or).await, ["d-loud", "g-muted"]);
    let not = json!({ "operator": "NOT", "conditions": [{ "kind": "direct" }] });
    assert_eq!(chat_query(&backend, not).await, ["g-loud", "g-muted"]);
    let and = json!({ "operator": "AND", "conditions": [
        { "kind": "group" }, { "muted": false },
    ] });
    assert_eq!(chat_query(&backend, and).await, ["g-loud"]);
}

/// L1064: `blocked` and `presence` each restrict `ChatContact/query`.
#[tokio::test]
async fn contact_query_filters_blocked_and_presence() {
    let backend = seeded(
        "ChatContact",
        &[
            (
                "blocked-online",
                json!({ "identity": "a", "blocked": true, "presence": "online" }),
            ),
            (
                "open-online",
                json!({ "identity": "b", "blocked": false, "presence": "online" }),
            ),
            (
                "open-away",
                json!({ "identity": "c", "blocked": false, "presence": "away" }),
            ),
        ],
    );
    let query = |filter: Value| {
        let backend = &backend;
        async move {
            let HandlerOutput { primary: resp, .. } = handle_contact_query(
                backend,
                &(),
                json!({ "accountId": ACCOUNT_ID, "filter": filter }),
            )
            .await
            .expect("ChatContact/query");
            sorted_ids(&resp)
        }
    };
    assert_eq!(
        query(json!({ "blocked": false })).await,
        ["open-away", "open-online"]
    );
    assert_eq!(
        query(json!({ "presence": "online" })).await,
        ["blocked-online", "open-online"]
    );
    assert_eq!(
        query(json!({ "blocked": false, "presence": "online" })).await,
        ["open-online"]
    );
}

fn message_backend() -> MemoryBackend {
    let msg = |extra: Value| {
        let mut m = json!({ "chatId": "chat1", "receivedAt": "2026-01-01T00:00:00Z" });
        for (k, v) in extra.as_object().expect("object") {
            m[k] = v.clone();
        }
        m
    };
    let backend = seeded(
        "Message",
        &[
            (
                "topic-a",
                msg(json!({ "topicId": "a", "subject": "Topic A" })),
            ),
            (
                "topic-b",
                msg(json!({ "topicId": "b", "subject": "Topic B" })),
            ),
            (
                "with-file",
                msg(json!({ "attachments": [{ "blobId": "b1", "type": "text/plain" }] })),
            ),
            (
                "mentions-me",
                msg(json!({ "mentions": [{ "id": ACCOUNT_ID }] })),
            ),
        ],
    );
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "chat1",
        json!({ "id": "chat1", "kind": "group" }),
    );
    backend
}

async fn message_query(backend: &MemoryBackend, extra: Value) -> Vec<String> {
    let mut filter = json!({ "chatId": "chat1" });
    for (k, v) in extra.as_object().expect("object") {
        filter[k] = v.clone();
    }
    let HandlerOutput { primary: resp, .. } = handle_message_query(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": filter }),
    )
    .await
    .expect("Message/query");
    sorted_ids(&resp)
}

/// L1285: `topicId` matches equal values only.
#[tokio::test]
async fn message_query_filters_topic_id() {
    let backend = message_backend();
    assert_eq!(
        message_query(&backend, json!({ "topicId": "a" })).await,
        ["topic-a"]
    );
}

/// L1288: `hasAttachment` selects messages "with or without attachments".
#[tokio::test]
async fn message_query_filters_has_attachment_both_ways() {
    let backend = message_backend();
    assert_eq!(
        message_query(&backend, json!({ "hasAttachment": true })).await,
        ["with-file"]
    );
    assert_eq!(
        message_query(&backend, json!({ "hasAttachment": false })).await,
        ["mentions-me", "topic-a", "topic-b"]
    );
}

/// L1291: `hasMention` is a Boolean filter, so `false` selects the
/// messages that do not mention the owner.
#[tokio::test]
async fn message_query_has_mention_false_excludes_mentions() {
    let backend = message_backend();
    assert_eq!(
        message_query(&backend, json!({ "hasMention": false })).await,
        ["topic-a", "topic-b", "with-file"]
    );
}

/// L1279 and RFC 8620 §5.6 (the filter "as given in the query"): a
/// `text` filter the server cannot run is `unsupportedFilter` on
/// `Message/queryChanges` as on `Message/query`.
#[tokio::test]
async fn message_query_changes_refuses_text_filter() {
    let backend = message_backend();
    let err = handle_message_query_changes(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": { "chatId": "chat1", "text": "hello" },
            "sinceQueryState": "0",
        }),
    )
    .await
    .expect_err("text filter");
    assert_eq!(err, JmapError::unsupported_filter());
}

/// L1269: "Servers MUST return an `unsupportedFilter` error for any
/// request that omits `chatId` without also including `hasMention:
/// true`" applies to `Message/queryChanges` too.
#[tokio::test]
async fn message_query_changes_requires_chat_id_or_has_mention() {
    let backend = message_backend();
    let err = handle_message_query_changes(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": { "topicId": "a" },
            "sinceQueryState": "0",
        }),
    )
    .await
    .expect_err("no chatId");
    assert_eq!(err, JmapError::unsupported_filter());
}

/// L1389: `name` is a substring match.
#[tokio::test]
async fn space_query_filters_name_substring() {
    let member = json!([{ "id": ACCOUNT_ID }]);
    let backend = seeded(
        "Space",
        &[
            ("rust", json!({ "name": "rustaceans", "members": member })),
            ("go", json!({ "name": "gophers", "members": member })),
        ],
    );
    let HandlerOutput { primary: resp, .. } = handle_space_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "name": "stace" } }),
    )
    .await
    .expect("Space/query");
    assert_eq!(sorted_ids(&resp), ["rust"]);
}

async fn emoji_set(backend: &MemoryBackend, create: Value) -> Value {
    handle_emoji_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": create }),
    )
    .await
    .expect("CustomEmoji/set")
    .primary
}

/// L1482: `spaceId` restricts the result to that Space's emoji; absent,
/// server-global emoji are included.
#[tokio::test]
async fn emoji_query_filters_space_id() {
    let backend = MemoryBackend::new();
    let resp = emoji_set(
        &backend,
        json!({
            "global": { "name": "wave", "blobId": "b1" },
            "local": { "name": "crab", "blobId": "b2", "spaceId": "space1" },
        }),
    )
    .await;
    let local = resp["created"]["local"]["id"].as_str().expect("id");
    let global = resp["created"]["global"]["id"].as_str().expect("id");

    let query = |filter: Value| {
        let backend = &backend;
        async move {
            let HandlerOutput { primary: resp, .. } = handle_emoji_query(
                backend,
                &(),
                json!({ "accountId": ACCOUNT_ID, "filter": filter }),
            )
            .await
            .expect("CustomEmoji/query");
            sorted_ids(&resp)
        }
    };
    assert_eq!(query(json!({ "spaceId": "space1" })).await, [local]);
    let mut all = vec![local.to_owned(), global.to_owned()];
    all.sort();
    assert_eq!(query(json!({})).await, all);
}

/// RFC 8620 §5.6: `CustomEmoji/queryChanges` with the query's `spaceId`
/// filter adds only emoji that the query matches.
#[tokio::test]
async fn emoji_query_changes_applies_space_id_filter() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let filter = json!({ "spaceId": "space1" });
    let HandlerOutput {
        primary: before, ..
    } = handle_emoji_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": filter }),
    )
    .await
    .expect("CustomEmoji/query");

    let resp = emoji_set(
        &backend,
        json!({
            "global": { "name": "wave", "blobId": "b1" },
            "local": { "name": "crab", "blobId": "b2", "spaceId": "space1" },
        }),
    )
    .await;
    let local = resp["created"]["local"]["id"].as_str().expect("id");

    let HandlerOutput {
        primary: changes, ..
    } = handle_emoji_query_changes(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": filter,
            "sinceQueryState": before["queryState"],
        }),
    )
    .await
    .expect("CustomEmoji/queryChanges");
    assert_eq!(changes["added"], json!([{ "id": local, "index": 0 }]));
}
