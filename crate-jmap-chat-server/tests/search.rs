//! JMAP Search `where` on `Message/query` and `Message/queryChanges` in
//! the in-memory backend opted in with `MemoryBackend::with_search_capability`.
//!
//! Oracle: crate-jmap docs/specs/jmap-search.md (draft-atwood-jmap-search-00),
//! cited by section, and draft-atwood-jmap-chat-00 §Message/query for the
//! `chatId` / `hasMention` rule; expected ids are worked out by hand from
//! the Messages created below.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, MemoryBackend, seed_group_chat};
use jmap_chat_server::message::{
    handle_message_query, handle_message_query_changes, handle_message_set,
};
use jmap_search::{DataTypeSearchInfo, JMAP_SEARCH_URI, SearchCapability};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

/// §Examples "Chat Messages with a body type", the request copied
/// byte-for-byte.
const BODY_TYPE: &str = r#"["Message/query", {
  "accountId": "A1",
  "filter": {
    "chatId": "C1",
    "where": {
      "$or": [
        { "/bodyType": "text/markdown" },
        { "/reactions": { "$ne": {} } }
      ]
    }
  }
}, "0"]"#;

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:chat".to_owned(),
        JMAP_SEARCH_URI.to_owned(),
    ]
}

fn message_search(max_depth: u64, supports_text: bool) -> SearchCapability {
    SearchCapability::new(max_depth)
        .with_data_type("Message", DataTypeSearchInfo::new(supports_text))
}

/// A backend with Chat `C1` holding three Messages: plain, Markdown, and
/// plain with a reaction. Returns their ids in that order.
async fn setup(cap: Option<SearchCapability>) -> (MemoryBackend<()>, [String; 3]) {
    let mut backend = MemoryBackend::<()>::default();
    if let Some(cap) = cap {
        backend = backend.with_search_capability(cap);
    }
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_group_chat(&backend, "C1");
    let resp = handle_message_set(
        &backend,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "create": {
            "plain": { "chatId": "C1", "body": "Hello there", "bodyType": "text/plain",
                       "sentAt": "2026-01-01T00:00:00Z" },
            "md": { "chatId": "C1", "body": "*Release* notes", "bodyType": "text/markdown",
                    "sentAt": "2026-01-01T00:00:01Z" },
            "liked": { "chatId": "C1", "body": "Lunch?", "bodyType": "text/plain",
                       "sentAt": "2026-01-01T00:00:02Z" }
        } }),
        &using(),
    )
    .await
    .expect("Message/set");
    let id = |k: &str| {
        resp.primary["created"][k]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{}", resp.primary))
            .to_owned()
    };
    let ids = [id("plain"), id("md"), id("liked")];
    let resp = handle_message_set(
        &backend,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": {
            &ids[2]: { "reactions/r1": { "emoji": "x", "senderId": ACCOUNT_ID,
                                         "sentAt": "2026-01-01T00:00:03Z" } }
        } }),
        &using(),
    )
    .await
    .expect("Message/set update");
    assert!(
        resp.primary["updated"].get(&ids[2]).is_some(),
        "{}",
        resp.primary
    );
    (backend, ids)
}

async fn query(b: &MemoryBackend<()>, filter: Value, using: &[String]) -> Result<Value, JmapError> {
    handle_message_query(
        b,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": filter }),
        using,
    )
    .await
    .map(|o| o.primary)
}

