//! draft-atwood-jmap-cid-00 §3 — capability registration and
//! capability value object.
//!
//! Provides the capability URI constant [`JMAP_CID_URI`] and the
//! [`CidCapability`] value object that mirrors the wire shape of
//! the capability's value.

use serde::{Deserialize, Serialize};

/// The JMAP capability URI for the Blob Content Identifiers
/// extension (draft-atwood-jmap-cid-00 §3).
///
/// Present as a key in both the session-level `capabilities` object
/// and in each account's `accountCapabilities` object. The value of
/// the key is a (currently empty) JSON object — see
/// [`CidCapability`] for the typed wire shape.
pub const JMAP_CID_URI: &str = "urn:ietf:params:jmap:cid";

/// Value object of the `urn:ietf:params:jmap:cid` capability
/// (draft-atwood-jmap-cid-00 §3).
///
/// The draft currently specifies an empty value object: when a
/// server advertises CID, the value of the
/// [`JMAP_CID_URI`] key in both the session-level `capabilities`
/// object and per-account `accountCapabilities` is `{}`. The
/// `#[non_exhaustive]` attribute keeps the door open for the draft
/// to add capability-level fields (e.g. an enumerated digest
/// algorithm set when CID generalises beyond SHA-256) without a
/// breaking change to consumers that destructure the struct.
///
/// The [`extra`](Self::extra) field is the workspace
/// extras-preservation surface (workspace AGENTS.md
/// "extras-preservation policy"): unknown vendor / site / future
/// IETF fields on the wire JSON survive deserialize and serialize
/// untouched.
///
/// ## Example
///
/// ```
/// use jmap_cid_types::CidCapability;
///
/// // Empty value object per the current draft revision.
/// let cap: CidCapability = serde_json::from_str("{}")?;
/// assert!(cap.extra.is_empty());
///
/// // Vendor extension survives round-trip.
/// let cap: CidCapability = serde_json::from_str(
///     r#"{"acmeCorpFastDigest": true}"#,
/// )?;
/// assert_eq!(
///     cap.extra.get("acmeCorpFastDigest"),
///     Some(&serde_json::Value::Bool(true)),
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CidCapability {
    /// Catch-all for vendor / site / future-IETF fields not
    /// covered by the typed fields above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_object_round_trips() {
        // Oracle: draft-atwood-jmap-cid-00 §3 — value object is
        // currently empty `{}`.
        let cap: CidCapability = serde_json::from_str("{}").unwrap();
        assert!(cap.extra.is_empty());
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn vendor_extras_preserved_through_round_trip() {
        // Workspace extras-preservation policy (JMAP-lbdy):
        // unknown fields survive deserialize and re-emerge on
        // serialize.
        let input = r#"{"acmeCorpFastDigest":true,"vendor.example:limit":1024}"#;
        let cap: CidCapability = serde_json::from_str(input).unwrap();
        assert_eq!(cap.extra.len(), 2);
        assert_eq!(
            cap.extra.get("acmeCorpFastDigest"),
            Some(&serde_json::Value::Bool(true)),
        );
        assert_eq!(
            cap.extra.get("vendor.example:limit"),
            Some(&serde_json::Value::Number(1024.into())),
        );
        // Serialize and deserialize again to assert byte-shape
        // round-trip (key order is preserve by serde_json::Map).
        let round_tripped = serde_json::to_string(&cap).unwrap();
        let cap2: CidCapability = serde_json::from_str(&round_tripped).unwrap();
        assert_eq!(cap, cap2);
    }

    #[test]
    fn default_constructs_empty() {
        let cap = CidCapability::default();
        assert!(cap.extra.is_empty());
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, "{}");
    }
}
