//! IRI ↔ URI mapping (RFC 3987 §3.1, §3.2) for draft-atwood-jmap-updates-00
//! §7.1: "URIs in JMAP are ASCII. A client or server that holds an
//! Internationalized Resource Identifier (RFC 3987), for example one a user
//! typed, SHOULD map it to a URI before sending it, and a client MAY convert
//! a URI back to an IRI for display. ... A domain name SHOULD be sent in its
//! A-label form."

use std::fmt;

use iri_string::types::{IriStr, UriStr};

/// Why an IRI or URI could not be mapped.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IriError {
    /// The input is not an absolute IRI (for [`iri_to_uri`]) or URI (for
    /// [`uri_to_iri`]).
    Syntax,
    /// The host is not a valid internationalized domain name (UTS #46).
    Host,
}

impl fmt::Display for IriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "not an absolute RFC 3987 IRI / RFC 3986 URI",
            Self::Host => "host is not a valid internationalized domain name",
        })
    }
}

impl std::error::Error for IriError {}

/// Replace the host of `s` (whose scheme is `scheme` and authority
/// `authority`) with `map(host)`.
fn with_host(
    s: &str,
    scheme: &str,
    authority: &str,
    userinfo: Option<&str>,
    host: &str,
    port: Option<&str>,
    new_host: &str,
) -> String {
    let start = scheme.len() + 3; // "scheme://"
    let rest = &s[start + authority.len()..];
    let mut out = String::with_capacity(s.len() + new_host.len());
    out.push_str(&s[..start]);
    if let Some(u) = userinfo {
        out.push_str(u);
        out.push('@');
    }
    out.push_str(new_host);
    if let Some(p) = port {
        out.push(':');
        out.push_str(p);
    }
    out.push_str(rest);
    debug_assert!(s[start..].starts_with(authority) && authority.contains(host));
    out
}

/// Map an absolute IRI to a URI (RFC 3987 §3.1): a non-ASCII registered
/// host becomes its A-label (UTS #46 ToASCII), and every other non-ASCII
/// character is percent-encoded as UTF-8.
///
/// # Errors
///
/// [`IriError::Syntax`] when `iri` is not an absolute IRI;
/// [`IriError::Host`] when its host is not a valid domain name.
///
/// ```rust
/// use jmap_base_client::iri_to_uri;
///
/// assert_eq!(
///     iri_to_uri("https://bücher.example/straße?q=ü#ä").unwrap(),
///     "https://xn--bcher-kva.example/stra%C3%9Fe?q=%C3%BC#%C3%A4"
/// );
/// ```
pub fn iri_to_uri(iri: &str) -> Result<String, IriError> {
    let parsed = IriStr::new(iri).map_err(|_| IriError::Syntax)?;
    let mut ascii_host = None;
    if let (Some(authority), Some(c)) = (parsed.authority_str(), parsed.authority_components())
        && !c.host().is_ascii()
    {
        let host = idna::domain_to_ascii(c.host()).map_err(|_| IriError::Host)?;
        ascii_host = Some(with_host(
            iri,
            parsed.scheme_str(),
            authority,
            c.userinfo(),
            c.host(),
            c.port(),
            &host,
        ));
    }
    let source = ascii_host.as_deref().unwrap_or(iri);
    let parsed = IriStr::new(source).map_err(|_| IriError::Syntax)?;
    Ok(parsed.encode_to_uri().to_string())
}

/// `true` for characters RFC 3987 §3.2 says to leave percent-encoded when
/// converting to an IRI: C1 controls, the bidirectional formatting
/// characters (§4.1), and noncharacters.
fn keep_encoded(c: char) -> bool {
    let u = c as u32;
    (0x80..0xA0).contains(&u)
        || matches!(u, 0x200E | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
        || (0xFDD0..=0xFDEF).contains(&u)
        || (u & 0xFFFE) == 0xFFFE
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decode the percent-encoded UTF-8 of non-ASCII characters in `s`, leaving
/// ASCII percent-encodings, invalid UTF-8 and the characters of
/// [`keep_encoded`] as they are (RFC 3987 §3.2 steps 1 to 3).
fn decode_non_ascii(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            // `s` is ASCII here (a URI), so byte and char boundaries agree.
            out.push(b[i] as char);
            i += 1;
            continue;
        }
        // Collect the run of %XX octets starting here.
        let mut bytes = Vec::new();
        let mut j = i;
        while j + 2 < b.len() && b[j] == b'%' {
            match (hex(b[j + 1]), hex(b[j + 2])) {
                (Some(h), Some(l)) => bytes.push(h * 16 + l),
                _ => break,
            }
            j += 3;
        }
        if bytes.is_empty() {
            out.push('%');
            i += 1;
            continue;
        }
        // Emit each octet: decode a complete, allowed non-ASCII UTF-8
        // sequence, otherwise keep the original %XX text.
        let mut k = 0;
        while k < bytes.len() {
            let len = match bytes[k] {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => 1,
            };
            let decoded = (len > 1 && k + len <= bytes.len())
                .then(|| std::str::from_utf8(&bytes[k..k + len]).ok())
                .flatten()
                .and_then(|t| t.chars().next())
                .filter(|c| !keep_encoded(*c));
            match decoded {
                Some(c) => {
                    out.push(c);
                    k += len;
                }
                None => {
                    let at = i + 3 * k;
                    out.push_str(&s[at..at + 3]);
                    k += 1;
                }
            }
        }
        i += 3 * bytes.len();
    }
    out
}

