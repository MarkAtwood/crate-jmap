use crate::email::EmailAddress;
use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// An RFC 8621 §6 Identity object.
///
/// Stores information about an email address or domain the user may send from.
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
}
