// jmap-client — RFC 8620 base JMAP client: auth, session fetch, blob, SSE, WebSocket.
// See PLAN.md for the full implementation plan.
// Extension-specific clients (jmap-chat-client, jmap-mail-client) depend on this crate.

#![forbid(unsafe_code)]

pub mod auth;
pub mod blob;
pub mod client;
pub mod error;
pub mod push;
pub mod request;
pub mod sse;
pub mod ws;

pub use auth::{
    AuthProvider, BasicAuth, BearerAuth, CustomCaTransport, DefaultTransport, NoneAuth,
    TransportConfig,
};
pub use blob::BlobUploadResponse;
pub use client::{extract_response, ClientConfig, JmapClient};
pub use error::ClientError;
pub use push::StateChange;
pub use request::{AccountInfo, JmapRequestBuilder, Session, WebSocketCapability};
pub use sse::{parse_sse_block, SseEvent, SseFrame};
pub use ws::{connect_ws, WsFrame, WsSession};
