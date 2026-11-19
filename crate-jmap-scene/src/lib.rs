//! JMAP Scene
//! ([draft-atwood-jmap-scene-00](https://github.com/MarkAtwood/jmap-chat-spec)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-scene-types`](https://crates.io/crates/jmap-scene-types),
//!   always enabled.
//! - `client`: [`jmap-scene-client`](https://crates.io/crates/jmap-scene-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-scene-server`](https://crates.io/crates/jmap-scene-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_scene::types::{JMAP_SCENE_URI, JMAP_SCENE_WEBSOCKET_URI};
//!
//! assert_eq!(JMAP_SCENE_URI, "urn:ietf:params:jmap:scene");
//! assert_eq!(JMAP_SCENE_WEBSOCKET_URI, "urn:ietf:params:jmap:scene:websocket");
//! ```

#![forbid(unsafe_code)]

pub use jmap_scene_types as types;

#[cfg(feature = "client")]
pub use jmap_scene_client as client;

#[cfg(feature = "server")]
pub use jmap_scene_server as server;
