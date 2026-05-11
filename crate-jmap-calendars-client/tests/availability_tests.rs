//! Wiremock integration test for Principal/getAvailability.
//!
//! Oracle for response shape: draft-ietf-jmap-calendars-26 §2.2.
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, UTCDate};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test JMAP-7i4v.5 — Principal/getAvailability round-trip.
///
/// Oracle: draft-ietf-jmap-calendars-26 §2.2 — the response contains a list of
/// BusyPeriod objects within the queried time window. An empty list means the
/// principal has no busy time in the queried range.
///
/// Wire key for the principal id is "id", not "principalId" (§2.2 explicitly
/// uses "id" matching the principal object identifier).
#[tokio::test]
async fn principal_get_availability_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Principal/getAvailability",
            {
                "accountId": "A13824",
                "list": []
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
    let principal_id = Id::from("p-joe");
    let utc_start = UTCDate::new_validated("2024-06-15T09:00:00Z")
        .expect("hand-written RFC 8620 §1.4 UTCDate fixture must validate");
    let utc_end = UTCDate::new_validated("2024-06-15T10:00:00Z")
        .expect("hand-written RFC 8620 §1.4 UTCDate fixture must validate");
    let resp = sc
        .principal_get_availability(&principal_id, &utc_start, &utc_end, None, None)
        .await
        .expect("principal_get_availability_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert!(
        resp.list.is_empty(),
        "list must be empty — no busy periods in range"
    );

    // Verify the wire request uses "id" (not "principalId") per §2.2.
    let reqs = server
        .received_requests()
        .await
        .expect("principal_get_availability_round_trip: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("principal_get_availability_round_trip: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["id"],
        json!("p-joe"),
        "wire key must be 'id', not 'principalId'"
    );
    assert!(
        args.get("principalId").is_none(),
        "'principalId' must NOT appear in wire request"
    );
    assert_eq!(
        args["utcStart"],
        json!("2024-06-15T09:00:00Z"),
        "utcStart mismatch"
    );
    assert_eq!(
        args["utcEnd"],
        json!("2024-06-15T10:00:00Z"),
        "utcEnd mismatch"
    );
}
