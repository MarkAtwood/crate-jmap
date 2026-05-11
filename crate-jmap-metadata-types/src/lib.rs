//! JMAP Object Metadata extension data types.
//!
//! Implements the data types defined in
//! [draft-ietf-jmap-metadata-01](https://www.ietf.org/archive/id/draft-ietf-jmap-metadata-01.txt).
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
//!         assert_eq!(related_type, "Email");
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
pub use filter::MetadataFilterCondition;
pub use metadata::{Annotation, ImapMetadata, Metadata, WebDavMetadata};
