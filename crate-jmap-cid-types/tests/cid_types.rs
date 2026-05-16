//! Integration tests for jmap-cid-types.
//!
//! Independent oracles: the capability URI literal is asserted
//! against the value defined in draft-atwood-jmap-cid-00 §3, NOT
//! against the constant's own value.

use jmap_cid_types::JMAP_CID_URI;

// ---------------------------------------------------------------------------
// Capability URI
// ---------------------------------------------------------------------------

#[test]
fn capability_uri_matches_draft_00() {
    // Oracle: draft-atwood-jmap-cid-00 §3 (Capability).
    assert_eq!(JMAP_CID_URI, "urn:ietf:params:jmap:cid");
}
