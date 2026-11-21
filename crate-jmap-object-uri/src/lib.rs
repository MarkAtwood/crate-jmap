//! The `jmap` URI scheme and JMAP object URIs, defined by
//! draft-atwood-jmap-object-uri-00.
//!
//! A JMAP object URI, `jmap:<type>:<accountId>:<objectId>`, names one
//! RFC 8620 object by type name, account and id. It is a reference
//! relative to the Session or federation peer it was received from: it is
//! not globally unique and cannot be dereferenced on its own
//! (§Interpretation).
//!
//! - Syntax: [`JmapUri`] parses ([`str::parse`]), validates and formats
//!   ([`Display`](std::fmt::Display)) the URI; [`JmapUriError`] says why a
//!   string is not one. Both are `jmap-types` items, re-exported here
//!   because this crate is their natural home on crates.io. [`SCHEME`] is
//!   the scheme name, and [`is_jmap_uri`] tests for the scheme alone, for
//!   rules that cover every `jmap` URI form (§Other Forms).
//! - Registry: [`URI_TYPES`] is the "JMAP Object URI Types" registry
//!   (§JMAP Object URI Types Registry) as filled by the drafts that use the
//!   scheme; [`uri_type`] and [`uri_type_for_data_type`] look rows up.
//! - Resolution: [`resolve_type`] maps a URI to the data type, `/get`
//!   method and capability that dereference it; [`resolve`](fn@resolve) also checks the
//!   account and capability against a [`SessionView`], such as the RFC 8620
//!   §2 Session object as JSON (§Interpretation). [`ResolveError`] says why
//!   a URI cannot be resolved.
//!
//! # Example
//!
//! ```
//! use jmap_object_uri::{JmapUri, resolve};
//! use serde_json::json;
//!
//! let uri: JmapUri = "jmap:calendarevent:A13824:E6B2".parse().unwrap();
//! assert_eq!(uri.to_string(), "jmap:calendarevent:A13824:E6B2");
//!
//! let session = json!({
//!     "capabilities": { "urn:ietf:params:jmap:calendars": {} },
//!     "accounts": { "A13824": { "accountCapabilities": {
//!         "urn:ietf:params:jmap:calendars": {}
//!     } } }
//! });
//! let found = resolve(&uri, &session).unwrap();
//! assert_eq!(found.get_method(), "CalendarEvent/get");
//! assert_eq!(found.capability(), "urn:ietf:params:jmap:calendars");
//! assert_eq!(
//!     found.get_arguments(),
//!     json!({ "accountId": "A13824", "ids": ["E6B2"] })
//! );
//! ```
//!
//! No async and no I/O: resolving a URI never fetches anything (§Security
//! Considerations); the caller makes the `/get` call.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod registry;
mod resolve;

pub use jmap_types::{JmapUri, JmapUriError};
pub use registry::{URI_TYPES, UriTypeEntry, uri_type, uri_type_for_data_type};
pub use resolve::{Resolution, ResolveError, SessionView, resolve, resolve_type};

/// The URI scheme name `jmap` (draft-atwood-jmap-object-uri-00 §URI
/// Scheme Registration). Scheme names compare case-insensitively
/// (RFC 3986 §3.1).
pub const SCHEME: &str = "jmap";

/// Whether `uri` has the `jmap` scheme, compared case-insensitively
/// (RFC 3986 §3.1), whatever its scheme-specific part.
///
/// This is wider than "parses as a [`JmapUri`]": it also matches the
/// reserved `jmap://` authority forms and malformed `jmap:` strings, which
/// a receiver treats as unresolvable (§Other Forms). Use it for rules that
/// cover every `jmap` URI, such as draft-atwood-jmap-chat-federation-00
/// §User References in Peer Methods, which removes `MessageAction`s and
/// Endpoints whose `uri` has the scheme `jmap`.
///
/// ```
/// use jmap_object_uri::is_jmap_uri;
///
/// assert!(is_jmap_uri("jmap:task:A1:T9"));
/// assert!(is_jmap_uri("JMAP://reserved.example/x"));
/// assert!(!is_jmap_uri("https://tracker.example/T-1"));
/// assert!(!is_jmap_uri("urn:ietf:params:jmap:mail"));
/// ```
pub fn is_jmap_uri(uri: &str) -> bool {
    uri.split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(SCHEME))
}
