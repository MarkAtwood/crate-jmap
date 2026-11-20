//! JMAP Calendars
//! ([draft-ietf-jmap-calendars-34](https://datatracker.ietf.org/doc/draft-ietf-jmap-calendars/)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-calendars-types`](https://crates.io/crates/jmap-calendars-types),
//!   always enabled.
//! - `client`: [`jmap-calendars-client`](https://crates.io/crates/jmap-calendars-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-calendars-server`](https://crates.io/crates/jmap-calendars-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_calendars::types::JMAP_CALENDARS_URI;
//!
//! assert_eq!(JMAP_CALENDARS_URI, "urn:ietf:params:jmap:calendars");
//! ```

#![forbid(unsafe_code)]

pub use jmap_calendars_types as types;

#[cfg(feature = "client")]
pub use jmap_calendars_client as client;

#[cfg(feature = "server")]
pub use jmap_calendars_server as server;
