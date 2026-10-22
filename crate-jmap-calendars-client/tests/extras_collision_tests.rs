//! Wire-level tests that a caller-supplied `extra` entry cannot shadow a
//! typed field of the same `*Params` struct (bd:JMAP-tjvm.38).
//!
//! Oracles:
//!   - The `extra` rustdoc contract (workspace AGENTS.md "Caller
//!     contract — wire-key collisions" and the client `*Params` docs):
//!     on collision the typed-field value wins on the wire.
//!   - RFC 8259 §4: "The names within an object SHOULD be unique." The
//!     raw request body must therefore carry each colliding name once.
//!
//! Expected values are the hand-written typed values set by each test,
//! never values produced by the code under test.

#[path = "helpers.rs"]
mod helpers;

use jmap_calendars_client::{
    CalendarEventGetParams, CalendarEventParseParams, PrincipalGetAvailabilityParams,
};
use jmap_types::{Id, UTCDate};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mounts a single canned method response and returns the server.
async fn mock(method_name: &str, response_args: Value) -> MockServer {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[method_name, response_args, "r1"]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;
    server
}

/// Returns the raw body text and the parsed first method-call arguments.
async fn sent_args(server: &MockServer) -> (String, Value) {
    let reqs = server.received_requests().await.expect("recording enabled");
    let raw = String::from_utf8(reqs[0].body.clone()).expect("body is UTF-8");
    let body: Value = serde_json::from_str(&raw).expect("body is JSON");
    (raw, body["methodCalls"][0][1].clone())
}

#[tokio::test]
async fn calendar_event_get_typed_param_wins_over_colliding_extra() {
    let server = mock(
        "CalendarEvent/get",
        json!({ "accountId": "A13824", "state": "s1", "list": [], "notFound": [] }),
    )
    .await;
    let mut params = CalendarEventGetParams {
        expand_recurrences: Some(false),
        ..CalendarEventGetParams::default()
    };
    params.extra.insert("expandRecurrences".into(), json!(true));
    params.extra.insert("acmeCorpX".into(), json!("kept"));

    helpers::make_client(&server)
        .calendar_event_get(None, None, Some(params))
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    assert_eq!(args["expandRecurrences"], json!(false));
    assert_eq!(args["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"expandRecurrences\"").count(), 1);
}

#[tokio::test]
async fn calendar_event_parse_typed_param_wins_over_colliding_extra() {
    let server = mock(
        "CalendarEvent/parse",
        json!({ "accountId": "A13824", "parsed": null, "notFound": null, "notParsable": null }),
    )
    .await;
    let mut params = CalendarEventParseParams {
        properties: Some(vec!["title".to_owned()]),
        ..CalendarEventParseParams::default()
    };
    params.extra.insert("properties".into(), json!(["SPOOF"]));
    params.extra.insert("acmeCorpX".into(), json!("kept"));

    helpers::make_client(&server)
        .calendar_event_parse(&[Id::from("blob1")], Some(params))
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    assert_eq!(args["properties"], json!(["title"]));
    assert_eq!(args["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"properties\"").count(), 1);
    assert!(!raw.contains("SPOOF"));
}

#[tokio::test]
async fn principal_get_availability_typed_param_wins_over_colliding_extra() {
    let server = mock(
        "Principal/getAvailability",
        json!({ "accountId": "A13824", "list": [] }),
    )
    .await;
    let mut params = PrincipalGetAvailabilityParams {
        show_details: Some(false),
        ..PrincipalGetAvailabilityParams::default()
    };
    params.extra.insert("showDetails".into(), json!(true));
    params.extra.insert("acmeCorpX".into(), json!("kept"));
    let start = UTCDate::new_validated("2024-06-15T09:00:00Z").expect("RFC 8620 §1.4 UTCDate");
    let end = UTCDate::new_validated("2024-06-15T10:00:00Z").expect("RFC 8620 §1.4 UTCDate");

    helpers::make_client(&server)
        .principal_get_availability(&Id::from("p-joe"), &start, &end, Some(params))
        .await
        .expect("must succeed");

    let (raw, args) = sent_args(&server).await;
    assert_eq!(args["showDetails"], json!(false));
    assert_eq!(args["acmeCorpX"], json!("kept"));
    assert_eq!(raw.matches("\"showDetails\"").count(), 1);
}
