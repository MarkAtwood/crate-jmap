//! JMAP Chat
//! ([draft-atwood-jmap-chat-00](https://github.com/MarkAtwood/jmap-chat-spec)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-chat-types`](https://crates.io/crates/jmap-chat-types),
//!   always enabled.
//! - `client`: [`jmap-chat-client`](https://crates.io/crates/jmap-chat-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-chat-server`](https://crates.io/crates/jmap-chat-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_chat::types::JMAP_CHAT_URI;
//!
//! assert_eq!(JMAP_CHAT_URI, "urn:ietf:params:jmap:chat");
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub use jmap_chat_types as types;

#[cfg(feature = "client")]
pub use jmap_chat_client as client;

#[cfg(feature = "server")]
pub use jmap_chat_server as server;
