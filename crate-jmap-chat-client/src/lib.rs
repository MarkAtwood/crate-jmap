//! jmap-chat-client — auth-agnostic JMAP Chat HTTP client with WebSocket and SSE support.
//!
//! See PLAN.md for the full implementation plan.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_chat_client::JmapChatExt;
//! # use jmap_types::Id;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_chat_session(session);
//! // Look up previously-uploaded blobs by their content-addressed hashes.
//! // Pass `type_names: None` to accept any MIME type.
//! let blob_ids = [Id::from("sha256-...")];
//! let resolved = sc.blob_lookup(&blob_ids, None).await?;
//! # let _ = resolved;
//! # Ok(())
//! # }
//! ```
//!
//! # Re-exported types
//!
//! [`ClientError`] and the shared method response types (`GetResponse`,
//! `SetResponse`, `ChangesResponse`, `QueryResponse` and the rest) are
//! re-exports from `jmap-base-client` and `jmap-types`, not types defined
//! here. The `jmap-*` workspace crates are versioned and released together:
//! depend on the same version of this crate, `jmap-base-client` and
//! `jmap-types`.

#![forbid(unsafe_code)]

mod calendars;
mod methods;
mod session;
mod sse;
mod types;
mod utils;
mod ws;

pub use calendars::{RsvpStatus, own_participant_key, rsvp_patch};
pub use jmap_base_client::ClientError;
pub use jmap_chat_types::BodyType;
pub use methods::batch::{Batch, BatchResponse, CallHandle};
pub use methods::blob::{BlobConvertResponse, BlobLookupEntry, BlobLookupResponse, BlobObject};
pub use methods::quota::Quota;
pub use methods::{
    AddMemberInput, AddedItem, CALL_ID, ChangesResponse, ChatContactPatch, ChatCreateInput,
    ChatPatch, CustomEmojiCreateInput, CustomEmojiPatch, GetResponse, MessageCreateInput,
    MessagePatch, PartialObject, Patch, PresenceStatusPatch, PushSubscriptionCreateInput,
    PushSubscriptionCreateResponse, PushSubscriptionPatch, QueryChangesResponse, QueryResponse,
    ReactionChange, ServerRetryAfterError, SessionClient, SetError, SetResponse,
    SpaceAddCategoryInput, SpaceAddChannelInput, SpaceAddMemberInput, SpaceAddRoleInput,
    SpaceBanCreateInput, SpaceBanPatch, SpaceCreateInput, SpaceInviteCreateInput, SpaceJoinInput,
    SpaceJoinResponse, SpacePatch, SpaceUpdateCategoryInput, SpaceUpdateChannelInput,
    SpaceUpdateMemberInput, SpaceUpdateRoleInput, TypingResponse, UpdateMemberRoleInput,
    server_retry_after,
};
pub use session::{
    ChatCapability, ChatCapabilityRole, ChatFilenodeCapability, ChatPushCapability, ChatSessionExt,
    Endpoint,
};
pub use sse::{ChatSseEvent, ChatSseFrame, parse_chat_sse_block};
pub use types::{ChatMemberRole, QuotaResourceType, QuotaScope};
pub use utils::{
    TYPING_DECAY, TypingIndicators, broadcast_mentions_from_spans, format_receipt_timestamp,
    format_receipt_timestamp_at, mentions_from_spans,
};
pub use ws::{ChatWsExt, ChatWsFrame, parse_chat_ws_frame};

/// Extension trait adding JMAP Chat methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_chat_client::JmapChatExt;`
///
/// All JMAP Chat method calls are made through the [`SessionClient`] returned
/// by [`with_chat_session`](JmapChatExt::with_chat_session).
///
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`]. Sealing prevents downstream
/// divergence (e.g. `impl JmapChatExt for MySimulator`) and keeps
/// adding methods to the trait a non-breaking change.
pub trait JmapChatExt: sealed::Sealed {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Chat method calls are made through the returned [`SessionClient`].
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts ANY [`jmap_base_client::Session`],
    /// including one whose advertised capabilities do not include
    /// `urn:ietf:params:jmap:chat` or whose `primaryAccounts` map has
    /// no entry for the chat capability. The constructor performs no
    /// up-front validation and never fails — its return type is the
    /// infallible [`SessionClient`], not a `Result`.
    ///
    /// Capability and primary-account validation is deferred to every
    /// individual method call on the returned [`SessionClient`]. If
    /// the session is unsuitable, those per-method calls return
    /// [`ClientError::InvalidSession`] with a description like
    /// `"no primary account for urn:ietf:params:jmap:chat"`.
    ///
    /// Callers that want to guard at the binding site can pre-check
    /// the session before calling this method:
    ///
    /// ```ignore
    /// if session
    ///     .primary_account_id("urn:ietf:params:jmap:chat")
    ///     .is_none()
    /// {
    ///     // Session does not advertise a primary account for chat;
    ///     // every subsequent SessionClient method call would fail
    ///     // with ClientError::InvalidSession. Refuse here.
    ///     return Err(MyAppError::SessionMissingChatCapability);
    /// }
    /// let sc = client.with_chat_session(session);
    /// ```
    fn with_chat_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapChatExt for jmap_base_client::JmapClient {
    fn with_chat_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
            account_id: None,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapChatExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