/// Convert a URI to an IRI for display (RFC 3987 §3.2): percent-encoded
/// UTF-8 of non-ASCII characters is decoded, and an A-label host is shown
/// as its U-label. ASCII percent-encodings stay, so the result maps back to
/// the same URI. Never use the result to identify a resource; send the URI.
///
/// # Errors
///
/// [`IriError::Syntax`] when `uri` is not an RFC 3986 URI.
///
/// ```rust
/// use jmap_base_client::uri_to_iri;
///
/// assert_eq!(
///     uri_to_iri("https://xn--bcher-kva.example/stra%C3%9Fe%20x").unwrap(),
///     "https://bücher.example/straße%20x"
/// );
/// ```
pub fn uri_to_iri(uri: &str) -> Result<String, IriError> {
    let parsed = UriStr::new(uri).map_err(|_| IriError::Syntax)?;
    let mut source = uri.to_owned();
    if let (Some(authority), Some(c)) = (parsed.authority_str(), parsed.authority_components())
        && c.host()
            .split('.')
            .any(|l| l.len() > 4 && l[..4].eq_ignore_ascii_case("xn--"))
    {
        let (host, result) = idna::domain_to_unicode(c.host());
        if result.is_ok() {
            source = with_host(
                uri,
                parsed.scheme_str(),
                authority,
                c.userinfo(),
                c.host(),
                c.port(),
                &host,
            );
        }
    }
    // Decode only after the scheme and authority, so a host is never
    // percent-decoded (the U-label above already handled it).
    let split = parsed.scheme_str().len() + 1 + parsed.authority_str().map_or(0, |a| a.len() + 2);
    // The host swap leaves everything after the authority unchanged, so
    // measure that tail from the end (a U-label can be shorter than its
    // A-label).
    let tail_len = uri.len() - split;
    let (head, tail) = source.split_at(source.len() - tail_len);
    Ok(format!("{head}{}", decode_non_ascii(tail)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 3987 §3.1 example: "http://r&#xE9;sum&#xE9;.example.org" maps to
    /// "http://xn--rsum-bpad.example.org" (host) and the §3.1 path example
    /// "http://www.example.org/D%C3%BCrst".
    #[test]
    fn rfc3987_section_3_1_examples() {
        assert_eq!(
            iri_to_uri("http://résumé.example.org").unwrap(),
            "http://xn--rsum-bpad.example.org"
        );
        assert_eq!(
            iri_to_uri("http://www.example.org/Dürst").unwrap(),
            "http://www.example.org/D%C3%BCrst"
        );
    }

    /// RFC 3987 §3.2 example: "http://www.example.org/D%C3%BCrst" converts
    /// to "http://www.example.org/D&#xFC;rst", and "%FCrst" (not UTF-8)
    /// stays.
    #[test]
    fn rfc3987_section_3_2_examples() {
        assert_eq!(
            uri_to_iri("http://www.example.org/D%C3%BCrst").unwrap(),
            "http://www.example.org/Dürst"
        );
        assert_eq!(
            uri_to_iri("http://www.example.org/D%FCrst").unwrap(),
            "http://www.example.org/D%FCrst"
        );
    }

    /// RFC 3987 §4.1: bidi formatting characters stay encoded (U+202E is
    /// E2 80 AE).
    #[test]
    fn bidi_formatting_stays_encoded() {
        assert_eq!(
            uri_to_iri("https://example.org/a%E2%80%AEb").unwrap(),
            "https://example.org/a%E2%80%AEb"
        );
    }

    /// Userinfo, port and an ASCII host are preserved; an ASCII IRI is
    /// already a URI.
    #[test]
    fn keeps_other_components() {
        assert_eq!(
            iri_to_uri("https://u@bücher.example:8443/x").unwrap(),
            "https://u@xn--bcher-kva.example:8443/x"
        );
        assert_eq!(
            iri_to_uri("mailto:a@example.com").unwrap(),
            "mailto:a@example.com"
        );
        assert_eq!(
            uri_to_iri("https://u@xn--bcher-kva.example:8443/%C3%A4").unwrap(),
            "https://u@bücher.example:8443/ä"
        );
    }

    #[test]
    fn rejects_non_iris() {
        assert_eq!(iri_to_uri("/relative"), Err(IriError::Syntax));
        assert_eq!(uri_to_iri("https://example.org/ü"), Err(IriError::Syntax));
    }
}
