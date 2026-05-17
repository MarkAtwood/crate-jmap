//! JMAP Object Metadata extension data types.
//!
//! Implements the data types defined in
//! [draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/).
//! Types only — no method handlers, no async, no network I/O.
//!
//! ## Module layout
//!
//! - [`metadata`] — [`Metadata`], [`Annotation`], [`ImapMetadata`],
//!   [`WebDavMetadata`] (§2)
//! - [`capability`] — [`MetadataCapability`], [`JMAP_METADATA_URI`] (§1.2.1)
//! - [`filter`] — [`MetadataFilterCondition`] (§3.4.1)
//! - [`backend`] — [`MetadataProperty`] and `JmapObject` trait impls
//!
//! All public types are re-exported at the crate root.
//!
//! # Example
//!
//! ```rust
//! use jmap_metadata_types::{Annotation, Metadata};
//!
//! let json = r#"{
//!     "@type": "Annotation",
//!     "id": "MD789",
//!     "relatedType": "Email",
//!     "relatedId": "EM456",
//!     "isPrivate": true,
//!     "acme.example.com:workflowState": "pending-review"
//! }"#;
//!
//! let meta: Metadata = serde_json::from_str(json).unwrap();
//! match meta {
//!     Metadata::Annotation(Annotation { ref related_type, ref extra, .. }) => {
//!         // `related_type` is `Option<String>` so the §4.1 extended-`/get`
//!         // partial-response shape (which can omit it, per the §7.2
//!         // example) round-trips losslessly. Full `Metadata/get` and
//!         // `Metadata/set` responses always populate it.
//!         assert_eq!(related_type.as_deref(), Some("Email"));
//!         assert_eq!(
//!             extra.get("acme.example.com:workflowState"),
//!             Some(&serde_json::Value::String("pending-review".into())),
//!         );
//!     }
//!     _ => panic!("expected Annotation variant"),
//! }
//! ```

#![forbid(unsafe_code)]

pub mod backend;
pub mod capability;
pub mod filter;
pub mod metadata;

pub use backend::MetadataProperty;
pub use capability::{MetadataCapability, JMAP_METADATA_URI};
pub use filter::{MetadataFilter, MetadataFilterCondition};
pub use metadata::{Annotation, ImapMetadata, Metadata, WebDavMetadata};

/// Generic filter algebra from `jmap-types::query` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-metadata-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the canonical
/// [`jmap_mail_types::query`] re-exports from the workspace canonical
/// extension-types template.
///
/// [`jmap_mail_types::query`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/index.html
pub use jmap_types::query::{Filter, FilterOperator, Operator};
