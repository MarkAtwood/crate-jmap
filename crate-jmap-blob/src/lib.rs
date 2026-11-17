//! JMAP Blob Management
//! ([draft-ietf-jmap-blobext-03](https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/),
//! `urn:ietf:params:jmap:blob2`, with legacy
//! [RFC 9404](https://www.rfc-editor.org/rfc/rfc9404) `urn:ietf:params:jmap:blob`
//! support): umbrella crate re-exporting the component crates, so one
//! dependency covers them.
//!
//! - [`types`]: the [`jmap-blob-types`](https://crates.io/crates/jmap-blob-types),
//!   always enabled.
//! - `client`: [`jmap-blob-client`](https://crates.io/crates/jmap-blob-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-blob-server`](https://crates.io/crates/jmap-blob-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_blob::types::{BlobAccountCapability, JMAP_BLOB2_URI};
//!
//! assert_eq!(JMAP_BLOB2_URI, "urn:ietf:params:jmap:blob2");
//! let cap: BlobAccountCapability = serde_json::from_str(r#"{"maxSizeBlobSet": null,
//!     "maxDataSources": 64, "supportedTypeNames": [], "supportedDigestAlgorithms": []}"#)
//!     .unwrap();
//! assert_eq!(cap.max_data_sources, 64);
//! ```

#![forbid(unsafe_code)]

pub use jmap_blob_types as types;

#[cfg(feature = "client")]
pub use jmap_blob_client as client;

#[cfg(feature = "server")]
pub use jmap_blob_server as server;

/// The RFC 9404 capability URI, `urn:ietf:params:jmap:blob`.
///
/// Kept from the 0.0.1 release under its name there; new code names
/// [`types::JMAP_BLOB_URI`] for RFC 9404 or [`types::JMAP_BLOB2_URI`] for
/// draft-ietf-jmap-blobext-03.
pub const CAPABILITY: &str = types::JMAP_BLOB_URI;
