//! JMAP Object Metadata
//! ([draft-ietf-jmap-metadata-05](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-05)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-metadata-types`](https://crates.io/crates/jmap-metadata-types),
//!   always enabled.
//! - `client`: [`jmap-metadata-client`](https://crates.io/crates/jmap-metadata-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-metadata-server`](https://crates.io/crates/jmap-metadata-server),
//!   behind the `server` feature.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_metadata::types::{DataTypeMetadataInfo, JMAP_METADATA_URI};
//!
//! assert_eq!(JMAP_METADATA_URI, "urn:ietf:params:jmap:metadata");
//! let info: DataTypeMetadataInfo = serde_json::from_str(r#"{"namespaces": ["photography"],
//!     "supportsVendorNamespaces": false, "supportsPrivate": false, "maxDepth": 3}"#)
//!     .unwrap();
//! assert_eq!(info.max_depth, Some(3));
//! ```

#![forbid(unsafe_code)]

pub use jmap_metadata_types as types;

#[cfg(feature = "client")]
pub use jmap_metadata_client as client;

#[cfg(feature = "server")]
pub use jmap_metadata_server as server;
