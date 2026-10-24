//! draft-ietf-jmap-metadata-04 §2.1 — namespace identifier validation.
//!
//! Namespace identifiers are the keys of the `metadata` and
//! `privateMetadata` objects. Each identifier is either a *registered name*
//! (no dot) or a *domain name* (contains at least one dot).

/// Returns `true` if `name` is a valid registered namespace identifier
/// (draft-ietf-jmap-metadata-04 §2.1).
///
/// A registered name is a non-empty sequence of lowercase US-ASCII letters,
/// digits, hyphens (`-`), and underscores (`_`), with **no dot** (`.`).
/// Uppercase letters are rejected.
///
/// ```
/// use jmap_metadata_types::is_registered_namespace;
///
/// assert!(is_registered_namespace("photography"));
/// assert!(is_registered_namespace("my-namespace_v2"));
/// assert!(!is_registered_namespace("acme.example.com")); // domain name
/// assert!(!is_registered_namespace("")); // empty
/// assert!(!is_registered_namespace("has space")); // space
/// assert!(!is_registered_namespace("Photography")); // uppercase
/// ```
pub fn is_registered_namespace(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Returns `true` if `name` is a valid vendor (domain-name) namespace
/// identifier (draft-ietf-jmap-metadata-04 §2.1).
///
/// A domain-name namespace contains at least one dot (`.`) and is in DNS
/// form (e.g. `example.com`, `acme.example.org`). Every label must be a
/// lowercase LDH label: non-empty, only lowercase US-ASCII letters, digits
/// and hyphens, and not starting or ending with a hyphen. This rejects
/// uppercase ("Domain-name identifiers MUST be in lowercase"), U-labels
/// (internationalized labels must be "in A-label form") and characters
/// such as `/` that would collide with `/set` patch paths and `/get`
/// subselectors (§3.1). No DNS resolution is performed.
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
/// assert!(!is_vendor_namespace("Acme.example.com")); // uppercase
/// assert!(!is_vendor_namespace("acme.example.com/k")); // not LDH
/// ```
pub fn is_vendor_namespace(name: &str) -> bool {
    if name.is_empty() || !name.contains('.') {
        return false;
    }
    name.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
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
        assert!(is_registered_namespace("a1-b2_c3"));
    }

    // -04 §2.1: "a sequence of lowercase US-ASCII letters, digits, hyphens,
    // and underscores, with no dot".
    #[test]
    fn registered_rejects_uppercase() {
        assert!(!is_registered_namespace("A1-b2_c3"));
        assert!(!is_registered_namespace("Photography"));
        assert!(!is_registered_namespace("photographY"));
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

    // -04 §2.1: "Servers MUST reject ... a domain name containing uppercase
    // ASCII letters."
    #[test]
    fn vendor_rejects_uppercase() {
        assert!(!is_vendor_namespace("Acme.example.com"));
        assert!(!is_vendor_namespace("acme.EXAMPLE.com"));
        assert!(!is_vendor_namespace("acme.example.coM"));
    }

    // -04 §2.1: internationalized labels "in A-label form" (ASCII).
    #[test]
    fn vendor_accepts_a_label_rejects_u_label() {
        assert!(is_vendor_namespace("xn--mnchen-3ya.example"));
        assert!(!is_vendor_namespace("münchen.example"));
    }

    // -04 §2.1 (L342): a domain name "in DNS form"; §3.1 (L487-491): '/'
    // separates the namespace from the key in patch paths and
    // subselectors, so it cannot appear inside one.
    #[test]
    fn vendor_rejects_non_ldh_chars() {
        for bad in [
            "acme.example.com/k",
            "a.b~c",
            "a.b\"x",
            "a_b.example",
            "a.b:c",
            "a@b.c",
        ] {
            assert!(!is_vendor_namespace(bad), "{bad}");
            assert!(!is_valid_namespace(bad), "{bad}");
        }
    }

    #[test]
    fn vendor_rejects_edge_hyphens() {
        assert!(!is_vendor_namespace("-acme.example"));
        assert!(!is_vendor_namespace("acme-.example"));
        assert!(!is_vendor_namespace("acme.example-"));
        assert!(is_vendor_namespace("my-acme.example"));
        assert!(is_vendor_namespace("0day.example"));
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
        assert!(!is_valid_namespace("Photography"));
        assert!(!is_valid_namespace("Acme.example.com"));
    }
}
