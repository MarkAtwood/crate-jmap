//! RFC 8621 §6 Identity object.
//!
//! Provides [`Identity`] — stores information about an email address or domain
//! the user may send from.

use crate::email::EmailAddress;
use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// An RFC 8621 §6 Identity object.
///
/// Stores information about an email address or domain the user may send from.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// The id of the Identity (immutable; server-set).
    pub id: Id,
    /// The "From" name the client SHOULD use when creating a new Email
    /// from this Identity.  Defaults to `""`.
    pub name: String,
    /// The "From" email address the client MUST use (immutable).
    pub email: String,
    /// The Reply-To value the client SHOULD set.  `null` if not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<EmailAddress>>,
    /// The Bcc value the client SHOULD set.  `null` if not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bcc: Option<Vec<EmailAddress>>,
    /// Plaintext signature.  Defaults to `""`.
    pub text_signature: String,
    /// HTML snippet signature.  Defaults to `""`.
    pub html_signature: String,
    /// Whether the user may delete this Identity (server-set).
    pub may_delete: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Identity {
    /// Construct an [`Identity`] from its three identifying fields.
    ///
    /// `name`, `text_signature`, and `html_signature` default to `""`.
    /// `reply_to` and `bcc` default to `None`.
    pub fn new(id: Id, email: impl Into<String>, may_delete: bool) -> Self {
        Self {
            id,
            email: email.into(),
            may_delete,
            name: String::new(),
            reply_to: None,
            bcc: None,
            text_signature: String::new(),
            html_signature: String::new(),
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────

    /// `Identity.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn identity_preserves_vendor_extras() {
        let raw = json!({
            "id": "i1",
            "name": "Alice",
            "email": "alice@example.com",
            "textSignature": "-- Alice",
            "htmlSignature": "<p>-- Alice</p>",
            "mayDelete": true,
            "acmeCorpDepartment": "engineering"
        });
        let id: Identity = serde_json::from_value(raw).unwrap();
        assert_eq!(
            id.extra.get("acmeCorpDepartment").and_then(|v| v.as_str()),
            Some("engineering")
        );
        let back = serde_json::to_value(&id).unwrap();
        assert_eq!(back["acmeCorpDepartment"], "engineering");
    }
}
