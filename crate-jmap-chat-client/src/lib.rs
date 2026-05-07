//! jmap-chat-client — auth-agnostic JMAP Chat HTTP client with WebSocket and SSE support.
//!
//! See PLAN.md for the full implementation plan.

#![forbid(unsafe_code)]

pub mod methods;
pub mod session;
pub mod sse;
pub mod types;
pub mod utils;
pub mod ws;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, Patch, PushSubscriptionCreateResponse,
    QueryChangesResponse, QueryResponse, SessionClient, SetError, SetResponse, SpaceJoinResponse,
    TypingResponse,
};
pub use session::{ChatCapability, ChatPushCapability, ChatSessionExt};
pub use sse::{parse_chat_sse_block, ChatSseEvent, ChatSseFrame};
pub use ws::{ChatWsExt, ChatWsFrame};

/// Extension trait adding JMAP Chat methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_chat_client::JmapChatExt;`
///
/// All JMAP Chat method calls are made through the [`SessionClient`] returned
/// by [`with_chat_session`](JmapChatExt::with_chat_session).
pub trait JmapChatExt {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Chat method calls are made through the returned [`SessionClient`].
    fn with_chat_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapChatExt for jmap_base_client::JmapClient {
    fn with_chat_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}
