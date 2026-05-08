//! RFC 8620 base JMAP client: auth, session fetch, blob, SSE, and WebSocket.
//!
//! Extension-specific clients (`jmap-chat-client`, `jmap-mail-client`) depend on this crate.

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
pub use blob::{expand_url_template, BlobUploadResponse, DownloadBlobParams};
pub use client::{extract_response, ClientConfig, JmapClient};
pub use error::{ClientError, HttpError, InvalidHeaderValueError, WebSocketError};
pub use push::StateChange;
pub use request::{AccountInfo, JmapRequestBuilder, Session, WebSocketCapability};
pub use sse::{parse_sse_block, SseEvent, SseFrame};
pub use ws::{connect_ws, WsFrame, WsSession};
