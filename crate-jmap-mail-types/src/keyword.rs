//! System keyword constants and the [`Keyword`] newtype for JMAP Email (RFC 8621 §4.1.1).
//!
//! The [`Keyword`] type wraps a keyword string and serialises transparently as a
//! JSON string.  The constants below are `&str` values for the IANA-registered
//! system keywords.
//!
//! # Example
//!
//! ```rust
//! use jmap_mail_types::Keyword;
//! use jmap_mail_types::keyword;
//!
//! let kw = Keyword::from(keyword::SEEN);
//! // HashMap<Keyword, bool> also accepts &str via Borrow<str>:
//! // let is_seen = email.keywords.contains_key(keyword::SEEN);
//! ```

use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

/// A JMAP keyword string (RFC 8621 §4.1.1).
///
/// Keywords are used to tag [`crate::Email`] objects.  System keywords begin
/// with `$`; user-defined keywords must not.  The IANA-registered system
/// keywords are available as `&str` constants in this module (e.g.
/// [`SEEN`], [`FLAGGED`]).
///
/// `Keyword` serialises and deserialises transparently as a JSON string, so
/// `HashMap<Keyword, bool>` round-trips correctly with the JMAP wire format.
///
/// Because `Keyword` implements `Borrow<str>`, a `HashMap<Keyword, bool>` can
/// be queried with a bare `&str`:
///
/// ```rust
/// use jmap_mail_types::{keyword, Keyword};
/// use std::collections::HashMap;
///
/// let mut kw: HashMap<Keyword, bool> = HashMap::new();
/// kw.insert(Keyword::from(keyword::SEEN), true);
/// assert!(kw.contains_key(keyword::SEEN));
/// ```
///
/// No syntax validation is performed at construction time — keyword syntax
/// validation is the server's responsibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Keyword(String);

impl Keyword {
    /// Construct a [`Keyword`] from a string without validation.
    pub fn new(s: impl Into<String>) -> Self {
        Keyword(s.into())
    }
}

impl From<&str> for Keyword {
    fn from(s: &str) -> Self {
        Keyword(s.to_owned())
    }
}

impl From<String> for Keyword {
    fn from(s: String) -> Self {
        Keyword(s)
    }
}

impl AsRef<str> for Keyword {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Keyword {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Deref for Keyword {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The Email is a draft being composed by the user (RFC 8621 §4.1.1).
pub const DRAFT: &str = "$draft";

/// The Email has been read (RFC 8621 §4.1.1).
pub const SEEN: &str = "$seen";

/// The Email has been flagged for urgent/special attention (RFC 8621 §4.1.1).
pub const FLAGGED: &str = "$flagged";

/// The Email has been replied to (RFC 8621 §4.1.1).
pub const ANSWERED: &str = "$answered";

/// The Email has been forwarded (RFC 8621 §4.1.1 / IANA registry).
pub const FORWARDED: &str = "$forwarded";

/// The Email is highly likely to be phishing (RFC 8621 §4.1.1 / IANA registry).
pub const PHISHING: &str = "$phishing";

/// The Email is definitely spam (RFC 8621 §4.1.1 / IANA registry).
pub const JUNK: &str = "$junk";

/// The Email is definitely not spam (RFC 8621 §4.1.1 / IANA registry).
pub const NOT_JUNK: &str = "$notjunk";

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: RFC 8621 §4.1.1 — keyword wire strings include the leading `$`.
    #[test]
    fn keyword_wire_strings() {
        assert_eq!(DRAFT, "$draft");
        assert_eq!(SEEN, "$seen");
        assert_eq!(FLAGGED, "$flagged");
        assert_eq!(ANSWERED, "$answered");
        assert_eq!(FORWARDED, "$forwarded");
        assert_eq!(PHISHING, "$phishing");
        assert_eq!(JUNK, "$junk");
        assert_eq!(NOT_JUNK, "$notjunk");
    }

    /// Oracle: Keyword implements Borrow<str> so HashMap<Keyword, bool>
    /// can be queried with a bare &str constant.
    #[test]
    fn keyword_usable_as_hashmap_key() {
        let mut keywords = std::collections::HashMap::new();
        keywords.insert(Keyword::from(SEEN), true);
        keywords.insert(Keyword::from(FLAGGED), true);
        assert!(keywords.contains_key(SEEN));
        assert!(keywords.contains_key(FLAGGED));
        assert!(!keywords.contains_key(DRAFT));
    }

    /// Oracle: Keyword serialises transparently as a JSON string.
    #[test]
    fn keyword_roundtrips_as_json_string() {
        let kw = Keyword::from(SEEN);
        let json = serde_json::to_string(&kw).expect("serialize");
        assert_eq!(json, "\"$seen\"");
        let back: Keyword = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kw);
    }

    /// Oracle: Keyword::new and From impls produce equal values.
    #[test]
    fn keyword_construction() {
        let a = Keyword::new("$seen");
        let b = Keyword::from("$seen");
        let c = Keyword::from("$seen".to_owned());
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a.as_ref(), "$seen");
        assert_eq!(a.to_string(), "$seen");
    }
}
