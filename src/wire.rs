use crate::id::{Id, State};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A JMAP method invocation: `[method_name, arguments, call_id]`.
///
/// Serializes as a 3-element JSON array per RFC 8620 §3.2.
pub type Invocation = (String, serde_json::Value, String);

/// JMAP request envelope (RFC 8620 §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct JmapRequest {
    /// Capability URIs this request uses, e.g. `["urn:ietf:params:jmap:core"]`.
    pub using: Vec<String>,
    /// Ordered list of method invocations.
    #[serde(rename = "methodCalls")]
    pub method_calls: Vec<Invocation>,
    /// Client-supplied creation ID map (optional, RFC 8620 §3.3).
    #[serde(rename = "createdIds", skip_serializing_if = "Option::is_none")]
    pub created_ids: Option<HashMap<Id, Id>>,
}

/// JMAP response envelope (RFC 8620 §3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct JmapResponse {
    /// Ordered list of method responses (same 3-tuple structure as requests).
    #[serde(rename = "methodResponses")]
    pub method_responses: Vec<Invocation>,
    /// Opaque server state token. Changes when any data type's state advances.
    #[serde(rename = "sessionState")]
    pub session_state: State,
    /// Maps client-supplied creation IDs to server-assigned IDs.
    /// Omitted when no objects were created in the batch (RFC 8620 §3.4).
    #[serde(rename = "createdIds", skip_serializing_if = "Option::is_none")]
    pub created_ids: Option<HashMap<Id, Id>>,
}

impl JmapRequest {
    /// Construct a [`JmapRequest`].
    ///
    /// Required because the struct is `#[non_exhaustive]` and cannot be
    /// built with a struct literal outside this crate.
    pub fn new(
        using: Vec<String>,
        method_calls: Vec<Invocation>,
        created_ids: Option<HashMap<Id, Id>>,
    ) -> Self {
        Self {
            using,
            method_calls,
            created_ids,
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
        created_ids: Option<HashMap<Id, Id>>,
    ) -> Self {
        Self {
            method_responses,
            session_state,
            created_ids,
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
        assert_eq!(req.method_calls[0].0, "method1");
        assert_eq!(req.method_calls[0].2, "c1");
        assert_eq!(req.method_calls[2].0, "method3");
        // method3 has empty args {}
        assert_eq!(req.method_calls[2].1, json!({}));
        assert!(req.created_ids.is_none());
    }

    // Oracle: tests/fixtures/rfc8620-response.json (RFC 8620 §3.4.1)
    #[test]
    fn response_deserializes_from_rfc_fixture() {
        let raw = include_str!("../tests/fixtures/rfc8620-response.json");
        let resp: JmapResponse = serde_json::from_str(raw).expect("deserialize JmapResponse");
        assert_eq!(resp.session_state.as_ref(), "75128aab4b1b");
        assert_eq!(resp.method_responses.len(), 4);
        assert_eq!(resp.method_responses[0].0, "method1");
        assert_eq!(resp.method_responses[3].0, "error");
        assert_eq!(resp.method_responses[3].2, "c3");
        assert!(resp.created_ids.is_none());
    }

    // Oracle: RFC 8620 §3.3 — field name must be "methodCalls" (camelCase).
    #[test]
    fn request_serializes_camelcase() {
        let req = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core".into()],
            method_calls: vec![],
            created_ids: None,
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
        let mut ids = std::collections::HashMap::new();
        ids.insert(Id::from("c0"), Id::from("server-1"));
        let resp = JmapResponse {
            method_responses: vec![],
            session_state: "s-1".into(),
            created_ids: Some(ids),
        };
        let v = serde_json::to_value(&resp).expect("serialize");
        assert_eq!(v["createdIds"]["c0"], "server-1");
    }

    // Oracle: RFC 8620 §3.2 — Invocation serializes as a 3-element JSON array.
    #[test]
    fn invocation_is_three_element_array() {
        let inv: Invocation = ("m/get".into(), json!({"accountId": "a1"}), "c0".into());
        let j = serde_json::to_string(&inv).expect("serialize");
        assert_eq!(j, r#"["m/get",{"accountId":"a1"},"c0"]"#);
    }
}
