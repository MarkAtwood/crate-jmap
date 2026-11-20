//! JMAP Contacts
//! ([RFC 9610](https://www.rfc-editor.org/rfc/rfc9610)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-contacts-types`](https://crates.io/crates/jmap-contacts-types),
//!   always enabled.
//! - `client`: [`jmap-contacts-client`](https://crates.io/crates/jmap-contacts-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-contacts-server`](https://crates.io/crates/jmap-contacts-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_contacts::types::JMAP_CONTACTS_URI;
//!
//! assert_eq!(JMAP_CONTACTS_URI, "urn:ietf:params:jmap:contacts");
//! ```

#![forbid(unsafe_code)]

pub use jmap_contacts_types as types;

#[cfg(feature = "client")]
pub use jmap_contacts_client as client;

#[cfg(feature = "server")]
pub use jmap_contacts_server as server;
