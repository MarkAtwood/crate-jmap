//! draft-atwood-jmap-chat-mimi-00 §The urn:ietf:params:jmap:chat:mimi
//! Capability and §MLS Messages — capability URI, account-level
//! capability object, and body-type constants.

use serde::{Deserialize, Deserializer, Serialize};

use crate::Base64Url;

/// Capability URI for the JMAP Chat MIMI provider extension
/// (draft-atwood-jmap-chat-mimi-00 §Capability).
///
/// "This capability is meaningful only when `urn:ietf:params:jmap:chat`
/// is also present." The session-level value is an empty object `{}`
/// and is not modelled; the account-level value is [`MimiCapability`].
pub const JMAP_CHAT_MIMI_URI: &str = "urn:ietf:params:jmap:chat:mimi";

/// `bodyType` of every Message in a Chat with a `roomId`: one
/// FanoutMessage (MIMI protocol §5.5) per Message
/// (draft-atwood-jmap-chat-mimi-00 §application/mimi-fanout).
///
/// A server advertising [`JMAP_CHAT_MIMI_URI`] "MUST include
/// `\"application/mimi-fanout\"` in `supportedBodyTypes`". Only servers
/// create Messages of this type.
pub const MIMI_FANOUT_BODY_TYPE: &str = "application/mimi-fanout";

/// Media type of an MLSMessage (RFC 9420 §17.10) that a client sends with
/// `Room/send` (draft-atwood-jmap-chat-mimi-00 §message/mls).
pub const MLS_MESSAGE_BODY_TYPE: &str = "message/mls";

/// Account-level capability object for `"urn:ietf:params:jmap:chat:mimi"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:chat:mimi"]`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Account-Level Capability Object
///
/// # Strictness
///
/// No struct-level `#[serde(default)]`: the draft lists every field
/// without marking any optional, so a missing field is a deserialize
/// error. An empty `cipherSuites` is also rejected, because the draft
/// says it "MUST NOT be empty".
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::MimiCapability;
///
/// let cap: MimiCapability = serde_json::from_str(r#"{
///     "userUri": "mimi://b.example/u/bob",
///     "roomIdPrefix": "mimi://b.example/r/",
///     "hubSender": "AAEC",
///     "cipherSuites": [1],
///     "maxKeyPackages": 100
/// }"#)?;
/// assert_eq!(cap.cipher_suites, [1]);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MimiCapability {
    /// The account owner's MIMI user URI. The server uses it as
    /// `requestingUser` and `sendingUri` on the owner's behalf.
    pub user_uri: String,
    /// The prefix of every room ID this server accepts in `Room/create`,
    /// for example `"mimi://a.example/r/"`.
    pub room_id_prefix: String,
    /// The ExternalSender (RFC 9420 §12.1.8.1) with which this server
    /// signs as hub of the rooms it hosts, as the unpadded base64url
    /// encoding of its TLS-encoded octets.
    pub hub_sender: Base64Url,
    /// The MLS cipher suites (RFC 9420 §17.1, `uint16 CipherSuite`) this
    /// server accepts in KeyPackages and new rooms. Never empty after
    /// deserialize.
    #[serde(deserialize_with = "non_empty_cipher_suites")]
    pub cipher_suites: Vec<u16>,
    /// The maximum number of KeyPackage records, last-resort KeyPackages
    /// included, the server stores for this account.
    pub max_key_packages: u64,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn non_empty_cipher_suites<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u16>, D::Error> {
    let suites = Vec::<u16>::deserialize(d)?;
    if suites.is_empty() {
        return Err(serde::de::Error::invalid_length(
            0,
            &"at least one MLS cipher suite",
        ));
    }
    Ok(suites)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Hand-written from the field list of draft-atwood-jmap-chat-mimi-00
    /// §Account-Level Capability Object. `roomIdPrefix` is the draft's
    /// example value; the user URI follows the §Examples form
    /// "mimi://b.example/u/bob". `hubSender` reuses the §Conventions
    /// base64url example string, since only its encoding is checked here.
    /// Cipher suites 1 and 3 are registered in RFC 9420 §17.1.
    fn sample() -> serde_json::Value {
        json!({
            "userUri": "mimi://a.example/u/alice",
            "roomIdPrefix": "mimi://a.example/r/",
            "hubSender": "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f",
            "cipherSuites": [1, 3],
            "maxKeyPackages": 100
        })
    }

    #[test]
    fn capability_uri_matches_draft() {
        // Oracle: draft §The urn:ietf:params:jmap:chat:mimi Capability.
        assert_eq!(JMAP_CHAT_MIMI_URI, "urn:ietf:params:jmap:chat:mimi");
    }

    #[test]
    fn body_types_match_draft() {
        // Oracle: draft §MLS Messages: "every Message has `bodyType`
        // `\"application/mimi-fanout\"`"; RFC 9420 §17.10 registers
        // `message/mls`.
        assert_eq!(MIMI_FANOUT_BODY_TYPE, "application/mimi-fanout");
        assert_eq!(MLS_MESSAGE_BODY_TYPE, "message/mls");
    }

    #[test]
    fn account_capability_round_trips() {
        let raw = sample();
        let cap: MimiCapability = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(cap.user_uri, "mimi://a.example/u/alice");
        assert_eq!(cap.room_id_prefix, "mimi://a.example/r/");
        assert_eq!(
            cap.hub_sender.as_str(),
            "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f"
        );
        assert_eq!(cap.cipher_suites, [1, 3]);
        assert_eq!(cap.max_key_packages, 100);
        assert!(cap.extra.is_empty());
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
    }

    #[test]
    fn empty_cipher_suites_rejected() {
        // Oracle: draft §Account-Level Capability Object: `cipherSuites`
        // "MUST NOT be empty."
        let mut raw = sample();
        raw["cipherSuites"] = json!([]);
        assert!(serde_json::from_value::<MimiCapability>(raw).is_err());
    }

    #[test]
    fn cipher_suite_outside_uint16_rejected() {
        // Oracle: RFC 9420 §17.1: "VALUE is represented as a 16-bit
        // integer: uint16 CipherSuite;".
        let mut raw = sample();
        raw["cipherSuites"] = json!([65536]);
        assert!(serde_json::from_value::<MimiCapability>(raw).is_err());
    }

    #[test]
    fn each_field_required() {
        // Oracle: the draft lists all five fields with no "optional"
        // marker.
        for field in [
            "userUri",
            "roomIdPrefix",
            "hubSender",
            "cipherSuites",
            "maxKeyPackages",
        ] {
            let mut raw = sample();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<MimiCapability>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn padded_hub_sender_rejected() {
        // Oracle: draft §Conventions: byte strings are the "unpadded
        // base64url encoding".
        let mut raw = sample();
        raw["hubSender"] = json!("AA==");
        assert!(serde_json::from_value::<MimiCapability>(raw).is_err());
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = sample();
        raw["vendor.example:hubRegion"] = json!("eu");
        let cap: MimiCapability = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            cap.extra.get("vendor.example:hubRegion"),
            Some(&json!("eu"))
        );
        assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
    }
}
