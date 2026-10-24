//! A misspelt FilterOperator key or FilterCondition property is
//! `unsupportedFilter`, never an empty, match-all condition (bd:JMAP-6azm).
//!
//! Oracle: RFC 8620 §5.5 — a FilterOperator object has the properties
//! `operator` and `conditions`, and "`unsupportedFilter`: The filter is
//! syntactically valid, but the server cannot process it." Condition
//! property names: RFC 9670 Principal/query (`email`) and ShareNotification/query (`objectType`). Without the refusal, each bad filter below
//! would decode as a condition with every property unset, which matches
//! every record.

mod common;

use common::MemoryBackend;
use jmap_server::HandlerOutput;
use jmap_sharing_server::{
    handle_principal_query, handle_principal_query_changes, handle_share_notification_query,
    handle_share_notification_query_changes,
};
use jmap_types::JmapError;
use serde_json::{Value, json};

type Handler = for<'a> fn(
    &'a MemoryBackend,
    &'a (),
    Value,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<HandlerOutput, JmapError>> + 'a>,
>;

/// `(method, handler, spec condition, condition with a misspelt key)`,
/// each `/query` followed by its `/queryChanges`.
fn cases() -> [(&'static str, Handler, Value, Value); 4] {
    [
        (
            "Principal/query",
            |b, c, a| Box::pin(handle_principal_query(b, c, a, &[])),
            json!({ "email": "a@example.com" }),
            json!({ "emails": "a@example.com" }),
        ),
        (
            "Principal/queryChanges",
            |b, c, a| Box::pin(handle_principal_query_changes(b, c, a, &[])),
            json!({ "email": "a@example.com" }),
            json!({ "emails": "a@example.com" }),
        ),
        (
            "ShareNotification/query",
            |b, c, a| Box::pin(handle_share_notification_query(b, c, a)),
            json!({ "objectType": "Calendar" }),
            json!({ "objectTypes": "Calendar" }),
        ),
        (
            "ShareNotification/queryChanges",
            |b, c, a| Box::pin(handle_share_notification_query_changes(b, c, a)),
            json!({ "objectType": "Calendar" }),
            json!({ "objectTypes": "Calendar" }),
        ),
    ]
}

#[tokio::test]
async fn misspelt_operator_or_condition_key_is_unsupported_filter() {
    let backend = MemoryBackend::new_with_accounts(&["acc1"]);
    for (method, handler, condition, typo) in cases() {
        let operator_typo = json!({ "opperator": "AND", "conditions": [condition] });
        for filter in [operator_typo, typo] {
            let args = json!({ "accountId": "acc1", "sinceQueryState": "0", "filter": filter });
            let err = handler(&backend, &(), args)
                .await
                .expect_err("a misspelt filter key must be refused");
            assert_eq!(
                err.error_type.to_string(),
                "unsupportedFilter",
                "{method} filter {filter}: {err:?}"
            );
        }
    }
}

/// Negative control: with no filter the same `/query` calls succeed, so
/// the refusals above come from the filter alone.
#[tokio::test]
async fn query_without_filter_succeeds() {
    let backend = MemoryBackend::new_with_accounts(&["acc1"]);
    for (method, handler, _, _) in cases().into_iter().step_by(2) {
        handler(&backend, &(), json!({ "accountId": "acc1" }))
            .await
            .unwrap_or_else(|e| panic!("{method}: {e:?}"));
    }
}
