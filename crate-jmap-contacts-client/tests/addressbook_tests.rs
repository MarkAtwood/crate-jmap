//! Wiremock integration tests for AddressBook/get, /changes, /set.
//!
//! Oracle for all response shapes: draft-ietf-jmap-contacts-10 §2 and RFC 8620 §5.
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test JMAP-kh21.1 #1 — AddressBook/get returns the AddressBook list.
///
/// Oracle: draft-ietf-jmap-contacts-10 §2.1 — passing ids=null returns all
/// AddressBooks. Response shape from spec §4.1 example.
#[tokio::test]
async fn addressbook_get_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "AddressBook/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [
                    {
                        "id": "ab-1",
                        "name": "Personal",
                        "description": null,
                        "sortOrder": 0,
                        "isDefault": true,
                        "isSubscribed": true,
                        "shareWith": null,
                        "myRights": {
                            "mayRead": true,
                            "mayWrite": true,
                            "mayShare": false,
                            "mayDelete": false
                        }
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

    let sc = helpers::make_client(&server).await;
    let resp = sc
        .address_book_get(None, None)
        .await
        .expect("addressbook_get_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.state, "s5", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 address book");
    assert_eq!(resp.list[0].id.as_ref(), "ab-1", "id mismatch");
    assert_eq!(resp.list[0].name, "Personal", "name mismatch");
    assert!(resp.list[0].is_default, "isDefault must be true");
    assert!(resp.list[0].my_rights.may_read, "mayRead must be true");
}

/// Test JMAP-kh21.1 #2 — AddressBook/changes sends sinceState in the request.
///
/// Oracle: draft-ietf-jmap-contacts-10 §2.2 — sinceState is a required argument.
/// RFC 8620 §5.2 — changes response shape.
#[tokio::test]
async fn addressbook_changes_sends_since_state() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "AddressBook/changes",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s11",
                "hasMoreChanges": false,
                "created": ["ab-new"],
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

    let sc = helpers::make_client(&server).await;
    let resp = sc
        .address_book_changes("s10", None)
        .await
        .expect("addressbook_changes_sends_since_state: must succeed");

    assert_eq!(resp.old_state, "s10", "oldState mismatch");
    assert_eq!(resp.new_state, "s11", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");
    assert!(
        resp.created.iter().any(|id| id.as_ref() == "ab-new"),
        "created must contain ab-new"
    );

    // Verify sinceState was sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("addressbook_changes_sends_since_state: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("addressbook_changes_sends_since_state: request body must be valid JSON");
    assert_eq!(
        body["methodCalls"][0][1]["sinceState"],
        json!("s10"),
        "sinceState must be s10 in wire request"
    );
}

/// Test JMAP-kh21.1 #3 — AddressBook/set create round-trip.
///
/// Oracle: draft-ietf-jmap-contacts-10 §2.3 — /set create returns server-assigned id
/// in the created map. RFC 8620 §5.3.
#[tokio::test]
async fn addressbook_set_create_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "AddressBook/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "created": {
                    "newAb": {
                        "id": "server-ab-id",
                        "name": "Work",
                        "description": null,
                        "sortOrder": 0,
                        "isDefault": false,
                        "isSubscribed": true,
                        "shareWith": null,
                        "myRights": {
                            "mayRead": true,
                            "mayWrite": true,
                            "mayShare": false,
                            "mayDelete": true
                        }
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

    let sc = helpers::make_client(&server).await;
    let create_obj = json!({
        "newAb": {
            "name": "Work",
            "isSubscribed": true
        }
    });
    let resp = sc
        .address_book_set(Some(create_obj), None, None, None)
        .await
        .expect("addressbook_set_create_round_trip: must succeed");

    assert_eq!(resp.new_state, "s2", "newState mismatch");
    let created = resp.created.expect("created must be present");
    assert!(
        created.contains_key("newAb"),
        "created must contain 'newAb' key"
    );
    assert_eq!(
        created["newAb"].id.as_ref(),
        "server-ab-id",
        "server-assigned id mismatch"
    );
    assert_eq!(created["newAb"].name, "Work", "name mismatch");
}

/// Test JMAP-kh21.1 #4 — AddressBook/set with onDestroyRemoveContents sends the wire key.
///
/// Oracle: draft-ietf-jmap-contacts-10 §2.3 — onDestroyRemoveContents is the wire key
/// (contacts-10 uses "contents" not "contacts"). Verify the exact JSON key appears.
#[tokio::test]
async fn addressbook_set_on_destroy_remove_contents() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "AddressBook/set",
            {
                "accountId": "A13824",
                "oldState": "s5",
                "newState": "s6",
                "created": null,
                "updated": null,
                "destroyed": ["ab-old"],
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

    let sc = helpers::make_client(&server).await;
    let params = jmap_contacts_client::AddressBookSetParams {
        on_destroy_remove_contents: Some(true),
        on_success_set_is_default: None,
    };
    let resp = sc
        .address_book_set(None, None, Some(vec!["ab-old"]), Some(params))
        .await
        .expect("addressbook_set_on_destroy_remove_contents: must succeed");

    assert_eq!(resp.new_state, "s6", "newState mismatch");
    let destroyed = resp.destroyed.expect("destroyed must be present");
    assert!(
        destroyed.iter().any(|id| id.as_ref() == "ab-old"),
        "destroyed must contain ab-old"
    );

    // Verify onDestroyRemoveContents appears in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("addressbook_set_on_destroy_remove_contents: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("addressbook_set_on_destroy_remove_contents: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["onDestroyRemoveContents"],
        json!(true),
        "onDestroyRemoveContents must be true in wire request"
    );
}
