//! ChatBackend trait and supporting types for JMAP Chat method handlers.
//!
//! Consumers implement [`ChatBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations are here.
//!
//! Marker traits and property selector enums live in `jmap-types` and
//! `jmap-chat-types` respectively; they are re-exported here for convenience.

pub use jmap_chat_types::backend::{
    ChatContactProperty, ChatProperty, MessageProperty, ReadPositionProperty, SpaceProperty,
};
pub use jmap_chat_types::space_set::{SpaceMetadataPatch, SpacePatchOp};
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, SetError, SetErrorType, SetObject,
};

// ---------------------------------------------------------------------------
// Space/set structural-mutation result
// ---------------------------------------------------------------------------

/// The outcome of a single [`SpacePatchOp`] applied by
/// [`ChatBackend::apply_space_patch`].
///
/// `op_index` is the zero-based index of the op within the input `Vec`, used
/// by handlers to construct a descriptive error message identifying which
/// per-key entry failed (e.g. `addRoles[2] failed: ...`).
///
/// `outcome` is:
/// - `Ok(Some(id))` — the op produced a new server-assigned id (e.g.
///   [`SpacePatchOp::AddRole`], [`SpacePatchOp::AddChannel`],
///   [`SpacePatchOp::AddCategory`]). The handler reports this id back to
///   the client via the `/set` response.
/// - `Ok(None)` — the op completed but produced no id (every `Remove*` and
///   `Update*` variant).
/// - `Err(SetError)` — the op was rejected (e.g. permission denied, target
///   id not found, role hierarchy violation, count limit exceeded).
///
/// Per RFC 8620 §5.3 `/set`, an update target is per-target atomic on the
/// wire: it appears in exactly one of `updated` or `notUpdated`. If **any**
/// `OpResult` in the returned `Vec` has an `Err`, the handler reports the
/// containing update target in `notUpdated`. The handler is free to choose
/// which `Err` to surface; the reference handler surfaces the first.
///
/// This type lives in `jmap-chat-server` (not `jmap-chat-types`) because
/// [`SetError`] is defined in `jmap-server` and `jmap-chat-types` cannot
/// depend on it (per the workspace dependency rule: types crates depend
/// only on `jmap-types`, `serde`, `serde_json`).
///
/// `#[non_exhaustive]` preserves the ability to add fields without a
/// SemVer break (e.g. an audit-log handle, commit ordering hint,
/// permission-diagnostic annotation). External consumers MUST construct
/// instances via [`OpResult::ok`] or [`OpResult::err`] rather than
/// field-init syntax.
///
/// # Field-addition policy
///
/// Future fields added to `OpResult` are an **additive non-breaking
/// change**: the new field MUST have a `Default` value (typical for
/// `Option` / `Vec` / `HashMap`) and the new field is exposed via a
/// `with_*`-style builder setter rather than being appended as a third
/// positional argument to [`OpResult::ok`] or [`OpResult::err`]. The
/// `ok(op_index, id)` and `err(op_index, error)` constructors therefore
/// stay stable across the crate's pre-1.0 lifetime, matching the
/// [`ChatLimits`]-style canonical workspace pattern for
/// `#[non_exhaustive]` types with builder setters.
#[non_exhaustive]
#[derive(Debug)]
pub struct OpResult {
    /// Zero-based index of the originating op in the input `Vec<SpacePatchOp>`.
    pub op_index: usize,
    /// The outcome of applying that op.
    pub outcome: Result<Option<jmap_types::Id>, SetError>,
}

impl OpResult {
    /// Construct an `Ok` result for a successfully applied op.
    ///
    /// The contained `Id` is the server-assigned id when the op was a
    /// create (the kit's response includes it under `created[create_id].id`);
    /// pass `None` for updates and destroys, which have no id to surface.
    #[must_use]
    pub const fn ok(op_index: usize, id: Option<jmap_types::Id>) -> Self {
        Self {
            op_index,
            outcome: Ok(id),
        }
    }

    /// Construct an `Err` result for a rejected op.
    ///
    /// The kit's `handle_space_set` surfaces this error to the JMAP
    /// `notUpdated` map for the containing update target.
    #[must_use]
    pub const fn err(op_index: usize, error: SetError) -> Self {
        Self {
            op_index,
            outcome: Err(error),
        }
    }

    /// Builder-style setter for [`Self::op_index`].
    ///
    /// Lets callers update the originating op index after construction.
    /// Useful when re-indexing a result produced for one op into the
    /// position of another (e.g. when filtering a batch).
    #[must_use]
    pub const fn with_op_index(mut self, op_index: usize) -> Self {
        self.op_index = op_index;
        self
    }

    /// Builder-style setter for [`Self::outcome`].
    ///
    /// Lets callers replace the outcome after construction. Useful for
    /// retry / fallback flows that re-classify a transient error into a
    /// success or vice versa before surfacing the result to the
    /// handler.
    #[must_use]
    pub fn with_outcome(mut self, outcome: Result<Option<jmap_types::Id>, SetError>) -> Self {
        self.outcome = outcome;
        self
    }
}

// ---------------------------------------------------------------------------
// Per-Space content limits
// ---------------------------------------------------------------------------

/// Implementation-defined per-Space content limits enforced by
/// `Space/set` handlers.
///
/// Per draft-atwood-jmap-chat-00 §Space/set (spec commit `80d5e11`,
/// 2026-05-11), each `add*` op on a Space MUST return an `overQuota`
/// SetError (RFC 8620 §5.3) when the resulting count would exceed a
/// server-defined limit. The spec does not name or normatively define
/// the cap values; they are implementation-defined and may vary per
/// account or per tenant.
///
/// Backends override [`ChatBackend::limits`] to supply their own values.
/// The default impl returns conservative reference-impl-grade values
/// suitable for tests and single-tenant dev servers; production
/// deployments are expected to override.
///
/// Client visibility of these caps (when desired) is via JMAP Quotas
/// (`urn:ietf:params:jmap:quotas`, RFC 9425), not via this struct.
/// The `urn:ietf:params:jmap:chat` session capability does NOT advertise
/// the count caps (the previous cap-advertising fields `maxRolesPerSpace`,
/// `maxSpaceMembers`, `maxChannelsPerSpace`, `maxCategoriesPerSpace`
/// were removed from the draft in spec commit `80d5e11`).
///
/// See the workspace `AGENTS.md` "Backend caps and limits" section for
/// the cross-extension pattern this struct establishes for JMAP Chat.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatLimits {
    /// Maximum number of roles per Space. Enforced against the
    /// resulting count after applying every `addRoles` entry in a
    /// `Space/set` update patch.
    pub max_roles_per_space: u32,
    /// Maximum number of members per Space. Enforced against the
    /// resulting count after applying every `addMembers` entry in a
    /// `Space/set` update patch.
    pub max_space_members: u32,
    /// Maximum number of channels per Space. Enforced against the
    /// resulting count after applying every `addChannels` entry in a
    /// `Space/set` update patch. The current channel count is the sum
    /// of `space.uncategorizedChannelIds.len()` and every
    /// `space.categories[].channelIds.len()`.
    pub max_channels_per_space: u32,
    /// Maximum number of categories per Space. Enforced against the
    /// resulting count after applying every `addCategories` entry in
    /// a `Space/set` update patch.
    pub max_categories_per_space: u32,
    /// Maximum number of members per group Chat. Enforced against the
    /// resulting count after applying every `addMembers` entry in a
    /// `Chat/set` update patch.
    pub max_group_chat_members: u32,
    /// `bodyType` values the account advertises as `supportedBodyTypes`
    /// and `Message/set` accepts (draft-atwood-jmap-chat-00
    /// §Account-Level Capability Object). Defaults to
    /// [`crate::message::SUPPORTED_BODY_TYPES`]. The list "MUST include
    /// `"text/plain"` unless the server accepts only end-to-end encrypted
    /// body types"; `Message/set` rejects a `bodyType` not listed here,
    /// including the implied `"text/plain"` of an omitted `bodyType`.
    pub supported_body_types: &'static [&'static str],
    /// Listed types that a companion specification reserves for
    /// server-created Messages (e.g. `"application/mimi-fanout"`,
    /// draft-atwood-jmap-chat-mimi-00). `Message/set` create rejects
    /// them with an `invalidProperties` SetError naming `bodyType`;
    /// update does not. Defaults to empty.
    pub server_reserved_body_types: &'static [&'static str],
    /// `bodyType` values that indicate an end-to-end encrypted body.
    /// Defaults to [`crate::message::ENCRYPTED_BODY_TYPES`]. For these
    /// types `Message/set` create and update reject a non-empty
    /// `mentions`, `broadcastMentions`, `attachments`, `actions`,
    /// `replyTo`, `threadRootId`, `topicId` or `subject`
    /// (draft-atwood-jmap-chat-00 §Fields Inside the Ciphertext). A MIMI
    /// deployment adds `"application/mimi-fanout"`, which "is an
    /// encrypted body type for the purposes of" that draft
    /// (draft-atwood-jmap-chat-mimi-00 §MLS Messages).
    pub encrypted_body_types: &'static [&'static str],
    /// The account's `supportsThreads` capability value
    /// (draft-atwood-jmap-chat-00 §Thread Model). When `true`,
    /// `Message/get` computes `replyCount` and `unreadReplyCount`, and
    /// `Message/query` accepts the `threadRootId` filter, which the
    /// backend's `query_objects` must then implement. Defaults to `false`.
    pub supports_threads: bool,
    /// The account's `maxBodyBytes` capability value
    /// (draft-atwood-jmap-chat-00 §Account-Level Capability Object):
    /// "Maximum UTF-8 byte length of a Message `body`." `Message/set`
    /// create rejects a longer `body` with a `tooLarge` SetError.
    /// Defaults to 100,000.
    pub max_body_bytes: u64,
    /// The account's `maxAttachmentBytes` capability value: "Maximum size
    /// in bytes of a single attachment blob." `Message/set` create
    /// rejects an Attachment whose `size` exceeds it with a `tooLarge`
    /// SetError, before consulting [`ChatBackend::attachment_blob`].
    /// Defaults to 50,000,000.
    pub max_attachment_bytes: u64,
    /// The account's `maxAttachmentsPerMessage` capability value:
    /// "Maximum number of attachments per message." `Message/set` create
    /// rejects more with a `tooLarge` SetError. Defaults to 16.
    pub max_attachments_per_message: u64,
}

impl Default for ChatLimits {
    fn default() -> Self {
        Self::new(50, 10_000, 500, 100)
    }
}

impl ChatLimits {
    /// Construct a [`ChatLimits`] with the four Space cap fields
    /// specified in declaration order: roles, members, channels,
    /// categories. [`Self::max_group_chat_members`] defaults to 1000;
    /// override it with [`Self::with_max_group_chat_members`].
    ///
    /// The struct is `#[non_exhaustive]` so external callers (tests
    /// using `MemoryBackend::set_limits_for_test`, production backends
    /// overriding [`ChatBackend::limits`]) need a constructor to build
    /// one. Future cap-field additions to [`ChatLimits`] are an
    /// additive non-breaking change because the constructor stays
    /// stable; callers wanting to override the new field combine
    /// `ChatLimits::new(..)` with [`Self::with_max_roles_per_space`]
    /// / [`Self::with_max_space_members`] /
    /// [`Self::with_max_channels_per_space`] /
    /// [`Self::with_max_categories_per_space`] and a future analogous
    /// setter.
    pub const fn new(
        max_roles_per_space: u32,
        max_space_members: u32,
        max_channels_per_space: u32,
        max_categories_per_space: u32,
    ) -> Self {
        Self {
            max_roles_per_space,
            max_space_members,
            max_channels_per_space,
            max_categories_per_space,
            max_group_chat_members: 1_000,
            supported_body_types: crate::message::SUPPORTED_BODY_TYPES,
            server_reserved_body_types: &[],
            encrypted_body_types: crate::message::ENCRYPTED_BODY_TYPES,
            supports_threads: false,
            max_body_bytes: 100_000,
            max_attachment_bytes: 50_000_000,
            max_attachments_per_message: 16,
        }
    }

    /// Builder-style setter for [`Self::max_roles_per_space`].
    #[must_use]
    pub const fn with_max_roles_per_space(mut self, max: u32) -> Self {
        self.max_roles_per_space = max;
        self
    }

    /// Builder-style setter for [`Self::max_space_members`].
    #[must_use]
    pub const fn with_max_space_members(mut self, max: u32) -> Self {
        self.max_space_members = max;
        self
    }

    /// Builder-style setter for [`Self::max_channels_per_space`].
    #[must_use]
    pub const fn with_max_channels_per_space(mut self, max: u32) -> Self {
        self.max_channels_per_space = max;
        self
    }

    /// Builder-style setter for [`Self::max_categories_per_space`].
    #[must_use]
    pub const fn with_max_categories_per_space(mut self, max: u32) -> Self {
        self.max_categories_per_space = max;
        self
    }

    /// Builder-style setter for [`Self::max_group_chat_members`].
    #[must_use]
    pub const fn with_max_group_chat_members(mut self, max: u32) -> Self {
        self.max_group_chat_members = max;
        self
    }

