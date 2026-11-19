//! JMAP Sharing
//! ([RFC 9670](https://www.rfc-editor.org/rfc/rfc9670)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-sharing-types`](https://crates.io/crates/jmap-sharing-types),
//!   always enabled.
//! - `client`: [`jmap-sharing-client`](https://crates.io/crates/jmap-sharing-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-sharing-server`](https://crates.io/crates/jmap-sharing-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_sharing::types::{JMAP_PRINCIPALS_URI, PrincipalsCapability};
//!
//! assert_eq!(JMAP_PRINCIPALS_URI, "urn:ietf:params:jmap:principals");
//! let cap: PrincipalsCapability =
//!     serde_json::from_str(r#"{"currentUserPrincipalId": "P1"}"#).unwrap();
//! assert!(cap.current_user_principal_id.is_some());
//! ```

#![forbid(unsafe_code)]

pub use jmap_sharing_types as types;

#[cfg(feature = "client")]
pub use jmap_sharing_client as client;

#[cfg(feature = "server")]
pub use jmap_sharing_server as server;
