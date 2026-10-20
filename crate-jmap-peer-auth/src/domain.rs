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
    /// or other non-hostname characters, and any IPv4 address literal.
    /// A name whose last label is a number (all decimal digits, or `0x`
    /// and hex digits) is taken as an IPv4 literal: the WHATWG URL host
    /// parser ("ends in a number") and `inet_aton` read `192.0.2.1`,
    /// `127.1` and `0x7f.1` as addresses, not names.
    pub fn parse(name: &str) -> Result<Self, InvalidPeerDomain> {
        let name = name.strip_suffix('.').unwrap_or(name);
        if name.is_empty() {
            return Err(InvalidPeerDomain);
        }
        let ascii = idna::domain_to_ascii_strict(name).map_err(|_| InvalidPeerDomain)?;
        if ends_in_number(&ascii) {
            return Err(InvalidPeerDomain);
        }
        Ok(Self(ascii))
    }

    /// The domain as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// WHATWG URL Standard §3.5 "ends in a number checker", for a lowercase
/// name with no trailing dot: the last label is all ASCII digits, or is
/// `0x` followed only by hex digits.
fn ends_in_number(ascii: &str) -> bool {
    let last = ascii.rsplit('.').next().unwrap_or(ascii);
    match last.strip_prefix("0x") {
        Some(hex) => hex.bytes().all(|b| b.is_ascii_hexdigit()),
        None => !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()),
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
