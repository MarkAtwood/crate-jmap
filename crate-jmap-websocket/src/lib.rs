//! JMAP over WebSocket, [RFC 8887](https://www.rfc-editor.org/rfc/rfc8887)
//! (`urn:ietf:params:jmap:websocket`).
//!
//! This is a support crate (workspace AGENTS.md §Support crates): no
//! `*-types` or `*-server` crate depends on it, and a deployment opts in.
//!
//! Always available, with no async runtime:
//!
//! - [`CAPABILITY`], [`SUBPROTOCOL`] and [`offers_jmap`] for the
//!   Session object (§3) and the opening handshake (§4.2).
//! - [`WebSocketCapability`], the §3 capability object.
//! - The §4.3 messages: [`WsRequest`], [`WebSocketPushEnable`] and
//!   [`WebSocketPushDisable`] from the client ([`ClientMessage`]);
//!   [`WsResponse`], [`WsRequestError`] and [`WsStateChange`] from the
//!   server ([`ServerMessage`]).
//!
//! Feature `server`: [`server::Connection`], a session driver that turns
//! incoming text frames into [`jmap_server::Dispatcher`] calls and writes
//! the answers and push notifications back, over any `Stream` and `Sink`
//! of messages, so it fits axum, tokio-tungstenite or another stack. Also
//! [`server::PushHub`], an in-memory state-change source with `pushState`
//! replay.
//!
//! Feature `tungstenite`: `tungstenite::serve` runs a connection over a
//! tokio-tungstenite `WebSocketStream`.
//!
//! The authenticated HTTP request that opens the WebSocket, and the TLS
//! under it (RFC 8887 §4.1, §5.1), are the deployment's.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod capability;
mod frame;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "tungstenite")]
pub mod tungstenite;

pub use capability::WebSocketCapability;
pub use frame::{
    ClientMessage, ServerMessage, WebSocketPushDisable, WebSocketPushEnable, WsRequest,
    WsRequestError, WsResponse, WsStateChange,
};

pub use jmap_types;

#[cfg(feature = "server")]
pub use futures;
#[cfg(feature = "server")]
pub use jmap_server;

#[cfg(feature = "tungstenite")]
pub use tokio_tungstenite;

/// The JMAP capability URI of this extension (RFC 8887 §3, §6): the key
/// of [`WebSocketCapability`] in the Session's `capabilities`.
pub const CAPABILITY: &str = "urn:ietf:params:jmap:websocket";

/// The WebSocket subprotocol name (RFC 8887 §4.2, §6.1). The client MUST
/// list it in `Sec-WebSocket-Protocol`, and the server's reply MUST carry
/// it for a JMAP connection to be established.
pub const SUBPROTOCOL: &str = "jmap";

/// Whether a `Sec-WebSocket-Protocol` request header value lists the
/// [`SUBPROTOCOL`] (RFC 8887 §4.2).
///
/// The header is a comma-separated list of tokens (RFC 6455 §11.3.4);
/// tokens are compared exactly, since subprotocol names are
/// case-sensitive. A server that gets `false` must not complete the
/// handshake as a JMAP connection.
///
/// ```
/// assert!(jmap_websocket::offers_jmap("chat, jmap"));
/// assert!(!jmap_websocket::offers_jmap("JMAP"));
/// ```
pub fn offers_jmap(header_value: &str) -> bool {
    header_value.split(',').any(|t| t.trim() == SUBPROTOCOL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8887 §6.1 registers the subprotocol identifier `jmap`; §3 names
    /// the capability `urn:ietf:params:jmap:websocket`. 0.0.1 shipped
    /// `CAPABILITY` with the same value.
    #[test]
    fn constants_match_rfc_8887() {
        assert_eq!(CAPABILITY, "urn:ietf:params:jmap:websocket");
        assert_eq!(SUBPROTOCOL, "jmap");
    }

    /// RFC 8887 §4.4 handshake: `Sec-WebSocket-Protocol: jmap`. RFC 6455
    /// §11.3.4 makes the header a comma list of tokens.
    #[test]
    fn offers_jmap_reads_the_token_list() {
        assert!(offers_jmap("jmap"));
        assert!(offers_jmap("soap,jmap"));
        assert!(offers_jmap(" soap , jmap "));
        assert!(!offers_jmap("soap"));
        assert!(!offers_jmap("jmapx"));
        assert!(!offers_jmap(""));
    }
}
