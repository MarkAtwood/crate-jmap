//! JMAP Sieve Scripts types (RFC 9661).
//!
//! Covers [`SieveScript`], [`SieveCapability`], and [`SieveAccountCapability`]
//! for the `urn:ietf:params:jmap:sieve` capability.
//!
//! All items are gated by the `sieve` feature flag (enabled at the module level
//! in `lib.rs` via `#[cfg(feature = "sieve")]`).

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Capability URI for JMAP Sieve Scripts.
pub const JMAP_SIEVE_SCRIPTS_URI: &str = "urn:ietf:params:jmap:sieve";

/// A Sieve script stored on the server (RFC 9661 §3).
///
/// The struct is `#[non_exhaustive]` to allow future fields without a breaking
/// version bump. Construct with [`SieveScript::new`] rather than struct-literal
/// syntax; set optional fields directly after construction.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SieveScript {
    /// The object id (server-set; immutable).
    pub id: Id,

    /// The user-visible name for this script.  `None` serializes as JSON `null`.
    pub name: Option<String>,

    /// The blob id of the script content.
    pub blob_id: Id,

    /// Whether this script is the active (executing) script for the account.
    pub is_active: bool,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SieveScript {
    /// Construct a [`SieveScript`] from its required fields.
    ///
    /// The `name` field defaults to `None`. Set it directly after construction
    /// if needed.
    pub fn new(id: Id, blob_id: Id, is_active: bool) -> Self {
        Self {
            id,
            name: None,
            blob_id,
            is_active,
            extra: serde_json::Map::new(),
        }
    }
}

/// Server-level Sieve capability object (RFC 9661 §2).
///
/// Advertised under the `urn:ietf:params:jmap:sieve` key in the JMAP Session
/// `capabilities` object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SieveCapability {
    /// A string identifying the Sieve implementation.
    pub implementation: String,
}

impl SieveCapability {
    /// Construct a [`SieveCapability`] with the given implementation string.
    pub fn new(implementation: String) -> Self {
        Self { implementation }
    }
}

/// Per-account Sieve capability object (RFC 9661 §2).
///
/// Advertised under the `urn:ietf:params:jmap:sieve` key in the JMAP Session
/// `accountCapabilities` object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SieveAccountCapability {
    /// Maximum byte length of a script name.
    pub max_size_script_name: u64,

    /// Maximum byte size of a single Sieve script.  `None` serializes as JSON `null`.
    pub max_size_script: Option<u64>,

    /// Maximum number of Sieve scripts per account.  `None` serializes as JSON `null`.
    pub max_number_scripts: Option<u64>,

    /// Maximum number of Sieve redirect actions per script execution.
    /// `None` serializes as JSON `null`.
    pub max_number_redirects: Option<u64>,

    /// List of Sieve extensions supported by this server.
    pub sieve_extensions: Vec<String>,

    /// Notification methods supported.  `None` serializes as JSON `null`.
    pub notification_methods: Option<Vec<String>>,

    /// URIs for external list sources.  `None` serializes as JSON `null`.
    pub external_lists: Option<Vec<String>>,
}

