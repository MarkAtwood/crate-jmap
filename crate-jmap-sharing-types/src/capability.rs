//! RFC 9670 §1.5 capability structs and URI constants.
//!
//! Provides [`PrincipalsCapability`] and [`PrincipalsOwnerCapability`], and
//! the URI string constants for both capability identifiers.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// Capability URI for the JMAP Principals capability (RFC 9670 §1.5.1).
///
/// Appears in both the JMAP Session-level `capabilities` object and in
/// Account `accountCapabilities`.  The value at the session level is an
/// empty object; the value in `accountCapabilities` is a
/// [`PrincipalsCapability`] object.
pub const JMAP_PRINCIPALS_URI: &str = "urn:ietf:params:jmap:principals";

/// Capability URI for the JMAP Principals owner capability (RFC 9670 §1.5.2).
///
/// Appears **only** in Account `accountCapabilities` (never in the Session-level
/// `capabilities` object).  Its presence indicates that the Account is owned by a
/// Principal.  The value is a [`PrincipalsOwnerCapability`] object.
pub const JMAP_PRINCIPALS_OWNER_URI: &str = "urn:ietf:params:jmap:principals:owner";

/// Value of `urn:ietf:params:jmap:principals` in an Account's `accountCapabilities`
/// (RFC 9670 §1.5.1).
///
/// Contains information about how the Principals data type is supported in this
/// Account, and identifies which Principal corresponds to the authenticated user.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalsCapability {
    /// The id of the Principal in this Account that corresponds to the user
    /// fetching this object, if any.  `null` if no corresponding Principal exists.
    pub current_user_principal_id: Option<Id>,
}

/// Value of `urn:ietf:params:jmap:principals:owner` in an Account's
/// `accountCapabilities` (RFC 9670 §1.5.2).
///
/// Present only on Accounts that are owned by a Principal.  Identifies both
/// the Account that holds the Principal object and the Principal itself.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalsOwnerCapability {
    /// The id of the Account with the `urn:ietf:params:jmap:principals` capability
    /// that contains the corresponding Principal object.
    pub account_id_for_principal: Id,

    /// The id of the Principal that owns this Account.
    pub principal_id: Id,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn principals_capability_with_principal_id() {
        // Oracle: hand-written from RFC 9670 §1.5.1 field description.
        let json = json!({ "currentUserPrincipalId": "P99" });
        let cap: PrincipalsCapability =
            serde_json::from_value(json).expect("deserialize PrincipalsCapability");
        assert_eq!(
            cap.current_user_principal_id.as_ref().map(|id| id.as_ref()),
            Some("P99")
        );
    }

    #[test]
    fn principals_capability_null_principal_id() {
        // Oracle: currentUserPrincipalId is Id|null — must handle null.
        let json = json!({ "currentUserPrincipalId": null });
        let cap: PrincipalsCapability =
            serde_json::from_value(json).expect("deserialize null currentUserPrincipalId");
        assert!(cap.current_user_principal_id.is_none());

        // Re-serialization: null must appear as null, not be absent.
        let serialized = serde_json::to_value(&cap).expect("serialize");
        assert_eq!(
            serialized["currentUserPrincipalId"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn principals_capability_roundtrip() {
        let json = json!({ "currentUserPrincipalId": "P42" });
        let cap: PrincipalsCapability = serde_json::from_value(json.clone()).expect("deserialize");
        let serialized = serde_json::to_value(&cap).expect("serialize");
        let cap2: PrincipalsCapability =
            serde_json::from_value(serialized).expect("deserialize again");
        assert_eq!(cap, cap2);
    }

    #[test]
    fn principals_owner_capability_roundtrip() {
        // Oracle: hand-written from RFC 9670 §1.5.2 field descriptions.
        let json = json!({
            "accountIdForPrincipal": "acc-principals",
            "principalId": "P2342fnddd20"
        });
        let cap: PrincipalsOwnerCapability =
            serde_json::from_value(json.clone()).expect("deserialize PrincipalsOwnerCapability");
        assert_eq!(cap.account_id_for_principal, "acc-principals");
        assert_eq!(cap.principal_id, "P2342fnddd20");

        let serialized = serde_json::to_value(&cap).expect("serialize");
        let cap2: PrincipalsOwnerCapability =
            serde_json::from_value(serialized).expect("deserialize again");
        assert_eq!(cap, cap2);
    }

    #[test]
    fn uri_constants() {
        assert_eq!(JMAP_PRINCIPALS_URI, "urn:ietf:params:jmap:principals");
        assert_eq!(
            JMAP_PRINCIPALS_OWNER_URI,
            "urn:ietf:params:jmap:principals:owner"
        );
    }
}
