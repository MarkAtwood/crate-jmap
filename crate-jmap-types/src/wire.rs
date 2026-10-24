//! RFC 8620 §3 JMAP request/response envelope types ([`JmapRequest`], [`JmapResponse`], [`Invocation`]).

use crate::id::{Id, State};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A JMAP method invocation (RFC 8620 §3.2).
///
/// On the wire this is a 3-element JSON array
/// `[name, arguments, callId]`; in Rust the elements are named fields.
/// [`From`] conversions to and from `(String, Value, String)` are
/// provided for code that already holds the tuple form.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    /// Method name, e.g. `"Email/get"`, or `"error"` in a response.
    pub name: String,
    /// Method arguments (request) or result (response) object.
    pub arguments: serde_json::Value,
    /// Client-chosen method call id, echoed back in the response.
    pub call_id: String,
}

impl Invocation {
    /// Construct an [`Invocation`] from its three wire elements.
    pub fn new(
        name: impl Into<String>,
        arguments: serde_json::Value,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            arguments,
            call_id: call_id.into(),
        }
    }
}

impl From<(String, serde_json::Value, String)> for Invocation {
    fn from((name, arguments, call_id): (String, serde_json::Value, String)) -> Self {
        Self {
            name,
            arguments,
            call_id,
        }
    }
}

impl From<Invocation> for (String, serde_json::Value, String) {
    fn from(inv: Invocation) -> Self {
        (inv.name, inv.arguments, inv.call_id)
    }
}

impl Serialize for Invocation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (&self.name, &self.arguments, &self.call_id).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Invocation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <(String, serde_json::Value, String)>::deserialize(deserializer).map(Self::from)
    }
}

/// JMAP request envelope (RFC 8620 §3.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JmapRequest {
    /// Capability URIs this request uses, e.g. `["urn:ietf:params:jmap:core"]`.
    pub using: Vec<String>,
    /// Ordered list of method invocations.
    #[serde(rename = "methodCalls")]
    pub method_calls: Vec<Invocation>,
    /// Client-supplied creation ID map (optional, RFC 8620 §3.3).
    #[serde(rename = "createdIds", skip_serializing_if = "Option::is_none")]
    pub created_ids: Option<BTreeMap<Id, Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// JMAP response envelope (RFC 8620 §3.4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JmapResponse {
    /// Ordered list of method responses (same `[name, arguments, callId]`
    /// structure as requests).
    #[serde(rename = "methodResponses")]
    pub method_responses: Vec<Invocation>,
    /// Opaque server state token. Changes when any data type's state advances.
    #[serde(rename = "sessionState")]
    pub session_state: State,
    /// Maps client-supplied creation IDs to server-assigned IDs.
    /// Omitted when no objects were created in the batch (RFC 8620 §3.4).
    #[serde(rename = "createdIds", skip_serializing_if = "Option::is_none")]
    pub created_ids: Option<BTreeMap<Id, Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl JmapRequest {
    /// Construct a [`JmapRequest`].
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// built with a struct literal outside this crate.
    pub fn new(
        using: Vec<String>,
        method_calls: Vec<Invocation>,
        created_ids: Option<BTreeMap<Id, Id>>,
    ) -> Self {
        Self {
            using,
            method_calls,
            created_ids,
            extra: serde_json::Map::new(),
        }
    }
}

