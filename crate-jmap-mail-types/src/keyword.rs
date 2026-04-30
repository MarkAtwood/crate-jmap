// System keyword constants for JMAP Email (RFC 8621 §4.1.1).
//
// These constants are the wire-format strings for the system-defined JMAP
// keywords registered in the IANA "IMAP and JMAP Keywords" registry.
// They appear as keys in the `Email.keywords` map.
//
// Usage:
//
// ```rust
// use jmap_mail_types::keyword;
//
// let is_seen = email.keywords.contains_key(keyword::SEEN);
// ```

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

    /// Oracle: constants can be used as HashMap keys (they are &str).
    #[test]
    fn keyword_usable_as_hashmap_key() {
        let mut keywords = std::collections::HashMap::new();
        keywords.insert(SEEN.to_owned(), true);
        keywords.insert(FLAGGED.to_owned(), true);
        assert!(keywords.contains_key(SEEN));
        assert!(keywords.contains_key(FLAGGED));
        assert!(!keywords.contains_key(DRAFT));
    }
}
