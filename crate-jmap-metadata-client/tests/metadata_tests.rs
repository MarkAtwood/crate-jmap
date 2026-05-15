//! Wiremock integration tests for Metadata/get, /changes, /set, /query,
//! /queryChanges.
//!
//! Oracle for all response shapes: draft-ietf-jmap-metadata-01 §3 and
//! RFC 8620 §5.
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test JMAP-06zp.4 #1 — Metadata/get returns the Metadata list.
///
/// Oracle: draft-ietf-jmap-metadata-01 §3.2 — passing ids=null returns all
/// Metadata objects. Response shape from §1.6 example.
#[tokio::test]
async fn metadata_get_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [
                    {
                        "@type": "Annotation",
                        "id": "MD1",
                        "relatedType": "Email",
                        "relatedId": "EM1",
                        "isPrivate": false,
                        "acme.example.com:color": "blue"
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
        .metadata_get(None, None, None)
        .await
        .expect("metadata_get_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.state, "s5", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 metadata object");
    assert_eq!(
        resp.list[0].id().map(|id| id.as_ref()),
        Some("MD1"),
        "id mismatch"
    );
    assert_eq!(resp.list[0].related_type(), "Email", "relatedType mismatch");
    assert!(!resp.list[0].is_private(), "isPrivate must be false");
}

/// Test JMAP-06zp.4 #2 — Metadata/changes sends sinceState in the request.
///
/// Oracle: RFC 8620 §5.2 — sinceState is a required argument.
/// RFC 8620 §5.2 — changes response shape.
#[tokio::test]
async fn metadata_changes_sends_since_state() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/changes",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s11",
                "hasMoreChanges": false,
                "created": ["MD-new"],
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
        .metadata_changes(&jmap_types::State::from("s10"), None, None)
        .await
        .expect("metadata_changes_sends_since_state: must succeed");

    assert_eq!(resp.old_state, "s10", "oldState mismatch");
    assert_eq!(resp.new_state, "s11", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");
    assert!(
        resp.created.iter().any(|id| id.as_ref() == "MD-new"),
        "created must contain MD-new"
    );

    // Verify sinceState was sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("metadata_changes_sends_since_state: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("metadata_changes_sends_since_state: request body must be valid JSON");
    assert_eq!(
        body["methodCalls"][0][1]["sinceState"],
        json!("s10"),
        "sinceState must be s10 in wire request"
    );
}

/// Test JMAP-06zp.4 #3 — Metadata/changes with filterRelatedType and
/// filterMetadataType sends those keys in the wire request.
///
/// Oracle: draft-ietf-jmap-metadata-01 §3.3 — exact wire field names.
#[tokio::test]
async fn metadata_changes_passes_filters() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/changes",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s11",
                "hasMoreChanges": false,
                "created": [],
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
    let params = jmap_metadata_client::MetadataChangesParams {
        filter_related_type: Some("Email".into()),
        filter_metadata_type: Some(vec!["Annotation".into()]),
        extra: serde_json::Map::new(),
    };
    let _resp = sc
        .metadata_changes(&jmap_types::State::from("s10"), None, Some(params))
        .await
        .expect("metadata_changes_passes_filters: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("metadata_changes_passes_filters: must have recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("metadata_changes_passes_filters: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filterRelatedType"],
        json!("Email"),
        "filterRelatedType must be 'Email' in wire request"
    );
    assert_eq!(
        args["filterMetadataType"],
        json!(["Annotation"]),
        "filterMetadataType must be [\"Annotation\"] in wire request"
    );
}

/// Test JMAP-06zp.4 #4 — Metadata/set create round-trip.
///
/// Oracle: draft-ietf-jmap-metadata-01 §3.1 + RFC 8620 §5.3 — /set create
/// returns server-assigned id in the created map.
#[tokio::test]
async fn metadata_set_create_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "created": {
                    "newMeta": {
                        "@type": "Annotation",
                        "id": "server-md-id",
                        "relatedType": "Email",
                        "relatedId": "EM1",
                        "isPrivate": false,
                        "acme.example.com:tag": "important"
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
        "newMeta": {
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false,
            "acme.example.com:tag": "important"
        }
    });
    let resp = sc
        .metadata_set(Some(create_obj), None, None, None, None)
        .await
        .expect("metadata_set_create_round_trip: must succeed");

    assert_eq!(resp.new_state, "s2", "newState mismatch");
    let created = resp.created.expect("created must be present");
    assert!(
        created.contains_key("newMeta"),
        "created must contain 'newMeta' key"
    );
    let meta = &created["newMeta"];
    assert_eq!(
        meta.id().map(|id| id.as_ref()),
        Some("server-md-id"),
        "server-assigned id mismatch"
    );
}

/// Test JMAP-06zp.4 #5 — Metadata/query with filter and sort sends the
/// expected wire shape.
///
/// Oracle: draft-ietf-jmap-metadata-01 §3.4 + RFC 8620 §5.5.
#[tokio::test]
async fn metadata_query_with_filter_and_sort() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["MD1", "MD2"],
                "total": 2
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
    let filter = json!({ "relatedType": "Email" });
    let sort = json!([{ "property": "id", "isAscending": true }]);
    let resp = sc
        .metadata_query(
            Some(filter.clone()),
            Some(sort.clone()),
            Some(0),
            Some(50),
            None,
        )
        .await
        .expect("metadata_query_with_filter_and_sort: must succeed");

    assert_eq!(resp.query_state, "qs1", "queryState mismatch");
    assert_eq!(resp.ids.len(), 2, "ids count mismatch");

    let reqs = server
        .received_requests()
        .await
        .expect("metadata_query_with_filter_and_sort: must have recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("metadata_query_with_filter_and_sort: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["filter"], filter, "filter must be passed through");
    assert_eq!(args["sort"], sort, "sort must be passed through");
    assert_eq!(args["position"], json!(0), "position must be 0");
    assert_eq!(args["limit"], json!(50), "limit must be 50");
}

