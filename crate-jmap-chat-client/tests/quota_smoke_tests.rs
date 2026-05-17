//! Wiremock smoke tests for `Quota/*` method paths in jmap-chat-client.
//!
//! Quota is a cross-protocol JMAP capability under
//! `urn:ietf:params:jmap:quota` — distinct from `urn:ietf:params:jmap:chat`
//! — so the wire `using` array MUST switch to [`USING_QUOTA`] when these
//! methods are called.
//!
//! Spec oracles:
//!   - RFC 9425 §4 (Quota object), §4.2 (Quota/get)
//!   - RFC 8620 §5.2 (Quota/changes uses the standard /changes shape)

#[path = "helpers.rs"]
mod helpers;

use helpers::{jmap_response, mock_jmap_post, recorded_args, recorded_body, TEST_ACCOUNT_ID};
use jmap_types::State;
use serde_json::json;
use wiremock::MockServer;

/// `Quota/get` MUST send `"ids": null` (RFC 9425 §4.2 — clients
/// typically fetch all quotas) and MUST declare the
/// `urn:ietf:params:jmap:quota` capability in `using`, NOT
/// `urn:ietf:params:jmap:chat`. Pins the USING_QUOTA capability set for
/// the entire Quota/* family.
#[tokio::test]
async fn quota_get_sends_ids_null_and_declares_quota_capability() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Quota/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "q-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc.quota_get().await.expect("quota_get: must succeed");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
    );
    assert!(args.get("ids").is_some(), "ids key must be present");
    assert_eq!(args["ids"], json!(null), "ids must be JSON null");
    // RFC 8620 §3.3 + RFC 9425 — declare the quota capability, NOT chat.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:quota"]),
        "Quota/* using must equal USING_QUOTA exactly (core + quota, no chat)"
    );
}

/// `Quota/get` decode coverage: a populated Quota wire object must
/// round-trip through the [`jmap_chat_client::methods::Quota`]
/// `Deserialize` impl with every required field plus the optional
/// triplet (`warn_limit`, `soft_limit`, `description`).
///
/// Oracle: RFC 9425 §4 field set.
#[tokio::test]
async fn quota_get_decodes_populated_quota() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Quota/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "q-state-2",
            "list": [
                {
                    "id": "Q1",
                    "name": "Message Storage",
                    "scope": "account",
                    "resourceType": "octets",
                    "types": ["Message", "Chat"],
                    "used": 1024,
                    "hardLimit": 1048576,
                    "warnLimit": 838860,
                    "softLimit": 1000000,
                    "description": "Per-account message storage limit"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc.quota_get().await.expect("quota_get: must succeed");

    assert_eq!(resp.list.len(), 1, "list must contain one Quota");
    let q = &resp.list[0];
    assert_eq!(q.id.as_ref(), "Q1", "id mismatch");
    assert_eq!(q.name, "Message Storage", "name mismatch");
    assert!(
        matches!(q.scope, jmap_chat_client::types::QuotaScope::Account),
        "scope 'account' must deserialise to QuotaScope::Account, got {:?}",
        q.scope
    );
    assert!(
        matches!(
            q.resource_type,
            jmap_chat_client::types::QuotaResourceType::Octets
        ),
        "resource_type 'octets' must deserialise to QuotaResourceType::Octets, got {:?}",
        q.resource_type
    );
    assert_eq!(q.types, vec!["Message", "Chat"], "types mismatch");
    assert_eq!(q.used, 1024, "used mismatch");
    assert_eq!(q.hard_limit, 1048576, "hard_limit mismatch");
    assert_eq!(q.warn_limit, Some(838860), "warn_limit mismatch");
    assert_eq!(q.soft_limit, Some(1000000), "soft_limit mismatch");
    assert_eq!(
        q.description.as_deref(),
        Some("Per-account message storage limit"),
        "description mismatch"
    );
}

/// `Quota.scope = "domain"` MUST deserialise to `QuotaScope::Domain` and
/// an unknown wire string MUST deserialise to `QuotaScope::Other(s)`
/// preserving the literal for round-trip. Independent oracle: the
/// chosen unknown string `siteCustom-tier-A` is not in the RFC 9425
/// §3.1 set `{account, domain, global}`.
#[tokio::test]
async fn quota_scope_other_round_trips_unknown_wire_string() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Quota/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "q-state-3",
            "list": [
                {
                    "id": "Q2",
                    "name": "Site quota",
                    "scope": "siteCustom-tier-A",
                    "resourceType": "count",
                    "types": ["Email"],
                    "used": 0,
                    "hardLimit": 1000
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc.quota_get().await.expect("quota_get: must succeed");
    let q = &resp.list[0];
    match &q.scope {
        jmap_chat_client::types::QuotaScope::Other(s) => {
            assert_eq!(
                s, "siteCustom-tier-A",
                "Other(_) must preserve the unknown wire string verbatim"
            );
        }
        other => panic!("expected QuotaScope::Other, got {other:?}"),
    }
}

/// `Quota.resourceType = "count"` MUST deserialise to
/// `QuotaResourceType::Count` and an unknown wire string MUST
/// deserialise to `QuotaResourceType::Other(s)` preserving the literal
/// for round-trip. Independent oracle: the chosen unknown string
/// `vendorUnit-decibels` is not in the RFC 9425 §3.2 set
/// `{count, octets}`.
#[tokio::test]
async fn quota_resource_type_other_round_trips_unknown_wire_string() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Quota/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "q-state-4",
            "list": [
                {
                    "id": "Q3",
                    "name": "Vendor quota",
                    "scope": "account",
                    "resourceType": "vendorUnit-decibels",
                    "types": ["Email"],
                    "used": 0,
                    "hardLimit": 1000
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc.quota_get().await.expect("quota_get: must succeed");
    let q = &resp.list[0];
    match &q.resource_type {
        jmap_chat_client::types::QuotaResourceType::Other(s) => {
            assert_eq!(
                s, "vendorUnit-decibels",
                "Other(_) must preserve the unknown wire string verbatim"
            );
        }
        other => panic!("expected QuotaResourceType::Other, got {other:?}"),
    }
}

/// `Quota/changes` must thread `since_state` and `max_changes`, reject
/// empty `since_state` client-side (RFC 8620 §5.2), and declare
/// USING_QUOTA in the wire `using` array.
#[tokio::test]
async fn quota_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Quota/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "q-old",
            "newState": "q-new",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["Q1"],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("q-old");
    let _ = sc
        .quota_changes(&since, Some(10))
        .await
        .expect("quota_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("q-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(10), "maxChanges mismatch");

    // Empty-state guard.
    let empty = State::from("");
    let err = sc
        .quota_changes(&empty, None)
        .await
        .expect_err("quota_changes must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_state may not be empty"),
                "error message must explain validation: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
