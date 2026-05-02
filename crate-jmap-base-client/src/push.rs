//! Canonical push notification types shared by SSE and WebSocket transports.
//! Spec: RFC 8620 §7.1 (Push Subscriptions)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use jmap_types::{Id, State};

/// A state change push notification (RFC 8620 §7.1).
///
/// Sent over both SSE (as a push event) and WebSocket (as a frame type).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateChange {
    /// For each account that changed: maps data-type name to the new [`State`] token.
    ///
    /// Outer key: account [`Id`].  Inner key: JMAP data-type name (e.g. `"Email"`).
    /// Inner value: new opaque state string; pass to `Email/changes` etc. as `sinceState`.
    pub changed: HashMap<Id, HashMap<String, State>>,
}
