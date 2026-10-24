//! `/query` handlers that call `query_objects` directly pass a backend
//! `BackendQueryError::Method` through unchanged (bd JMAP-yt1w.24).
//!
//! Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt,
//! Section 5.5 "/query"):
//!
//! > The following additional errors may be returned instead of the
//! > "Foo/query" response:
//! >
//! > "unsupportedFilter": The "filter" is syntactically valid, but the
//! > server cannot process it.
//!
//! Whether a syntactically valid filter can be processed is known only to
//! the backend, so its `unsupportedFilter` (type and description) MUST
//! reach the client, not a redacted `serverFail`.

mod common;

use common::{FaultyBackend, INJECTED_QUERY_METHOD_ERROR_DESCRIPTION};
use jmap_mail_server::{handle_email_query, handle_mailbox_query, handle_submission_query};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

const ACCOUNT_ID: &str = "acct1";

fn backend_with_query_fault(type_name: &'static str) -> FaultyBackend {
    let backend = FaultyBackend::new();
    backend.inner.register_account(&Id::from(ACCOUNT_ID));
    backend.inject(type_name, "query");
    backend
}

fn wire(err: JmapError) -> Value {
    serde_json::to_value(&err).expect("JmapError must serialize")
}

fn expected() -> Value {
    json!({
        "type": "unsupportedFilter",
        "description": INJECTED_QUERY_METHOD_ERROR_DESCRIPTION,
    })
}

#[tokio::test]
async fn email_query_passes_backend_method_error_through() {
    let backend = backend_with_query_fault("Email");
    let err = handle_email_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }), &[])
        .await
        .expect_err("backend unsupportedFilter must fail Email/query");
    assert_eq!(wire(err), expected());
}

#[tokio::test]
async fn mailbox_query_passes_backend_method_error_through() {
    let backend = backend_with_query_fault("Mailbox");
    let err = handle_mailbox_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }), &[])
        .await
        .expect_err("backend unsupportedFilter must fail Mailbox/query");
    assert_eq!(wire(err), expected());
}

#[tokio::test]
async fn submission_query_passes_backend_method_error_through() {
    let backend = backend_with_query_fault("EmailSubmission");
    let err = handle_submission_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
        .await
        .expect_err("backend unsupportedFilter must fail EmailSubmission/query");
    assert_eq!(wire(err), expected());
}
