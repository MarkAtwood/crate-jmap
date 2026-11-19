//! `SessionClient::filenode_set` with conditional
//! [`SetOptions`](jmap_base_client::SetOptions) (bd JMAP-y1yu.21;
//! docs/design/conditional.md §8, DECISIONS.md decision 8).
//!
//! Oracles: draft-ietf-jmap-conditional-02 examples 5.1 (force-with-lease
//! request and its `stateMismatch` error) and 5.8 (atomic name swap), and
//! §4.2 (`atomicFailure` names the offending ids).

mod helpers;

use std::collections::BTreeMap;

use jmap_base_client::{ClientError, ConditionalError, SetOptions};
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
            "methodCalls": [["FileNode/set", expected, "r1"]]
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
        json!(["error", {"type": "stateMismatch", "failed": ["f42"]}, "r1"]),
    )
    .await;

    let mut conditional = ConditionalSetArgs::default();
    conditional
        .if_unchanged_by
        .insert("f42".to_owned(), Some(patch(json!({"blobId": "G_old"}))));
    let mut options = SetOptions::default();
    options.conditional = conditional;
    let update = BTreeMap::from([(
        Id::from("f42"),
        patch(json!({"blobId": "G_new", "modified": "2026-05-01T09:30:00Z"})),
    )]);

    let result = helpers::make_client(&server)
        .filenode_set(None, Some(update), None, None, None, Some(options))
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
    let options = || {
        let mut conditional = ConditionalSetArgs::default();
        conditional.atomic = true;
        let mut options = SetOptions::default();
        options.conditional = conditional;
        Some(options)
    };
    let update = || {
        Some(BTreeMap::from([
            (Id::from("a"), patch(json!({"name": "previous.txt"}))),
            (Id::from("b"), patch(json!({"name": "current.txt"}))),
        ]))
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
        }, "r1"]),
    )
    .await;
    let response = helpers::make_client(&server)
        .filenode_set(None, update(), None, None, None, options())
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
        }, "r1"]),
    )
    .await;
    let result = helpers::make_client(&server)
        .filenode_set(None, update(), None, None, None, options())
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

/// Default options send the request `filenode_set` sent before
/// `SetOptions`: no conditional arguments and no conditional URI.
#[tokio::test]
async fn default_options_add_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .and(wiremock::matchers::body_json(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:filenode"],
            "methodCalls": [["FileNode/set", {"accountId": "A13824", "destroy": ["x"]}, "r1"]]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [["FileNode/set", {
                "accountId": "A13824", "oldState": "1", "newState": "2", "destroyed": ["x"]
            }, "r1"]],
            "sessionState": "s1"
        })))
        .mount(&server)
        .await;
    let client = helpers::make_client(&server);
    for options in [None, Some(SetOptions::default())] {
        client
            .filenode_set(None, None, Some(vec![Id::from("x")]), None, None, options)
            .await
            .expect("plain FileNode/set");
    }
}

/// A restore adds `restore` and the object-history URI.
#[tokio::test]
async fn restore_adds_the_object_history_uri() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .and(wiremock::matchers::body_json(json!({
            "using": [
                "urn:ietf:params:jmap:core",
                "urn:ietf:params:jmap:filenode",
                "urn:ietf:params:jmap:object-history"
            ],
            "methodCalls": [["FileNode/set", {"accountId": "A13824", "restore": {"x": null}}, "r1"]]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [["FileNode/set", {
                "accountId": "A13824", "oldState": "1", "newState": "2",
                "restored": {"x": null}
            }, "r1"]],
            "sessionState": "s1"
        })))
        .mount(&server)
        .await;
    let mut options = SetOptions::default();
    options.restore.insert(Id::from("x"), None);
    let response = helpers::make_client(&server)
        .filenode_set(None, None, None, None, None, Some(options))
        .await
        .expect("restore");
    assert!(response.extra.contains_key("restored"), "{response:?}");
}
