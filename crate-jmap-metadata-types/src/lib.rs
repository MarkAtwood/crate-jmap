//! JMAP Object Metadata extension data types.
//!
//! Implements the data types defined in
//! [draft-ietf-jmap-metadata-05](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/).
//! Types only — no method handlers, no async, no network I/O.
//!
//! ## Architecture change from -01 to -02
//!
//! Since -02, the draft has no standalone `Metadata` object type. Instead,
//! each listed data type gains `metadata` (shared) and `privateMetadata`
//! (per-user) properties — plain `String[Object]` maps keyed by namespace
//! identifier.
//!
//! This crate provides the helper types that consumers need:
//!
//! - Crate root — [`DataTypeMetadataInfo`], [`JMAP_METADATA_URI`] (§1.2.1)
//! - `filter` — [`MetadataTextMatch`], [`WithMetadata`] (§3.6)
//! - `namespace` — namespace identifier validation (§2.1)
//!
//! All public types and functions are re-exported at the crate root.
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types
//! ([`MetadataTextMatch`] and [`WithMetadata`], whose `metadata` /
//! `privateMetadata` match values are user-chosen text). `tracing` or `log`
//! macros that interpolate one of these values with `?` (debug) or `%`
//! (display) emit its full contents to the configured sink: the text a user
//! searches their (possibly private) metadata for, which can be names,
//! addresses, or other personal notes. That data is typically regulated as
//! personal data under GDPR, CCPA, and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the searched-for metadata text.
//! tracing::debug!(filter = ?condition, "running query");
//!
//! // GOOD — emits only which namespace path was matched.
//! tracing::debug!(path = %text_match.path, "running query");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.
//!
//! # Example
//!
//! ```rust
//! use jmap_metadata_types::{DataTypeMetadataInfo, is_valid_namespace};
//!
//! // Server advertises per-type metadata capability
//! let json = r#"{
//!     "namespaces": ["photography"],
//!     "supportsVendorNamespaces": false,
//!     "supportsPrivate": false,
//!     "maxDepth": 3
//! }"#;
//! let info: DataTypeMetadataInfo = serde_json::from_str(json).unwrap();
//! assert_eq!(info.namespaces, vec!["photography"]);
//! assert_eq!(info.max_depth, Some(3));
//!
//! // Validate namespace identifiers
//! assert!(is_valid_namespace("photography"));       // registered
//! assert!(is_valid_namespace("acme.example.com"));  // vendor domain
//! assert!(!is_valid_namespace(""));                 // empty
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod capability;
mod filter;
mod namespace;

pub use capability::{DataTypeMetadataInfo, JMAP_METADATA_URI};
pub use filter::{MetadataTextMatch, WithMetadata};
pub use namespace::{is_registered_namespace, is_valid_namespace, is_vendor_namespace};