impl SieveAccountCapability {
    /// Construct a [`SieveAccountCapability`] from the two required fields.
    ///
    /// All optional fields default to `None`.
    pub fn new(max_size_script_name: u64, sieve_extensions: Vec<String>) -> Self {
        Self {
            max_size_script_name,
            max_size_script: None,
            max_number_scripts: None,
            max_number_redirects: None,
            sieve_extensions,
            notification_methods: None,
            external_lists: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: hand-written JSON fixture derived from RFC 9661 §3
    /// field definitions — blobId, isActive, name wire names.
    #[test]
    fn sieve_script_full_roundtrip() {
        let json = r#"{"id":"s1","name":"vacation","blobId":"b1","isActive":true}"#;
        let script: SieveScript = serde_json::from_str(json).expect("must parse");
        assert_eq!(script.id.as_ref(), "s1");
        assert_eq!(script.name.as_deref(), Some("vacation"));
        assert_eq!(script.blob_id.as_ref(), "b1");
        assert!(script.is_active);
        let back = serde_json::to_string(&script).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: SieveScript with name=null serializes as JSON null (spec §3 — String|null field).
    #[test]
    fn sieve_script_null_name_serializes_as_null() {
        // Deserialize JSON that omits the key — name becomes None.
        let json_in = r#"{"id":"s2","name":null,"blobId":"b2","isActive":false}"#;
        let script: SieveScript = serde_json::from_str(json_in).expect("must parse");
        assert!(script.name.is_none());
        // Serialize must produce `"name":null`, not omit the key.
        let back = serde_json::to_string(&script).expect("serialize");
        assert_eq!(back, json_in);
    }

    /// Oracle: SieveScript::new sets name to None; serialized output includes `"name":null`.
    #[test]
    fn sieve_script_new_defaults() {
        let script = SieveScript::new(Id::from("s3"), Id::from("b3"), true);
        assert_eq!(script.id.as_ref(), "s3");
        assert!(script.name.is_none());
        assert_eq!(script.blob_id.as_ref(), "b3");
        assert!(script.is_active);
        let json = serde_json::to_string(&script).expect("serialize");
        // name must be present as null, not omitted (spec §3 String|null field).
        assert!(
            json.contains("\"name\":null"),
            "name must be null when None, not absent"
        );
    }

    /// Oracle: SieveAccountCapability wire names from RFC 9661 §2.
    /// Nullable fields (maxSizeScript, maxNumberRedirects, etc.) must be present
    /// as JSON null when None — spec example shows `"maxNumberRedirects": null`.
    #[test]
    fn sieve_account_capability_roundtrip() {
        // Full round-trip: all nullable fields present as null except the two with values.
        let json = r#"{"maxSizeScriptName":512,"maxSizeScript":65536,"maxNumberScripts":5,"maxNumberRedirects":null,"sieveExtensions":["fileinto","reject"],"notificationMethods":null,"externalLists":null}"#;
        let cap: SieveAccountCapability = serde_json::from_str(json).expect("must parse");
        assert_eq!(cap.max_size_script_name, 512);
        assert_eq!(cap.max_size_script, Some(65536));
        assert_eq!(cap.max_number_scripts, Some(5));
        assert!(cap.max_number_redirects.is_none());
        assert_eq!(cap.sieve_extensions, vec!["fileinto", "reject"]);
        assert!(cap.notification_methods.is_none());
        assert!(cap.external_lists.is_none());
        let back = serde_json::to_string(&cap).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: SieveAccountCapability::new — all nullable optional fields serialize as null.
    #[test]
    fn sieve_account_capability_minimal() {
        let cap = SieveAccountCapability::new(256, vec!["fileinto".to_owned()]);
        let json = serde_json::to_string(&cap).expect("serialize");
        assert!(json.contains("\"maxSizeScriptName\""));
        assert!(json.contains("\"sieveExtensions\""));
        // Nullable fields must be present as null, not omitted (spec §2).
        assert!(
            json.contains("\"maxSizeScript\":null"),
            "must be null when None"
        );
        assert!(
            json.contains("\"maxNumberScripts\":null"),
            "must be null when None"
        );
        assert!(
            json.contains("\"maxNumberRedirects\":null"),
            "must be null when None"
        );
        assert!(
            json.contains("\"notificationMethods\":null"),
            "must be null when None"
        );
        assert!(
            json.contains("\"externalLists\":null"),
            "must be null when None"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────

    /// `SieveScript.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn sieve_script_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "s1",
            "name": "vacation",
            "blobId": "b1",
            "isActive": true,
            "acmeCorpSyntaxValidated": true
        });
        let script: SieveScript = serde_json::from_value(raw).unwrap();
        assert_eq!(
            script
                .extra
                .get("acmeCorpSyntaxValidated")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&script).unwrap();
        assert_eq!(back["acmeCorpSyntaxValidated"], true);
    }

    /// Oracle: SieveCapability round-trips with camelCase wire name.
    #[test]
    fn sieve_capability_roundtrip() {
        let json = r#"{"implementation":"Cyrus Sieve 3.0"}"#;
        let cap: SieveCapability = serde_json::from_str(json).expect("must parse");
        assert_eq!(cap.implementation, "Cyrus Sieve 3.0");
        let back = serde_json::to_string(&cap).expect("serialize");
        assert_eq!(back, json);
    }
}
