//! JMAP Object History
//! ([draft-ietf-jmap-object-history-03](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-object-history-03)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-object-history-types`](https://crates.io/crates/jmap-object-history-types),
//!   always enabled.
//! - `client`: [`jmap-object-history-client`](https://crates.io/crates/jmap-object-history-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-object-history-server`](https://crates.io/crates/jmap-object-history-server),
//!   behind the `server` feature.
//!
//! It also keeps [`CAPABILITY`], the one item the 0.0.1 placeholder
//! published.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_object_history::types::{JMAP_OBJECT_HISTORY_URI, ObjectHistoryAccountCapability};
//!
//! assert_eq!(JMAP_OBJECT_HISTORY_URI, "urn:ietf:params:jmap:object-history");
//! assert_eq!(jmap_object_history::CAPABILITY, JMAP_OBJECT_HISTORY_URI);
//! let cap = ObjectHistoryAccountCapability::new(
//!     Some(2_592_000),
//!     vec!["ContactCard".to_owned(), "Email".to_owned()],
//! );
//! assert!(cap.supports_type("Email"));
//! ```

#![forbid(unsafe_code)]

pub use jmap_object_history_types as types;

#[cfg(feature = "client")]
pub use jmap_object_history_client as client;

#[cfg(feature = "server")]
pub use jmap_object_history_server as server;

/// The Object History capability URI,
/// `urn:ietf:params:jmap:object-history`.
///
/// Kept from the 0.0.1 release under its name there; new code names
/// [`types::JMAP_OBJECT_HISTORY_URI`].
pub const CAPABILITY: &str = types::JMAP_OBJECT_HISTORY_URI;
