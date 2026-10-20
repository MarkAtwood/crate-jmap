//! Peer-role access control (draft-atwood-jmap-chat-federation-00
//! §Account-Level Capability L260, §Server-to-Server Methods L302).
//!
//! Oracle: the draft text quoted on each test and the method list of
//! §Server-to-Server Methods, written out by hand below.

use jmap_chat_federation_server::{check_method_access, FederationCaller, PEER_METHODS};

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
    assert_eq!(err.error_type, "forbidden", "{method}");
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
