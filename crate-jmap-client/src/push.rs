// Canonical push notification types shared by SSE and WebSocket transports.
// Spec: RFC 8620 §7.1 (Push Subscriptions)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A state change push notification (RFC 8620 §7.1).
///
/// Sent over both SSE (as a push event) and WebSocket (as a frame type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateChange {
    /// For each account that changed: maps data-type name to the new state string.
    pub changed: HashMap<String, HashMap<String, String>>,
}
