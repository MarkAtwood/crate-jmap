//! `/query` refuses filter clauses and sort properties the draft does not
//! define (bd:JMAP-ops7.7).
//!
//! Oracle: RFC 8620 §5.5 errors — "`unsupportedSort`: The sort is
//! syntactically valid, but includes a property the server does not
//! support sorting on" and "`unsupportedFilter`: The filter is
//! syntactically valid, but the server cannot process it." Filter and
//! sort property lists: draft-atwood-jmap-chat-00 §ChatContact/query,
//! §Chat/query, §Message/query, §Space/query, §CustomEmoji/query.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::message::{handle_message_query, handle_message_query_changes};
use jmap_chat_server::{
    handle_chat_query, handle_contact_query, handle_emoji_query, handle_space_query,
};
use jmap_server::HandlerOutput;
use jmap_types::{Id, JmapError};
use serde_json::{json, Value};

type Handler = for<'a> fn(
    &'a MemoryBackend,
    &'a (),
    Value,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<HandlerOutput, JmapError>> + 'a>,
>;

/// The five `/query` handlers by method name.
fn handlers() -> [(&'static str, Handler); 5] {
    [
        ("Chat/query", |b, c, a| Box::pin(handle_chat_query(b, c, a))),
        ("Message/query", |b, c, a| {
            Box::pin(handle_message_query(b, c, a))
        }),
        ("Space/query", |b, c, a| {
            Box::pin(handle_space_query(b, c, a))
        }),
        ("ChatContact/query", |b, c, a| {
            Box::pin(handle_contact_query(b, c, a))
        }),
        ("CustomEmoji/query", |b, c, a| {
            Box::pin(handle_emoji_query(b, c, a))
        }),
    ]
}

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend
}

/// A spec filter condition for each method. `Message/query` needs
/// `chatId` (§Message/query "All requests MUST include a `chatId`
/// filter, unless ... `hasMention: true`").
fn spec_condition(method: &str) -> Value {
    match method {
        "Chat/query" => json!({ "muted": false }),
        "Message/query" => json!({ "chatId": "c1" }),
        "Space/query" => json!({ "isPublic": true }),
        "ChatContact/query" => json!({ "blocked": false }),
        "CustomEmoji/query" => json!({ "spaceId": "s1" }),
        _ => unreachable!("{method}"),
    }
}

/// A spec sort property for each method: §ChatContact/query "Sort
/// properties: `lastSeenAt`, ..."; the others' "Default sort".
fn spec_sort_property(method: &str) -> &'static str {
    match method {
        "Chat/query" => "lastMessageAt",
        "Message/query" => "receivedAt",
        "Space/query" | "CustomEmoji/query" => "name",
        "ChatContact/query" => "lastSeenAt",
        _ => unreachable!("{method}"),
    }
}

async fn query(handler: Handler, filter: Value, sort: Value) -> Result<Value, JmapError> {
    let backend = backend();
    handler(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": filter, "sort": sort }),
    )
    .await
    .map(|HandlerOutput { primary: resp, .. }| resp)
}

/// A condition naming an undefined clause next to a spec clause is
/// `unsupportedFilter`; the spec clause alone is accepted.
#[tokio::test]
async fn unknown_filter_clause_is_unsupported_filter() {
    for (method, handler) in handlers() {
        let ok = spec_condition(method);
        let mut bad = ok.clone();
        bad["acmeUnknownClause"] = json!("x");

        query(handler, ok, json!([]))
            .await
            .unwrap_or_else(|e| panic!("{method} spec filter: {e:?}"));
        let err = query(handler, bad, json!([])).await.expect_err(method);
        assert_eq!(err.error_type.to_string(), "unsupportedFilter", "{method}");
    }
}

/// An undefined clause nested under a FilterOperator is refused too
/// (RFC 8620 §5.5 FilterOperator `conditions`).
#[tokio::test]
async fn unknown_clause_under_operator_is_unsupported_filter() {
    let handler = handlers()[2].1;
    let filter = json!({
        "operator": "OR",
        "conditions": [{ "isPublic": true }, { "acmeUnknownClause": "x" }],
    });
    let err = query(handler, filter, json!([]))
        .await
        .expect_err("Space/query");
    assert_eq!(err.error_type.to_string(), "unsupportedFilter");
}

/// RFC 8620 §5.5 FilterOperator has exactly `operator` and `conditions`.
/// A misspelt `operator` key must not decode as an empty, match-all
/// condition (jmap-types `Filter` "Malformed-operator hazard").
#[tokio::test]
async fn misspelt_operator_is_unsupported_filter() {
    for (method, handler) in handlers() {
        let filter = json!({ "opperator": "AND", "conditions": [spec_condition(method)] });
        let err = query(handler, filter, json!([])).await.expect_err(method);
        assert_eq!(err.error_type.to_string(), "unsupportedFilter", "{method}");
    }
}

/// RFC 8620 §5.5 types `filter` as "FilterOperator|FilterCondition|null"
/// and both are objects ("A *FilterCondition* is an "object""), so a
/// non-object value is "of the wrong type" (§3.6.2 `invalidArguments`),
/// not "syntactically valid" (§5.5 `unsupportedFilter`). §5.6 reuses the
/// same `filter` type for `/queryChanges`.
#[tokio::test]
async fn non_object_filter_is_invalid_arguments() {
    for filter in [json!(42), json!("x"), json!(true), json!([])] {
        for (method, handler) in handlers() {
            let err = query(handler, filter.clone(), json!([]))
                .await
                .expect_err(method);
            assert_eq!(
                err.error_type.to_string(),
                "invalidArguments",
                "{method} {filter}"
            );
        }
        let backend = backend();
        let err = handle_message_query_changes(
            &backend,
            &(),
            json!({ "accountId": ACCOUNT_ID, "filter": filter, "sinceQueryState": "0" }),
        )
        .await
        .expect_err("Message/queryChanges");
        assert_eq!(
            err.error_type.to_string(),
            "invalidArguments",
            "Message/queryChanges {filter}"
        );
    }
}

/// A sort on an undefined property is `unsupportedSort`; the spec
/// property is accepted.
#[tokio::test]
async fn unknown_sort_property_is_unsupported_sort() {
    for (method, handler) in handlers() {
        let ok = json!([{ "property": spec_sort_property(method) }]);
        let bad = json!([{ "property": "acmeUnknownProperty" }]);

        query(handler, spec_condition(method), ok)
            .await
            .unwrap_or_else(|e| panic!("{method} spec sort: {e:?}"));
        let err = query(handler, spec_condition(method), bad)
            .await
            .expect_err(method);
        assert_eq!(err.error_type.to_string(), "unsupportedSort", "{method}");
    }
}

/// draft-atwood-jmap-chat-00 §Input Validation: "`sentAt`, `editedAt`:
/// store as-is; never use for ordering or expiry."
#[tokio::test]
async fn message_sort_by_sent_at_is_unsupported_sort() {
    let handler = handlers()[1].1;
    let err = query(
        handler,
        json!({ "chatId": "c1" }),
        json!([{ "property": "sentAt" }]),
    )
    .await
    .expect_err("Message/query");
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}
