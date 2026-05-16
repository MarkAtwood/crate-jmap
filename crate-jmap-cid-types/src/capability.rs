//! draft-atwood-jmap-cid-00 §3 — capability registration.
//!
//! Provides the capability URI constant [`JMAP_CID_URI`].
//!
//! Per the draft (§3) the value of the
//! `urn:ietf:params:jmap:cid` key in both the session-level
//! `capabilities` object and each account's `accountCapabilities`
//! object is an empty JSON object. The typed `CidCapability` value
//! object that mirrors that shape is tracked separately (see
//! `PLAN.md` "Public API (current state)") and will land in this
//! same module alongside this constant when it ships.

/// The JMAP capability URI for the Blob Content Identifiers
/// extension (draft-atwood-jmap-cid-00 §3).
///
/// Present as a key in both the session-level `capabilities` object
/// and in each account's `accountCapabilities` object. The value of
/// the key is an empty JSON object per the current draft revision.
pub const JMAP_CID_URI: &str = "urn:ietf:params:jmap:cid";
