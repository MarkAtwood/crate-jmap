//! A misspelt FilterOperator key or FilterCondition property is
//! `unsupportedFilter`, never an empty, match-all condition (bd:JMAP-6azm).
//!
//! Oracle: RFC 8620 §5.5 — a FilterOperator object has the properties
//! `operator` and `conditions`, and "`unsupportedFilter`: The filter is
//! syntactically valid, but the server cannot process it." Condition
//! property names: draft-atwood-jmap-vtc-00 §VTCCall/query (`state`),
//! §VTCParticipant/query, §VTCRecording/query and §VTCLivestream/query
//! (`callId`). The VTCCall misspelt-property case lives in
//! `call_query.rs`.
//!
//! The VTCParticipant filters keep a top-level `callId`, because an
//! unscoped participant filter is `unsupportedFilter` anyway: without the
//! refusal each would decode as a condition on `callId` alone and succeed.

mod common;

use common::*;
use jmap_server::HandlerOutput;
use jmap_types::JmapError;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_call_query, handle_vtc_call_query_changes, handle_vtc_livestream_query,
    handle_vtc_livestream_query_changes, handle_vtc_participant_query,
    handle_vtc_participant_query_changes, handle_vtc_recording_query,
    handle_vtc_recording_query_changes,
};
use serde_json::{json, Value};

type Handler = for<'a> fn(
    &'a MemoryBackend,
    &'a String,
    Value,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<HandlerOutput, JmapError>> + 'a>,
>;

/// `(method, handler, spec condition, filter with a misspelt operator
/// key, condition with a misspelt property)`, each `/query` followed by
/// its `/queryChanges`.
fn cases() -> Vec<(&'static str, Handler, Value, Value, Option<Value>)> {
    let call = json!({ "state": "active" });
    let call_op = json!({ "opperator": "AND", "conditions": [call.clone()] });
    let scoped = json!({ "callId": "C1" });
    let participant_op = json!({ "opperator": "AND", "conditions": [], "callId": "C1" });
    let participant_typo = json!({ "callId": "C1", "userID": ALICE });
    let media_op = json!({ "opperator": "AND", "conditions": [scoped.clone()] });
    let media_typo = json!({ "callID": "C1" });
    vec![
        (
            "VTCCall/query",
            |b, c, a| Box::pin(handle_vtc_call_query(b, c, a)),
            call.clone(),
            call_op.clone(),
            None,
        ),
        (
            "VTCCall/queryChanges",
            |b, c, a| Box::pin(handle_vtc_call_query_changes(b, c, a)),
            call,
            call_op,
            None,
        ),
        (
            "VTCParticipant/query",
            |b, c, a| Box::pin(handle_vtc_participant_query(b, c, a)),
            scoped.clone(),
            participant_op.clone(),
            Some(participant_typo.clone()),
        ),
        (
            "VTCParticipant/queryChanges",
            |b, c, a| Box::pin(handle_vtc_participant_query_changes(b, c, a)),
            scoped.clone(),
            participant_op,
            Some(participant_typo),
        ),
        (
            "VTCRecording/query",
            |b, c, a| Box::pin(handle_vtc_recording_query(b, c, a)),
            scoped.clone(),
            media_op.clone(),
            Some(media_typo.clone()),
        ),
        (
            "VTCRecording/queryChanges",
            |b, c, a| Box::pin(handle_vtc_recording_query_changes(b, c, a)),
            scoped.clone(),
            media_op.clone(),
            Some(media_typo.clone()),
        ),
        (
            "VTCLivestream/query",
            |b, c, a| Box::pin(handle_vtc_livestream_query(b, c, a)),
            scoped.clone(),
            media_op.clone(),
            Some(media_typo.clone()),
        ),
        (
            "VTCLivestream/queryChanges",
            |b, c, a| Box::pin(handle_vtc_livestream_query_changes(b, c, a)),
            scoped,
            media_op,
            Some(media_typo),
        ),
    ]
}

async fn run(handler: Handler, filter: Value) -> Result<HandlerOutput, JmapError> {
    let backend = media_backend();
    let args = json!({ "accountId": ACCOUNT, "sinceQueryState": "0", "filter": filter });
    handler(&backend, &BOB.to_owned(), args).await
}

#[tokio::test]
async fn misspelt_operator_or_condition_key_is_unsupported_filter() {
    for (method, handler, _, operator_typo, typo) in cases() {
        for filter in std::iter::once(operator_typo).chain(typo) {
            let err = run(handler, filter.clone())
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

/// Negative control: each spec condition alone is accepted by `/query`,
/// so the refusals above come from the misspelt key alone.
#[tokio::test]
async fn spec_condition_succeeds() {
    for (method, handler, condition, _, _) in cases().into_iter().step_by(2) {
        run(handler, condition)
            .await
            .unwrap_or_else(|e| panic!("{method}: {e:?}"));
    }
}
