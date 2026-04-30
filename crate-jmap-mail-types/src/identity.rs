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
}

impl Identity {
    /// Construct an [`Identity`] from its three identifying fields.
    ///
    /// `name`, `text_signature`, and `html_signature` default to `""`.
    /// `reply_to` and `bcc` default to `None`.
    pub fn new(id: impl Into<Id>, email: impl Into<String>, may_delete: bool) -> Self {
        Self {
            id: id.into(),
            email: email.into(),
            may_delete,
            name: String::new(),
            reply_to: None,
            bcc: None,
            text_signature: String::new(),
            html_signature: String::new(),
        }
    }
}
