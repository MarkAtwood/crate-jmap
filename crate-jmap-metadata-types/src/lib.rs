//! JMAP Object Metadata extension data types.
//!
//! Implements the data types defined in
//! [draft-ietf-jmap-metadata-02](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/).
//! Types only — no method handlers, no async, no network I/O.
//!
//! ## Architecture change from -01 to -02
//!
//! draft-ietf-jmap-metadata-02 eliminates the standalone `Metadata` object
//! type entirely. Instead, each opted-in data type gains `metadata` (shared)
//! and `privateMetadata` (per-user) properties — plain `String[Object]` maps
//! keyed by namespace identifier.
//!
//! This crate provides the helper types that consumers need:
//!
//! - [`capability`] — [`DataTypeMetadataInfo`], [`JMAP_METADATA_URI`] (§1.2.1)
//! - [`filter`] — [`MetadataTextMatch`] (§3.5)
//! - [`namespace`] — namespace identifier validation (§2.1)
//!
//! All public types and functions are re-exported at the crate root.
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

#![forbid(unsafe_code)]

pub mod capability;
pub mod filter;
pub mod namespace;

pub use capability::{DataTypeMetadataInfo, JMAP_METADATA_URI};
pub use filter::MetadataTextMatch;
pub use namespace::{is_registered_namespace, is_valid_namespace, is_vendor_namespace};
