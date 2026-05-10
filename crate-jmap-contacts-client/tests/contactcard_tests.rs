//! Wiremock integration tests for ContactCard/get, /changes, /set, /copy.
//!
//! Oracle for all response shapes: RFC 9610 §3 and RFC 8620 §5.
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.
//! Minimal ContactCard fixture per JSContact RFC 9553 §2.

#[path = "helpers.rs"]
mod helpers;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test JMAP-kh21.2 #1 — ContactCard/get returns the ContactCard list.
///
/// Oracle: RFC 9610 §3.1 — passing ids=null returns all
/// ContactCards for the account. Minimal JSContact card fixture per RFC 9553 §2.
#[tokio::test]
async fn contact_card_get_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/get",
            {
                "accountId": "A13824",
                "state": "s7",
                "list": [
                    {
                        "id": "card1",
                        "addressBookIds": { "ab1": true }
                    }
                ],
                "notFound": []
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
        .contact_card_get(None, None)
        .await
        .expect("contact_card_get_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.state, "s7", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 card");
    let card_id = resp.list[0].id.as_ref().expect("id must be present");
    assert_eq!(card_id.as_ref(), "card1", "card id mismatch");
}

/// Test JMAP-kh21.2 #2 — ContactCard/changes sends sinceState in the request.
///
/// Oracle: RFC 9610 §3.2 — sinceState is a required argument.
/// RFC 8620 §5.2 — changes response shape.
#[tokio::test]
async fn contact_card_changes_sends_since_state() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/changes",
            {
                "accountId": "A13824",
                "oldState": "s20",
                "newState": "s21",
                "hasMoreChanges": false,
                "created": ["card-new"],
                "updated": [],
                "destroyed": []
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
        .contact_card_changes(&jmap_types::State::from("s20"), None)
        .await
        .expect("contact_card_changes_sends_since_state: must succeed");

    assert_eq!(resp.old_state, "s20", "oldState mismatch");
    assert_eq!(resp.new_state, "s21", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");
    assert!(
        resp.created.iter().any(|id| id.as_ref() == "card-new"),
        "created must contain card-new"
    );

    // Verify sinceState was sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("contact_card_changes_sends_since_state: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("contact_card_changes_sends_since_state: request body must be valid JSON");
    assert_eq!(
        body["methodCalls"][0][1]["sinceState"],
        json!("s20"),
        "sinceState must be s20 in wire request"
    );
}

/// Test JMAP-kh21.2 #3 — ContactCard/set create round-trip.
///
/// Oracle: RFC 9610 §3.3 — /set create returns server-assigned id
/// in the created map. RFC 8620 §5.3. Minimal JSContact card fixture per RFC 9553 §2.
#[tokio::test]
async fn contact_card_set_create_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "created": {
                    "newCard": {
                        "id": "server-card-id",
                        "addressBookIds": { "ab1": true }
                    }
                },
                "updated": null,
                "destroyed": null,
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

    let sc = helpers::make_client(&server);
    // Minimal JSContact card fixture per RFC 9553 §2.
    let create_obj = json!({
        "newCard": {
            "addressBookIds": { "ab1": true },
            "name": { "full": "Alice Smith" }
        }
    });
    let resp = sc
        .contact_card_set(Some(create_obj), None, None)
        .await
        .expect("contact_card_set_create_round_trip: must succeed");

    assert_eq!(resp.new_state, "s2", "newState mismatch");
    let created = resp.created.expect("created must be present");
    assert!(
        created.contains_key("newCard"),
        "created must contain 'newCard' key"
    );
    let card_id = created["newCard"]
        .id
        .as_ref()
        .expect("server id must be present");
    assert_eq!(card_id.as_ref(), "server-card-id", "server id mismatch");
}

/// Test JMAP-kh21.2 #4 — ContactCard/copy sends fromAccountId in the wire request.
///
/// Oracle: RFC 8620 §5.4 /copy — fromAccountId is a required argument.
/// Verify the exact wire key appears in the request body.
#[tokio::test]
async fn contact_card_copy_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/copy",
            {
                "accountId": "A13824",
                "oldState": null,
                "newState": "s3",
                "created": {
                    "k1": {
                        "id": "copied-card-id",
                        "addressBookIds": { "ab1": true }
                    }
                },
                "updated": null,
                "destroyed": null,
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

    let sc = helpers::make_client(&server);
    let create_obj = json!({
        "k1": {
            "id": "source-card-id",
            "addressBookIds": { "ab1": true }
        }
    });
    let resp = sc
        .contact_card_copy(&jmap_types::Id::from("src-account-1"), create_obj)
        .await
        .expect("contact_card_copy_round_trip: must succeed");

    assert_eq!(resp.new_state, "s3", "newState mismatch");
    let created = resp.created.expect("created must be present");
    assert!(
        created.contains_key("k1"),
        "created must contain 'k1' creation key"
    );
    let card_id = created["k1"]
        .id
        .as_ref()
        .expect("server id must be present");
    assert_eq!(
        card_id.as_ref(),
        "copied-card-id",
        "copied card id mismatch"
    );

    // Verify fromAccountId appears in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("contact_card_copy_round_trip: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("contact_card_copy_round_trip: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["fromAccountId"],
        json!("src-account-1"),
        "fromAccountId must be src-account-1 in wire request"
    );
    assert_eq!(
        args["accountId"],
        json!("A13824"),
        "accountId must be A13824 in wire request"
    );
}
