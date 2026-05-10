//! Wiremock integration tests for EmailSubmission/set.
//!
//! Key test: verifying the onSuccessUpdateEmail wire path (RFC 8621 §7.5),
//! which is the unique feature distinguishing EmailSubmission/set from generic /set.
//!
//! Oracle: RFC 8621 §7.5.1 example (lines 4715-4732).

#[path = "helpers.rs"]
mod helpers;

use std::collections::HashMap;

use jmap_mail_client::EmailSubmissionSetParams;
use jmap_types::PatchObject;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: EmailSubmission/set create round-trip returns the created submission.
///
/// Oracle: RFC 8621 §7.5 response example (lines 4715-4724).
#[tokio::test]
async fn email_submission_set_create_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/set",
            {
                "accountId": "A13824",
                "oldState": "es1",
                "newState": "es2",
                "created": {
                    "k1490": {
                        "id": "ES-3bab7f9a",
                        "identityId": "I64588216",
                        "emailId": "M7f6ed5bcfd",
                        "threadId": "T1",
                        "envelope": null,
                        "sendAt": "2024-06-15T10:00:00Z",
                        "undoStatus": "final",
                        "deliveryStatus": null,
                        "dsnBlobIds": [],
                        "mdnBlobIds": []
                    }
                },
                "notCreated": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .email_submission_set(
            Some(json!({
                "k1490": {
                    "identityId": "I64588216",
                    "emailId": "M7f6ed5bcfd"
                }
            })),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("email_submission_set_create_round_trip: must succeed");

    let created = resp.created.as_ref().expect("created must be Some");
    assert_eq!(
        created["k1490"].id.as_ref(),
        "ES-3bab7f9a",
        "created[k1490].id must match"
    );
    assert!(resp.not_created.is_none(), "notCreated must be absent");
}

/// Test 2: EmailSubmission/set with onSuccessUpdateEmail sends the key at the top level.
///
/// Oracle: RFC 8621 §7.5 request example (lines 4675-4689).
/// The creation reference "#k1490" is a literal wire key — the "#" prefix is part of
/// the key name itself (a JMAP creation reference), not an encoding artifact.
#[tokio::test]
async fn email_submission_set_on_success_update_email() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/set",
            {
                "accountId": "A13824",
                "oldState": "es5",
                "newState": "es6",
                "created": {
                    "k1490": {
                        "id": "ES-success",
                        "identityId": "I64588216",
                        "emailId": "M7f6ed5bcfd",
                        "threadId": "T1",
                        "envelope": null,
                        "sendAt": "2024-06-15T10:00:00Z",
                        "undoStatus": "final",
                        "deliveryStatus": null,
                        "dsnBlobIds": [],
                        "mdnBlobIds": []
                    }
                },
                "notCreated": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    // Patch the draft keyword off the email upon successful send.
    // Oracle: RFC 8621 §7.5 — onSuccessUpdateEmail is Id[PatchObject].
    let mut on_success = HashMap::new();
    let mut patch_map = serde_json::Map::new();
    patch_map.insert("keywords/$draft".to_owned(), serde_json::Value::Null);
    on_success.insert("#k1490".to_owned(), PatchObject::from_map(patch_map));
    let params = EmailSubmissionSetParams {
        on_success_update_email: Some(on_success),
        on_success_destroy_email: None,
    };
    sc.email_submission_set(
        Some(json!({
            "k1490": {
                "identityId": "I64588216",
                "emailId": "M7f6ed5bcfd"
            }
        })),
        None,
        None,
        None,
        Some(params),
    )
    .await
    .expect("email_submission_set_on_success_update_email: must succeed");

    // Verify onSuccessUpdateEmail was sent at the correct level in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args["onSuccessUpdateEmail"].is_object(),
        "onSuccessUpdateEmail must be an object in the request: {args}"
    );
    // The "#k1490" key must be present as-is — "#" is part of the creation reference key.
    assert!(
        args["onSuccessUpdateEmail"]["#k1490"].is_object(),
        "onSuccessUpdateEmail must contain key '#k1490': {args}"
    );
}

/// Test 3: EmailSubmission/set with params=None omits both onSuccess keys from the request.
///
/// Oracle: RFC 8621 §7.5 — onSuccessUpdateEmail and onSuccessDestroyEmail are optional
/// method arguments; they MUST be absent from the wire when not requested.
#[tokio::test]
async fn email_submission_set_no_on_success_when_none() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/set",
            {
                "accountId": "A13824",
                "oldState": null,
                "newState": "es1",
                "created": null,
                "notCreated": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    sc.email_submission_set(None, None, None, None, None)
        .await
        .expect("email_submission_set_no_on_success_when_none: must succeed");

    // Verify neither onSuccess key appears in the request.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("onSuccessUpdateEmail").is_none(),
        "onSuccessUpdateEmail must be absent when params=None: {args}"
    );
    assert!(
        args.get("onSuccessDestroyEmail").is_none(),
        "onSuccessDestroyEmail must be absent when params=None: {args}"
    );
}

// email_submission_set_destroy_with_empty_id_guard was deleted in JMAP-6by7.2
// (typed-Id refactor): the test passed `Some(vec![""])` to assert that the
// empty-string destroy id was rejected. Under typed-Id, `destroy: Option<Vec<Id>>`
// makes the call site itself a compile error — `""` is not an `Id` and the
// only fallible path (`Id::new_validated("")`) returns Err at the test's
// input-construction site. The bug is impossible to express through the
// typed API.
