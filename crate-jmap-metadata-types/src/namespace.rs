//! draft-ietf-jmap-metadata-02 §2.1 — namespace identifier validation.
//!
//! Namespace identifiers are the keys of the `metadata` and
//! `privateMetadata` objects. Each identifier is either a *registered name*
//! (no dot) or a *domain name* (contains at least one dot).

/// Returns `true` if `name` is a valid registered namespace identifier
/// (draft-ietf-jmap-metadata-02 §2.1).
///
/// A registered name is a non-empty sequence of US-ASCII letters, digits,
/// hyphens (`-`), and underscores (`_`), with **no dot** (`.`).
///
/// ```
/// use jmap_metadata_types::is_registered_namespace;
///
/// assert!(is_registered_namespace("photography"));
/// assert!(is_registered_namespace("my-namespace_v2"));
/// assert!(!is_registered_namespace("acme.example.com")); // domain name
/// assert!(!is_registered_namespace("")); // empty
/// assert!(!is_registered_namespace("has space")); // space
/// ```
pub fn is_registered_namespace(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Returns `true` if `name` is a valid vendor (domain-name) namespace
/// identifier (draft-ietf-jmap-metadata-02 §2.1).
///
/// A domain-name namespace contains at least one dot (`.`) and is in DNS
/// form (e.g. `example.com`, `acme.example.org`). This function validates
/// the structural requirement (contains a dot, non-empty labels, printable
/// ASCII) but does **not** perform DNS resolution or full RFC 5321 validation.
///
/// ```
/// use jmap_metadata_types::is_vendor_namespace;
///
/// assert!(is_vendor_namespace("acme.example.com"));
/// assert!(is_vendor_namespace("a.b"));
/// assert!(!is_vendor_namespace("photography")); // no dot → registered
/// assert!(!is_vendor_namespace(".leading.dot")); // empty label
/// assert!(!is_vendor_namespace("trailing.dot.")); // empty label
/// assert!(!is_vendor_namespace("")); // empty
/// ```
pub fn is_vendor_namespace(name: &str) -> bool {
    if name.is_empty() || !name.contains('.') {
        return false;
    }
    // Every label must be non-empty and contain only printable ASCII
    // (no whitespace, no control chars).
    name.split('.').all(|label| {
        !label.is_empty()
            && label
                .bytes()
                .all(|b| b.is_ascii_graphic())
    })
}

/// Returns `true` if `name` is a valid namespace identifier of either kind
/// (registered name or vendor domain name).
///
/// Equivalent to `is_registered_namespace(name) || is_vendor_namespace(name)`.
///
/// ```
/// use jmap_metadata_types::is_valid_namespace;
///
/// assert!(is_valid_namespace("photography"));
/// assert!(is_valid_namespace("acme.example.com"));
/// assert!(!is_valid_namespace(""));
/// assert!(!is_valid_namespace("has space"));
/// ```
pub fn is_valid_namespace(name: &str) -> bool {
    is_registered_namespace(name) || is_vendor_namespace(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- registered namespace ------------------------------------------------

    #[test]
    fn registered_simple_name() {
        assert!(is_registered_namespace("photography"));
    }

    #[test]
    fn registered_with_hyphens_underscores_digits() {
        assert!(is_registered_namespace("my-namespace_v2"));
        assert!(is_registered_namespace("a"));
        assert!(is_registered_namespace("A1-b2_c3"));
    }

    #[test]
    fn registered_rejects_empty() {
        assert!(!is_registered_namespace(""));
    }

    #[test]
    fn registered_rejects_dot() {
        assert!(!is_registered_namespace("has.dot"));
    }

    #[test]
    fn registered_rejects_space() {
        assert!(!is_registered_namespace("has space"));
    }

    #[test]
    fn registered_rejects_non_ascii() {
        assert!(!is_registered_namespace("café"));
    }

    #[test]
    fn registered_rejects_special_chars() {
        assert!(!is_registered_namespace("foo@bar"));
        assert!(!is_registered_namespace("foo/bar"));
        assert!(!is_registered_namespace("foo:bar"));
    }

    // -- vendor namespace ----------------------------------------------------

    #[test]
    fn vendor_simple_domain() {
        assert!(is_vendor_namespace("acme.example.com"));
        assert!(is_vendor_namespace("a.b"));
    }

    #[test]
    fn vendor_rejects_no_dot() {
        assert!(!is_vendor_namespace("nodot"));
    }

    #[test]
    fn vendor_rejects_empty() {
        assert!(!is_vendor_namespace(""));
    }

    #[test]
    fn vendor_rejects_leading_dot() {
        assert!(!is_vendor_namespace(".leading"));
    }

    #[test]
    fn vendor_rejects_trailing_dot() {
        assert!(!is_vendor_namespace("trailing."));
    }

    #[test]
    fn vendor_rejects_consecutive_dots() {
        assert!(!is_vendor_namespace("a..b"));
    }

    #[test]
    fn vendor_rejects_space_in_label() {
        assert!(!is_vendor_namespace("a. b"));
    }

    // -- is_valid_namespace --------------------------------------------------

    #[test]
    fn valid_accepts_both_kinds() {
        assert!(is_valid_namespace("photography"));
        assert!(is_valid_namespace("acme.example.com"));
    }

    #[test]
    fn valid_rejects_invalid() {
        assert!(!is_valid_namespace(""));
        assert!(!is_valid_namespace("has space"));
        assert!(!is_valid_namespace(".leading"));
    }
}
