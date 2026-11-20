//! JMAP Tasks
//! ([draft-ietf-jmap-tasks-06](https://datatracker.ietf.org/doc/draft-ietf-jmap-tasks/)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-tasks-types`](https://crates.io/crates/jmap-tasks-types),
//!   always enabled.
//! - `client`: [`jmap-tasks-client`](https://crates.io/crates/jmap-tasks-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-tasks-server`](https://crates.io/crates/jmap-tasks-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_tasks::types::JMAP_TASKS_URI;
//!
//! assert_eq!(JMAP_TASKS_URI, "urn:ietf:params:jmap:tasks");
//! ```

#![forbid(unsafe_code)]

pub use jmap_tasks_types as types;

#[cfg(feature = "client")]
pub use jmap_tasks_client as client;

#[cfg(feature = "server")]
pub use jmap_tasks_server as server;
