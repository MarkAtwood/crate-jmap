//! Peer-role access control (draft-atwood-jmap-chat-federation-00
//! §Account-Level Capability L260, §Server-to-Server Methods L302).
//!
//! Oracle: the draft text quoted on each test and the method list of
//! §Server-to-Server Methods, written out by hand below.

use std::sync::Arc;

use jmap_chat_federation_server::{
    check_method_access, method_access_guard, FederationCaller, PEER_METHODS,
};
use jmap_server::{Dispatcher, HandlerFuture, HandlerOutput, Invocation, JmapHandler, JmapRequest};
use serde_json::{json, Value};

#[derive(Clone)]
struct Caller {
    peer: bool,
}

impl FederationCaller for Caller {
    fn is_peer(&self) -> bool {
        self.peer
    }
}

const PEER: Caller = Caller { peer: true };
const OWNER: Caller = Caller { peer: false };

/// The section headings under §Server-to-Server Methods: Peer/deliver,
/// Peer/receipt, Peer/typing, Peer/retract, Peer/groupUpdate (required)
/// and Peer/subscribePresence, Peer/unsubscribePresence, Peer/presence
/// (optional).
const SPEC_PEER_METHODS: [&str; 8] = [
    "Peer/deliver",
    "Peer/receipt",
    "Peer/typing",
    "Peer/retract",
    "Peer/groupUpdate",
    "Peer/subscribePresence",
    "Peer/unsubscribePresence",
    "Peer/presence",
];

/// Methods of draft-atwood-jmap-chat-00 and RFC 8620 that are not Peer/*.
const OWNER_METHODS: [&str; 6] = [
    "Chat/get",
    "Message/set",
    "ChatContact/query",
    "Space/set",
    "Core/echo",
    "Blob/copy",
];

fn assert_forbidden(caller: &Caller, method: &str) {
    let err = check_method_access(caller, method).expect_err(method);
    assert_eq!(err.error_type.to_string(), "forbidden", "{method}");
}

#[test]
fn peer_methods_match_the_draft_list() {
    let mut ours = PEER_METHODS;
    let mut spec = SPEC_PEER_METHODS;
    ours.sort_unstable();
    spec.sort_unstable();
    assert_eq!(ours, spec);
}

/// L260: "An account with `role: \"peer\"` is granted access only to the
/// Peer/* methods defined in this document."
#[test]
fn peer_caller_may_call_every_peer_method() {
    for method in SPEC_PEER_METHODS {
        assert!(check_method_access(&PEER, method).is_ok(), "{method}");
    }
}

/// L260: "Callers authenticated as a peer and operating against an account
/// where `role` is `\"peer\"` MUST receive `forbidden` for any method not
/// listed in {{peer-methods}}."
#[test]
fn peer_caller_is_forbidden_other_methods() {
    for method in OWNER_METHODS {
        assert_forbidden(&PEER, method);
    }
}

/// L260: "any method not listed in {{peer-methods}}" includes a Peer/*
/// name the draft does not define.
#[test]
fn peer_caller_is_forbidden_unlisted_peer_name() {
    assert_forbidden(&PEER, "Peer/unknown");
}

/// L302: "Callers without the `\"peer\"` role MUST receive `forbidden` for
/// all methods in this section."
#[test]
fn non_peer_caller_is_forbidden_every_peer_method() {
    for method in SPEC_PEER_METHODS {
        assert_forbidden(&OWNER, method);
    }
}

/// L260: "An account with `role: \"owner\"` has full access to all JMAP
/// Chat methods."
#[test]
fn owner_caller_may_call_other_methods() {
    for method in OWNER_METHODS {
        assert!(check_method_access(&OWNER, method).is_ok(), "{method}");
    }
}

/// L302 forbids only "the methods in this section"; a Peer/* name the
/// draft does not define is left to the dispatcher (`unknownMethod`).
#[test]
fn owner_caller_is_not_forbidden_unlisted_peer_name() {
    assert!(check_method_access(&OWNER, "Peer/unknown").is_ok());
}

/// Answers every call with an empty object.
struct Empty;

impl JmapHandler<Caller> for Empty {
    fn call(&self, _: String, _: String, _: Value, _: Caller) -> HandlerFuture {
        Box::pin(async { Ok(HandlerOutput::new(json!({}))) })
    }
}

async fn guarded_dispatch(caller: Caller) -> Vec<(String, Value)> {
    let mut d: Dispatcher<Caller> = Dispatcher::new();
    d.register("Chat/get", Arc::new(Empty));
    d.register("Peer/deliver", Arc::new(Empty));
    d.set_guard(method_access_guard);
    let req = JmapRequest::new(
        vec!["urn:ietf:params:jmap:core".into()],
        vec![
            Invocation::new("Chat/get", json!({}), "c0"),
            Invocation::new("Peer/deliver", json!({}), "c1"),
        ],
        None,
    );
    d.dispatch(req, caller, "s0".into())
        .await
        .method_responses
        .into_iter()
        .map(|i| (i.name, i.arguments))
        .collect()
}

/// 33702a5 L277: "Callers authenticated as a peer and operating against an
/// account where `role` is `\"peer\"` MUST receive `forbidden` for any
/// method not listed in {{peer-methods}}" — enforced by the dispatcher
/// before a registered handler runs.
#[tokio::test]
async fn dispatcher_guard_forbids_peer_caller_other_methods() {
    assert_eq!(
        guarded_dispatch(PEER).await,
        vec![
            ("error".into(), json!({"type": "forbidden"})),
            ("Peer/deliver".into(), json!({})),
        ]
    );
}

/// 33702a5 L319: "Callers without the `\"peer\"` role MUST receive
/// `forbidden` for all methods in this section" — enforced by the
/// dispatcher before a registered handler runs.
#[tokio::test]
async fn dispatcher_guard_forbids_non_peer_caller_peer_methods() {
    assert_eq!(
        guarded_dispatch(OWNER).await,
        vec![
            ("Chat/get".into(), json!({})),
            ("error".into(), json!({"type": "forbidden"})),
        ]
    );
}
