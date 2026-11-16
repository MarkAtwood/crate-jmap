//! `SetCall`, `JmapClient::set_call`, `ConditionalError` and
//! `Session::supports_conditional` (bd JMAP-y1yu.6;
//! docs/design/conditional.md §8, §10).
//!
//! Oracles: draft-ietf-jmap-conditional-02 example 5.9 (request JSON) and
//! 5.1 (error JSON), §2.1 (the capability), §3.3, §4.2 and §4.3 (the error
//! types); draft-ietf-jmap-object-history-03 §5.1 (`restore`).

use std::collections::BTreeMap;

use jmap_base_client::{ClientError, ConditionalError, JmapClient, NoneAuth, SetCall};
use jmap_types::conditional::ConditionalSetArgs;
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CONDITIONAL: &str = "urn:ietf:params:jmap:conditional";
const FILENODE: &str = "urn:ietf:params:jmap:filenode";

fn patch(v: Value) -> PatchObject {
    serde_json::from_value(v).expect("patch")
}

/// Example 5.9's call.
fn example_5_9() -> SetCall {
    let mut call = SetCall::default();
    call.account_id = Some(Id::from("u1"));
    let mut conditional = ConditionalSetArgs::default();
    conditional.atomic = true;
    conditional
        .if_unchanged_by
        .insert("r".to_owned(), Some(patch(json!({"blobId": "G_old"}))));
    call.conditional = conditional;
    call.update
        .insert(Id::from("t"), patch(json!({"name": "report.txt"})));
    call.destroy.push(Id::from("r"));
    call
}

#[test]
fn set_call_matches_example_5_9() {
    let call = example_5_9();
    assert_eq!(
        call.to_arguments().unwrap(),
        json!({
            "accountId": "u1",
            "atomic": true,
            "ifUnchangedBy": { "r": { "blobId": "G_old" } },
            "update":  { "t": { "name": "report.txt" } },
            "destroy": [ "r" ]
        })
    );
    assert_eq!(
        call.using(&[FILENODE]),
        ["urn:ietf:params:jmap:core", FILENODE, CONDITIONAL]
    );
}

/// With only `restore` set, the conditional options are default: nothing
/// of them is sent and `using` has no conditional URI (decision 8).
#[test]
fn restore_without_conditional_options() {
    let mut call = SetCall::default();
    call.account_id = Some(Id::from("u1"));
    call.restore.insert(Id::from("x"), None);
    call.restore
        .insert(Id::from("y"), Some(patch(json!({"name": "restored"}))));
    assert_eq!(
        call.to_arguments().unwrap(),
        json!({"accountId": "u1", "restore": {"x": null, "y": {"name": "restored"}}})
    );
    let history = "urn:ietf:params:jmap:objecthistory";
    assert_eq!(
        call.using(&[history]),
        ["urn:ietf:params:jmap:core", history]
    );
}

#[test]
fn typed_fields_win_over_extra() {
    let mut call = SetCall::default();
    call.account_id = Some(Id::from("u1"));
    call.extra.insert("accountId".to_owned(), json!("other"));
    call.extra.insert("onExists".to_owned(), json!("replace"));
    assert_eq!(
        call.to_arguments().unwrap(),
        json!({"accountId": "u1", "onExists": "replace"})
    );
}

fn error(v: Value) -> JmapError {
    serde_json::from_value(v).expect("JmapError")
}

#[test]
fn conditional_errors_from_the_wire() {
    // Example 5.1.
    assert_eq!(
        ConditionalError::from_method_error(&error(
            json!({"type": "stateMismatch", "failed": ["f42"]})
        )),
        Some(ConditionalError::StateMismatch {
            failed: Some(vec![Id::from("f42")])
        })
    );
    assert_eq!(
        ConditionalError::from_method_error(&error(json!({"type": "stateMismatch"}))),
        Some(ConditionalError::StateMismatch { failed: None })
    );
    let Some(ConditionalError::AtomicFailure(details)) =
        ConditionalError::from_method_error(&error(json!({
            "type": "atomicFailure",
            "notUpdated": {"t": {"type": "alreadyExists"}}
        })))
    else {
        panic!("atomicFailure");
    };
    let not_updated = details.not_updated.expect("notUpdated");
    assert_eq!(not_updated.len(), 1);
    assert!(not_updated.contains_key(&Id::from("t")));
    assert_eq!(
        ConditionalError::from_method_error(&error(json!({"type": "cannotApplyAtomically"}))),
        Some(ConditionalError::CannotApplyAtomically)
    );
    assert_eq!(
        ConditionalError::from_method_error(&error(json!({"type": "serverFail"}))),
        None
    );
}

#[test]
fn supports_conditional_reads_the_capability() {
    let session = |caps: Value| -> jmap_base_client::Session {
        serde_json::from_value(json!({
            "capabilities": caps, "accounts": {}, "primaryAccounts": {}, "username": "",
            "apiUrl": "https://example.com/api/", "downloadUrl": "https://example.com/d",
            "uploadUrl": "https://example.com/u", "eventSourceUrl": "https://example.com/e",
            "state": ""
        }))
        .expect("session")
    };
    assert!(!session(json!({"urn:ietf:params:jmap:core": {}})).supports_conditional());
    assert!(session(json!({CONDITIONAL: {}})).supports_conditional());
}

/// End to end: the request carries the arguments and `using`; a
/// `stateMismatch` answer comes back as a [`ConditionalError`].
#[tokio::test]
async fn set_call_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .and(body_partial_json(json!({
            "using": ["urn:ietf:params:jmap:core", FILENODE, CONDITIONAL],
            "methodCalls": [["FileNode/set", example_5_9().to_arguments().unwrap(), "0"]]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [["error", {"type": "stateMismatch", "failed": ["r"]}, "0"]],
            "sessionState": "s1"
        })))
        .mount(&server)
        .await;
    let client = JmapClient::new(
        jmap_base_client::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("client");
    let session: jmap_base_client::Session = serde_json::from_value(json!({
        "capabilities": {CONDITIONAL: {}}, "accounts": {}, "primaryAccounts": {}, "username": "",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/d", server.uri()), "uploadUrl": format!("{}/u", server.uri()),
        "eventSourceUrl": format!("{}/e", server.uri()), "state": ""
    }))
    .expect("session");

    let result: Result<BTreeMap<String, Value>, ClientError> = client
        .set_call(&session, "FileNode/set", &[FILENODE], &example_5_9())
        .await;
    let Err(ClientError::MethodError(err)) = result else {
        panic!("expected a method error, got {result:?}");
    };
    assert_eq!(
        ConditionalError::from_method_error(&err),
        Some(ConditionalError::StateMismatch {
            failed: Some(vec![Id::from("r")])
        })
    );
}
