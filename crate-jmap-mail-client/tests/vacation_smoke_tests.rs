//! Wiremock smoke tests for VacationResponse/set.
//!
//! VacationResponse is a singleton (RFC 8621 §8) — its only meaningful /set
//! operation is `update` against the well-known id `singleton`. The wire
//! request must carry the PatchObject as the `update` map.
//!
//! Oracles:
//!   - RFC 8621 §8.1 — VacationResponse object (id, isEnabled, fromDate,
//!     toDate, subject, textBody, htmlBody)
//!   - RFC 8621 §8.2 — VacationResponse/set semantics (singleton, "singleton" id)
//!   - RFC 8620 §5.3 — /set wire envelope
//!   - RFC 8620 §5.3 (PatchObject) — partial updates via JSON-Pointer keys

#[path = "helpers.rs"]
mod helpers;

use std::collections::HashMap;

use jmap_types::{Id, PatchObject};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// VacationResponse/set with a PatchObject update must serialize the patch
/// map under the `update` wire key, keyed by the singleton id.
///
/// PatchObject is `#[serde(transparent)]` over its inner JSON-Pointer map, so
/// the wire shape must be a plain object with patch keys (no wrapping).
#[tokio::test]
async fn vacation_response_set_patch_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "VacationResponse/set",
            {
                "accountId": "A13824",
                "oldState": "v1",
                "newState": "v2",
                "updated": {
                    "singleton": null
                },
                "notUpdated": null
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
    // Build the PatchObject the caller would pass: enable + replace subject.
    let mut patch = serde_json::Map::new();
    patch.insert("isEnabled".to_owned(), json!(true));
    patch.insert("subject".to_owned(), json!("Out of office"));
    patch.insert(
        "textBody".to_owned(),
        json!("I will respond when I return."),
    );
    let patch_obj = PatchObject::from(patch);

    let mut update = HashMap::new();
    update.insert(Id::from("singleton"), patch_obj);

    let _ = sc
        .vacation_response_set(Some(update))
        .await
        .expect("vacation_response_set: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");

    let wire_update = &args["update"];
    assert!(
        wire_update.is_object(),
        "update must be a JSON object, got {wire_update:?}"
    );
    let singleton_patch = &wire_update["singleton"];
    assert!(
        singleton_patch.is_object(),
        "singleton patch must be a JSON object (PatchObject is transparent), got {singleton_patch:?}"
    );
    // The inner patch fields must appear at the top of the patch object —
    // no wrapping in a "patches" key, no array of operations.
    assert_eq!(
        singleton_patch["isEnabled"],
        json!(true),
        "patch isEnabled must be true"
    );
    assert_eq!(
        singleton_patch["subject"],
        json!("Out of office"),
        "patch subject must be passed through"
    );
    assert_eq!(
        singleton_patch["textBody"],
        json!("I will respond when I return."),
        "patch textBody must be passed through"
    );

    // The /set wire envelope omits create/destroy because the caller passed
    // None (this method doesn't accept them at all for VacationResponse, which
    // is a singleton — see RFC 8621 §8.2).
    assert!(args.get("create").is_none(), "create must be omitted");
    assert!(args.get("destroy").is_none(), "destroy must be omitted");

    // RFC 8621 §1.3.3: VacationResponse requires
    // `urn:ietf:params:jmap:vacationresponse` and is independent of
    // `urn:ietf:params:jmap:mail` (no mail-typed references).
    let using = body["using"].as_array().expect("using must be array");
    assert!(
        using.contains(&json!("urn:ietf:params:jmap:vacationresponse")),
        "VacationResponse/set must send urn:ietf:params:jmap:vacationresponse \
         (RFC 8621 §1.3.3); got: {using:?}"
    );
    assert!(
        !using.contains(&json!("urn:ietf:params:jmap:mail")),
        "VacationResponse/set must NOT send urn:ietf:params:jmap:mail \
         (VacationResponse is a standalone capability); got: {using:?}"
    );
}

/// VacationResponse/set with `update: None` must omit the `update` wire key
/// entirely. A no-op /set is rare but legal (RFC 8620 §5.3 — all of create,
/// update, destroy are optional).
#[tokio::test]
async fn vacation_response_set_no_update_omits_key() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "VacationResponse/set",
            {
                "accountId": "A13824",
                "oldState": "v1",
                "newState": "v1",
                "created": null,
                "updated": null,
                "destroyed": null
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
    let _ = sc
        .vacation_response_set(None)
        .await
        .expect("vacation_response_set: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert!(
        args.get("update").is_none(),
        "update must be omitted when caller passes None"
    );
}
