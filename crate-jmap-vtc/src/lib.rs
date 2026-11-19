//! JMAP VTC
//! ([draft-atwood-jmap-vtc-00](https://github.com/MarkAtwood/jmap-chat-spec)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-vtc-types`](https://crates.io/crates/jmap-vtc-types),
//!   always enabled.
//! - `client`: [`jmap-vtc-client`](https://crates.io/crates/jmap-vtc-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-vtc-server`](https://crates.io/crates/jmap-vtc-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_vtc::types::{JMAP_VTC_URI, JMAP_VTC_WEBSOCKET_URI};
//!
//! assert_eq!(JMAP_VTC_URI, "urn:ietf:params:jmap:vtc");
//! assert_eq!(JMAP_VTC_WEBSOCKET_URI, "urn:ietf:params:jmap:vtc:websocket");
//! ```

#![forbid(unsafe_code)]

pub use jmap_vtc_types as types;

#[cfg(feature = "client")]
pub use jmap_vtc_client as client;

#[cfg(feature = "server")]
pub use jmap_vtc_server as server;
