//! W3C DID Core §3.1 DID syntax: the [`Did`] newtype, and the did:web
//! domain extractor [`Did::web_domain`] (draft-atwood-jmap-did-00
//! §Phishing via `did:web`).
//!
//! The grammar is the DID Core §3.1 ABNF:
//!
//! ```text
//! did                = "did:" method-name ":" method-specific-id
//! method-name        = 1*method-char
//! method-char        = %x61-7A / DIGIT
//! method-specific-id = *( *idchar ":" ) 1*idchar
//! idchar             = ALPHA / DIGIT / "." / "-" / "_" / pct-encoded
//! pct-encoded        = "%" HEXDIG HEXDIG
//! ```

use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A DID that conforms to the W3C DID Core §3.1 syntax.
///
/// draft-atwood-jmap-did-00 §Account-Level Capability Object: the
/// account's `did` "MUST be a valid DID as defined in DID-CORE
/// Section 3.1". Parsing checks the generic syntax only, not any
/// method-specific syntax (DID Core §8.1).
///
/// # Strictness
///
/// The `did:` prefix must be lowercase. RFC 5234 quoted strings are
/// case-insensitive, but RFC 3986 §3.1 makes lowercase the canonical
/// scheme form, and every DID in DID Core and the drafts uses it.
/// Nothing is normalized: comparison is byte-exact.
///
/// # Example
///
/// ```
/// use jmap_did_types::Did;
///
/// let did: Did = "did:web:example.com%3A3000:user:alice".parse()?;
/// assert_eq!(did.method(), "web");
/// assert_eq!(did.web_domain(), Some("example.com"));
/// # Ok::<(), jmap_did_types::DidError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Did(String);

/// Error from parsing a [`Did`]: the string does not match the DID Core
/// §3.1 ABNF.
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DidError {
    /// The string does not start with `did:`.
    MissingPrefix,
    /// The method name is empty, has a character other than `a-z` or
    /// `0-9`, or is not followed by `:`.
    InvalidMethodName,
    /// The method-specific identifier is empty, ends with `:`, or has a
    /// character other than an idchar, `:` or a `%XX` escape.
    InvalidMethodSpecificId,
}

impl fmt::Display for DidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingPrefix => "DID does not start with \"did:\"",
            Self::InvalidMethodName => "DID method name is not 1*(a-z / 0-9) followed by \":\"",
            Self::InvalidMethodSpecificId => "DID method-specific-id does not match DID Core §3.1",
        })
    }
}

impl std::error::Error for DidError {}

impl Did {
    /// Parses `s` against the DID Core §3.1 ABNF.
    pub fn parse(s: &str) -> Result<Self, DidError> {
        s.parse()
    }

    /// The DID as a string, exactly as parsed.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The method name, for example `"key"` or `"web"`: the value
    /// compared against [`DidCapability::supported_methods`](crate::DidCapability).
    pub fn method(&self) -> &str {
        let rest = &self.0["did:".len()..];
        rest.split_once(':').map_or(rest, |(method, _)| method)
    }

    /// The method-specific identifier: everything after the second `:`.
    pub fn method_specific_id(&self) -> &str {
        let rest = &self.0["did:".len()..];
        rest.split_once(':').map_or("", |(_, id)| id)
    }

    /// The domain of a `did:web` DID, the component that
    /// draft-atwood-jmap-did-00 §Phishing via `did:web` says clients
    /// "SHOULD highlight".
    ///
    /// This is the host of the first colon-delimited segment, without
    /// any percent-encoded `%3A<port>` suffix. The return value is
    /// `None` when the DID is not `did:web`, when the host is empty,
    /// when the host is an IPv4 address (did:web: "it MUST NOT include
    /// IP addresses"), or when the segment has any other percent-escape
    /// (a domain name has none).
    pub fn web_domain(&self) -> Option<&str> {
        if self.method() != "web" {
            return None;
        }
        let segment = self.method_specific_id().split(':').next()?;
        let host = match segment.find('%') {
            None => segment,
            Some(i) => {
                let (host, suffix) = segment.split_at(i);
                let port = suffix
                    .strip_prefix("%3A")
                    .or_else(|| suffix.strip_prefix("%3a"))?;
                if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                host
            }
        };
        if host.is_empty() || host.parse::<Ipv4Addr>().is_ok() {
            return None;
        }
        Some(host)
    }
}

fn is_idchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')
}

fn valid_method_specific_id(id: &[u8]) -> bool {
    if id.last().is_none_or(|&b| b == b':') {
        return false;
    }
    let mut i = 0;
    while i < id.len() {
        match id[i] {
            b'%' => {
                let hex = id.get(i + 1..i + 3);
                if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                    return false;
                }
                i += 3;
            }
            b if b == b':' || is_idchar(b) => i += 1,
            _ => return false,
        }
    }
    true
}

fn validate(s: &str) -> Result<(), DidError> {
    let rest = s.strip_prefix("did:").ok_or(DidError::MissingPrefix)?;
    let (method, id) = rest.split_once(':').ok_or(DidError::InvalidMethodName)?;
    if method.is_empty()
        || !method
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err(DidError::InvalidMethodName);
    }
    if !valid_method_specific_id(id.as_bytes()) {
        return Err(DidError::InvalidMethodSpecificId);
    }
    Ok(())
}

impl FromStr for Did {
    type Err = DidError;

    fn from_str(s: &str) -> Result<Self, DidError> {
        validate(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl TryFrom<String> for Did {
    type Error = DidError;

    fn try_from(s: String) -> Result<Self, DidError> {
        validate(&s)?;
        Ok(Self(s))
    }
}

impl From<Did> for String {
    fn from(did: Did) -> Self {
        did.0
    }
}

impl AsRef<str> for Did {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Did {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
