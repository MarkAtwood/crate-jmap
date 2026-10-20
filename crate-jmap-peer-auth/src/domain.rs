//! Peer domains (draft-atwood-jmap-chat-federation-00 §Terminology).

use std::fmt;
use std::str::FromStr;

/// A peer domain in the form the draft compares it in.
///
/// draft-atwood-jmap-chat-federation-00 §Terminology: "A peer domain is
/// compared in lowercase A-label form, without a trailing dot or port."
/// [`PeerDomain::parse`] produces that form, so two `PeerDomain` values
/// name the same peer exactly when they are equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerDomain(String);

/// The input is not a DNS domain name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a peer domain (a DNS domain name)")]
pub struct InvalidPeerDomain;

impl PeerDomain {
    /// Parses a DNS domain name into its compared form.
    ///
    /// One trailing dot is removed, and U-labels are converted to
    /// lowercase A-labels with the UTS 46 strict rules (STD3 characters,
    /// DNS length limits).
    ///
    /// # Errors
    ///
    /// [`InvalidPeerDomain`] when the input is empty or is not a domain
    /// name under those rules, including any input with a port, a path
    /// or other non-hostname characters.
    pub fn parse(name: &str) -> Result<Self, InvalidPeerDomain> {
        let name = name.strip_suffix('.').unwrap_or(name);
        if name.is_empty() {
            return Err(InvalidPeerDomain);
        }
        idna::domain_to_ascii_strict(name)
            .map(Self)
            .map_err(|_| InvalidPeerDomain)
    }

    /// The domain as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PeerDomain {
    type Err = InvalidPeerDomain;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::parse(name)
    }
}

impl fmt::Display for PeerDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for PeerDomain {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
