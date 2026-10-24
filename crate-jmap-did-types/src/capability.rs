//! draft-atwood-jmap-did-00 §Capability — capability URI and the
//! session-level and account-level capability objects.

use serde::{Deserialize, Deserializer, Serialize};

/// The JMAP capability URI for the DID extension
/// (draft-atwood-jmap-did-00 §Capability).
///
/// "A server supporting this extension MUST advertise the
/// `urn:ietf:params:jmap:did` capability in the JMAP Session resource."
/// The session-level value is [`DidCapability`]; the account-level value
/// is [`DidAccountCapability`].
pub const JMAP_DID_URI: &str = "urn:ietf:params:jmap:did";

/// Session-level capability object for `"urn:ietf:params:jmap:did"`.
///
/// Found at `capabilities["urn:ietf:params:jmap:did"]`.
///
/// Spec: draft-atwood-jmap-did-00 §Session-Level Capability Object.
///
/// # Strictness
///
/// `supportedMethods` is required and must not be empty, because
/// "Servers MUST support at least one method". A missing
/// `verificationEndpoint` reads as `None`; serialize always writes it,
/// as `null` when `None`.
///
/// # Example
///
/// ```
/// use jmap_did_types::DidCapability;
///
/// let cap: DidCapability = serde_json::from_str(r#"{
///     "supportedMethods": ["key", "web"],
///     "verificationEndpoint": null
/// }"#)?;
/// assert_eq!(cap.supported_methods, ["key", "web"]);
/// assert_eq!(cap.verification_endpoint, None);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCapability {
    /// The DID method names (DID Core §8, for example `"key"` or
    /// `"web"`) the server can resolve and verify. Every account's bound
    /// DID uses one of them. Never empty after deserialize.
    #[serde(deserialize_with = "non_empty_methods")]
    pub supported_methods: Vec<String>,
    /// A URL where clients can submit a DID for server-side
    /// verification, or `None` if the server offers no such endpoint.
    pub verification_endpoint: Option<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn non_empty_methods<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let methods = Vec::<String>::deserialize(d)?;
    if methods.is_empty() {
        return Err(serde::de::Error::invalid_length(
            0,
            &"at least one DID method",
        ));
    }
    Ok(methods)
}

/// Account-level capability object for `"urn:ietf:params:jmap:did"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:did"]`.
///
/// Spec: draft-atwood-jmap-did-00 §Account-Level Capability Object.
///
/// # Strictness
///
/// `did` is required. The draft requires it to be a valid DID
/// (DID Core §3.1); this type keeps it as an unvalidated string. A
/// missing `didDocumentUri`, `previousDid` or `keyCustody` reads as
/// `None`; serialize always writes all three, as `null` when `None`.
///
/// # Example
///
/// ```
/// use jmap_did_types::{DidAccountCapability, KeyCustody};
///
/// let cap: DidAccountCapability = serde_json::from_str(r#"{
///     "did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
///     "didDocumentUri": null,
///     "previousDid": null,
///     "keyCustody": "user"
/// }"#)?;
/// assert_eq!(cap.key_custody, Some(KeyCustody::User));
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidAccountCapability {
    /// The DID bound to this account.
    pub did: String,
    /// A URI where the DID document can be fetched, or `None`, which
    /// means the client resolves the DID by its method's standard
    /// mechanism. A convenience hint, not a binding constraint: a client
    /// that cannot reach it SHOULD fall back to standard resolution.
    pub did_document_uri: Option<String>,
    /// The DID bound to this account before the most recent DID
    /// migration, or `None` when there was none or the server's
    /// transition period has expired.
    pub previous_did: Option<String>,
    /// Who holds the private keys of the bound DID's `authentication`
    /// verification relationship. `None` means the server does not
    /// disclose custody, and "clients and peers SHOULD NOT infer `"user"`
    /// from `null`" (draft-atwood-jmap-did-00 L195, jmap-chat-spec 6bcd9d8).
    pub key_custody: Option<KeyCustody>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The `keyCustody` value of [`DidAccountCapability`]
/// (draft-atwood-jmap-did-00 §Account-Level Capability Object).
///
/// The JSON `null` value ("does not disclose custody") is
/// `Option::<KeyCustody>::None`, not a variant of this enum.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyCustody {
    /// `"server"`: the server, or a party operating on its behalf, holds
    /// at least one of the private keys.
    Server,
    /// `"user"`: the server holds none of the private keys.
    User,
    /// Any value not defined by the draft. The wire string is preserved
    /// for round-trip. It asserts neither custody model.
    Other(String),
}

jmap_types::impl_string_enum!(KeyCustody, "a JMAP DID keyCustody string",
    "server" => Server,
    "user"   => User,
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_supported_methods_rejected() {
        // Oracle: draft-atwood-jmap-did-00.md L118: "Servers MUST support
        // at least one method."
        let raw = json!({"supportedMethods": [], "verificationEndpoint": null});
        assert!(serde_json::from_value::<DidCapability>(raw).is_err());
    }

    #[test]
    fn missing_did_rejected() {
        // Oracle: draft-atwood-jmap-did-00.md L141-142: `did` (String) is
        // "The DID bound to this account", not nullable.
        let raw = json!({"didDocumentUri": null, "previousDid": null, "keyCustody": null});
        assert!(serde_json::from_value::<DidAccountCapability>(raw).is_err());
    }
}
