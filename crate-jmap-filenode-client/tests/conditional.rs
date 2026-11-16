//! `SessionClient::filenode_set_conditional` (bd JMAP-y1yu.18;
//! docs/design/conditional.md §8, DECISIONS.md decision 8).
//!
//! Oracles: draft-ietf-jmap-conditional-02 examples 5.1 (force-with-lease
//! request and its `stateMismatch` error) and 5.8 (atomic name swap), and
//! §4.2 (`atomicFailure` names the offending ids).

mod helpers;

use jmap_base_client::{ClientError, ConditionalError, SetCall};
use jmap_types::conditional::ConditionalSetArgs;
use jmap_types::{Id, PatchObject};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const USING: [&str; 3] = [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:filenode",
    "urn:ietf:params:jmap:conditional",
];

fn patch(v: Value) -> PatchObject {
    serde_json::from_value(v).expect("patch")
}

/// Mount a mock answering a FileNode/set whose arguments include
/// `expected` with `answer` as the method response.
async fn mock(server: &MockServer, expected: Value, answer: Value) {
    Mock::given(method("POST"))
        .and(path("/api/"))
        .and(body_partial_json(json!({
            "using": USING,
            "methodCalls": [["FileNode/set", expected, "0"]]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [answer], "sessionState": "s1"
        })))
        .mount(server)
        .await;
}

/// Example 5.1: the force-with-lease request, answered with the example's
/// `stateMismatch`.
#[tokio::test]
async fn example_5_1_force_with_lease() {
    let server = MockServer::start().await;
    mock(
        &server,
        json!({
            "accountId": "A13824",
            "ifUnchangedBy": { "f42": { "blobId": "G_old" } },
            "update": { "f42": { "blobId": "G_new", "modified": "2026-05-01T09:30:00Z" } }
        }),
        json!(["error", {"type": "stateMismatch", "failed": ["f42"]}, "0"]),
    )
    .await;

    let mut conditional = ConditionalSetArgs::default();
    conditional
        .if_unchanged_by
        .insert("f42".to_owned(), Some(patch(json!({"blobId": "G_old"}))));
    let mut call = SetCall::default();
    call.conditional = conditional;
    call.update.insert(
        Id::from("f42"),
        patch(json!({"blobId": "G_new", "modified": "2026-05-01T09:30:00Z"})),
    );

    let result = helpers::make_client(&server)
        .filenode_set_conditional(call)
        .await;
    let Err(ClientError::MethodError(err)) = result else {
        panic!("expected stateMismatch, got {result:?}");
    };
    assert_eq!(
        ConditionalError::from_method_error(&err),
        Some(ConditionalError::StateMismatch {
            failed: Some(vec![Id::from("f42")])
        })
    );
}

/// Example 5.8: the atomic swap; once as a success, once answered with an
/// `atomicFailure` naming the offender.
#[tokio::test]
async fn example_5_8_atomic_swap() {
    let swap = || {
        let mut call = SetCall::default();
        let mut conditional = ConditionalSetArgs::default();
        conditional.atomic = true;
        call.conditional = conditional;
        call.update
            .insert(Id::from("a"), patch(json!({"name": "previous.txt"})));
        call.update
            .insert(Id::from("b"), patch(json!({"name": "current.txt"})));
        call
    };
    let request = json!({
        "accountId": "A13824",
        "atomic": true,
        "update": {
            "a": { "name": "previous.txt" },
            "b": { "name": "current.txt" }
        }
    });

    let server = MockServer::start().await;
    mock(
        &server,
        request.clone(),
        json!(["FileNode/set", {
            "accountId": "A13824", "oldState": "1", "newState": "2",
            "updated": {"a": null, "b": null}
        }, "0"]),
    )
    .await;
    let response = helpers::make_client(&server)
        .filenode_set_conditional(swap())
        .await
        .expect("swap");
    let updated = response.updated;
    assert!(updated.contains_key(&Id::from("a")) && updated.contains_key(&Id::from("b")));

    let server = MockServer::start().await;
    mock(
        &server,
        request,
        json!(["error", {
            "type": "atomicFailure",
            "notUpdated": {"b": {"type": "alreadyExists", "existingId": "c"}}
        }, "0"]),
    )
    .await;
    let result = helpers::make_client(&server)
        .filenode_set_conditional(swap())
        .await;
    let Err(ClientError::MethodError(err)) = result else {
        panic!("expected atomicFailure, got {result:?}");
    };
    let Some(ConditionalError::AtomicFailure(details)) = ConditionalError::from_method_error(&err)
    else {
        panic!("atomicFailure: {err:?}");
    };
    let offenders: Vec<Id> = details
        .not_updated
        .expect("notUpdated")
        .into_keys()
        .collect();
    assert_eq!(offenders, [Id::from("b")]);
}