    /// Builder-style setter for [`Self::supported_body_types`].
    #[must_use]
    pub const fn with_supported_body_types(mut self, types: &'static [&'static str]) -> Self {
        self.supported_body_types = types;
        self
    }

    /// Builder-style setter for [`Self::server_reserved_body_types`].
    #[must_use]
    pub const fn with_server_reserved_body_types(mut self, types: &'static [&'static str]) -> Self {
        self.server_reserved_body_types = types;
        self
    }

    /// Builder-style setter for [`Self::encrypted_body_types`].
    #[must_use]
    pub const fn with_encrypted_body_types(mut self, types: &'static [&'static str]) -> Self {
        self.encrypted_body_types = types;
        self
    }

    /// Builder-style setter for [`Self::supports_threads`].
    #[must_use]
    pub const fn with_supports_threads(mut self, supports: bool) -> Self {
        self.supports_threads = supports;
        self
    }

    /// Builder-style setter for [`Self::max_body_bytes`].
    #[must_use]
    pub const fn with_max_body_bytes(mut self, max: u64) -> Self {
        self.max_body_bytes = max;
        self
    }

    /// Builder-style setter for [`Self::max_attachment_bytes`].
    #[must_use]
    pub const fn with_max_attachment_bytes(mut self, max: u64) -> Self {
        self.max_attachment_bytes = max;
        self
    }

    /// Builder-style setter for [`Self::max_attachments_per_message`].
    #[must_use]
    pub const fn with_max_attachments_per_message(mut self, max: u64) -> Self {
        self.max_attachments_per_message = max;
        self
    }
}

// ---------------------------------------------------------------------------
// Attachment blob verification
// ---------------------------------------------------------------------------

/// The stored blob behind an Attachment `blobId`, as reported by
/// [`ChatBackend::attachment_blob`].
///
/// `handle_message_set` compares these against the Attachment's `size`
/// and `sha256` (draft-atwood-jmap-chat-00 §Attachment, `size`: "Servers
/// MUST verify against actual content"; `sha256`: "Servers SHOULD
/// verify").
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentBlob {
    /// Byte count of the stored blob.
    pub size: u64,
    /// SHA-256 digest of the stored blob's bytes.
    pub sha256: [u8; 32],
}

impl AttachmentBlob {
    /// Construct an [`AttachmentBlob`] from the stored blob's byte count
    /// and SHA-256 digest.
    #[must_use]
    pub const fn new(size: u64, sha256: [u8; 32]) -> Self {
        Self { size, sha256 }
    }
}

// ---------------------------------------------------------------------------
// Custom emoji authorization gate
// ---------------------------------------------------------------------------

/// Identifies the kind of `CustomEmoji/set` operation being authorized by
/// [`ChatBackend::may_set_custom_emoji`].
///
/// Per draft-atwood-jmap-chat-00 commit `9344aec` (2026-05-11, "refactor:
/// implementation-defined emoji authorization"), authorization for
/// `CustomEmoji/set` is fully implementation-defined for both server-global
/// and Space-scoped emoji. The kit forwards the op kind so the backend can
/// apply different policies per op (e.g. allow create+update but require
/// elevated rights for destroy).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmojiSetOp {
    /// `CustomEmoji/set` `create` — a new emoji is being added.
    Create,
    /// `CustomEmoji/set` `update` — an existing emoji's mutable fields
    /// (currently `name` and `blobId`) are being patched.
    Update,
    /// `CustomEmoji/set` `destroy` — an existing emoji is being removed.
    Destroy,
}

/// Why [`ChatBackend::join_space_with_invite`] refused a redemption.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteJoinRefusal {
    /// The invite, or the Space it names, no longer exists.
    NotFound,
    /// The invite's `uses` has reached `maxUses`.
    Exhausted,
    /// The owner is already a member of the Space.
    AlreadyMember,
}

// ---------------------------------------------------------------------------
// Slow-mode rate-limit gate
// ---------------------------------------------------------------------------

/// Outcome of a [`ChatBackend::slow_mode_check`] rejection.
///
/// Per draft-atwood-jmap-chat-00 §Chat `slowModeSeconds` plus spec commit
/// `de60acb` (2026-05-11) which softened the manage-channels exemption to
/// SHOULD: a non-exempt member who sends faster than the configured rate
/// MUST be rejected with a `rateLimit` SetError (RFC 8620 §5.3) carrying a
/// `serverRetryAfter` UTCDate that tells the client when it may retry.
///
/// `retry_after` is a fully-formed [`jmap_types::UTCDate`] — the backend
/// has already done the arithmetic (typically "now + remaining slow-mode
/// window"). The handler serialises it verbatim onto the wire as the
/// `serverRetryAfter` SetError extra field.
///
/// `#[non_exhaustive]` preserves the ability to add fields without a
/// SemVer break (e.g. an error code, sender-facing retry hint string,
/// "this is your N-th throttle in M minutes" diagnostic counter).
/// External consumers MUST construct instances via [`SlowModeError::new`]
/// rather than field-init syntax.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SlowModeError {
    /// When the rate-limited sender may retry.
    pub retry_after: jmap_types::UTCDate,
}

impl SlowModeError {
    /// Construct a [`SlowModeError`] from a precomputed retry-after
    /// [`UTCDate`].
    ///
    /// [`UTCDate`]: jmap_types::UTCDate
    #[must_use]
    pub const fn new(retry_after: jmap_types::UTCDate) -> Self {
        Self { retry_after }
    }
}

impl std::fmt::Display for SlowModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "slow-mode rate limit; retry after {}",
            self.retry_after.as_ref()
        )
    }
}

impl std::error::Error for SlowModeError {}