impl JmapResponse {
    /// Construct a [`JmapResponse`].
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// built with a struct literal outside this crate.
    pub fn new(
        method_responses: Vec<Invocation>,
        session_state: State,
        created_ids: Option<BTreeMap<Id, Id>>,
    ) -> Self {
        Self {
            method_responses,
            session_state,
            created_ids,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Oracle: tests/fixtures/rfc8620-request.json (RFC 8620 §3.3.1)
    #[test]
    fn request_deserializes_from_rfc_fixture() {
        let raw = include_str!("../tests/fixtures/rfc8620-request.json");
        let req: JmapRequest = serde_json::from_str(raw).expect("deserialize JmapRequest");
        assert_eq!(req.using.len(), 2);
        assert_eq!(req.using[0], "urn:ietf:params:jmap:core");
        assert_eq!(req.using[1], "urn:ietf:params:jmap:mail");
        assert_eq!(req.method_calls.len(), 3);
        assert_eq!(req.method_calls[0].name, "method1");
        assert_eq!(
            req.method_calls[0].arguments,
            json!({"arg1": "arg1data", "arg2": "arg2data"})
        );
        assert_eq!(req.method_calls[0].call_id, "c1");
        assert_eq!(req.method_calls[2].name, "method3");
        // method3 has empty args {}
        assert_eq!(req.method_calls[2].arguments, json!({}));
        assert!(req.created_ids.is_none());
    }

    // Oracle: tests/fixtures/rfc8620-response.json (RFC 8620 §3.4.1)
    #[test]
    fn response_deserializes_from_rfc_fixture() {
        let raw = include_str!("../tests/fixtures/rfc8620-response.json");
        let resp: JmapResponse = serde_json::from_str(raw).expect("deserialize JmapResponse");
        assert_eq!(resp.session_state.as_ref(), "75128aab4b1b");
        assert_eq!(resp.method_responses.len(), 4);
        assert_eq!(resp.method_responses[0].name, "method1");
        assert_eq!(resp.method_responses[3].name, "error");
        assert_eq!(resp.method_responses[3].call_id, "c3");
        assert!(resp.created_ids.is_none());
    }

    // Oracle: RFC 8620 §3.3 — field name must be "methodCalls" (camelCase).
    #[test]
    fn request_serializes_camelcase() {
        let req = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core".into()],
            method_calls: vec![],
            created_ids: None,
            extra: serde_json::Map::new(),
        };
        let j = serde_json::to_string(&req).expect("serialize");
        assert!(
            j.contains("\"methodCalls\""),
            "must use camelCase methodCalls"
        );
        assert!(!j.contains("\"method_calls\""), "must not use snake_case");
    }

    // Oracle: RFC 8620 §3.4 — field names "methodResponses" and "sessionState".
    #[test]
    fn response_serializes_camelcase() {
        let resp = JmapResponse {
            method_responses: vec![],
            session_state: "s-1".into(),
            created_ids: None,
            extra: serde_json::Map::new(),
        };
        let j = serde_json::to_string(&resp).expect("serialize");
        assert!(j.contains("\"methodResponses\""));
        assert!(j.contains("\"sessionState\""));
    }

    // Oracle: RFC 8620 §3.4 — createdIds omitted when no objects were created.
    #[test]
    fn created_ids_absent_when_none() {
        let resp = JmapResponse {
            method_responses: vec![],
            session_state: "s-1".into(),
            created_ids: None,
            extra: serde_json::Map::new(),
        };
        let j = serde_json::to_string(&resp).expect("serialize");
        assert!(
            !j.contains("createdIds"),
            "createdIds must be absent when None"
        );
    }

    // Oracle: RFC 8620 §3.4 — createdIds present when objects were created.
    #[test]
    fn created_ids_present_when_some() {
        let mut ids = std::collections::BTreeMap::new();
        ids.insert(Id::from("c0"), Id::from("server-1"));
        let resp = JmapResponse {
            method_responses: vec![],
            session_state: "s-1".into(),
            created_ids: Some(ids),
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&resp).expect("serialize");
        assert_eq!(v["createdIds"]["c0"], "server-1");
    }

    // Oracle: RFC 8620 §3.2 — Invocation serializes as a 3-element JSON array.
    #[test]
    fn invocation_is_three_element_array() {
        let inv = Invocation::new("m/get", json!({"accountId": "a1"}), "c0");
        let j = serde_json::to_string(&inv).expect("serialize");
        assert_eq!(j, r#"["m/get",{"accountId":"a1"},"c0"]"#);
    }

    // Oracle: RFC 8620 §3.3.1 example request; the methodCalls array
    // re-serializes to the same JSON value element-for-element.
    #[test]
    fn rfc_request_method_calls_round_trip() {
        let raw = include_str!("../tests/fixtures/rfc8620-request.json");
        let original: serde_json::Value = serde_json::from_str(raw).expect("parse fixture");
        let req: JmapRequest = serde_json::from_str(raw).expect("deserialize JmapRequest");
        let back = serde_json::to_value(&req.method_calls).expect("serialize");
        assert_eq!(back, original["methodCalls"]);
    }

    // Oracle: RFC 8620 §3.2 — the Invocation "is a tuple, represented as a
    // JSON array containing three elements". Any other shape is rejected.
    #[test]
    fn invocation_rejects_wrong_arity() {
        assert!(serde_json::from_str::<Invocation>(r#"["m/get",{}]"#).is_err());
        assert!(serde_json::from_str::<Invocation>(r#"["m/get",{},"c0","x"]"#).is_err());
        assert!(serde_json::from_str::<Invocation>(r#"{"name":"m/get"}"#).is_err());
    }

    #[test]
    fn invocation_tuple_conversions() {
        let inv = Invocation::from(("m/get".to_owned(), json!({}), "c0".to_owned()));
        assert_eq!(inv.name, "m/get");
        assert_eq!(inv.arguments, json!({}));
        assert_eq!(inv.call_id, "c0");
        let t: (String, serde_json::Value, String) = inv.into();
        assert_eq!(t, ("m/get".to_owned(), json!({}), "c0".to_owned()));
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.1) ───────────────────
    //
    // Round-trip preservation tests asserting vendor / site / private-
    // extension fields survive deserialize/serialize unchanged on the
    // envelope types. Per workspace AGENTS.md "Extras-preservation policy
    // for vendor/site fields".

    /// `JmapRequest.extra` captures vendor envelope-level fields and
    /// preserves them on re-serialize.
    #[test]
    fn request_preserves_vendor_extras() {
        let raw = json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["m1", {}, "c0"]],
            "acmeCorpClientTraceId": "trace-7"
        });
        let req: JmapRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(
            req.extra
                .get("acmeCorpClientTraceId")
                .and_then(|v| v.as_str()),
            Some("trace-7"),
            "vendor field must land in extra: {:?}",
            req.extra
        );
        let back = serde_json::to_value(&req).unwrap();
        assert_eq!(
            back["acmeCorpClientTraceId"], "trace-7",
            "vendor field must survive serialize"
        );
    }

    /// `JmapResponse.extra` captures vendor envelope-level fields and
    /// preserves them on re-serialize.
    #[test]
    fn response_preserves_vendor_extras() {
        let raw = json!({
            "methodResponses": [],
            "sessionState": "s-1",
            "acmeCorpServerHostName": "srv-3"
        });
        let resp: JmapResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(
            resp.extra
                .get("acmeCorpServerHostName")
                .and_then(|v| v.as_str()),
            Some("srv-3")
        );
        let back = serde_json::to_value(&resp).unwrap();
        assert_eq!(back["acmeCorpServerHostName"], "srv-3");
    }

    /// Empty extras must NOT add any keys to the wire form — `skip_serializing_if`
    /// keeps the byte shape identical to the pre-migration envelope.
    #[test]
    fn empty_extras_omitted_from_wire() {
        let req = JmapRequest::new(vec!["urn:ietf:params:jmap:core".into()], vec![], None);
        let v = serde_json::to_value(&req).expect("serialize");
        let obj = v.as_object().expect("object");
        // Expected wire keys: using + methodCalls. createdIds skipped (None).
        // extras skipped (empty).
        assert_eq!(
            obj.len(),
            2,
            "empty extras + skipped optionals must yield exactly using+methodCalls; got {v}"
        );
        assert!(obj.contains_key("using"));
        assert!(obj.contains_key("methodCalls"));
    }

    /// Pins the workspace extras-preservation caller-contract for the
    /// foundation envelope type [`JmapRequest`].
    ///
    /// Per workspace AGENTS.md "Caller contract — wire-key collisions"
    /// (lines 292-312), callers MUST NOT insert keys into `extra` whose
    /// names match a typed field on the same struct. The contract is
    /// enforced at the doc/convention layer, not via a wrapper type.
    /// This test locks in the current `#[serde(flatten)]` behavior so a
    /// future serde change cannot silently alter the contract:
    ///
    /// 1. Serialize succeeds and emits BOTH the typed field's value and
    ///    the colliding extras entry verbatim, producing JSON with two
    ///    entries for the same key. RFC 8259 §4 calls this "unpredictable",
    ///    and downstream parsers vary in which value wins.
    /// 2. The crate's own `Deserialize` impl rejects the duplicate-key
    ///    wire form with a "duplicate field" error, so a serialize +
    ///    deserialize round-trip surfaces the contract violation rather
    ///    than silently propagating it.
    ///
    /// Companion regression marker in the extension-types canonical:
    /// `crate-jmap-mail-types/src/email.rs`
    /// `extra_collision_with_typed_field_round_trip_fails`.
    /// Filed under bd:JMAP-6xs8.4.
    ///
    /// Independent oracle: the expected duplicate-key count (two
    /// `"methodCalls":` substring occurrences) is built by string
    /// counting against the serialized bytes, not by the code under test.
    #[test]
    fn request_extra_collision_with_typed_field_round_trip_fails() {
        let mut req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![Invocation::new("real-method", json!({}), "c-real")],
            None,
        );
        // Caller-contract violation: inserting a key whose name matches
        // a typed field on the same struct.
        req.extra
            .insert("methodCalls".into(), json!([["acmeFake", {}, "c-fake"]]));

        // (1) Serialize succeeds and emits both keys.
        let serialised = serde_json::to_string(&req).expect("serialize must succeed");
        let occurrences = serialised.matches("\"methodCalls\":").count();
        assert_eq!(
            occurrences, 2,
            "wire output must contain two duplicate methodCalls keys; \
             got {serialised}"
        );

        // (2) Deserialize on the same bytes must reject with duplicate
        // field. RFC 8259 §4 calls duplicate keys "unpredictable" and
        // the workspace's strict-deserialize stance rejects them.
        let err = serde_json::from_str::<JmapRequest>(&serialised)
            .expect_err("deserialize must reject duplicate-key wire form");
        assert!(
            err.to_string().contains("duplicate field"),
            "error must mention duplicate field; got: {err}"
        );
    }

    // Oracle: workspace map-ordering policy (bd:JMAP-pdux) — wire-format
    // maps serialize in sorted key order. Expected bytes are hand-written.
    #[test]
    fn created_ids_serialize_in_sorted_key_order() {
        let mut ids = std::collections::BTreeMap::new();
        ids.insert(Id::from("c2"), Id::from("server-2"));
        ids.insert(Id::from("c1"), Id::from("server-1"));
        let resp = JmapResponse {
            method_responses: vec![],
            session_state: "s-1".into(),
            created_ids: Some(ids),
            extra: serde_json::Map::new(),
        };
        let j = serde_json::to_string(&resp).expect("serialize");
        assert!(
            j.contains(r#""createdIds":{"c1":"server-1","c2":"server-2"}"#),
            "createdIds keys must be sorted: {j}"
        );
    }
}