/// Test JMAP-06zp.4 #6 — Metadata/queryChanges round-trip.
///
/// Oracle: draft-ietf-jmap-metadata-01 §3.5 + RFC 8620 §5.6.
#[tokio::test]
async fn metadata_query_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs1",
                "newQueryState": "qs2",
                "total": 3,
                "removed": ["MD-gone"],
                "added": [{ "id": "MD-new", "index": 1 }]
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
        .metadata_query_changes(&jmap_types::State::from("qs1"), None, None)
        .await
        .expect("metadata_query_changes_round_trip: must succeed");

    assert_eq!(resp.old_query_state, "qs1", "oldQueryState mismatch");
    assert_eq!(resp.new_query_state, "qs2", "newQueryState mismatch");
    assert_eq!(resp.total, Some(3), "total mismatch");
    assert_eq!(resp.removed.len(), 1, "removed count mismatch");
    assert_eq!(resp.added.len(), 1, "added count mismatch");
    assert_eq!(resp.added[0].id.as_ref(), "MD-new", "added id mismatch");
    assert_eq!(resp.added[0].index, 1, "added index mismatch");

    // Verify sinceQueryState was sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("metadata_query_changes_round_trip: must have recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("metadata_query_changes_round_trip: request body must be valid JSON");
    assert_eq!(
        body["methodCalls"][0][1]["sinceQueryState"],
        json!("qs1"),
        "sinceQueryState must be qs1 in wire request"
    );
}

/// Test JMAP-wzq9.1 — Metadata/set passes `ifInState` in the wire request
/// when `if_in_state` is `Some`.
///
/// Oracle: RFC 8620 §5.3 — `ifInState` is a top-level /set arg the server
/// uses for optimistic-concurrency guard. Wire field name is camelCase.
#[tokio::test]
async fn metadata_set_passes_if_in_state() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/set",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s11",
                "created": null,
                "updated": null,
                "destroyed": ["MD-gone"],
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
    let state = jmap_types::State::from("s10");
    let _resp = sc
        .metadata_set(
            None,
            None,
            Some(vec![jmap_types::Id::from("MD-gone")]),
            Some(&state),
            None,
        )
        .await
        .expect("metadata_set_passes_if_in_state: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("metadata_set_passes_if_in_state: must have recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("metadata_set_passes_if_in_state: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["ifInState"],
        json!("s10"),
        "ifInState must be 's10' in wire request"
    );
}

/// Test JMAP-wzq9.2 — `MetadataSetParams.extra` flattens into the
/// `Metadata/set` wire request.
///
/// Oracle: workspace extras-preservation policy — vendor extras MUST round-
/// trip into the args object. Uses `acmeCorpAuditFlag` (synthetic) so the
/// assertion is independent of any draft-defined typed field.
#[tokio::test]
async fn metadata_set_propagates_params_extras() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "created": null,
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
    let mut params = jmap_metadata_client::MetadataSetParams::default();
    params.extra.insert("acmeCorpAuditFlag".into(), json!(true));
    let _resp = sc
        .metadata_set(None, None, None, None, Some(params))
        .await
        .expect("metadata_set_propagates_params_extras: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("metadata_set_propagates_params_extras: must have recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("metadata_set_propagates_params_extras: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["acmeCorpAuditFlag"],
        json!(true),
        "acmeCorpAuditFlag from MetadataSetParams.extra must propagate to args"
    );
}

/// Test JMAP-wzq9.2 — extras keys colliding with typed wire fields do NOT
/// overwrite the typed value (`Map::entry(...).or_insert` semantics).
///
/// Oracle: documented collision contract on `MetadataSetParams.extra` and
/// every sibling params struct. A caller passing `extra["accountId"]` is
/// silently ignored; the session-derived accountId wins.
#[tokio::test]
async fn metadata_set_params_extras_do_not_overwrite_account_id() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Metadata/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "created": null,
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
    let mut params = jmap_metadata_client::MetadataSetParams::default();
    // Caller-supplied extras attempt to overwrite a typed field.
    params.extra.insert("accountId".into(), json!("HIJACKED"));
    let _resp = sc
        .metadata_set(None, None, None, None, Some(params))
        .await
        .expect("metadata_set_params_extras_do_not_overwrite_account_id: must succeed");

    let reqs = server.received_requests().await.expect(
        "metadata_set_params_extras_do_not_overwrite_account_id: must have recorded requests",
    );
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).expect(
        "metadata_set_params_extras_do_not_overwrite_account_id: request body must be valid JSON",
    );
    let args = &body["methodCalls"][0][1];
    // The session-derived accountId must win over the extras attempt.
    assert_eq!(
        args["accountId"],
        json!("A13824"),
        "accountId from session must win over extras hijack attempt"
    );
}
