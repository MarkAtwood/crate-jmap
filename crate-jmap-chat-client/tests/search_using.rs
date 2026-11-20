//! `Message/query` and `Message/queryChanges` with a JMAP Search `where`
//! carry `urn:ietf:params:jmap:search` in `using`.
//!
//! Oracle: crate-jmap docs/specs/jmap-search.md §Opting In a Data Type: a
//! data type accepts `where` "only in requests whose `using` includes
//! `urn:ietf:params:jmap:search`"; the URI is copied from §Capability, not
//! from the code under test.

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    QUERY_STATE_NEW, QUERY_STATE_OLD, TEST_ACCOUNT_ID, TEST_CHAT_ID, jmap_response, mock_jmap_post,
    recorded_body,
};
use jmap_chat_types::{MessageFilter, MessageFilterCondition};
use jmap_types::{Filter, Id, State};
use serde_json::{Value, json};
use wiremock::MockServer;

const SEARCH: &str = "urn:ietf:params:jmap:search";

fn filter(where_: Option<Value>) -> MessageFilter {
    let mut cond = MessageFilterCondition::default();
    cond.chat_id = Some(Id::from(TEST_CHAT_ID));
    cond.where_ = where_;
    Filter::Condition(cond)
}

fn query_response() -> Value {
    jmap_response(
        "Message/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": QUERY_STATE_NEW,
            "canCalculateChanges": false,
            "position": 0,
            "ids": []
        }),
    )
}

fn using_has_search(body: &Value) -> bool {
    body["using"]
        .as_array()
        .expect("using array")
        .iter()
        .any(|u| u == SEARCH)
}

#[tokio::test]
async fn message_query_with_where_adds_search() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response()).await;
    let sc = helpers::make_client(&server);
    let w = json!({ "/bodyType": "text/markdown" });
    sc.message_query(
        Some(&filter(Some(w.clone()))),
        None,
        &jmap_base_client::QueryParams::default(),
    )
    .await
    .expect("message_query must succeed");
    let body = recorded_body(&server).await;
    assert!(using_has_search(&body), "{body}");
    assert_eq!(body["methodCalls"][0][1]["filter"]["where"], w);
}

#[tokio::test]
async fn message_query_without_where_keeps_using() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response()).await;
    let sc = helpers::make_client(&server);
    sc.message_query(
        Some(&filter(None)),
        None,
        &jmap_base_client::QueryParams::default(),
    )
    .await
    .expect("message_query must succeed");
    let body = recorded_body(&server).await;
    assert!(!using_has_search(&body), "{body}");
    assert!(body["methodCalls"][0][1]["filter"].get("where").is_none());
}

#[tokio::test]
async fn message_query_changes_with_where_adds_search() {
    let server = MockServer::start().await;
    mock_jmap_post(
        &server,
        jmap_response(
            "Message/queryChanges",
            json!({
                "accountId": TEST_ACCOUNT_ID,
                "oldQueryState": QUERY_STATE_OLD,
                "newQueryState": QUERY_STATE_NEW,
                "removed": [],
                "added": []
            }),
        ),
    )
    .await;
    let sc = helpers::make_client(&server);
    sc.message_query_changes(
        &State::from(QUERY_STATE_OLD),
        None,
        Some(&filter(Some(json!({})))),
        None,
        None,
        None,
    )
    .await
    .expect("message_query_changes must succeed");
    assert!(using_has_search(&recorded_body(&server).await));
}

#[tokio::test]
async fn batch_with_where_adds_search() {
    let server = MockServer::start().await;
    mock_jmap_post(&server, query_response()).await;
    let sc = helpers::make_client(&server);
    let mut batch = sc.batch().expect("batch must start");
    batch
        .message_query_get(
            Some(&filter(Some(json!({ "$not": {} })))),
            None,
            &jmap_base_client::QueryParams::default(),
            None,
        )
        .expect("message pair must build");
    sc.send_batch(batch).await.expect("send_batch must succeed");
    assert!(using_has_search(&recorded_body(&server).await));
}
