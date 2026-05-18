//! RFC 8620 base JMAP client: auth, session fetch, blob, SSE, and WebSocket.
//!
//! Extension-specific clients (`jmap-chat-client`, `jmap-mail-client`) depend on this crate.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_base_client::{JmapClient, auth::{DefaultTransport, BearerAuth}, client::ClientConfig};
//! # async fn example() -> Result<(), jmap_base_client::ClientError> {
//! let auth = BearerAuth::new("...")?;
//! let client = JmapClient::new(
//!     DefaultTransport,
//!     auth,
//!     "https://jmap.example.com",
//!     ClientConfig::default(),
//! )?;
//! let session = client.fetch_session().await?;
//! # let _ = session;
//! # Ok(())
//! # }
//! ```

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
pub use blob::{expand_url_template, BlobUploadResponse, DownloadBlobParams, UploadBlobParams};
pub use client::{extract_response, ClientConfig, JmapClient};
pub use error::{
    ClientError, HttpError, InvalidHeaderValueError, ParseCategory, ParseError, SerializeError,
    WebSocketError,
};
pub use push::StateChange;
pub use request::{AccountInfo, JmapRequestBuilder, Session, WebSocketCapability};
pub use sse::{parse_sse_block, SseEvent, SseFrame};
pub use ws::{connect_ws, WsFrame, WsReceiver, WsSender, WsSession};
