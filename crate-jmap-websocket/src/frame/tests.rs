//! Oracles: the RFC 8887 §4.4 example messages, copied verbatim from
//! rfc8887.txt (indentation kept), and the property lists of §4.3.2 to
//! §4.3.5.3.

use super::*;
use jmap_types::{Id, State};
use serde_json::json;
use std::collections::BTreeMap;

const PUSH_ENABLE_EXAMPLE: &str = r#"
   {
     "@type": "WebSocketPushEnable",
     "dataTypes": [ "Mailbox", "Email" ],
     "pushState": "aaa"
   }"#;

const STATE_CHANGE_EXAMPLE: &str = r#"
                                    {
                                      "@type": "StateChange",
                                      "changed": {
                                        "a456": {
                                          "Mailbox": "d35ecb040aab"
                                        }
                                      },
                                      "pushState": "bbb"
                                    }"#;

const REQUEST_EXAMPLE: &str = r#"
   {
     "@type": "Request",
     "id": "R1",
     "using": [ "urn:ietf:params:jmap:core" ],
     "methodCalls": [
       [
         "Core/echo", {
           "hello": true,
           "high": 5
         },
         "b3ff"
       ]
     ]
   }"#;

const RESPONSE_EXAMPLE: &str = r#"
                                    {
                                      "@type": "Response",
                                      "requestId": "R1",
                                      "methodResponses": [
                                        [
                                          "Core/echo", {
                                            "hello": true,
                                            "high": 5
                                          },
                                          "b3ff"
                                        ]
                                      ]
                                    }"#;

const REQUEST_ERROR_EXAMPLE: &str = r#"
                                    {
                                      "@type": "RequestError",
                                      "requestId": null,
                                      "type":
                                "urn:ietf:params:jmap:error:notJSON",
                                      "status": 400,
                                      "detail":
                                "The request did not parse as I-JSON."
                                    }"#;

/// The second StateChange example, as printed: the comma after the
/// `changed` object is missing, so it is not JSON.
const STATE_CHANGE_EXAMPLE_2: &str = r#"
                                    {
                                      "@type": "StateChange",
                                      "changed": {
                                        "a123": {
                                          "Email": "0af7a512ce70"
                                        }
                                      }
                                      "pushState": "ccc"
                                    }"#;

fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn push_enable_example_round_trips() {
    let m: WebSocketPushEnable = serde_json::from_str(PUSH_ENABLE_EXAMPLE).unwrap();
    assert_eq!(
        m.data_types,
        Some(vec!["Mailbox".to_owned(), "Email".to_owned()])
    );
    assert_eq!(m.push_state.as_deref(), Some("aaa"));
    assert!(m.extra.is_empty());
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        value(PUSH_ENABLE_EXAMPLE)
    );
    let c: ClientMessage = serde_json::from_str(PUSH_ENABLE_EXAMPLE).unwrap();
    assert_eq!(c, ClientMessage::PushEnable(m));
}

/// §4.3.5.2: `dataTypes` is `String[]|null`; "If null, changes will be
/// pushed for all supported data types." So `null` is written, not
/// dropped, and `pushState` (optional) is dropped when absent.
#[test]
fn push_enable_writes_null_data_types() {
    let m = WebSocketPushEnable::new(None, None);
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        json!({"@type": "WebSocketPushEnable", "dataTypes": null})
    );
    let m: WebSocketPushEnable =
        serde_json::from_value(json!({"@type": "WebSocketPushEnable"})).unwrap();
    assert_eq!(m.data_types, None);
}

/// Workspace extras-preservation policy.
#[test]
fn push_enable_preserves_unknown_members() {
    let wire = json!({"@type": "WebSocketPushEnable", "dataTypes": null, "x-v": 1});
    let m: WebSocketPushEnable = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(m.extra["x-v"], 1);
    assert_eq!(serde_json::to_value(&m).unwrap(), wire);
}

/// §4.3.5.3: "This MUST be the string "WebSocketPushDisable"." and it
/// has no other property.
#[test]
fn push_disable_is_only_type() {
    assert_eq!(
        serde_json::to_value(WebSocketPushDisable::new()).unwrap(),
        json!({"@type": "WebSocketPushDisable"})
    );
    let c: ClientMessage = serde_json::from_str(r#"{"@type":"WebSocketPushDisable"}"#).unwrap();
    assert_eq!(c, ClientMessage::PushDisable(WebSocketPushDisable::new()));
}

#[test]
fn state_change_example_round_trips() {
    let m: WsStateChange = serde_json::from_str(STATE_CHANGE_EXAMPLE).unwrap();
    assert_eq!(m.push_state.as_deref(), Some("bbb"));
    let mailbox = &m.state_change.changed[&Id::from("a456")]["Mailbox"];
    assert_eq!(mailbox.as_ref(), "d35ecb040aab");
    assert!(
        m.state_change.extra.is_empty(),
        "{:?}",
        m.state_change.extra
    );
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        value(STATE_CHANGE_EXAMPLE)
    );
    let s: ServerMessage = serde_json::from_str(STATE_CHANGE_EXAMPLE).unwrap();
    assert_eq!(s, ServerMessage::StateChange(m));
}

/// The second §4.4 StateChange example is not JSON as printed (a comma
/// is missing before `"pushState"`). With the comma it reads as the
/// prose says: Email state `0af7a512ce70` on account `a123`, pushState
/// `ccc`.
#[test]
fn state_change_example_2_needs_its_missing_comma() {
    assert!(serde_json::from_str::<Value>(STATE_CHANGE_EXAMPLE_2).is_err());
    let missing = "}\n                                      \"pushState\"";
    assert!(STATE_CHANGE_EXAMPLE_2.contains(missing));
    let fixed = STATE_CHANGE_EXAMPLE_2.replace(missing, "},\n\"pushState\"");
    let m: WsStateChange = serde_json::from_str(&fixed).unwrap();
    assert_eq!(m.push_state.as_deref(), Some("ccc"));
    assert_eq!(
        m.state_change.changed[&Id::from("a123")]["Email"].as_ref(),
        "0af7a512ce70"
    );
}

