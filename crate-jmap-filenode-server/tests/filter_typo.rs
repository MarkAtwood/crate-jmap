//! A misspelt FilterOperator key or FilterCondition property is
//! `unsupportedFilter`, never an empty, match-all condition (bd:JMAP-6azm).
//!
//! Oracle: RFC 8620 §5.5 — a FilterOperator object has the properties
//! `operator` and `conditions`, and "`unsupportedFilter`: The filter is
//! syntactically valid, but the server cannot process it." Condition
//! property names: draft-ietf-jmap-filenode §FileNode/query (`isTopLevel`). Without the refusal, each bad filter below
//! would decode as a condition with every property unset, which matches
//! every record.

mod common;

use common::MemoryBackend;
use jmap_filenode_server::{handle_filenode_query, handle_filenode_query_changes};
use jmap_server::HandlerOutput;
use jmap_types::Id;
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
fn cases() -> [(&'static str, Handler, Value, Value); 2] {
    [
        (
            "FileNode/query",
            |b, c, a| Box::pin(handle_filenode_query(b, c, a, &[])),
            json!({ "isTopLevel": true }),
            json!({ "isToplevel": true }),
        ),
        (
            "FileNode/queryChanges",
            |b, c, a| Box::pin(handle_filenode_query_changes(b, c, a, &[])),
            json!({ "isTopLevel": true }),
            json!({ "isToplevel": true }),
        ),
    ]
}

#[tokio::test]
async fn misspelt_operator_or_condition_key_is_unsupported_filter() {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
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
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    for (method, handler, _, _) in cases().into_iter().step_by(2) {
        handler(&backend, &(), json!({ "accountId": "acc1" }))
            .await
            .unwrap_or_else(|e| panic!("{method}: {e:?}"));
    }
}
