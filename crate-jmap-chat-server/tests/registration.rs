//! `register_chat_handlers` routes every draft-atwood-jmap-chat-00 method
//! name (bd:JMAP-w8fu.18).
//!
//! Oracle: the method headings of draft-atwood-jmap-chat-00.md (spec HEAD
//! 6c38e2f, lines 1042-1521), copied by hand into [`SPEC_METHODS`], and
//! RFC 8620 §3.6.2 (rfc8620.txt:1187): "`unknownMethod`: The server does
//! not recognise this method name." A method name missing from, or
//! misspelled in, the registration table therefore surfaces as an
//! `unknownMethod` error.
//!
//! RFC 8620 §5.1: a successful `/get` response carries a `list` argument.

mod common;

use std::sync::Arc;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::register_chat_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::json;

/// Every `### Type/method` heading in draft-atwood-jmap-chat-00.md, in
/// document order.
const SPEC_METHODS: &[&str] = &[
    "ChatContact/get",
    "ChatContact/changes",
    "ChatContact/set",
    "ChatContact/query",
    "ChatContact/queryChanges",
    "Chat/get",
    "Chat/changes",
    "Chat/set",
    "Chat/query",
    "Chat/queryChanges",
    "Chat/typing",
    "Message/get",
    "Message/changes",
    "Message/set",
    "Message/query",
    "Message/queryChanges",
    "Space/get",
    "Space/changes",
    "Space/set",
    "Space/query",
    "Space/queryChanges",
    "Space/join",
    "ReadPosition/get",
    "ReadPosition/changes",
    "ReadPosition/set",
    "PresenceStatus/get",
    "PresenceStatus/changes",
    "PresenceStatus/set",
    "CustomEmoji/get",
    "CustomEmoji/changes",
    "CustomEmoji/set",
    "CustomEmoji/query",
    "CustomEmoji/queryChanges",
    "SpaceInvite/get",
    "SpaceInvite/changes",
    "SpaceInvite/set",
    "SpaceBan/get",
    "SpaceBan/changes",
    "SpaceBan/set",
];

async fn call(method: &str) -> JmapResponse {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let mut dispatcher = Dispatcher::new();
    register_chat_handlers(&mut dispatcher, Arc::new(backend));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
        "methodCalls": [[method, {"accountId": ACCOUNT_ID}, "c0"]]
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn is_unknown_method(resp: &JmapResponse) -> bool {
    let inv = &resp.method_responses[0];
    inv.name == "error" && inv.arguments["type"] == "unknownMethod"
}

#[tokio::test]
async fn every_spec_method_is_registered() {
    for method in SPEC_METHODS {
        let resp = call(method).await;
        assert!(
            !is_unknown_method(&resp),
            "{method} is not registered: {:?}",
            resp.method_responses[0]
        );
    }
}

#[tokio::test]
async fn get_methods_route_to_a_get_handler() {
    for method in SPEC_METHODS.iter().filter(|m| m.ends_with("/get")) {
        let resp = call(method).await;
        let inv = &resp.method_responses[0];
        assert_eq!(inv.name, *method, "{method}: {:?}", inv.arguments);
        assert!(
            inv.arguments["list"].is_array(),
            "{method}: {}",
            inv.arguments
        );
    }
}

/// Negative control: a name the draft does not define is not routed, so
/// the check above can fail.
#[tokio::test]
async fn non_spec_method_is_unknown() {
    let resp = call("Chat/copy").await;
    assert!(is_unknown_method(&resp), "{:?}", resp.method_responses[0]);
}
