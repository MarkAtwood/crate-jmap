//! Canonical push notification types shared by SSE and WebSocket transports.
//! Spec: RFC 8620 §7.1 (Push Subscriptions)
//!
//! [`StateChange`] is defined in `jmap-types` so servers and clients share one
//! type; it is re-exported here and at the crate root.

pub use jmap_types::StateChange;
