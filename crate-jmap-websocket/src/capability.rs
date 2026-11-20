//! The RFC 8887 §3 capability object.

use serde::{Deserialize, Serialize};

/// The value of the [`CAPABILITY`](crate::CAPABILITY) key in a Session's
/// `capabilities` (RFC 8887 §3).
///
/// ```
/// use jmap_websocket::WebSocketCapability;
///
/// let cap = WebSocketCapability::new("wss://server.example.com/jmap/ws/", true);
/// assert_eq!(
///     serde_json::to_value(&cap).unwrap(),
///     serde_json::json!({"url": "wss://server.example.com/jmap/ws/", "supportsPush": true}),
/// );
/// ```
///
/// Unknown members are kept in [`extra`](Self::extra) (workspace
/// extras-preservation policy, AGENTS.md).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketCapability {
    /// `url`: the wss-URI of the WebSocket URL endpoint (RFC 6455 §3).
    /// This type does not check the scheme.
    pub url: String,
    /// `supportsPush`: whether the server sends push notifications over
    /// the WebSocket (RFC 8887 §4.3.5). Required on the wire; a missing
    /// value reads as `false`.
    #[serde(default)]
    pub supports_push: bool,
    /// Vendor, site and future-spec members not covered above.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl WebSocketCapability {
    /// A capability for endpoint `url` with `supportsPush` set to
    /// `supports_push` and no extension members.
    pub fn new(url: impl Into<String>, supports_push: bool) -> Self {
        Self {
            url: url.into(),
            supports_push,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// RFC 8887 §3 example, verbatim.
    const RFC_EXAMPLE: &str = r#"{
     "url": "wss://server.example.com/jmap/ws/",
     "supportsPush": true
   }"#;

    #[test]
    fn rfc_8887_section_3_example_round_trips() {
        let cap: WebSocketCapability = serde_json::from_str(RFC_EXAMPLE).unwrap();
        assert_eq!(cap.url, "wss://server.example.com/jmap/ws/");
        assert!(cap.supports_push);
        assert!(cap.extra.is_empty());
        let expected: serde_json::Value = serde_json::from_str(RFC_EXAMPLE).unwrap();
        assert_eq!(serde_json::to_value(&cap).unwrap(), expected);
    }

    /// Workspace extras-preservation policy: an unknown member survives a
    /// round trip.
    #[test]
    fn unknown_member_is_preserved() {
        let wire = json!({"url": "wss://h/ws", "supportsPush": false, "x-vendor": {"a": 1}});
        let cap: WebSocketCapability = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(cap.extra["x-vendor"], json!({"a": 1}));
        assert_eq!(serde_json::to_value(&cap).unwrap(), wire);
    }
}