/// RFC 8887 §4.3.5.1: `pushState` is optional; without it the frame is a
/// plain RFC 8620 §7.1 StateChange.
#[test]
fn state_change_without_push_state() {
    let mut types = BTreeMap::new();
    types.insert("Email".to_owned(), State::from("s1"));
    let mut changed = BTreeMap::new();
    changed.insert(Id::from("a"), types);
    let m = WsStateChange::new(StateChange::new(changed), None);
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        json!({"@type": "StateChange", "changed": {"a": {"Email": "s1"}}})
    );
}

#[test]
fn request_example_round_trips() {
    let m: WsRequest = serde_json::from_str(REQUEST_EXAMPLE).unwrap();
    assert_eq!(m.id.as_deref(), Some("R1"));
    assert_eq!(m.request.using, vec!["urn:ietf:params:jmap:core"]);
    let call = &m.request.method_calls[0];
    assert_eq!(call.name, "Core/echo");
    assert_eq!(call.arguments, json!({"hello": true, "high": 5}));
    assert_eq!(call.call_id, "b3ff");
    assert!(m.request.extra.is_empty(), "{:?}", m.request.extra);
    assert_eq!(serde_json::to_value(&m).unwrap(), value(REQUEST_EXAMPLE));
    let c: ClientMessage = serde_json::from_str(REQUEST_EXAMPLE).unwrap();
    assert_eq!(c, ClientMessage::Request(m));
}

/// §4.3.2: `id` is optional; it is not written when absent.
#[test]
fn request_without_id_omits_it() {
    let m = WsRequest::new(JmapRequest::new(vec![], vec![], None), None);
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        json!({"@type": "Request", "using": [], "methodCalls": []})
    );
}

/// The §4.4 Response example omits `sessionState`, which RFC 8620 §3.4
/// requires of every Response, so as printed it is not a Response. With
/// a `sessionState` added it reads back with `requestId` `R1` and
/// serializes to the same object.
#[test]
fn response_example_needs_session_state() {
    assert!(serde_json::from_str::<WsResponse>(RESPONSE_EXAMPLE).is_err());
    let mut wire = value(RESPONSE_EXAMPLE);
    wire["sessionState"] = json!("75128aab4b1b");
    let m: WsResponse = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(m.request_id.as_deref(), Some("R1"));
    assert_eq!(m.response.method_responses[0].call_id, "b3ff");
    assert!(m.response.extra.is_empty(), "{:?}", m.response.extra);
    assert_eq!(serde_json::to_value(&m).unwrap(), wire);
    let s: ServerMessage = serde_json::from_value(wire).unwrap();
    assert_eq!(s, ServerMessage::Response(m));
}

/// §4.3.3: `requestId` is "optional; MUST be returned if an identifier is
/// included in the request", so a response to an id-less request has none.
#[test]
fn response_without_request_id_omits_it() {
    let m = WsResponse::new(JmapResponse::new(vec![], State::from("s"), None), None);
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        json!({"@type": "Response", "methodResponses": [], "sessionState": "s"})
    );
}

#[test]
fn request_error_example_round_trips() {
    let m: WsRequestError = serde_json::from_str(REQUEST_ERROR_EXAMPLE).unwrap();
    assert_eq!(m.request_id, None);
    assert_eq!(m.problem.problem_type, "urn:ietf:params:jmap:error:notJSON");
    assert_eq!(m.problem.status, Some(400));
    assert_eq!(
        m.problem.detail.as_deref(),
        Some("The request did not parse as I-JSON.")
    );
    assert!(m.problem.extra.is_empty(), "{:?}", m.problem.extra);
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        value(REQUEST_ERROR_EXAMPLE)
    );
    let s: ServerMessage = serde_json::from_str(REQUEST_ERROR_EXAMPLE).unwrap();
    assert_eq!(s, ServerMessage::RequestError(m));
}

/// RFC 8620 §3.6.1.1 `limit` example, carried over WebSocket with the
/// §4.3.4 members: `limit` stays a problem member, `requestId` is echoed.
#[test]
fn request_error_with_limit_and_request_id() {
    let wire = json!({
        "@type": "RequestError",
        "requestId": "R7",
        "type": "urn:ietf:params:jmap:error:limit",
        "limit": "maxSizeRequest",
        "status": 400,
        "detail": "The request is larger than the server is willing to process."
    });
    let m: WsRequestError = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(m.request_id.as_deref(), Some("R7"));
    assert_eq!(m.problem.limit.as_deref(), Some("maxSizeRequest"));
    assert_eq!(serde_json::to_value(&m).unwrap(), wire);
}

/// Each message type "MUST be the string" naming it: a direct parse with
/// the wrong `@type` fails; the message enums need `@type`.
#[test]
fn wrong_or_missing_type_is_rejected() {
    assert!(
        serde_json::from_value::<WsRequest>(json!({
            "@type": "Response", "using": [], "methodCalls": []
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<WebSocketPushDisable>(json!({"@type": "WebSocketPushEnable"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ClientMessage>(json!({"using": [], "methodCalls": []})).is_err()
    );
    assert!(serde_json::from_value::<ClientMessage>(json!({"@type": "Response"})).is_err());
    assert!(serde_json::from_value::<ServerMessage>(json!({"@type": "Request"})).is_err());
    assert!(serde_json::from_value::<ServerMessage>(json!({"@type": 7})).is_err());
}
