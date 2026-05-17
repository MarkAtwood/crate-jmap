//! Wiremock smoke tests for ParticipantIdentity/* methods.
//!
//! Verifies wire-format passthrough for the methods that previously had
//! no production-path coverage (JMAP-uuoi.1).
//!
//! Oracle for response shapes:
//!   - ParticipantIdentity/get: draft-ietf-jmap-calendars-26 §3.1 + RFC 8620 §5.1
//!   - ParticipantIdentity/changes: §3.2 + RFC 8620 §5.2
//!   - ParticipantIdentity/set: §3.3 + RFC 8620 §5.3

#[path = "helpers.rs"]
mod helpers;

use std::collections::HashMap;

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Oracle: draft-ietf-jmap-calendars-26 §3.1 / RFC 8620 §5.1 —
/// `ParticipantIdentity/get` returns the configured participant
/// identities for the authenticated user. Verifies the basic /get
/// shape: response deserializes into the typed
/// [`ParticipantIdentity`](jmap_calendars_types::ParticipantIdentity)
/// list, and `ids`/`properties` thread through to the wire.
#[tokio::test]
async fn participant_identity_get_basic_shape() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ParticipantIdentity/get",
            {
                "accountId": "A13824",
                "state": "ps1",
                "list": [
                    {
                        "id": "pi-1",
                        "name": "Jane Doe",
                        "calendarAddress": "mailto:jane@example.com",
                        "isDefault": true
                    },
                    {
                        "id": "pi-2",
                        "name": "Jane (alt)",
                        "calendarAddress": "mailto:jane.alt@example.com",
                        "isDefault": false
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
        .participant_identity_get(None, Some(&["name", "calendarAddress", "isDefault"]))
        .await
        .expect("participant_identity_get: must succeed");
    assert_eq!(resp.list.len(), 2, "two identities expected");
    assert_eq!(
        resp.list[0]
            .id
            .as_ref()
            .expect("participant identity id must be present in /get response")
            .as_ref(),
        "pi-1",
        "id mismatch"
    );
    assert_eq!(resp.list[0].name, "Jane Doe", "name mismatch");
    assert_eq!(
        resp.list[0].calendar_address, "mailto:jane@example.com",
        "calendarAddress mismatch"
    );
    assert!(resp.list[0].is_default, "first identity must be default");
    assert!(
        !resp.list[1].is_default,
        "second identity must not be default"
    );

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("ids").is_none(),
        "ids must be absent when caller passes None"
    );
    assert_eq!(
        args["properties"],
        json!(["name", "calendarAddress", "isDefault"]),
        "properties array mismatch"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §3.2 / RFC 8620 §5.2 —
/// `ParticipantIdentity/changes` carries `sinceState`.
#[tokio::test]
async fn participant_identity_changes_basic_shape() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ParticipantIdentity/changes",
            {
                "accountId": "A13824",
                "oldState": "ps1",
                "newState": "ps2",
                "hasMoreChanges": false,
                "created": ["pi-3"],
                "updated": ["pi-1"],
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
    let since = State::from("ps1");
    let resp = sc
        .participant_identity_changes(&since, Some(10))
        .await
        .expect("participant_identity_changes: must succeed");
    assert_eq!(resp.created, vec![Id::from("pi-3")], "created mismatch");
    assert_eq!(resp.updated, vec![Id::from("pi-1")], "updated mismatch");
    assert!(resp.destroyed.is_empty(), "destroyed must be empty");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["sinceState"], json!("ps1"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(10), "maxChanges mismatch");
}

/// Oracle: draft-ietf-jmap-calendars-26 §3.3 / RFC 8620 §5.3 —
/// `ParticipantIdentity/set` accepts create, update, and destroy.
/// Verifies the create-map round-trip: a caller-supplied creation id
/// keys into the response `created` map, and the response carries the
/// server-assigned record id.
#[tokio::test]
async fn participant_identity_set_create_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ParticipantIdentity/set",
            {
                "accountId": "A13824",
                "oldState": "ps2",
                "newState": "ps3",
                "created": {
                    "newPi": {
                        "id": "pi-new",
                        "name": "Jane (work)",
                        "calendarAddress": "mailto:jane@work.example.com",
                        "isDefault": false
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
    // Construct without an `id` field; ParticipantIdentity.id is Option<Id>
    // and on /set create the server assigns the id (RFC 8620 §5.3).
    let identity: jmap_calendars_types::ParticipantIdentity = serde_json::from_value(json!({
        "name": "Jane (work)",
        "calendarAddress": "mailto:jane@work.example.com",
        "isDefault": false
    }))
    .expect("ParticipantIdentity must deserialize from §3 example shape");
    let mut create_map = HashMap::new();
    create_map.insert("newPi".to_owned(), identity);
    let resp = sc
        .participant_identity_set(Some(create_map), None, None, None)
        .await
        .expect("participant_identity_set: must succeed");

    assert_eq!(resp.new_state, "ps3", "newState mismatch");
    let created = resp.created.expect("created map must be present");
    assert!(
        created.contains_key("newPi"),
        "created must contain 'newPi' creation-id key"
    );
    assert_eq!(
        created["newPi"]
            .id
            .as_ref()
            .expect("server-assigned id must be present in created entry")
            .as_ref(),
        "pi-new",
        "server-assigned id mismatch"
    );

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    let create = args["create"]
        .as_object()
        .expect("create must be a JSON object");
    assert!(
        create.contains_key("newPi"),
        "create map must contain 'newPi' creation-id key"
    );
    assert_eq!(
        create["newPi"]["calendarAddress"],
        json!("mailto:jane@work.example.com"),
        "calendarAddress passthrough mismatch"
    );
    // RFC 8620 §5.3: id MUST NOT be set by the client on creation. The
    // server-side handler will reject any present id with invalidProperties.
    // Pinning the wire shape here so a regression that re-adds id to the
    // create-map (e.g. by making ParticipantIdentity.id required again) is
    // caught immediately rather than at deploy time against a strict server.
    assert!(
        create["newPi"].get("id").is_none(),
        "id must not be set by client on creation per RFC 8620 §5.3, got: {}",
        create["newPi"]
    );
    assert!(
        args.get("update").is_none(),
        "update must be absent when caller passes None"
    );
    assert!(
        args.get("destroy").is_none(),
        "destroy must be absent when caller passes None"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §3.3 / RFC 8620 §5.3 —
/// destroy-only `ParticipantIdentity/set` carries only the `destroy`
/// list. Verifies that omitting `create` and `update` (passing `None`)
/// keeps those keys off the wire.
#[tokio::test]
async fn participant_identity_set_destroy_only_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ParticipantIdentity/set",
            {
                "accountId": "A13824",
                "oldState": "ps3",
                "newState": "ps4",
                "created": null,
                "updated": null,
                "destroyed": ["pi-old"],
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
    let destroy_ids = [Id::from("pi-old")];
    let _ = sc
        .participant_identity_set(None, None, Some(&destroy_ids), None)
        .await
        .expect("participant_identity_set: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["destroy"], json!(["pi-old"]), "destroy array mismatch");
    assert!(
        args.get("create").is_none(),
        "create must be absent when None passed"
    );
    assert!(
        args.get("update").is_none(),
        "update must be absent when None passed"
    );
}
