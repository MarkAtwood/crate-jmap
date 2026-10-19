//! Wire shape of `Mention` and `BroadcastMention` (bd:JMAP-yt1w.17).
//!
//! Oracle: draft-atwood-jmap-chat-00 at jmap-chat-spec 0983787, which
//! removed `offset` and `length` from both types.

use jmap_chat_types::message::{BroadcastMention, Mention};
use serde_json::json;

/// §Mention: "A Mention carries no position in `body`." Its only field is
/// `id`, so `{"id": ...}` deserializes and serializes back unchanged.
#[test]
fn mention_is_id_only() {
    let wire = json!({ "id": "c1" });
    let m: Mention = serde_json::from_value(wire.clone()).expect("Mention from {id}");
    assert_eq!(serde_json::to_value(&m).expect("serialize"), wire);
}

/// §BroadcastMention: "Like Mention, a BroadcastMention carries no position
/// in `body`." Its only field is `scope`, so `{"scope": ...}` deserializes
/// and serializes back unchanged.
#[test]
fn broadcast_mention_is_scope_only() {
    let wire = json!({ "scope": "here" });
    let bm: BroadcastMention =
        serde_json::from_value(wire.clone()).expect("BroadcastMention from {scope}");
    assert_eq!(serde_json::to_value(&bm).expect("serialize"), wire);
}
