//! Wiremock integration tests for Principal/set.
//!
//! Oracle for response shapes: RFC 9670 §2.3 (set).
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "common/mod.rs"]
mod common;

use jmap_types::Id;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: Principal/set with destroy list — server returns destroyed array.
///
/// Oracle: RFC 9670 §2.3 — destroy is a list of Principal IDs to delete.
/// Response destroyed array must contain the IDs that were successfully removed.
#[tokio::test]
async fn principal_set_destroy_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Principal/set",
            {
                "accountId": "u33084183",
                "oldState": "s1",
                "newState": "s2",
                "created": null,
                "updated": null,
                "destroyed": ["p-joe"],
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = common::make_client(&server);
    let resp = sc
        .principal_set(None, None, Some(vec![Id::from("p-joe")]))
        .await
        .expect("principal_set_destroy_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "u33084183", "accountId mismatch");
    assert_eq!(resp.new_state, "s2", "newState mismatch");
    let destroyed = resp
        .destroyed
        .as_ref()
        .expect("principal_set_destroy_round_trip: destroyed must be Some");
    assert!(
        destroyed.iter().any(|id| id.as_ref() == "p-joe"),
        "destroyed must contain p-joe"
    );
}

/// Test 2: Principal/set create returns forbidden in notCreated.
///
/// Oracle: RFC 9670 §2.3 — servers may reject create operations with a
/// `forbidden` SetError. The notCreated map uses caller-supplied creation
/// keys (RFC 8620 §5.3).
#[tokio::test]
async fn principal_set_create_returns_forbidden() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Principal/set",
            {
                "accountId": "u33084183",
                "oldState": "s1",
                "newState": "s1",
                "created": null,
                "updated": null,
                "destroyed": null,
                "notCreated": {
                    "c1": {
                        "type": "forbidden",
                        "description": "Principals may not be created by clients"
                    }
                },
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = common::make_client(&server);
    let create_obj = json!({
        "c1": {
            "type": "individual",
            "name": "New Principal"
        }
    });
    let resp = sc
        .principal_set(Some(create_obj), None, None)
        .await
        .expect("principal_set_create_returns_forbidden: must succeed at transport level");

    let not_created = resp
        .not_created
        .as_ref()
        .expect("principal_set_create_returns_forbidden: notCreated must be Some");
    let err = not_created
        .get("c1")
        .expect("principal_set_create_returns_forbidden: c1 must be in notCreated");
    assert_eq!(
        err.error_type, "forbidden",
        "error type must be forbidden per RFC 9670 §2.3"
    );
}
