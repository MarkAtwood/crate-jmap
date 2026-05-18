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
//!
//! # `extra` field equality and the `serde_json/preserve_order` feature (bd:JMAP-6r7c.43)
//!
//! Every public deserializable struct in this crate carries an
//! `extra: serde_json::Map<String, serde_json::Value>` field per the
//! workspace extras-preservation policy (see workspace AGENTS.md). Several
//! of these structs also derive `PartialEq` / `Eq` so callers can write
//! `assert_eq!(a, b)` in tests and `if state_a == state_b { ... }` in
//! application code.
//!
//! The derived `PartialEq` impl compares the `extra` field via
//! [`serde_json::Map`]'s `PartialEq` impl, whose semantics depend on a
//! third-party feature flag:
//!
//! - **Default (this workspace's posture).** `serde_json::Map` is
//!   BTreeMap-backed; equality is order-insensitive (keys are stored in
//!   lexicographic order regardless of insertion order).
//! - **`serde_json/preserve_order` enabled anywhere in the dep graph.**
//!   `serde_json::Map` switches to `IndexMap` (insertion-order preserved);
//!   equality becomes order-sensitive.
//!
//! Two values constructed with the same `extra` entries inserted in
//! different orders therefore compare EQUAL under the default
//! configuration and UNEQUAL under `preserve_order`. The feature flag is
//! a global toggle: any crate in the consumer's dep graph that enables
//! `preserve_order` flips the semantics for every crate in the graph.
//!
//! **In-scope structs**: [`BlobUploadResponse`], [`Session`],
//! [`AccountInfo`], [`WebSocketCapability`], and [`StateChange`].
//!
//! Workspace memory note: `jmap-base-client` (and the full workspace)
//! does NOT enable `preserve_order`. Consumers in the same posture get
//! deterministic `==` behaviour on `extra`. Consumers that enable
//! `preserve_order` elsewhere and need stable equality independent of
//! that flag should compare the serialised forms instead:
//!
//! ```rust,ignore
//! let a_json = serde_json::to_value(&a)?;
//! let b_json = serde_json::to_value(&b)?;
//! assert_eq!(a_json, b_json);
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
    AuthHeader, AuthProvider, BasicAuth, BearerAuth, CustomCaTransport, DefaultTransport,
    HttpClient, NoneAuth, TransportConfig,
};
pub use blob::{expand_url_template, BlobUploadResponse, DownloadBlobParams, UploadBlobParams};
pub use client::{extract_response, ClientConfig, JmapClient};
pub use error::{
    ClientError, HttpError, InvalidHeaderValueError, ParseCategory, ParseError, SerializeError,
    WebSocketError,
};
pub use push::StateChange;
pub use request::{
    AccountInfo, AccountName, JmapRequestBuilder, JmapUrl, JmapUrlTemplate, Session, Username,
    WebSocketCapability,
};
pub use sse::{parse_sse_block, SseEvent, SseFrame};
pub use ws::{connect_ws, WsFrame, WsReceiver, WsSender, WsSession};
