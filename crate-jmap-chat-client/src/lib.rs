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

#![forbid(unsafe_code)]

pub mod methods;
pub mod session;
pub mod sse;
pub mod types;
pub mod utils;
pub mod ws;

pub use jmap_base_client::ClientError;
pub use jmap_chat_types::BodyType;
pub use methods::blob::{BlobConvertResponse, BlobLookupEntry, BlobLookupResponse, BlobObject};
pub use methods::quota::Quota;
pub use methods::{
    AddMemberInput, AddedItem, ChangesResponse, ChatContactPatch, ChatContactQueryInput,
    ChatCreateInput, ChatPatch, ChatQueryInput, ContactSortProperty, CustomEmojiCreateInput,
    CustomEmojiQueryInput, GetResponse, MessageCreateInput, MessagePatch, MessageQueryInput, Patch,
    PresenceStatusPatch, PushSubscriptionCreateInput, PushSubscriptionCreateResponse,
    PushSubscriptionPatch, QueryChangesResponse, QueryResponse, ReactionChange, SessionClient,
    SetError, SetResponse, SpaceAddCategoryInput, SpaceAddChannelInput, SpaceAddMemberInput,
    SpaceAddRoleInput, SpaceBanCreateInput, SpaceCreateInput, SpaceInviteCreateInput,
    SpaceJoinInput, SpaceJoinResponse, SpacePatch, SpaceQueryInput, SpaceUpdateCategoryInput,
    SpaceUpdateChannelInput, SpaceUpdateMemberInput, SpaceUpdateRoleInput, TypingResponse,
    UpdateMemberRoleInput,
};
pub use session::{ChatCapability, ChatPushCapability, ChatSessionExt};
pub use sse::{parse_chat_sse_block, ChatSseEvent, ChatSseFrame};
pub use types::{ChatMemberRole, ContactPresenceFilter, QuotaResourceType, QuotaScope};
pub use utils::{format_receipt_timestamp, format_receipt_timestamp_at};
pub use ws::{ChatWsExt, ChatWsFrame};

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
    /// infallible [`methods::SessionClient`], not a `Result`.
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
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapChatExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
