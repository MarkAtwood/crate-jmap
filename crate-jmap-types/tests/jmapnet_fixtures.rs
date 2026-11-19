//! Cross-implementation fixtures from JMAP.Net (github.com/JMAP-Net/JMAP.Net
//! at db60d47, MIT, Copyright (c) 2025 JMAP.Net), bd JMAP-48o0. They are
//! that library's serializer output, checked by its own test suite (165 of
//! 166 passing; the failure is an HTTP limit test, not a fixture). JMAP.Net
//! targets draft-ietf-jmap-calendars-26 and RFC 8620; where it and this
//! crate disagree the draft text decides (calendars-34 here), and each
//! disagreement is asserted with its citation. See tests/fixtures/jmapnet/README.md.

use jmap_types::problem::ProblemDetails;
use jmap_types::{
    ChangesResponse, GetResponse, JmapError, JmapRequest, JmapResponse, QueryChangesResponse,
    QueryResponse, SetResponse,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/jmapnet/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("JSON")
}

/// Parse `name` as `T` and serialize it back: RFC 8620 shapes must survive
/// the round trip unchanged, every member included.
fn round_trips<T: DeserializeOwned + Serialize>(name: &str) {
    let original = fixture(name);
    let parsed: T = serde_json::from_value(original.clone())
        .unwrap_or_else(|e| panic!("{name} does not parse: {e}"));
    assert_eq!(serde_json::to_value(parsed).unwrap(), original, "{name}");
}

#[test]
fn request_with_result_reference() {
    round_trips::<JmapRequest>("jmap-request-with-result-reference");
}

#[test]
fn response_with_created_ids() {
    round_trips::<JmapResponse>("jmap-response-with-created-ids");
}

#[test]
fn method_error_invalid_arguments() {
    round_trips::<JmapError>("jmap-error-invalid-arguments");
}

#[test]
fn request_level_problem_details_limit() {
    round_trips::<ProblemDetails>("problem-details-limit");
}

#[test]
fn get_response_minimal() {
    round_trips::<GetResponse<Value>>("get-response-minimal");
}

#[test]
fn changes_response() {
    round_trips::<ChangesResponse>("changes-response");
}

#[test]
fn query_response_with_total() {
    round_trips::<QueryResponse>("query-response-with-total");
}

#[test]
fn query_changes_response() {
    round_trips::<QueryChangesResponse>("query-changes-response");
}

#[test]
fn set_response_with_errors() {
    round_trips::<SetResponse<Value>>("set-response-with-errors");
}