fn ids(resp: &Value) -> Vec<String> {
    let mut v: Vec<String> = resp["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    v.sort();
    v
}

fn sorted<const N: usize>(v: [&String; N]) -> Vec<String> {
    let mut v: Vec<String> = v.into_iter().cloned().collect();
    v.sort();
    v
}

fn error_type(e: JmapError) -> String {
    e.error_type.to_string()
}

/// §Examples "Chat Messages with a body type": Markdown or reacted
/// Messages; the example's `accountId` is replaced by this backend's.
#[tokio::test]
async fn body_type_example() {
    let (b, [_, md, liked]) = setup(Some(message_search(4, false))).await;
    let v: Value = serde_json::from_str(BODY_TYPE).unwrap();
    let resp = query(&b, v[1]["filter"].clone(), &using()).await.unwrap();
    assert_eq!(ids(&resp), sorted([&md, &liked]));
    // §Query Changes: "SHOULD return `canCalculateChanges: false`".
    assert_eq!(resp["canCalculateChanges"], json!(false));
}

/// §Opting In a Data Type; §Errors: `unsupportedFilter` without the
/// capability, without `Message` in `dataTypes`, or without the URI in
/// `using`.
#[tokio::test]
async fn where_needs_capability_and_using() {
    let filter = json!({ "chatId": "C1", "where": {} });
    let (b, _) = setup(None).await;
    assert_eq!(
        error_type(query(&b, filter.clone(), &using()).await.unwrap_err()),
        "unsupportedFilter"
    );
    let other = SearchCapability::new(4).with_data_type("Email", DataTypeSearchInfo::new(true));
    let (b, _) = setup(Some(other)).await;
    assert_eq!(
        error_type(query(&b, filter.clone(), &using()).await.unwrap_err()),
        "unsupportedFilter"
    );
    let (b, all) = setup(Some(message_search(4, false))).await;
    let chat_only = vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:chat".to_owned(),
    ];
    assert_eq!(
        error_type(query(&b, filter.clone(), &chat_only).await.unwrap_err()),
        "unsupportedFilter"
    );
    let resp = query(&b, filter, &using()).await.unwrap();
    assert_eq!(ids(&resp), sorted([&all[0], &all[1], &all[2]]));
}

/// §The where FilterCondition Property: "A type's own restrictions on the
/// shape of its filter still apply: for example, {{JMAP-CHAT}} requires a
/// `Message/query` filter to be a single FilterCondition that includes
/// `chatId` or `hasMention: true`, and `where` does not lift that
/// requirement."
#[tokio::test]
async fn where_does_not_lift_the_chat_id_rule() {
    let (b, _) = setup(Some(message_search(4, false))).await;
    let e = query(&b, json!({ "where": {} }), &using())
        .await
        .unwrap_err();
    assert_eq!(error_type(e), "unsupportedFilter");
}

/// §Errors and §Full-Text Matching: `$text` is `unsupportedFilter` unless
/// `supportsText`; this backend matches it in `body`, ignoring case.
/// Depth past `maxDepth` is `queryTooComplex`; malformed is
/// `invalidArguments`.
#[tokio::test]
async fn text_depth_and_malformed() {
    let text = json!({ "chatId": "C1", "where": { "$text": "RELEASE" } });
    let (b, _) = setup(Some(message_search(1, false))).await;
    assert_eq!(
        error_type(query(&b, text.clone(), &using()).await.unwrap_err()),
        "unsupportedFilter"
    );
    let deep = json!({ "chatId": "C1", "where": { "$not": { "$not": {} } } });
    assert_eq!(
        error_type(query(&b, deep, &using()).await.unwrap_err()),
        "queryTooComplex"
    );
    let bad = json!({ "chatId": "C1", "where": { "body": "x" } });
    assert_eq!(
        error_type(query(&b, bad, &using()).await.unwrap_err()),
        "invalidArguments"
    );

    let (b, [_, md, _]) = setup(Some(message_search(4, true))).await;
    assert_eq!(ids(&query(&b, text, &using()).await.unwrap()), [md]);
}

/// §Collation: `$contains` under `i;unicode-casemap` ignores case; without
/// `$collation` it compares code points.
#[tokio::test]
async fn collation_on_body() {
    let (b, [plain, ..]) = setup(Some(message_search(4, false))).await;
    let q = |w: Value| json!({ "chatId": "C1", "where": w });
    let folded = q(json!({ "/body": { "$contains": "HELLO", "$collation": "i;unicode-casemap" } }));
    assert_eq!(ids(&query(&b, folded, &using()).await.unwrap()), [plain]);
    let exact = q(json!({ "/body": { "$contains": "HELLO" } }));
    assert!(ids(&query(&b, exact, &using()).await.unwrap()).is_empty());
}

/// §Query Changes: "Servers MAY return `cannotCalculateChanges` for any
/// `/queryChanges` call whose filter uses `where`".
#[tokio::test]
async fn query_changes_with_where() {
    let (b, _) = setup(Some(message_search(4, false))).await;
    let e = handle_message_query_changes(
        &b,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceQueryState": "0",
                "filter": { "chatId": "C1", "where": {} } }),
        &using(),
    )
    .await
    .unwrap_err();
    assert_eq!(error_type(e), "cannotCalculateChanges");
}
