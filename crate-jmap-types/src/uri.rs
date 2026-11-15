//! RFC 3986 URI syntax checks (draft-atwood-jmap-updates-00 §7.1).
//!
//! "A property, argument, or Session member whose value is a URI or URL
//! SHOULD hold a URI as defined in RFC 3986 Section 3. Unless its defining
//! specification allows a relative reference, it SHOULD be a URI and not a
//! relative reference." URIs in JMAP are ASCII: an IRI is mapped to a URI
//! (RFC 3987 §3.1) before it is sent; `jmap-base-client` has the mapping.
//!
//! The workspace's `url` crate implements the WHATWG URL Standard, which
//! normalizes and accepts input RFC 3986 rejects, so it is not used here
//! (decision 29). Identifying URIs, such as capability URIs, compare as
//! exact strings (RFC 3986 §6.2.1) unless their specification names
//! another comparison.

use std::fmt;

/// Why a string is not an RFC 3986 URI or URI reference.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriError {
    /// The string does not match the RFC 3986 grammar, for example because
    /// it has a space, a non-ASCII character or a bad percent-encoding.
    Syntax,
    /// The string is a relative reference where a URI with a scheme is
    /// required (RFC 3986 §4.2).
    Relative,
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "not an RFC 3986 URI",
            Self::Relative => "a relative reference where an RFC 3986 URI is required",
        })
    }
}

impl std::error::Error for UriError {}

/// Check that `s` is an RFC 3986 §3 `URI`: a scheme, then hierarchical
/// part, optional query and optional fragment. Fragments are allowed.
///
/// # Errors
///
/// [`UriError::Relative`] for a well-formed relative reference,
/// [`UriError::Syntax`] for anything else that is not a URI.
///
/// ```rust
/// use jmap_types::uri::{UriError, check_uri};
///
/// assert!(check_uri("https://example.com/a?b#c").is_ok());
/// assert!(check_uri("urn:ietf:params:jmap:core").is_ok());
/// assert_eq!(check_uri("/a/b"), Err(UriError::Relative));
/// assert_eq!(check_uri("https://example.com/a b"), Err(UriError::Syntax));
/// ```
pub fn check_uri(s: &str) -> Result<(), UriError> {
    if iri_string::types::UriStr::new(s).is_ok() {
        return Ok(());
    }
    if iri_string::types::UriReferenceStr::new(s).is_ok() {
        Err(UriError::Relative)
    } else {
        Err(UriError::Syntax)
    }
}

/// Check that `s` is an RFC 3986 §4.1 `URI-reference`: a URI or a relative
/// reference, for properties whose specification allows relative
/// references.
///
/// # Errors
///
/// [`UriError::Syntax`] when it is neither.
pub fn check_uri_reference(s: &str) -> Result<(), UriError> {
    iri_string::types::UriReferenceStr::new(s)
        .map(|_| ())
        .map_err(|_| UriError::Syntax)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 3986 §1.1.2 examples are all URIs.
    #[test]
    fn rfc3986_examples_are_uris() {
        for u in [
            "ftp://ftp.is.co.za/rfc/rfc1808.txt",
            "http://www.ietf.org/rfc/rfc2396.txt",
            "ldap://[2001:db8::7]/c=GB?objectClass?one",
            "mailto:John.Doe@example.com",
            "news:comp.infosystems.www.servers.unix",
            "tel:+1-816-555-1212",
            "telnet://192.0.2.16:80/",
            "urn:oasis:names:specification:docbook:dtd:xml:4.1.2",
        ] {
            assert_eq!(check_uri(u), Ok(()), "{u}");
        }
    }

    /// RFC 3986 §5.4 relative references are references, not URIs.
    #[test]
    fn relative_references() {
        for r in ["g", "./g", "g/", "/g", "//g", "?y", "g?y", "#s", "../g"] {
            assert_eq!(check_uri(r), Err(UriError::Relative), "{r}");
            assert_eq!(check_uri_reference(r), Ok(()), "{r}");
        }
    }

    /// RFC 3986 §2: no spaces, no raw non-ASCII (that is an IRI, RFC 3987),
    /// and `%` must start a percent-encoding.
    #[test]
    fn syntax_errors() {
        for s in [
            "https://example.com/a b",
            "https://example.com/ü",
            "https://example.com/%zz",
            "",
        ] {
            assert_eq!(
                check_uri(s),
                if s.is_empty() {
                    Err(UriError::Relative)
                } else {
                    Err(UriError::Syntax)
                },
                "{s:?}"
            );
        }
        assert_eq!(
            check_uri_reference("https://example.com/a b"),
            Err(UriError::Syntax)
        );
    }
}