// ---------------------------------------------------------------------------
// ChatBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP Chat method handlers.
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are defined on the [`JmapBackend`]
/// supertrait. Only write operations and type introspection are here.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl ChatBackend>` when sharing across tasks.
///
/// # Async vs sync method convention
///
/// `ChatBackend` carves its methods into two groups:
///
/// 1. **Mutation / hot-path hooks are async.** [`ChatBackend::create_object`],
///    [`ChatBackend::update_object`], [`ChatBackend::destroy_object`],
///    [`ChatBackend::apply_space_patch`],
///    [`ChatBackend::apply_space_metadata_patch`],
///    [`ChatBackend::expire_message`],
///    [`ChatBackend::read_and_burn_message`],
///    [`ChatBackend::slow_mode_check`],
///    [`ChatBackend::is_contact_blocked`], and
///    [`ChatBackend::may_set_custom_emoji`] all return
///    `impl Future + Send`. These methods consult per-account state,
///    may issue I/O, and run on the JMAP request hot path; the async
///    signature lets production backends `.await` storage / cache /
///    policy lookups without runtime-pinning hazards.
///
/// 2. **Policy / capability hooks are sync.** [`ChatBackend::supports_type`],
///    [`ChatBackend::generate_invite_code`], [`ChatBackend::limits`],
///    [`ChatBackend::protect_last_admin`], and
///    [`ChatBackend::retains_edit_history`] are synchronous methods.
///    These hooks answer deployment-policy or capability questions
///    whose answer SHOULD be derivable from in-process state (a
///    startup-loaded config snapshot, a cached per-account record, a
///    CSPRNG handle). The sync shape keeps the trait surface easy to
///    implement for single-process and reference backends, which are
///    the dominant use case for a kit crate.
///
/// The carve-up is intentional and is NOT a "we forgot to async these"
/// regression. Production backends that need to consult an async
/// source for any of the five sync hooks have two options: (a)
/// pre-cache the answer at startup or on a background refresh; or (b)
/// file a follow-up bead proposing the conversion for that specific
/// method, with the production constraint documented. Per-method
/// rustdoc notes ("`# Why sync, not async`" / "`# Fallibility and
/// async — known limitations`") spell out the workaround on each
/// affected method. Flipping any of the sync hooks to async is a
/// major-version breaking change for downstream implementors;
/// converting them all preemptively for hypothetical multi-tenant
/// SaaS deployments would over-engineer the kit-vs-product posture
/// described in workspace `AGENTS.md`.
pub trait ChatBackend: JmapBackend {
    /// Create a new object.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    ///
    /// # Id assignment
    ///
    /// The backend mints the new object's id, calls `build(&id)` exactly
    /// once to obtain the complete object, and stores that object under
    /// `id` in the same atomic write. `build` sets the object's `id` field
    /// to the id it is given; the backend stores the built object as is.
    /// RFC 8620 §5.3 lists `id` among the "server-set properties".
    ///
    /// `Message/set` builds an owner-composed Message with `senderMsgId`
    /// equal to that id (draft-atwood-jmap-chat-00 §Message `senderMsgId`:
    /// "Equals `id` for messages composed by the owner."). A
    /// server-originated Message ([`crate::store_server_message`]) carries
    /// the `senderMsgId` its originator chose. In both cases the backend
    /// stores `senderMsgId` unchanged.
    ///
    /// # Per-type uniqueness contracts
    ///
    /// For most types the (id) primary key is the only uniqueness
    /// invariant the backend must enforce, and the backend's normal
    /// server-assigned-id discipline closes that. A few types carry
    /// additional uniqueness invariants on non-id fields that the
    /// backend MUST enforce atomically with the create — the handler
    /// pre-checks defensively but cannot close the
    /// two-concurrent-requests race window.
    ///
    /// - **`ReadPosition`** — at most one record per
    ///   `(account_id, chat_id)`. Clients cannot create one
    ///   (`ReadPosition/set` create is `forbidden`); the server creates
    ///   it on first message delivery in
    ///   [`crate::message::handle_message_set`] and on
    ///   [`crate::space::handle_space_join`], which pre-check via a
    ///   sequential scan, but two concurrent requests for
    ///   the same `chatId` can both pass the pre-check and then both
    ///   reach `create_object`. The
    ///   backend MUST enforce a unique constraint on
    ///   `(account_id, chat_id)` at the storage layer (typically a
    ///   composite unique index in a database backend) and surface a
    ///   duplicate as `BackendSetError::SetError(SetError::new(
    ///   SetErrorType::AlreadyExists).with_existing_id(canonical))`.
    ///   See draft-atwood-jmap-chat-00 §ReadPosition.
    /// - **`ChatContact`** — at most one record per
    ///   `(account_id, identity)`, compared octet for octet.
    ///   [`crate::contact::handle_contact_set`] does not pre-check: the
    ///   backend MUST reject a duplicate atomically with the create as
    ///   `BackendSetError::SetError(SetError::new(
    ///   SetErrorType::AlreadyExists).with_existing_id(existing))`, where
    ///   `existing` is the id of the ChatContact that holds the
    ///   `identity`. See draft-atwood-jmap-chat-00 §ChatContact/set.
    /// - **`CustomEmoji`** — at most one emoji per `(account_id,
    ///   spaceId, name)`, an absent `spaceId` being the server-global
    ///   scope. [`crate::emoji::handle_emoji_set`] does not pre-check:
    ///   the backend MUST reject a duplicate create as
    ///   `BackendSetError::SetError(SetError::new(
    ///   SetErrorType::AlreadyExists).with_existing_id(existing))` and,
    ///   in `update_object`, a rename onto a held name as an
    ///   `invalidProperties` SetError naming `name`. See
    ///   draft-atwood-jmap-chat-00 §CustomEmoji `name`.
    /// - **`Chat`** with `kind == Direct` — at most one direct chat
    ///   per `(account_id, contact_id)`. The handler at
    ///   [`crate::chat::handle_chat_set`] uses optimistic
    ///   create-then-validate with rollback; backends MAY enforce a
    ///   storage-level unique constraint to eliminate the rollback
    ///   path entirely, but it is not required because the
    ///   handler's rollback closes the race correctly. See
    ///   draft-atwood-jmap-chat-00 §Chat.
    ///
    /// The reference `MemoryBackend` does NOT
    /// enforce the `ReadPosition` `(account, chatId)` uniqueness
    /// constraint at the storage layer — it is single-Mutex
    /// per-call so concurrent racing cannot occur in practice, but a
    /// production backend that holds different locks per record (or
    /// no lock at all between read and write) MUST.
    ///
    /// # Errors
    ///
    /// Reject a create that RFC 8620 §5.3 defines an error for with
    /// [`BackendSetError::SetError`] carrying the matching
    /// [`SetErrorType`]; the handler places it verbatim in `notCreated`.
    /// For create these are `forbidden`, `overQuota`, `tooLarge`,
    /// `rateLimit` and `invalidProperties`. An `invalidProperties` error
    /// SHOULD name in `properties` "*all* the properties that were
    /// invalid", for example `chatId` when a Message names a Chat that does
    /// not exist.
    ///
    /// Return [`BackendSetError::Other`] only for unexpected storage
    /// failures. The handler reports it as `serverFail` without the
    /// error's `Display` text, which tells the client to retry; a
    /// deterministic rejection returned as `Other` invites retries that
    /// cannot succeed.
    ///
    /// # State and change log
    ///
    /// RFC 8620 §5.1 defines the state string over "*all* the data of
    /// this type in the account": "If the data changes, this string MUST
    /// change." Every successful `create_object`,
    /// [`update_object`](Self::update_object) and
    /// [`destroy_object`](Self::destroy_object) MUST advance
    /// [`get_state`](JmapBackend::get_state) for `O` in `account_id` and
    /// record the id as created, updated or destroyed so
    /// [`get_changes`](JmapBackend::get_changes) reports it.
    ///
    /// When [`ChatLimits::supports_threads`] is `true`, `Message/get`
    /// returns the server-computed `replyCount` and `unreadReplyCount`
    /// (draft-atwood-jmap-chat-00 §Message), so a write that changes them
    /// changes those Messages. The backend MUST then also record as
    /// updated in `Message` state:
    ///
    /// - on a Message create or destroy (including
    ///   [`expire_message`](Self::expire_message) and
    ///   [`read_and_burn_message`](Self::read_and_burn_message)), the
    ///   still-stored Message its `replyTo` names, whose `replyCount`
    ///   changed;
    /// - on a ReadPosition update that changes `lastReadMessageId`, each
    ///   thread root (no `threadRootId`) in that Chat with a direct reply
    ///   whose id moved across `lastReadMessageId`, whose
    ///   `unreadReplyCount` changed.
    ///
    /// The write, the state advance and the change record MUST be atomic
    /// with respect to other callers: the reference `MemoryBackend` does
    /// all three under one mutex acquisition, and a database backend uses
    /// one transaction. A call that returns `Err` MUST leave the stored
    /// data, the state and the change log unchanged.
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        build: F,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&jmap_types::Id) -> O + Send;

    /// Apply a partial update (patch) to an existing object.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    ///
    /// # Patch format
    ///
    /// `patch` is an RFC 8620 §5.3 PatchObject, not an RFC 7396 JSON
    /// Merge Patch: "The keys are a path in JSON Pointer format
    /// \[RFC6901\], with an implicit leading "/"". So `{"a/b": null}`
    /// removes member `b` of property `a`; it does not touch a top-level
    /// property literally named `a/b`. A `null` value sets the
    /// property's default if it has one, otherwise removes it. A pointer
    /// into an array, a pointer whose parent does not exist, or two keys
    /// where one pointer is a prefix of the other "MUST be rejected with
    /// an "invalidPatch" error".
    ///
    /// Implementors SHOULD apply `patch` with
    /// [`jmap_server::apply_patch_object`], which enforces these rules
    /// all-or-nothing, and map any [`jmap_server::PatchPathError`] to
    /// [`jmap_server::SetErrorType::InvalidPatch`], as the reference
    /// `MemoryBackend` does.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object), plus these RFC 8620
    /// §5.3 rules for update:
    ///
    /// - "If an id given cannot be found, the update or destroy MUST be
    ///   rejected with a "notFound" set error."
    /// - A patch that breaks the rules under "Patch format" is
    ///   `invalidPatch`.
    /// - A patched record that is invalid is `invalidProperties`. That
    ///   includes changing a server-set property, though "it is not an
    ///   error to include a server-set property in an update as long as
    ///   the value is identical to the current value on the server."
    ///
    /// # Atomicity
    ///
    /// RFC 8620 §5.3: the server "MUST NOT only commit part of an update
    /// to a single record". Apply the whole patch and validate the result
    /// before writing it, so that on any error the stored record is
    /// unchanged. State and change log follow
    /// [`create_object`](Self::create_object) ("State and change log"); an
    /// `Ok(None)` return still advances the state.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object), plus these RFC 8620
    /// §5.3 rules for destroy: an unknown id, including one already
    /// destroyed, is `notFound`, and a destroy that "would violate an ACL
    /// or other permissions policy" is `forbidden`.
    ///
    /// `destroy_object::<Space>` MUST return `forbidden` unless the caller
    /// holds `manage_space` in that Space, checked atomically with the
    /// destroy (draft-atwood-jmap-chat-00 §Destroying a Space: "The caller
    /// MUST hold `"manage_space"`"). The handler does no check of its own.
    ///
    /// # Cascades
    ///
    /// `destroy_object::<Message>` MUST also remove the id from its
    /// Chat's `pinnedMessageIds` (draft-atwood-jmap-chat-00 §Chat:
    /// "When a pinned Message is hard-deleted, the server MUST remove
    /// its id from this list."), as
    /// [`expire_message`](Self::expire_message) does. That change is
    /// a Chat update.
    ///
    /// `destroy_object::<Space>` MUST also destroy every channel Chat
    /// whose `spaceId` is the Space and every Message in those Chats
    /// (draft-atwood-jmap-chat-00 §Destroying a Space: "Cascades to all
    /// channel Chats and their Messages. Hard-deletes all records; no
    /// tombstones are retained."). Those are Chat and Message destroys.
    ///
    /// Every Chat destroy, including those cascades, MUST also destroy
    /// the Chat's ReadPosition (draft-atwood-jmap-chat-00 §ReadPosition:
    /// the server "destroys it when the Chat is destroyed"). That is a
    /// ReadPosition destroy.
    ///
    /// If the backend itself changes another record because of the
    /// destroy, it MUST advance that record type's state and record the
    /// change in the same atomic step, so `/changes` on that type sees it.
    /// State and change log for `O` follow
    /// [`create_object`](Self::create_object) ("State and change log").
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Called after a Chat is destroyed, so a companion extension can
    /// update the records that reference it. Examples:
    /// draft-atwood-jmap-chat-tasks-00 §Lifecycle ("the affected Tasks'
    /// `chatId` fields MUST be cleared") and draft-atwood-jmap-vtc-00
    /// §VTCCall ("the server MUST clear the corresponding field to
    /// `null` on all VTCCalls that reference it").
    ///
    /// # When the handler calls this
    ///
    /// Once for each Chat that a request destroys, after the destroy
    /// succeeds:
    ///
    /// - a `Chat/set` destroy;
    /// - each channel in a successful `Space/set` `removeChannels` patch;
    /// - each channel Chat whose `spaceId` is a Space that `Space/set`
    ///   destroys (draft-atwood-jmap-chat-00 §Destroying a Space:
    ///   "Cascades to all channel Chats"). These calls come before
    ///   [`Self::on_space_destroyed`] for that Space.
    ///
    /// It is not called when the handler deletes a duplicate direct Chat
    /// it created in the same request, because no response ever named
    /// that id.
    ///
    /// The destroy is already committed, so the hook cannot fail the
    /// request. A backend handles its own errors here. A change it makes
    /// to another record type MUST advance that type's state, as in
    /// [`Self::destroy_object`] ("Cascades").
    ///
    /// # Default implementation
    ///
    /// Does nothing. A backend that implements an extension which
    /// references Chats overrides it.
    fn on_chat_destroyed(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = ()> + Send {
        let _ = (caller, account_id, chat_id);
        async {}
    }

    /// Called after a `Space/set` destroy succeeds, so a companion
    /// extension can update the records that reference the Space.
    /// Examples: draft-atwood-jmap-vtc-00 §VTCCall (clear `spaceId` on
    /// VTCCalls) and draft-atwood-jmap-chat-filenode-00 §Space Deletion
    /// ("the server SHOULD also destroy the associated file tree").
    ///
    /// The handler calls it once per destroyed Space, after
    /// [`Self::on_chat_destroyed`] for each of its channel Chats. The
    /// error and state rules of [`Self::on_chat_destroyed`] apply.
    ///
    /// # Default implementation
    ///
    /// Does nothing. A backend that implements an extension which
    /// references Spaces overrides it.
    fn on_space_destroyed(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = ()> + Send {
        let _ = (caller, account_id, space_id);
        async {}
    }

    /// Called after a `SpaceBan/set` create succeeds, with the stored
    /// ban, so a companion extension can apply the ban to its own
    /// records. Example: draft-atwood-jmap-vtc-00 §Access Control "Space
    /// ban enforcement" (the server "MUST set `leftAt` on that
    /// VTCParticipant record"; "The ban takes effect immediately on
    /// active calls").
    ///
    /// The handler calls it once per created ban. The error and state
    /// rules of [`Self::on_chat_destroyed`] apply.
    ///
    /// # Default implementation
    ///
    /// Does nothing. A backend that implements an extension which must
    /// enforce Space bans overrides it.
    fn on_space_ban_created(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ban: &jmap_chat_types::SpaceBan,
    ) -> impl std::future::Future<Output = ()> + Send {
        let _ = (caller, account_id, ban);
        async {}
    }

    /// Called when the owner's action must be sent to other participants'
    /// servers: a new Message, a retraction (`deletedForAll`), an edit, a
    /// reaction added or removed, a group Chat change, a read receipt, a
    /// typing signal or a presence change. A backend that implements
    /// draft-atwood-jmap-chat-federation-00 turns the
    /// [`OutboundEvent`](crate::OutboundEvent) into `Peer/deliver`
    /// (`message`, `edit` or `reactionUpdate`), `Peer/retract`,
    /// `Peer/groupUpdate`, `Peer/receipt`, `Peer/typing` or `Peer/presence`
    /// calls, typically by enqueueing the durable ones (§Persistent Queue).
    /// [`OutboundEvent::Typing`](crate::OutboundEvent::Typing) also drives
    /// the local `typing` push to the Chat's other local participants.
    ///
    /// # When the handler calls this
    ///
    /// - `Message/set` create:
    ///   [`OutboundEvent::NewMessage`](crate::OutboundEvent::NewMessage), after the Message is written
    ///   (draft-atwood-jmap-chat-00 §Message/set: "enqueues the message for
    ///   outbound delivery").
    /// - `Message/set` update setting `deletedForAll: true`:
    ///   [`OutboundEvent::Retract`](crate::OutboundEvent::Retract), after validation and *before* the
    ///   update is written (§Peer/retract "Sender Behavior": "before
    ///   marking the local record"). If the write then fails, the event
    ///   has still been reported.
    /// - `Message/set` update editing `body`, `bodyType`, `mentions` or
    ///   `broadcastMentions` and not deleting the Message:
    ///   [`OutboundEvent::Edit`](crate::OutboundEvent::Edit), after the update is written.
    /// - `Chat/set` create of a group Chat, or update that changes a group
    ///   Chat's members, roles, `name`, `description` or `avatarBlobId`:
    ///   one [`OutboundEvent::GroupUpdate`](crate::OutboundEvent::GroupUpdate) per action, after the write
    ///   (draft-atwood-jmap-chat-00 §Chat/set).
    ///
    /// - `Message/set` update patching `reactions/<senderReactionId>` and
    ///   not deleting the Message:
    ///   one [`OutboundEvent::ReactionUpdate`](crate::OutboundEvent::ReactionUpdate) per key that adds a
    ///   reaction or removes a stored one, after the update is written
    ///   (draft-atwood-jmap-chat-00 §Adding and Removing Reactions).
    /// - `Message/set` update setting a non-null `readAt` on a Message whose
    ///   `senderId` is not the accountId:
    ///   [`OutboundEvent::Receipt`](crate::OutboundEvent::Receipt), after
    ///   the update (or the `burnOnRead` hard-delete) is written, unless
    ///   the effective `receiptSharing` (`Chat.receiptSharing`, else
    ///   `PresenceStatus.receiptSharing`) is `false`
    ///   (draft-atwood-jmap-chat-federation-00 §Peer/receipt "Sender
    ///   Behavior").
    /// - `Chat/typing`: [`OutboundEvent::Typing`](crate::OutboundEvent::Typing),
    ///   after the request is validated (draft-atwood-jmap-chat-00
    ///   §Chat/typing).
    /// - `PresenceStatus/set` update naming `presence`, `statusText` or
    ///   `statusEmoji`: [`OutboundEvent::Presence`](crate::OutboundEvent::Presence),
    ///   after the update is written
    ///   (draft-atwood-jmap-chat-federation-00 §Peer/subscribePresence).
    ///
    /// The hook cannot fail the request. A backend handles its own errors
    /// here.
    ///
    /// # Default implementation
    ///
    /// Does nothing, for a server that does not federate.
    fn on_outbound_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        event: &crate::OutboundEvent,
    ) -> impl std::future::Future<Output = ()> + Send {
        let _ = (caller, account_id, event);
        async {}
    }

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    ///
    /// # Types in scope
    ///
    /// Implementors should answer for the object types this trait
    /// covers (draft-atwood-jmap-chat-00): `Chat`, `Message`, `Space`,
    /// `SpaceInvite`, `SpaceBan`, `ChatContact`, `CustomEmoji`,
    /// `ReadPosition`, and `PresenceStatus`. Backends MAY answer for
    /// object types from sibling extensions if they also implement those
    /// traits. A backend that does not recognise `O` SHOULD return
    /// `false`.
    ///
    /// # Callers
    ///
    /// The handler library in this crate does not call this method. The
    /// server consumer calls it when building the JMAP session capability
    /// response.
    ///
    /// # Per-account semantics
    ///
    /// Capabilities are per account (RFC 8620 §1.6.2, §2
    /// `accountCapabilities`), so the answer may vary with `caller` and
    /// `account_id`. Backends that support every type in this trait for
    /// every account can ignore both arguments and return `true`.
    ///
    /// # Why sync, not async
    ///
    /// Type-support is a deployment capability question answered from
    /// in-process state. A backend whose supported types come from
    /// configuration or a per-account record SHOULD load it ahead of the
    /// request and answer from the in-memory snapshot. See the
    /// trait-level "Async vs sync method convention" section above for
    /// the workspace rationale.
    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool;

    /// Generate a cryptographically random invite code.
    ///
    /// Implementations MUST use a CSPRNG seeded from OS entropy. The
    /// recommended choices are [`rand::rngs::OsRng`] or the [`getrandom`]
    /// crate directly. Do NOT use `rand::thread_rng()` for security-relevant
    /// output: although current `rand` versions document `ThreadRng` as
    /// cryptographically secure, its underlying algorithm is
    /// implementation-defined and has changed across releases, and the
    /// `rand` book explicitly routes security-sensitive callers to `OsRng`.
    ///
    /// The returned string must be unguessable — do NOT use timestamps,
    /// sequential counters, or non-CSPRNG sources.
    ///
    /// # Output shape contract
    ///
    /// Implementations MUST satisfy ALL of the following for the kit's
    /// constant-time-compare guarantee (see below) to hold:
    ///
    /// 1. **Fixed length across calls.** Every invocation of
    ///    `generate_invite_code` on a single backend MUST return a
    ///    string of the same byte length. A backend that returns
    ///    variable-length codes leaks a length oracle through the
    ///    constant-time-compare timing channel: `ct_eq` returns
    ///    `Choice(0)` cheaply on length mismatch, so an attacker can
    ///    learn whether their candidate code matches the stored
    ///    length even when the content compare is constant-time.
    /// 2. **Minimum 128 bits of CSPRNG entropy.** The reference
    ///    `MemoryBackend` impl in this crate (in `memory.rs`) emits
    ///    32 lowercase-hex characters (16 bytes = 128 bits) which is
    ///    the workspace floor. Production backends MAY use more; less
    ///    is forbidden. The trait method itself is abstract — no
    ///    default impl — so backends MUST provide one.
    /// 3. **ASCII-safe encoding.** The returned bytes MUST be 7-bit
    ///    ASCII printable, byte-length-equal-to-char-length, and
    ///    safe to use as-is in JMAP wire format and HTTP-style
    ///    URL paths (no `/`, `+`, `=`, or whitespace). Recommended
    ///    encodings: lowercase hex (the default), base32 unpadded,
    ///    base64url unpadded. Plain base64 with `/` is forbidden
    ///    because downstream URL-encoded redemption flows misroute
    ///    the slash. Non-ASCII output is forbidden because
    ///    helpers that take byte-prefix slices (e.g. the
    ///    `iso8601_before` partial-comparison family) panic on
    ///    multi-byte UTF-8 boundaries.
    /// 4. **Sufficient lifespan.** Codes MUST remain
    ///    discriminable from each other across the backend's
    ///    retention window. With 128 bits of entropy and
    ///    well-mixed CSPRNG output, collision probability is
    ///    cryptographically negligible up to 2^64 active codes.
    ///
    /// A backend that cannot meet any of points 1–3 MUST NOT
    /// expose `Space/join`'s invite-code redemption path, or MUST
    /// build its own redemption with a different constant-time
    /// argument. The kit's `handle_space_join` assumes the contract
    /// without re-validating each code's shape.
    ///
    /// # Constant-time comparison contract
    ///
    /// Consumers of the returned code (notably `Space/join` invite-code
    /// lookup) MUST compare it against attacker-supplied values in
    /// constant time using `subtle::ConstantTimeEq::ct_eq` or equivalent.
    /// The reference handler in `space::handle_space_join` already does
    /// this; backends that build their own invite-redemption paths must
    /// preserve the invariant. A plain `String == String` short-circuits
    /// at the first mismatched byte and exposes a byte-by-byte timing
    /// oracle for credential recovery.
    ///
    /// # Fallibility and async — known limitations
    ///
    /// This method is sync and infallible. Backends that mint codes
    /// via a remote source (HSM, KMS, hardened sandbox where
    /// `getrandom` is gated, etc.) cannot surface failure from this
    /// signature and cannot `.await` a network call. Such backends
    /// must pre-fetch a buffer of codes at startup and serve from it,
    /// or panic on entropy starvation. A future revision MAY change
    /// the signature to `async fn generate_invite_code(&self) ->
    /// Result<String, Self::Error>` to support remote / fallible
    /// entropy sources without forcing the pre-fetch workaround.
    ///
    /// [`rand::rngs::OsRng`]: https://docs.rs/rand/latest/rand/rngs/struct.OsRng.html
    /// [`getrandom`]: https://docs.rs/getrandom
    fn generate_invite_code(&self) -> String;

    /// Redeem the SpaceInvite `invite_id` for `Space/join`: add the owner
    /// (member entry `account_id`, no roles, `joinedAt` = `joined_at`) to
    /// the invite's Space and increment the invite's `uses`.
    ///
    /// Per draft-atwood-jmap-chat-00 §Space/join: "The `uses` increment
    /// and membership insertion MUST be performed within a single atomic
    /// operation so that concurrent redemptions cannot exceed `maxUses`."
    ///
    /// # Atomicity contract
    ///
    /// The `uses < maxUses` check, the already-member check, the member
    /// append and the `uses` increment MUST all take effect or none, and
    /// MUST be serialized against concurrent redemptions of the invite
    /// and concurrent membership writes to the Space (a transaction or
    /// compare-and-swap). Composing this from `update_object` calls loses
    /// that guarantee: a failed second write leaves a member admitted
    /// without a use counted, and concurrent joins both pass `maxUses`.
    ///
    /// # Account scope
    ///
    /// `account_id` is the request's `accountId`, which names the
    /// account hosting the Space; the handler resolves the invite code
    /// only within that account, and `invite_id` and its Space live
    /// there. Implementations need no server-wide invite-code index.
    ///
    /// # When the handler calls this
    ///
    /// Once, after the invite resolved by code and is unexpired, and
    /// after the ban and `closedAt` checks. The handler maps each
    /// [`InviteJoinRefusal`] to the draft's `invalidArguments` method
    /// error. There is no default: a non-atomic default would reinstate
    /// the race.
    fn join_space_with_invite(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        invite_id: &jmap_types::Id,
        joined_at: &jmap_types::UTCDate,
    ) -> impl std::future::Future<Output = Result<Result<(), InviteJoinRefusal>, Self::Error>> + Send;

    /// Apply a `readAt`-setting `patch` to the `burnOnRead` Message
    /// `message_id` and hard-delete it, as one atomic operation.
    ///
    /// Per draft-atwood-jmap-chat-00 §Message `burnOnRead`: "the
    /// receiving server MUST permanently hard-delete this message (row
    /// removal, not tombstone) immediately after setting `readAt`."
    ///
    /// # Atomicity contract
    ///
    /// Validate `patch` against the stored Message exactly as
    /// [`Self::update_object`] would, then remove the row, record the id
    /// in the `destroyed` set of the `Message` change log (bumping the
    /// `Message` state once), and unpin it from its Chat (§Chat
    /// `pinnedMessageIds`). All of it takes effect or none: on any error
    /// nothing is committed, so a retried `readAt` patch burns again.
    /// Composing this from `update_object` and [`Self::expire_message`]
    /// loses that guarantee: a failed delete leaves `readAt` stored, and
    /// a retry finds nothing left to set.
    ///
    /// # When the handler calls this
    ///
    /// `Message/set` update calls this instead of `update_object` when
    /// the patch sets a non-null `readAt` and the stored Message has
    /// `burnOnRead: true`. Return `SetError` `notFound` if the Message
    /// does not exist and the `update_object` `SetError` for an invalid
    /// patch. There is no default: a non-atomic default would reinstate
    /// the gap.
    fn read_and_burn_message(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        message_id: &jmap_types::Id,
        patch: jmap_types::PatchObject,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Implementation-defined per-Space content limits for this caller
    /// and account.
    ///
    /// Called by `handle_space_set` once per request, before
    /// dispatching to [`Self::apply_space_patch`], to enforce
    /// per-aggregate caps on roles, members, channels, and categories
    /// per Space (draft-atwood-jmap-chat-00 §Space/set, spec commit
    /// `80d5e11`), and by `handle_message_set` once per request to
    /// validate `bodyType` against [`ChatLimits::supported_body_types`]
    /// and [`ChatLimits::server_reserved_body_types`].
    ///
    /// The default implementation returns [`ChatLimits::default`],
    /// which carries conservative reference-impl-grade values. The
    /// `caller` and `account_id` arguments are plumbed even though the
    /// default impl ignores them, so production backends can vary caps
    /// per-account (Free vs. Pro tier, multi-tenant SaaS, etc.)
    /// without a future API break. Implementations SHOULD return
    /// quickly (in-process struct construction, possibly off a cached
    /// per-account record) — this method is called on the hot
    /// `Space/set` path.
    ///
    /// # Why sync, not async
    ///
    /// `Space/set` calls this once per request before dispatching the
    /// mutation; an async `.await` here would add a roundtrip to the
    /// hot path of every Space mutation. Production backends with
    /// per-tenant caps SHOULD cache the resolved [`ChatLimits`] for
    /// each account on first use and serve from the cache thereafter.
    /// See the trait-level "Async vs sync method convention" section
    /// above for the workspace rationale.
    ///
    /// Workspace cross-extension pattern: see `AGENTS.md` "Backend
    /// caps and limits".
    fn limits(&self, _caller: &Self::CallerCtx, _account_id: &jmap_types::Id) -> ChatLimits {
        ChatLimits::default()
    }

    /// Apply a sequence of structural mutations to a Space
    /// (draft-atwood-jmap-chat-00 §Space/set).
    ///
    /// `Space/set` `update` operations use semantic mutation keys
    /// (`addRoles`, `removeRoles`, `addMembers`, …) rather than RFC 8620
    /// JSON Pointer patches. The handler in `space::handle_space_set`
    /// parses the wire object, unfolds each array entry into a
    /// [`SpacePatchOp`] value, then calls this method with the resulting
    /// ordered `Vec`.
    ///
    /// # Ordering and atomicity
    ///
    /// Implementations SHOULD apply ops in input order and SHOULD provide
    /// best-effort transactional semantics so that a partial failure does
    /// not leave the Space in a half-updated state. The reference
    /// in-memory implementation locks the entire backend for the duration
    /// of the call. A database-backed implementation should wrap the
    /// sequence in a single transaction.
    ///
    /// # Permission and limit checks
    ///
    /// Permission gates (`manage_space`, `manage_roles`,
    /// `manage_members`, `manage_channels`) are backend-canonical per
    /// workspace AGENTS.md "Caller identity (foundation seam)": the
    /// reference handler does NOT apply gates before calling this
    /// method, and the backend is responsible for rejecting any op the
    /// caller is not authorized to perform.
    ///
    /// Per-aggregate count limits on roles, members, channels, and
    /// categories per Space are **backend-canonical** as of
    /// bd:JMAP-x2gd.44: the backend MUST query its own
    /// [`ChatBackend::limits`], project the post-patch counts for the
    /// target Space, and reject the whole patch with an `overQuota`
    /// SetError (RFC 8620 §5.3) if any aggregate would exceed its
    /// cap. The cap enforcement MUST be atomic with the mutation
    /// itself — typically inside the same database transaction or
    /// the same mutex critical section. Per
    /// draft-atwood-jmap-chat-00 §Space/set (spec commit `80d5e11`,
    /// 2026-05-11), this behavior is normative.
    ///
    /// The handler at [`crate::space::handle_space_set`] additionally
    /// runs a defense-in-depth pre-flight cap check before this
    /// method, but the pre-flight is non-load-bearing: a backend
    /// caller that bypasses the JMAP wire layer (admin tool,
    /// federation receiver, internal batch importer) and calls
    /// `apply_space_patch` directly MUST still see caps enforced.
    /// The pre-flight exists only to surface `overQuota` to the
    /// client as a single SetError without consuming a backend
    /// round-trip; backends MAY rely on the handler having pre-checked
    /// for performance, but MUST NOT rely on it for correctness.
    ///
    /// The role-position hierarchy check (members may only add or modify
    /// roles whose `position` is strictly less than their own
    /// highest-position role — draft §Space/set L1260, L1266) MUST
    /// be enforced by the backend because it is atomic with the
    /// mutation and depends on the current Space state.
    ///
    /// # Cross-type cascade contract
    ///
    /// Several `SpacePatchOp` variants have side effects on other JMAP
    /// types in the chat extension (`Chat`, `Message`). The wire-format
    /// `Space/set` response describes only the Space-side change, but
    /// `/changes` subscribers on the cascaded types depend on the
    /// backend bumping the relevant type state tokens. A backend that
    /// performs the cross-type mutations but forgets the state bumps
    /// silently desynchronises every multi-client subscriber — the
    /// failure is invisible to single-tenant smoke tests.
    ///
    /// The required cascades are:
    ///
    /// * `RemoveChannel` — MUST destroy the channel-kind `Chat` record
    ///   and every `Message` whose `chatId` matches the removed
    ///   channel (draft §Space/set L1283: "Cascades to all
    ///   Messages in those channels."), and the channel's
    ///   `ReadPosition` (draft §ReadPosition: the server "destroys it
    ///   when the Chat is destroyed"). The `Chat`, `Message` and
    ///   `ReadPosition` type state tokens MUST bump so their `/changes`
    ///   subscribers see the destruction.
    ///
    /// * `RemoveCategory` — MUST clear `Chat.categoryId` on every
    ///   channel-kind `Chat` whose `categoryId` named the removed
    ///   category (the channels fall back to uncategorized). The
    ///   `Chat` type state token MUST bump and the affected channel
    ///   ids MUST appear in the next `Chat/changes` entry's `updated`
    ///   list.
    ///
    /// * `AddCategory` / `UpdateCategory` with a non-empty `channelIds`
    ///   array — MUST set `Chat.categoryId` on each named channel
    ///   (relocating it from its previous category or from
    ///   uncategorized). The `Chat` type state token MUST bump and
    ///   the relocated channel ids MUST appear in `Chat/changes`
    ///   `updated`. This pins a regression class where channel
    ///   relocations silently bypass the `Chat/changes` log; a
    ///   backend that mutates `Chat.categoryId` without bumping the
    ///   `Chat` state token desynchronises every multi-client
    ///   subscriber.
    ///
    /// * `RemoveRole` — MUST strip the removed role id from
    ///   `roleIds` on every `SpaceMember` of this Space (draft
    ///   §Space/set L1263). Members embed directly in `Space`,
    ///   so the cascade is captured by the existing `Space` state
    ///   bump; no separate `Member/changes` rotation is required.
    ///
    /// * `RemoveMember` — MUST remove the member entry and keep
    ///   `Space.memberCount` consistent with `members.len()`. The
    ///   spec does not mandate cascade to the member's `ReadPosition`
    ///   records; the reference impl leaves them in place and treats
    ///   ReadPosition cleanup as implementation-defined. A backend
    ///   that destroys orphaned ReadPositions MUST bump the
    ///   `ReadPosition` type state token; a backend that retains
    ///   them MUST NOT.
    ///
    /// State-token bumps SHOULD be batched per call: one bump per
    /// affected type per `apply_space_patch` invocation, not one bump
    /// per op. The reference impl combines every channel created,
    /// updated, and destroyed by a patch into a single `Chat/changes`
    /// entry; doing the same in production keeps `/changes`
    /// rotation rates proportional to client-visible mutations rather
    /// than to internal op count.
    ///
    /// # Server-set field invariants
    ///
    /// Server-set fields on the target Space (`memberCount`,
    /// `updatedAt`, and any future analogous cached aggregates) MUST
    /// be kept consistent with the post-mutation state. Specifically,
    /// after any combination of `AddMember` and `RemoveMember` ops,
    /// `Space.memberCount` MUST equal `Space.members.len()` as
    /// observed by the next `Space/get`. Backends MAY store these as
    /// cached values that they recompute on every mutation, or MAY
    /// compute them lazily at read time; either is acceptable as
    /// long as `Space/get` returns values consistent with the
    /// returned `members` array. The natural-minimal "persist
    /// memberCount once and trust it" implementation is **not**
    /// correct: subsequent mutations to `members` would let
    /// `memberCount` drift silently.
    ///
    /// `Space.closedAt` is server-set the same way: when the caller
    /// removes themselves as the last member holding `manage_space`
    /// and other members remain, the backend MUST set `closedAt` in
    /// the same mutation (draft §Space/set `removeMembers`). The
    /// handler rejects every other update to a closed Space.
    ///
    /// # Per-op error semantics
    ///
    /// Backend rejections that originate from a single op carry both
    /// a typed [`SetErrorType`] and a human-readable `description`
    /// string. The kit's integration tests pin specific
    /// (`SetErrorType`, `description`-substring) pairs for the
    /// common failure modes; a backend that returns a different
    /// variant or an empty description will fail those tests when
    /// plugged in.
    ///
    /// The following table records the contract for the
    /// failure modes the kit exercises today:
    ///
    /// | Op + failure mode | `SetErrorType` | `description` MUST contain |
    /// |---|---|---|
    /// | Any op, caller lacks the spec-mandated permission | `Forbidden` | the permission identifier as a substring (`"manage_space"`, `"manage_channels"`, `"manage_members"`, `"manage_roles"`, or a deployment-specific permission name) |
    /// | `AddRole` / `UpdateRole` naming a role (or new `position`) at or above caller's highest role `position`; `AddMember` assigning, or `UpdateMember` adding or removing, such a role (draft §Space/set: "servers SHOULD enforce this"; a backend that enforces it uses this row) | `Forbidden` | `"hierarchy"` or `"position"` |
    /// | Any op or combination of ops that takes the Space from at least one member holding `manage_members` or `manage_space` to none (when [`Self::protect_last_admin`] returns `true`), other than self-removal | `InvalidProperties` (`properties`: the patch key(s) concerned, among `removeRoles`, `updateRoles`, `removeMembers`, `updateMembers`) | `"last-admin"` or `"manage_members"` |
    /// | `AddMember` with `userId` already a member | `InvalidProperties` (`properties: ["userId"]`) | implementation-defined; the reference impl names the duplicate userId |
    /// | `AddMember` with a `roleId` that does not name a role on this Space | `InvalidProperties` (`properties: ["roleIds"]`) | implementation-defined; the reference impl names the unknown roleId |
    /// | `RemoveRole` / `RemoveChannel` / `RemoveCategory` with an id that does not exist on this Space | `NotFound` | implementation-defined; the reference impl names the missing id |
    ///
    /// The "MUST contain ... as a substring" rule on permission-denied
    /// failures lets downstream UI / CLI consumers route
    /// permission-request flows by parsing the description, and lets
    /// users see "Permission denied: missing manage_space" rather
    /// than a bare "Permission denied". Deployments that need a
    /// programmatic alternative SHOULD additionally surface the
    /// missing permission via [`SetError::with_extra`] under a
    /// deployment-specific key — substring matching is the floor,
    /// not the ceiling.
    ///
    /// Note on routing: per-op rejections normally surface inside the
    /// returned [`OpResult`] vector, NOT as a top-level
    /// [`BackendSetError`] return. The handler at
    /// [`crate::space::handle_space_set`] applies the kit's per-
    /// update-target atomicity: any failing op in the patch fails the
    /// whole `update` target, so the test fixtures see the first-failing
    /// op's `(SetErrorType, description)` surface on
    /// `notUpdated[<spaceId>]`. The table above describes the
    /// per-op outcome content; the handler does not synthesise new
    /// variants or rewrite descriptions.
    ///
    /// # Return value
    ///
    /// On success, returns a `Vec<OpResult>` of the same length as `ops`,
    /// in input order. Each entry reports the outcome of one op (id
    /// assignment for `Add*` variants, error for rejections). The
    /// handler maps per-op errors back into the `/set` response shape
    /// per [`OpResult`]'s documentation.
    ///
    /// Returns [`BackendSetError::Other`] only for backend-level failures
    /// (the storage layer is unreachable, the account does not exist,
    /// `space_id` is unknown, etc.) — i.e. failures that prevent any op
    /// from being attempted. Per-op rejections (permission denied,
    /// invalid id, role hierarchy violation, etc.) go in the `outcome`
    /// field of the returned [`OpResult`] vector, not in an error return.
    fn apply_space_patch(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
        ops: Vec<SpacePatchOp>,
    ) -> impl std::future::Future<Output = Result<Vec<OpResult>, BackendSetError<Self::Error>>> + Send;

    /// Apply a JSON Merge Patch to a Space's top-level metadata fields
    /// (draft-atwood-jmap-chat-00 §Space/set, `name`, `description`,
    /// `iconBlobId`, `isPublic`, `isPubliclyPreviewable`).
    ///
    /// `Space/set` `update` operations carrying *only* top-level
    /// metadata fields (and not the semantic-mutation keys
    /// `addRoles` / `addMembers` / `addChannels` / `addCategories`
    /// etc.) route through this method instead of the generic
    /// [`Self::update_object`]. Mixed patches carrying both top-level
    /// metadata AND semantic-mutation keys call
    /// [`Self::apply_space_patch`] first (for the structural ops) and
    /// this method second (for the metadata). The handler at
    /// `space::handle_space_set` is responsible for the split and for
    /// stripping non-metadata keys before calling this method.
    ///
    /// # Permission and atomicity
    ///
    /// Per draft-atwood-jmap-chat-00, every top-level metadata field
    /// carries the marker "Mutable by members with `manage_space`
    /// permission". Implementations MUST gate the mutation on the
    /// caller's effective permissions in the target Space: verify the
    /// caller holds `manage_space` (resolved through the Space's role
    /// hierarchy) and reject the patch with
    /// [`SetErrorType::Forbidden`] if not. The caller is the owner, whose
    /// `members` entry is the `account_id` (draft §Terminology), so look
    /// the caller up by `account_id`, not by the principal, whether or
    /// not [`JmapBackend::principal_id`] returns `Some`; the same holds
    /// for every caller-membership or permission check in this trait.
    ///
    /// The check fires inside this method atomically with the
    /// mutation: snapshot the Space's role/member state, evaluate
    /// the permission gate, and apply the merge patch in one
    /// critical section. Backend canonical per workspace AGENTS.md.
    ///
    /// # `patch` shape
    ///
    /// The handler decodes the wire patch's metadata-key subset
    /// (`name`, `description`, `iconBlobId`, `isPublic`,
    /// `isPubliclyPreviewable`) into a [`SpaceMetadataPatch`],
    /// stripping structural mutation keys (`addRoles` etc.) before
    /// building it. Each field is `Option<_>`; nullable target fields
    /// use `Option<Clearable<T>>` so the backend can distinguish
    /// "wire null" (clear) from "absent" (unchanged). Backends MAY
    /// validate the values further (e.g. reject an `icon_blob_id`
    /// referencing a non-existent blob) and surface those as
    /// [`SetError`] returns. The expected `SetErrorType` for
    /// value-level validation failures is
    /// [`SetErrorType::InvalidProperties`] with the offending field
    /// name in `properties` — see the "Diagnostic content
    /// convention" section below for the blob-not-found shape
    /// specifically. (RFC 8620 §5.3 defines `blobNotFound` only as
    /// a method-level error; at the SetError level it surfaces as
    /// `InvalidProperties` naming the offending blob-id field.)
    ///
    /// # Return value
    ///
    /// The discriminator between `Ok(Some(space))` and `Ok(None)` is
    /// the **wire-visible Space shape**:
    ///
    /// - Return `Ok(None)` if the wire-visible `Space` (the shape
    ///   `Space/get` would serialise after applying this patch) is
    ///   identical to the result of mechanically applying the patch
    ///   keys to the prior state with no additional server-side
    ///   modification. The handler can then build the `/set` `updated`
    ///   entry from the input patch alone with no extra fetch.
    /// - Return `Ok(Some(space))` if the backend modified any
    ///   wire-visible property beyond what the client requested
    ///   (RFC 8620 §5.3 server-set field echo, e.g. a normalised
    ///   `name`, a derived `iconBlobId`, a server-bumped
    ///   `memberCount`, or any spec-defined server-set field that
    ///   `Space/get` would return). The handler echoes the returned
    ///   `Space` to the client.
    ///
    /// Server-internal bookkeeping that is NOT part of the
    /// wire-visible `Space` shape (e.g. private last-touched
    /// timestamps used only for `/changes` cache-keying, internal
    /// sequence numbers) MUST NOT itself drive the `Some` path —
    /// the discriminator is "would `Space/get` return a different
    /// shape?", not "did the backend touch anything internally?".
    /// A backend that bumps an internal timestamp on every metadata
    /// patch correctly returns `Ok(None)` when the wire shape is
    /// otherwise unchanged.
    ///
    /// `Err(BackendSetError::SetError(e))` with `e.kind ==
    /// SetErrorType::Forbidden` when the caller fails the
    /// `manage_space` gate.
    ///
    /// # Diagnostic content convention
    ///
    /// The `SetError.description` on a `manage_space`-gate rejection
    /// MUST contain `"manage_space"` as a substring. The kit's
    /// integration tests assert this (e.g.
    /// `tests/space_metadata_apply.rs`); a backend that returns a
    /// bare `SetError::new(SetErrorType::Forbidden)` with no
    /// `description` will fail those tests. Build the error via
    /// `SetError::new(SetErrorType::Forbidden).with_description(
    /// "manage_space permission required")` (or any other string
    /// that contains the literal `"manage_space"`).
    ///
    /// Future blob-validation rejections (e.g. `iconBlobId`
    /// referencing a non-existent blob, see the `patch` shape
    /// section above) SHOULD surface as
    /// `SetErrorType::InvalidProperties` with
    /// `properties: ["iconBlobId"]` and a description that names
    /// the offending blob id. The reference impl does not validate
    /// blobs today; production backends with a blob store SHOULD.
    ///
    /// Rationale for the separate metadata patch method: routing
    /// top-level metadata through the generic `update_object::<Space>`
    /// would bypass the permission gate, since the generic path is
    /// not permission-aware. This method exists so that top-level
    /// `Space/set` `update` requests carrying only metadata fields
    /// (`name` / `description` / `iconBlobId` / `isPublic` /
    /// `isPubliclyPreviewable`) route through a `manage_space`-gated
    /// path instead.
    fn apply_space_metadata_patch(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
        patch: jmap_chat_types::SpaceMetadataPatch,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_chat_types::Space>, BackendSetError<Self::Error>>,
    > + Send;

    /// Extra top-level Space properties owned by a Space extension, such
    /// as `calendarId` (draft-atwood-jmap-chat-calendars-00 §calendarId)
    /// or `taskListId` (draft-atwood-jmap-chat-tasks-00 §taskListId).
    ///
    /// `Space/set` accepts a listed key instead of rejecting it as
    /// `invalidProperties`. On `create`, a non-null value is stored in
    /// [`jmap_chat_types::Space::extra`] through `create_object`. In an
    /// `update` patch, the key travels in
    /// [`SpaceMetadataPatch::extra`] through
    /// [`Self::apply_space_metadata_patch`], so it is subject to that
    /// method's `manage_space` gate; a `null` value removes the property
    /// (RFC 7396). `Space/get` returns stored values like any other
    /// property. The non-member restricted view omits them.
    ///
    /// The handler does not validate values. A backend that must check
    /// one (for example, that `taskListId` names an existing TaskList on
    /// the same account) rejects it in `create_object` or
    /// `apply_space_metadata_patch` with an `invalidProperties` SetError
    /// naming the key. A listed key that is already a core Space property
    /// or a structural patch key keeps its core meaning.
    ///
    /// # Default implementation
    ///
    /// Returns an empty list, so no extension property is accepted.
    /// Backends that implement a Space extension override it.
    fn space_extension_properties(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> &'static [&'static str] {
        let _ = (caller, account_id);
        &[]
    }

    /// Whether the backend should reject any [`Self::apply_space_patch`]
    /// that would leave the target Space with zero members holding
    /// either `manage_members` or `manage_space` permission.
    ///
    /// # Invariant (by outcome, not by op type)
    ///
    /// The protection is invariant on the *post-patch* state of the
    /// (member, role, permission) graph, not on any single op variant.
    /// A backend that returns `true` from this method MUST reject every
    /// patch that takes the Space from at least one member holding
    /// `manage_members` or `manage_space` to none, including (non-exhaustively)
    /// the cases below. Self-removal (a `removeMembers` holding only the
    /// accountId, which references the owner per draft §Terminology) is
    /// exempt: draft §Space/set says
    /// "Last-administrator protection does not apply to self-removal."
    ///
    ///
    /// - `RemoveMember` that removes the last admin member.
    /// - `UpdateMember` that strips a role granting `manage_members` or
    ///   `manage_space` from a member who holds no other such role.
    /// - `UpdateRole` that removes `manage_members` or `manage_space`
    ///   permission from a role currently held by the last admin.
    /// - `RemoveRole` that drops a permission-granting role from the
    ///   graph when no other role grants the same permission to a
    ///   remaining member.
    /// - Any combination of the above within a single `ops` vector
    ///   whose cumulative effect is zero admins.
    ///
    /// Production backends should project the post-patch graph and
    /// reject when the admin count would reach zero, regardless of
    /// which op caused the transition.
    ///
    /// # Why this exists
    ///
    /// The 2026-05-12 design reversal dropped the `Space.ownerId` field:
    /// "who controls a Space" is now fully implementation-defined /
    /// out-of-band per draft-atwood-jmap-chat-00.
    /// Without a normative owner identity, the kit cannot enforce the
    /// previous "owner cannot be removed" rule. Instead the kit exposes
    /// this purely permission-graph-based knob: production backends that
    /// want to prevent the "no admin left" failure mode return `true`
    /// (the trait default); deployments with their own designated
    /// controller principal or external admin-tracking system can opt
    /// out by returning `false`.
    ///
    /// # Default implementation
    ///
    /// Returns `true` — production-safe by default. A third-party
    /// backend that does not override this method gets last-admin
    /// protection automatically.
    ///
    /// # Why the reference `MemoryBackend` overrides this to `false`
    ///
    /// The reference impl flips the default to keep existing
    /// integration tests (which do not seed admin memberships) passing
    /// unchanged. Tests that exercise the protection path opt in via
    /// `MemoryBackend::set_protect_last_admin_for_test(true)`.
    ///
    /// # Atomicity
    ///
    /// The check fires inside [`Self::apply_space_patch`] atomically
    /// with the candidate mutation: the backend snapshots the Space's
    /// member/role state, projects the post-patch admin count across
    /// all ops in the patch (member adds/removes, role-id changes,
    /// permission edits), and rejects the whole update target with an
    /// `invalidProperties` SetError naming the patch key(s) concerned
    /// if zero admins would remain (draft §Space/set `removeMembers`:
    /// "a refusal names the patch key concerned").
    ///
    /// # Why sync, not async
    ///
    /// This predicate is a deployment-policy flag: a backend either
    /// enforces the last-admin invariant or it does not. Backends
    /// that consult an external admin-tracking service to derive the
    /// answer SHOULD cache the per-account boolean and answer from
    /// the cache. See the trait-level "Async vs sync method
    /// convention" section above for the workspace rationale.
    fn protect_last_admin(&self, caller: &Self::CallerCtx, account_id: &jmap_types::Id) -> bool {
        let _ = (caller, account_id);
        true
    }

    /// Whether the backend retains edit history for messages.
    ///
    /// Per draft-atwood-jmap-chat-00 commit `0783fc4` ("condition
    /// MessageRevision push on edit-history retention") +
    /// `§Message editHistory`: servers MAY limit retained
    /// MessageRevisions, and when the server does not retain edit
    /// history, the `editHistory` field MUST be omitted from
    /// `Message/get` and (in principle) `Message/changes` responses.
    /// `Message/changes` per RFC 8620 §5.2 returns only id arrays,
    /// not Message objects, so the spec's "omit from /changes" is
    /// a no-op at the wire level — the kit only enforces the
    /// `Message/get` projection.
    ///
    /// # When the handler calls this
    ///
    /// `handle_message_get` consults this predicate once per call.
    /// When `false`, every returned `Message` has its
    /// `edit_history` field set to `None` before serialization,
    /// which serde's `skip_serializing_if = "Option::is_none"`
    /// collapses to a wire-absent field per spec.
    ///
    /// # Why sync, not async
    ///
    /// This is a static deployment-policy flag, not a per-call
    /// decision — it does not consult per-account state, principal
    /// identity, or per-message bounds. Production backends that
    /// need to vary retention per-account would file a follow-up
    /// bead for a parallel `retains_edit_history_for_account` hook
    /// rather than reshaping this method.
    ///
    /// # Default implementation
    ///
    /// Returns `false` — no retention. This matches the workspace's
    /// "kit defines the hook; consumer enforces the policy" posture.
    /// Production backends that retain edit history MUST override
    /// this to `true`; the reference `MemoryBackend` exposes a
    /// test-only setter for tests that need the retain-on path.
    fn retains_edit_history(&self) -> bool {
        false
    }

    /// Whether the backend keeps `Chat.unreadCount` itself instead of
    /// the kit recomputing it.
    ///
    /// draft-atwood-jmap-chat-00 §Chat `unreadCount` (jmap-chat-spec
    /// 33702a5 L629): "The value is advisory, for display such as
    /// badges. Servers SHOULD keep it accurate, but MAY update it some
    /// time after the change that affects it, MAY combine several such
    /// changes into one update, and MAY report a server-defined maximum
    /// in place of any larger count. Whenever the server changes the
    /// value, the Chat's state changes as for any other property
    /// ({{RFC8620}} Section 5.1)."
    ///
    /// # When the handlers call this
    ///
    /// When `false`, the kit keeps the count exact: `Chat/get` recomputes
    /// `unreadCount` from the account's Messages and ReadPositions, and
    /// every Message create or destroy, Message expiry, read-and-burn and
    /// ReadPosition update writes the recomputed count to each affected
    /// Chat through [`ChatBackend::update_object`], which advances the
    /// `Chat` state.
    ///
    /// When `true`, the kit does neither: `Chat/get` returns the stored
    /// `unreadCount` and no Chat is rewritten. The backend sees every
    /// write that moves the count (the `create_object`, `update_object`
    /// and `destroy_object` calls for Message and ReadPosition,
    /// [`ChatBackend::expire_message`] and
    /// [`ChatBackend::read_and_burn_message`]), and may defer, coalesce or
    /// cap its update. It MUST still advance the `Chat` state, and log the
    /// Chat as updated for `Chat/changes`, whenever the stored value
    /// changes (RFC 8620 §5.1: "If the data changes, this string MUST
    /// change").
    ///
    /// # Default implementation
    ///
    /// Returns `false`, the exact upkeep the spec's SHOULD asks for. A
    /// backend for which the per-event rewrite costs too much (it scans
    /// every Message in the account) overrides this to `true`.
    fn maintains_unread_count(&self) -> bool {
        false
    }

    /// Whether the [`ChatContact`] `contact_id` has `blocked: true` on the
    /// contact list of `account_id`.
    ///
    /// Delivery paths call this per recipient: `account_id` is the
    /// recipient (the owner of the contact list) and `contact_id` is the
    /// sender's or referenced ChatContact id *in that account*.
    /// draft-atwood-jmap-chat-00 §Blocked Contacts: "Typing and presence
    /// push events whose sending or referenced ChatContact has
    /// `blocked: true` on the receiving owner's contact list are
    /// similarly dropped server-side before delivery to any of the
    /// owner's clients". §ChatContact `id` "identifies the ChatContact only
    /// on this server", so `contact_id` is always resolved within
    /// `account_id`'s list; it is never looked up in the sender's account.
    ///
    /// [`ChatContact`]: jmap_chat_types::ChatContact
    ///
    /// # Callers
    ///
    /// [`crate::should_push`] calls it with the push owner's account and
    /// `Message.senderId`. [`crate::should_deliver_typing`] calls it with
    /// the typing event's recipient and `senderId`.
    /// [`crate::project_presence`] calls it with the presence event's
    /// recipient and `contactId`.
    /// [`crate::handle_chat_typing`] does not call it: the request carries
    /// the sender's `accountId`, and the recipient's account is known only
    /// to the transport that fans the event out, which calls
    /// [`crate::should_deliver_typing`] per recipient.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(false)` — no one is blocked. This is appropriate
    /// for backends that have not yet implemented contact lists, and
    /// matches the workspace's fail-open policy-hook posture.
    fn is_contact_blocked(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _contact_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(false) }
    }

    /// The owner's identity for `account_id`: the value of the
    /// `ownerUserId` Session field (draft-atwood-jmap-chat-00 §Session
    /// Object Extensions).
    ///
    /// [`crate::contact::handle_contact_set`] rejects a `ChatContact/set`
    /// create whose `identity` equals this value, octet for octet, with
    /// an `invalidProperties` SetError naming `identity`
    /// (draft-atwood-jmap-chat-00 §ChatContact/set: "the owner has no
    /// ChatContact").
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(None)`: the owner's identity is unknown and the check
    /// is skipped. Backends that serve `ownerUserId` MUST override this
    /// to return the same value.
    fn owner_user_id(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<String>, Self::Error>> + Send {
        async { Ok(None) }
    }

    /// Authorization gate consulted by `handle_emoji_set` before every
    /// `CustomEmoji/set` create, update, or destroy.
    ///
    /// Per draft-atwood-jmap-chat-00 commit `9344aec` (2026-05-11,
    /// "refactor: implementation-defined emoji authorization"):
    /// authorization for `CustomEmoji/set` is fully
    /// implementation-defined, for both server-global and Space-scoped
    /// emoji. When the caller is not authorized, the handler emits a
    /// `forbidden` SetError (RFC 8620 §5.3).
    ///
    /// # `target_space_id` semantics
    ///
    /// `target_space_id` is the **actual scope of the emoji this op
    /// targets**:
    ///
    /// - `None` — the emoji is server-global (its `spaceId` field is
    ///   absent / null).
    /// - `Some(id)` — the emoji is scoped to that Space (its `spaceId`
    ///   field equals `id`).
    ///
    /// For `Create`, the handler reads the scope from the create
    /// payload. For `Update` and `Destroy`, the handler pre-fetches
    /// the existing emoji via `get_objects` and passes its current
    /// `spaceId`. If the pre-fetch reports the id as not found, the
    /// handler skips this gate entirely and lets `update_object` /
    /// `destroy_object` surface a `notFound` SetError naturally — a
    /// non-existent target consumes no authorization decision.
    ///
    /// # When the handler calls this
    ///
    /// Exactly once per create/update/destroy entry in the request,
    /// AFTER wire-format validation has succeeded and BEFORE the
    /// corresponding `create_object` / `update_object` /
    /// `destroy_object` call. The handler skips the underlying mutation
    /// when the outer `Result` is `Ok(Err(_))` or when the outer is
    /// `Err(_)`. The wire mapping is:
    ///
    /// - `Ok(Ok(()))` — permit; the handler proceeds to the underlying
    ///   `create_object` / `update_object` / `destroy_object`.
    /// - `Ok(Err(set_err))` — deny with reason; the handler serialises
    ///   `set_err` into the appropriate `notCreated` / `notUpdated` /
    ///   `notDestroyed` map verbatim. Backends construct the SetError
    ///   with the type, description, and any `with_extra` fields the
    ///   deployment wants the client to see.
    /// - `Err(e)` — backend infrastructure failure; the handler maps
    ///   to a `serverFail` SetError.
    ///
    /// # Default implementation
    ///
    /// The default returns `Ok(Ok(()))` — every op is permitted. This
    /// matches the workspace's "kit defines the hook; consumer
    /// enforces the policy" posture (see the parallel design of
    /// [`Self::slow_mode_check`]). Production backends SHOULD override
    /// this method to apply the deployment's permission model — e.g.
    /// "only members of `target_space_id` may modify a Space-scoped
    /// emoji" or "only `manage_emoji` permission-holders may modify
    /// server-global emoji."
    ///
    /// # SetError shape recommendation
    ///
    /// Production backends SHOULD return
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)
    ///     .with_description("…")))` for authorization denials, mirroring
    /// the workspace convention for /set responses. The description
    /// should explain the deployment's denial reason without leaking
    /// internal policy details — e.g. "Space-scoped emoji on this
    /// Space requires `manage_channels` permission" rather than
    /// "principal abc123 is not in role admin on Space xyz789".
    ///
    /// # Foundation seam
    ///
    /// Backends that wish to identify the caller call
    /// [`jmap_server::JmapBackend::principal_id`] on `caller` to learn
    /// whether identity is wired, and compare `account_id` (the owner's
    /// member entry, draft §Terminology) against the Space's `members`
    /// list, or consult ACLs or whatever permission model the
    /// deployment uses. Backends whose
    /// `principal_id` returns `None` (the workspace default, see the
    /// "Caller identity (foundation seam)" section of the workspace
    /// `AGENTS.md`) cannot meaningfully implement identity-scoped
    /// authorization — they should either override `principal_id`
    /// first or return `Ok(Ok(()))` to match the reference posture.
    fn may_set_custom_emoji(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _target_space_id: Option<&jmap_types::Id>,
        _op: EmojiSetOp,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Authorization gate consulted by `handle_message_set` before a
    /// create or update with a non-empty `broadcastMentions` array is
    /// applied.
    ///
    /// Per draft-atwood-jmap-chat-00 §4.4.1: sending a `Message` with
    /// a non-empty `broadcastMentions` array requires the
    /// `"mention_broadcast"` Space permission. The server MUST reject
    /// with `forbidden` when the caller lacks the permission.
    ///
    /// For group/direct chats NOT part of any Space (`spaceId` is
    /// `None`), broadcast mentions are unrestricted per spec; the
    /// handler skips calling this method entirely in that case.
    ///
    /// # `space_id` semantics
    ///
    /// `space_id` is the Space that contains the target Chat (channel).
    /// The handler resolves it by pre-fetching the Chat and reading
    /// its `space_id` field. This method is only called when
    /// `space_id` is `Some` — the handler skips the call for
    /// group/direct chats with no Space.
    ///
    /// # `chat_id` semantics
    ///
    /// `chat_id` is the target channel Chat, whose `permissionOverrides`
    /// apply: per draft-atwood-jmap-chat-00 §Space Permission Resolution,
    /// "When determining whether a member may perform an action in a
    /// channel Chat, servers MUST evaluate permissions in this order",
    /// so an override may deny or allow `mention_broadcast` in one
    /// channel only.
    ///
    /// # When the handler calls this
    ///
    /// `handle_message_set` invokes `may_broadcast_mention` once per
    /// create entry or edit whose post-patch `broadcastMentions` is
    /// non-empty AND whose target Chat has a `spaceId`; rich-body
    /// `"broadcast"` spans are display-only and never trigger it
    /// (draft-atwood-jmap-chat-00 §Rich Body Format). The call
    /// fires AFTER structural validation (`validate_broadcast_mentions`)
    /// has succeeded and BEFORE `create_object` / `update_object`, so
    /// malformed payloads do not consume an authorization decision and
    /// an unauthorized message never touches storage.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))` — every op is permitted. This matches the
    /// workspace's "kit defines the hook; consumer enforces the policy"
    /// posture (see the parallel design of [`Self::may_set_custom_emoji`]).
    /// Production backends SHOULD override this method to check the
    /// caller's `mention_broadcast` permission in the target channel,
    /// resolved per §Space Permission Resolution (see
    /// [`resolve_channel_permissions`](crate::permissions::resolve_channel_permissions)).
    ///
    /// # SetError shape recommendation
    ///
    /// Production backends SHOULD return
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)
    ///     .with_description("sender lacks mention_broadcast permission in this channel")))`.
    fn may_broadcast_mention(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _space_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Authorization gate consulted by `handle_chat_set` before an update
    /// whose patch touches `pinnedMessageIds` is applied.
    ///
    /// Per draft-atwood-jmap-chat-00 §Chat `pinnedMessageIds`: "For group
    /// chats, only admins may modify this list. For direct chats, the
    /// owner may modify it freely. For channel chats, only members with
    /// the `"pin"` Space permission may modify this list." For a channel
    /// the `"pin"` permission is resolved per §Space Permission
    /// Resolution, so the channel's `permissionOverrides` apply.
    ///
    /// # When the handler calls this
    ///
    /// Once per update entry whose patch sets `pinnedMessageIds` (or a
    /// pointer path beneath it), AFTER wire-format validation and BEFORE
    /// `update_object`. A `chat_id` that does not resolve SHOULD yield
    /// `Ok(Ok(()))` so `update_object` surfaces `notFound`. The return
    /// mapping is the same as [`Self::may_set_custom_emoji`].
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))` — every op is permitted, matching the
    /// parallel hooks above. Production backends SHOULD override this
    /// method and deny with
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)))`.
    fn may_set_pinned_messages(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Authorization gate consulted by `handle_ban_set` before each
    /// SpaceBan create, update or destroy.
    ///
    /// Per draft-atwood-jmap-chat-00 §SpaceBan/set: create, update and
    /// destroy each "Requires `"ban"` permission in the Space."
    ///
    /// # When the handler calls this
    ///
    /// Once per entry, AFTER wire-format validation and BEFORE
    /// `create_object` / `update_object` / `destroy_object`. `space_id`
    /// is the create body's `spaceId`, or the stored ban's `spaceId` for
    /// update and destroy; an update or destroy whose ban does not
    /// resolve skips the gate so the backend surfaces `notFound`. The
    /// return mapping is the same as [`Self::may_set_custom_emoji`].
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))` — every op is permitted, matching the
    /// parallel hooks above. Production backends SHOULD override this
    /// method and deny with
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)))`.
    fn may_set_space_ban(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _space_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Authorization gate consulted by `handle_invite_set` before each
    /// SpaceInvite create or destroy.
    ///
    /// Per draft-atwood-jmap-chat-00 §SpaceInvite/set: create "Requires
    /// `"manage_members"` permission"; destroy "Requires
    /// `"manage_members"` permission or ownership of the invite."
    ///
    /// # When the handler calls this
    ///
    /// Once per entry, AFTER wire-format validation and BEFORE
    /// `create_object` / `destroy_object`. `space_id` is the create body's
    /// `spaceId`, or the stored invite's `spaceId` for destroy.
    /// `created_by` is `None` on create and the stored invite's
    /// `createdBy` on destroy. A destroy whose invite does not resolve
    /// skips the gate so the backend surfaces `notFound`. The return
    /// mapping is the same as [`Self::may_set_custom_emoji`].
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))` — every op is permitted, matching the
    /// parallel hooks above. Production backends SHOULD override this
    /// method and deny with
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)))`.
    fn may_set_space_invite(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _space_id: &jmap_types::Id,
        _created_by: Option<&jmap_types::Id>,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Visibility gate consulted by `handle_invite_get` and
    /// `handle_invite_changes` for each stored SpaceInvite.
    ///
    /// Per draft-atwood-jmap-chat-00 §SpaceInvite/get: "Only members of
    /// the Space may retrieve its invites. Members with
    /// `"manage_members"` permission see all invites; other members see
    /// only invites they created." §SpaceInvite/changes: "The same
    /// visibility rules as `SpaceInvite/get` apply".
    ///
    /// # When the handler calls this
    ///
    /// Once per invite returned by `get_objects`. `Ok(false)` hides the
    /// invite: `/get` reports a requested id as `notFound`, and
    /// `/changes` omits it from `created` and `updated`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)`, so every invite is visible, matching the
    /// parallel hooks above. Production backends SHOULD override this
    /// method; invite codes are join credentials.
    fn may_get_space_invite(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _invite: &jmap_chat_types::SpaceInvite,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) }
    }

    /// Visibility gate consulted by `handle_ban_get` for each stored
    /// SpaceBan.
    ///
    /// Per draft-atwood-jmap-chat-00 §SpaceBan/get: "Members with
    /// `"ban"` permission in the Space see all SpaceBan records for that
    /// Space. A banned user fetching their own account sees only SpaceBan
    /// records where `userId` is the accountId."
    ///
    /// # When the handler calls this
    ///
    /// Once per ban returned by `get_objects`. `Ok(false)` hides the ban,
    /// and a requested id is reported as `notFound`. `SpaceBan/changes`
    /// does not consult this hook, because the spec defines it as plain
    /// "Standard JMAP `/changes`".
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)`, so every ban is visible, matching the parallel
    /// hooks above. Production backends SHOULD override this method.
    fn may_get_space_ban(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _ban: &jmap_chat_types::SpaceBan,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) }
    }

    /// Authorization gate consulted by `handle_message_set` before each
    /// Message create.
    ///
    /// Per draft-atwood-jmap-chat-00 §Message/set: "The server MUST verify
    /// the sender is a current member of the target Chat. For group chats,
    /// this means a ChatMember record exists; for channel chats, the
    /// sender must be a member of the containing Space. Non-members MUST
    /// receive a SetError of type `forbidden`." and "For channel Chats,
    /// the server MUST verify the sender holds the `"send"` permission
    /// (resolved per {{space-permissions}}) before accepting the message;
    /// if absent, the server MUST return a SetError of type `forbidden`."
    /// [`resolve_channel_permissions`](crate::permissions::resolve_channel_permissions)
    /// implements that resolution.
    ///
    /// # When the handler calls this
    ///
    /// Once per create entry, AFTER wire-format validation and the
    /// closed-Chat and closed-Space checks, and BEFORE `create_object`.
    /// A `chat_id` that does not resolve SHOULD yield `Ok(Ok(()))` so
    /// `create_object` surfaces the normal error. The return mapping is
    /// the same as [`Self::may_set_custom_emoji`].
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))` — every create is permitted, matching the
    /// parallel hooks above. Production backends SHOULD override this
    /// method and deny with
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)))`.
    fn may_send_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Per-Chat restriction gate consulted by `handle_message_set` before
    /// each Message create, with the stored target Chat.
    ///
    /// For extensions that bind a Chat to something outside JMAP Chat and
    /// so restrict it, e.g. draft-atwood-jmap-chat-mimi-00 §message/mls:
    /// "The server MUST reject a `Message/set` create in a Chat with a
    /// `roomId` with a `forbidden` SetError." Extension properties of the
    /// Chat are in [`Chat::extra`](jmap_chat_types::Chat::extra).
    /// [`Self::may_send_message`] is the sender's membership and
    /// permission gate; this one sees the Chat itself.
    ///
    /// # When the handler calls this
    ///
    /// Once per create entry whose `chatId` resolves, AFTER wire-format
    /// validation and BEFORE the closed-Chat check and
    /// [`Self::may_send_message`]. The return mapping is the same as
    /// [`Self::may_set_custom_emoji`].
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))`: every create is permitted (fail-open, like
    /// the hooks above).
    fn may_create_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Per-Chat restriction gate consulted by `handle_message_set` before
    /// each Message update, with the stored Message, its stored Chat and
    /// the client's patch keys.
    ///
    /// For extension-bound Chats, e.g. draft-atwood-jmap-chat-mimi-00
    /// §message/mls: "The server MUST reject a `Message/set` update that
    /// changes `body`, `bodyType`, or `reactions` of a Message in a Chat
    /// with a `roomId` with an `invalidProperties` SetError naming the
    /// changed properties."
    ///
    /// # When the handler calls this
    ///
    /// Once per update entry whose Message and Chat resolve, AFTER the
    /// patch keys and `bodyType` are validated and BEFORE the sender,
    /// edit-limit and reaction checks and `update_object`. `patch_keys`
    /// are the keys exactly as the client sent them, so a reaction change
    /// appears as `reactions/<senderReactionId>`. The return mapping is
    /// the same as [`Self::may_set_custom_emoji`].
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))`: every update is permitted (fail-open, like
    /// the hooks above).
    fn may_update_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
        _message: &jmap_chat_types::Message,
        _patch_keys: &[&str],
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Per-Chat restriction gate consulted by `handle_chat_set` before
    /// each Chat update, with the stored Chat and the client's patch keys.
    ///
    /// For extension-bound Chats, e.g. draft-atwood-jmap-chat-mimi-00
    /// §Chat.roomId: "The server MUST reject, with `forbidden`, a
    /// `Chat/set` update on such a Chat that sets `name`, `description`,
    /// or `avatarBlobId` or that uses the `addMembers`, `removeMembers`,
    /// or `updateMemberRoles` patch key".
    ///
    /// # When the handler calls this
    ///
    /// Once per update entry whose Chat resolves, AFTER the patch keys
    /// are validated and BEFORE the closed-group, admin, member-patch and
    /// `pinnedMessageIds` checks and `update_object`. `patch_keys` are
    /// the keys exactly as the client sent them. The return mapping is
    /// the same as [`Self::may_set_custom_emoji`].
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))`: every update is permitted (fail-open, like
    /// the hooks above).
    fn may_update_chat(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
        _patch_keys: &[&str],
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Blob lookup consulted by `handle_message_set` for each Attachment
    /// of a Message create.
    ///
    /// Per draft-atwood-jmap-chat-00 §Attachment, `size`: "Servers MUST
    /// verify against actual content", and `sha256`: "Servers SHOULD
    /// verify". The handler compares the returned [`AttachmentBlob`]
    /// against the Attachment and rejects a mismatch with an
    /// `invalidProperties` SetError naming `attachments` (§Input
    /// Validation). The `sha256` comparison is constant-time
    /// (`subtle::ConstantTimeEq`), so implementations return the digest
    /// rather than comparing it themselves.
    ///
    /// # When the handler calls this
    ///
    /// Once per Attachment, AFTER the `maxAttachmentsPerMessage` and
    /// `maxAttachmentBytes` checks (§Denial of Service: "Enforce
    /// `maxBodyBytes` and `maxAttachmentBytes` at parse time, before any
    /// fetch or storage") and BEFORE `create_object`.
    ///
    /// # Return mapping
    ///
    /// - `Ok(Ok(Some(blob)))`: compare the Attachment against `blob`.
    /// - `Ok(Ok(None))`: the backend does not verify this blob; accept.
    /// - `Ok(Err(set_error))`: reject the create with `set_error`, e.g.
    ///   `invalidProperties` naming `attachments` for an unknown `blobId`
    ///   (RFC 8620 §5.3 defines `blobNotFound` only for upload paths).
    /// - `Err(e)`: `serverFail`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(None))`: no blob store, nothing verified, matching
    /// the fail-open hooks above. Backends with a blob store SHOULD
    /// override this method.
    fn attachment_blob(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<
        Output = Result<Result<Option<AttachmentBlob>, SetError>, Self::Error>,
    > + Send {
        async { Ok(Ok(None)) }
    }

    /// FileNode resolution consulted by `handle_message_set` for each
    /// create Attachment that carries `filenodeId`.
    ///
    /// Per draft-atwood-jmap-chat-filenode-00 §filenodeId: "When
    /// `filenodeId` is present on an Attachment, the fields `blobId`,
    /// `filename`, `contentType`, `size`, and `sha256` become OPTIONAL for
    /// that Attachment entry; the server MUST populate any absent fields
    /// from the referenced FileNode's properties before storage", and the
    /// server "MUST verify that the referenced FileNode exists and belongs
    /// to the same Space" and that "the sender has at least read access".
    /// `chat` is the message's target Chat. A deployment offering
    /// `urn:ietf:params:jmap:chat:filenode` implements this with
    /// `jmap_chat_filenode_server::resolve_filenode_attachment`.
    ///
    /// # When the handler calls this
    ///
    /// Once per such Attachment, AFTER the `maxAttachmentsPerMessage`
    /// check and the `may_send_message` gate, and BEFORE the
    /// draft-atwood-jmap-chat-00 §Input Validation checks, the
    /// `maxAttachmentBytes` check and [`Self::attachment_blob`], which all
    /// apply to the returned Attachment.
    ///
    /// # Return mapping
    ///
    /// - `Ok(Ok(Some(attachment)))`: store `attachment`, the entry with
    ///   its absent fields filled.
    /// - `Ok(Ok(None))`: the extension is not active; the entry must carry
    ///   all five fields, as an Attachment without `filenodeId` does.
    /// - `Ok(Err(set_error))`: reject the create with `set_error`.
    /// - `Err(e)`: `serverFail`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(None))`: no FileNode store, matching the fail-open
    /// hooks above.
    fn filenode_attachment(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat: &jmap_chat_types::Chat,
        _attachment: &jmap_chat_types::AttachmentCreate,
    ) -> impl std::future::Future<
        Output = Result<Result<Option<jmap_chat_types::Attachment>, SetError>, Self::Error>,
    > + Send {
        async { Ok(Ok(None)) }
    }

    /// Visibility gate consulted by `handle_message_get` and
    /// `handle_message_query`.
    ///
    /// Per draft-atwood-jmap-chat-00 §Message/query: "For channel Chats,
    /// the server MUST verify the caller holds the `"view"` permission
    /// before returning messages in `Message/get` or `Message/query`
    /// responses; messages in channels the caller cannot view MUST be
    /// omitted." The permission is resolved per §Space Permission
    /// Resolution, as in [`Self::may_send_message`].
    ///
    /// # When the handler calls this
    ///
    /// `Message/get` calls it once per distinct `chatId` among the
    /// returned messages; `Ok(false)` hides those messages, and a
    /// requested id is reported as `notFound`. `Message/query` calls it
    /// for the filter's top-level `chatId` and returns no ids when it
    /// yields `Ok(false)`. A query without `chatId` spans Chats, so the
    /// backend's `query_objects` MUST itself omit messages the caller
    /// cannot view; the handler cannot filter a page it did not cut.
    /// `Message/changes` does not consult this hook, because the spec
    /// defines it as plain "Standard JMAP `/changes`".
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)`, so every Chat is viewable, matching the
    /// parallel hooks above. Production backends SHOULD override this
    /// method.
    fn may_view_chat(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) }
    }

    /// Referential check consulted by `handle_chat_set` before an update
    /// that adds ids to `pinnedMessageIds` is applied.
    ///
    /// Per draft-atwood-jmap-chat-00 §Chat `pinnedMessageIds`: "Each id
    /// MUST be the id of a Message in this Chat; the server MUST reject a
    /// `Chat/set` update that would add any other id with
    /// `invalidProperties`."
    ///
    /// # When the handler calls this
    ///
    /// After [`Self::may_set_pinned_messages`] permits the update and
    /// BEFORE `update_object`, with `added` holding only the ids in the
    /// new list that are not already in the stored list. Ids already
    /// pinned are not re-checked: the requirement covers adding, and a
    /// stale id left behind by a backend that missed a hard delete must
    /// not make the list impossible to edit. Not called when `added` is
    /// empty or `chat_id` does not resolve.
    ///
    /// Return `Ok(Err(SetError))` with type `invalidProperties` and
    /// properties `["pinnedMessageIds"]` when any id in `added` is not a
    /// Message whose `chatId` is `chat_id`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(Ok(()))`, matching the parallel hooks above. Backends
    /// that store Messages SHOULD override it; the spec rule is a MUST.
    fn validate_pinned_message_ids(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
        _added: &[jmap_types::Id],
    ) -> impl std::future::Future<Output = Result<Result<(), SetError>, Self::Error>> + Send {
        async { Ok(Ok(())) }
    }

    /// Throttle gate consulted before a `Message/set` create lands on
    /// the rate-limited path.
    ///
    /// Per draft-atwood-jmap-chat-00 §Chat `slowModeSeconds` plus spec
    /// commit `de60acb` (2026-05-11, "soften slow-mode exemption to
    /// SHOULD with rationale"): a `Chat` with `slowModeSeconds > 0`
    /// throttles the rate at which a member may post messages. The
    /// spec SHOULD-exempts members holding the `manage_channels`
    /// permission, and servers MAY define additional exempt
    /// principals. The kit does not opine on the exemption set — the
    /// backend implements the policy.
    ///
    /// When the caller is throttled, the backend returns
    /// `Err(`[`SlowModeError`]`)` with `retry_after` set to a UTCDate
    /// the rate-limited sender may use to schedule a retry. The
    /// `Message/set` create handler maps the error onto a
    /// `rateLimit` SetError (RFC 8620 §5.3 `SetError.type`) whose
    /// `serverRetryAfter` extra field carries the UTCDate verbatim.
    ///
    /// # When the handler calls this
    ///
    /// `handle_message_set` invokes `slow_mode_check` once per create
    /// entry, after wire-format validation has succeeded and before
    /// `create_object`. A throttle rejection short-circuits to
    /// `notCreated[create_id] = { type: "rateLimit",
    /// serverRetryAfter: <UTCDate> }` and the create_object call is
    /// never made.
    ///
    /// # Default implementation
    ///
    /// The default returns `Ok(())` — no throttle. This is appropriate
    /// for backends that have not yet implemented rate-tracking and
    /// for single-tenant dev servers where no abuse path exists.
    /// Production backends SHOULD override this method with their own
    /// per-(account, chat, caller) rate tracker.
    ///
    /// # No state on the kit
    ///
    /// The kit deliberately does not provide a reference rate-tracker
    /// — that is deployment territory. The reference `MemoryBackend`
    /// keeps the default no-op behaviour for the same reason.
    fn slow_mode_check(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _chat_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), SlowModeError>> + Send {
        async { Ok(()) }
    }

    /// Hard-delete a message ("expire" it) due to a per-message expiry
    /// event.
    ///
    /// Called by [`crate::expiry::sweep_expired`] for each message whose
    /// `senderExpiresAt`, Chat `messageExpirySeconds` or
    /// `expiresAfterReadSeconds` deadline has passed
    /// (draft-atwood-jmap-chat-00 §Message, §Chat). The kit does NOT run a
    /// scheduler; consumers call the sweep, or this method directly, when
    /// a timer fires.
    ///
    /// # Contract
    ///
    /// Per draft-atwood-jmap-chat-00, an expiry MUST hard-delete the
    /// row (not tombstone) and the message MUST appear in the
    /// `destroyed` set of subsequent `Message/changes` results.
    /// Implementations therefore SHOULD:
    ///
    /// - Remove the underlying record so a subsequent `Message/get`
    ///   does not find it.
    /// - Bump the per-account `Message` state token and record the id
    ///   in the change log so `Message/changes` sees it as destroyed.
    /// - Remove the id from its Chat's `pinnedMessageIds` (§Chat
    ///   `pinnedMessageIds`: "When a pinned Message is hard-deleted, the
    ///   server MUST remove its id from this list."). The same applies to
    ///   `destroy_object::<Message>`.
    ///
    /// # Idempotency
    ///
    /// If the message no longer exists — because a previous
    /// `expire_message` call or [`Self::read_and_burn_message`] has
    /// already deleted it — this method MUST
    /// return `Ok(())`, AND MUST NOT bump the per-account `Message`
    /// state token in that case. The no-op case must be
    /// observationally indistinguishable from "the scheduler never
    /// fired" for `Message/changes` subscribers; a state bump on the
    /// retry path would surface a spurious empty change set on every
    /// re-fire and induce a poll storm. Expiry events are
    /// inherently retry-friendly (a scheduler may re-fire if it
    /// crashed mid-batch). The
    /// return type is `Result<(), Self::Error>` (not
    /// `BackendSetError`) precisely to prevent a `NotFound`
    /// SetError construction on this path: expiry is an internal
    /// scheduler hook, not a `/set`-shaped call, and there is no
    /// JMAP wire surface for per-message expiry rejection.
    ///
    /// A `Message/set` read of a `burnOnRead` Message does not use this
    /// hook; it calls [`Self::read_and_burn_message`] so the `readAt`
    /// write and the hard-delete are atomic. The expiry sweep
    /// ([`crate::sweep_expired`]) does call it for a read
    /// `burnOnRead` Message that reached the store another way.
    ///
    /// # Default implementation
    ///
    /// The default returns `Ok(())` without doing anything, so expired
    /// messages stay in the store and remain visible to `Message/get`.
    /// It is correct only for a backend on which no message ever
    /// expires. Backends whose messages can carry `senderExpiresAt`,
    /// `expiresAfterReadSeconds` or a Chat `messageExpirySeconds` MUST
    /// override this method: for `senderExpiresAt`,
    /// draft-atwood-jmap-chat-00 §Message says "servers MUST
    /// permanently delete this message — removing the row entirely,
    /// not leaving a tombstone — at or before this time".
    fn expire_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _message_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::SlowModeError;
    use jmap_types::UTCDate;

    #[test]
    fn slow_mode_error_display_contains_retry_after() {
        let err = SlowModeError::new(UTCDate::from("2026-05-16T12:34:56Z"));
        let rendered = format!("{err}");
        assert!(
            rendered.contains("slow-mode rate limit"),
            "Display missing prefix: {rendered}"
        );
        assert!(
            rendered.contains("2026-05-16T12:34:56Z"),
            "Display missing retry_after timestamp: {rendered}"
        );
    }

    #[test]
    fn slow_mode_error_is_std_error() {
        // Compile-time check that SlowModeError implements std::error::Error.
        // If this fails to compile, the trait impl regressed.
        fn assert_is_error<E: std::error::Error>(_: &E) {}
        let err = SlowModeError::new(UTCDate::from("2026-05-16T12:34:56Z"));
        assert_is_error(&err);
    }

    #[test]
    fn slow_mode_error_boxes_as_dyn_error() {
        // Independent oracle: the std error machinery itself. If
        // SlowModeError does not implement std::error::Error this
        // conversion does not compile.
        let err = SlowModeError::new(UTCDate::from("2026-05-16T12:34:56Z"));
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert!(boxed.to_string().contains("slow-mode rate limit"));
    }

    use super::OpResult;
    use jmap_server::backend::{SetError, SetErrorType};

    #[test]
    fn op_result_ok_carries_op_index_and_optional_id() {
        let r = OpResult::ok(3, Some(jmap_types::Id::from("space-1")));
        assert_eq!(r.op_index, 3);
        assert_eq!(r.outcome, Ok(Some(jmap_types::Id::from("space-1"))));
    }

    #[test]
    fn op_result_ok_with_none_id_for_update_or_destroy() {
        let r = OpResult::ok(0, None);
        assert_eq!(r.op_index, 0);
        assert_eq!(r.outcome, Ok(None));
    }

    #[test]
    fn op_result_err_carries_op_index_and_set_error() {
        let r = OpResult::err(7, SetError::new(SetErrorType::Forbidden));
        assert_eq!(r.op_index, 7);
        match r.outcome {
            Err(e) => assert_eq!(e.error_type, SetErrorType::Forbidden),
            Ok(_) => panic!("expected Err outcome"),
        }
    }

    #[test]
    fn op_result_with_op_index_replaces_field() {
        let r = OpResult::ok(0, None).with_op_index(42);
        assert_eq!(r.op_index, 42);
        assert_eq!(r.outcome, Ok(None));
    }

    #[test]
    fn op_result_with_outcome_replaces_field() {
        let r =
            OpResult::ok(0, None).with_outcome(Err(SetError::new(SetErrorType::InvalidProperties)));
        assert_eq!(r.op_index, 0);
        match r.outcome {
            Err(e) => assert_eq!(e.error_type, SetErrorType::InvalidProperties),
            Ok(_) => panic!("expected Err outcome after with_outcome"),
        }
    }

    // Const-context smoke tests — lock in the const fn markers so a
    // future refactor that accidentally drops `const` from any of
    // these constructors / builders is caught at compile time
    // (bd:JMAP-x2gd.79).

    use super::ChatLimits;

    /// `OpResult::ok` is callable in const context.
    #[test]
    fn op_result_ok_is_const_fn() {
        const _R: OpResult = OpResult::ok(0, None);
    }

    /// `ChatLimits::new` plus all four `with_*` setters chain in const
    /// context — proves the entire builder pipeline stays const.
    #[test]
    fn chat_limits_builder_is_const_fn() {
        const _LIMITS: ChatLimits = ChatLimits::new(1, 2, 3, 4)
            .with_max_roles_per_space(10)
            .with_max_space_members(20)
            .with_max_channels_per_space(30)
            .with_max_categories_per_space(40);
    }

    // Note: `SlowModeError::new` is `const fn` (clippy::nursery
    // `missing_const_for_fn` would re-fire if the marker were dropped),
    // but a const-context smoke test cannot be authored without a
    // const-constructible `UTCDate`. `UTCDate` wraps a `String`, and
    // `String` is not const-constructible from a non-empty literal in
    // stable Rust today. The clippy lint covers the regression case.
}
