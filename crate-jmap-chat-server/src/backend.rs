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
pub use jmap_chat_types::space_set::SpacePatchOp;
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
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
#[derive(Debug)]
pub struct OpResult {
    /// Zero-based index of the originating op in the input `Vec<SpacePatchOp>`.
    pub op_index: usize,
    /// The outcome of applying that op.
    pub outcome: Result<Option<jmap_types::Id>, SetError>,
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
/// these caps (the previous cap-advertising fields `maxRolesPerSpace`,
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
}

impl Default for ChatLimits {
    fn default() -> Self {
        Self {
            max_roles_per_space: 50,
            max_space_members: 10_000,
            max_channels_per_space: 500,
            max_categories_per_space: 100,
        }
    }
}

impl ChatLimits {
    /// Construct a [`ChatLimits`] with the four cap fields specified
    /// in declaration order: roles, members, channels, categories.
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
    pub fn new(
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
        }
    }

    /// Builder-style setter for [`Self::max_roles_per_space`].
    #[must_use]
    pub fn with_max_roles_per_space(mut self, max: u32) -> Self {
        self.max_roles_per_space = max;
        self
    }

    /// Builder-style setter for [`Self::max_space_members`].
    #[must_use]
    pub fn with_max_space_members(mut self, max: u32) -> Self {
        self.max_space_members = max;
        self
    }

    /// Builder-style setter for [`Self::max_channels_per_space`].
    #[must_use]
    pub fn with_max_channels_per_space(mut self, max: u32) -> Self {
        self.max_channels_per_space = max;
        self
    }

    /// Builder-style setter for [`Self::max_categories_per_space`].
    #[must_use]
    pub fn with_max_categories_per_space(mut self, max: u32) -> Self {
        self.max_categories_per_space = max;
        self
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

// ---------------------------------------------------------------------------
// Slow-mode rate-limit gate
// ---------------------------------------------------------------------------

/// Outcome of a [`ChatBackend::slow_mode_check`] rejection.
///
/// Per draft-atwood-jmap-chat-00 §Chat `slowModeSeconds` plus spec commit
/// `de60acb` (2026-05-11) which softened the manage-channels exemption to
/// SHOULD: a non-exempt member who sends faster than the configured rate
/// MUST be rejected with a `rateLimited` SetError carrying a
/// `serverRetryAfter` UTCDate that tells the client when it may retry.
///
/// `retry_after` is a fully-formed [`jmap_types::UTCDate`] — the backend
/// has already done the arithmetic (typically "now + remaining slow-mode
/// window"). The handler serialises it verbatim onto the wire as the
/// `serverRetryAfter` SetError extra field.
#[derive(Debug, Clone)]
pub struct SlowModeError {
    /// When the rate-limited sender may retry.
    pub retry_after: jmap_types::UTCDate,
}

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
pub trait ChatBackend: JmapBackend {
    /// Create a new object.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing object.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    /// Called by the server consumer (e.g. the session capability builder) —
    /// NOT called internally by the handler library. Backends that support all
    /// types unconditionally can return `true` always.
    fn supports_type<O: JmapObject>(&self) -> bool;

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
    /// # Constant-time comparison contract
    ///
    /// Consumers of the returned code (notably `Space/join` invite-code
    /// lookup) MUST compare it against attacker-supplied values in
    /// constant time using `subtle::ConstantTimeEq::ct_eq` or equivalent.
    /// The reference handler in `space::handle_space_join` already does
    /// this; backends that build their own invite-redemption paths must
    /// preserve the invariant. A plain `String == String` short-circuits
    /// at the first mismatched byte and exposes a byte-by-byte timing
    /// oracle for credential recovery. See bd:JMAP-sc1b.89.
    ///
    /// [`rand::rngs::OsRng`]: https://docs.rs/rand/latest/rand/rngs/struct.OsRng.html
    /// [`getrandom`]: https://docs.rs/getrandom
    fn generate_invite_code(&self) -> String;

    /// Implementation-defined per-Space content limits for this caller
    /// and account.
    ///
    /// Called by `handle_space_set` once per request, before
    /// dispatching to [`Self::apply_space_patch`], to enforce
    /// per-aggregate caps on roles, members, channels, and categories
    /// per Space (draft-atwood-jmap-chat-00 §Space/set, spec commit
    /// `80d5e11`).
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
    /// Handler-side permission gates (`manage_space`, `manage_roles`,
    /// `manage_members`, `manage_channels`) are tracked in
    /// `bd:JMAP-g7wu.2.4.7` and are NOT yet applied by the reference
    /// handler; the backend is responsible for rejecting any op the
    /// caller is not authorized to perform.
    ///
    /// Per-aggregate count limits on roles, members, channels, and
    /// categories per Space are applied by `handle_space_set` *before*
    /// it calls this method (bd:JMAP-g7wu.2.4.8). The handler queries
    /// the backend's [`ChatBackend::limits`] once per request,
    /// fetches the current Space, and rejects the whole update target
    /// with an `overQuota` SetError (RFC 8620 §5.3) if any aggregate
    /// would exceed its cap. This means an `apply_space_patch` call
    /// that originates from `handle_space_set` will not contain
    /// `Add*` ops that push the Space over cap. Backends called
    /// directly (bypassing the handler) MAY enforce caps a second
    /// time for defense in depth; the reference `MemoryBackend` does
    /// not. Per draft-atwood-jmap-chat-00 §Space/set (spec commit
    /// `80d5e11`, 2026-05-11), this behavior is normative.
    ///
    /// The role-position hierarchy check (members may only add or modify
    /// roles whose `position` is strictly less than their own
    /// highest-position role — draft §Space/set lines 1096, 1102) MUST
    /// be enforced by the backend because it is atomic with the
    /// mutation and depends on the current Space state. See
    /// `bd:JMAP-g7wu.2.4.3`.
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

    /// Predicate consulted by typing/presence fan-out paths: is the
    /// requesting caller blocked by the given contact?
    ///
    /// Per draft-atwood-jmap-chat-00 commit `d68b4e3` (2026-05-11,
    /// "close blocked-sender suppression gaps for typing/presence"):
    /// when the requesting account corresponds to a [`ChatContact`]
    /// whose `blocked` is `true` on the recipient's contact list, the
    /// server MUST silently suppress the ephemeral event for that
    /// recipient. The sender is NOT informed.
    ///
    /// [`ChatContact`]: jmap_chat_types::ChatContact
    ///
    /// # When the handler calls this
    ///
    /// `handle_chat_typing` consults this predicate when the target
    /// `Chat` is direct (a single recipient identified by
    /// `Chat.contact_id`). The kit does not implement transport-layer
    /// fan-out (push events to subscribers) — that is the consumer's
    /// responsibility — so the consultation result does not change
    /// the handler's wire response. The call site is the documented
    /// integration point: production transport that fans typing
    /// events out to subscribers SHOULD consult this predicate
    /// before each per-recipient event.
    ///
    /// Group / channel chats are skipped on the kit side. The kit
    /// handler has no way to enumerate fan-out recipients; that work
    /// belongs to the consumer's transport layer, which must call
    /// this predicate per recipient.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(false)` — no one is blocked. This is appropriate
    /// for backends that have not yet implemented contact lists, and
    /// it matches the workspace's "kit defines the hook; consumer
    /// enforces the policy" posture.
    fn is_contact_blocked(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _contact_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(false) }
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
    /// `destroy_object` call. On `Ok(false)` the handler maps to a
    /// `forbidden` SetError and skips the underlying mutation.
    /// On `Err(e)` the handler maps to a `serverFail` SetError.
    ///
    /// # Default implementation
    ///
    /// The default returns `Ok(true)` — every op is permitted. This
    /// matches the workspace's "kit defines the hook; consumer
    /// enforces the policy" posture (see the parallel design of
    /// [`Self::slow_mode_check`]). Production backends SHOULD override
    /// this method to apply the deployment's permission model — e.g.
    /// "only members of `target_space_id` may modify a Space-scoped
    /// emoji" or "only `manage_emoji` permission-holders may modify
    /// server-global emoji."
    ///
    /// # Foundation seam
    ///
    /// Backends that wish to identify the caller call
    /// [`jmap_server::JmapBackend::principal_id`] on `caller` and
    /// compare against the Space's `members` list, ACLs, or whatever
    /// permission model the deployment uses. Backends whose
    /// `principal_id` returns `None` (the workspace default, see the
    /// "Caller identity (foundation seam)" section of the workspace
    /// `AGENTS.md`) cannot meaningfully implement identity-scoped
    /// authorization — they should either override `principal_id`
    /// first or return `Ok(true)` to match the reference posture.
    fn may_set_custom_emoji(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _target_space_id: Option<&jmap_types::Id>,
        _op: EmojiSetOp,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) }
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
    /// `rateLimited` SetError (RFC 8620 §5.3 `SetError.type`) whose
    /// `serverRetryAfter` extra field carries the UTCDate verbatim.
    ///
    /// # When the handler calls this
    ///
    /// `handle_message_set` invokes `slow_mode_check` once per create
    /// entry, after wire-format validation has succeeded and before
    /// `create_object`. A throttle rejection short-circuits to
    /// `notCreated[create_id] = { type: "rateLimited",
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
    /// Called by the kit from two places:
    ///
    /// 1. The `Message/set` update handler when a patch sets `readAt` on
    ///    a message whose pre-patch `burnOnRead` is `true`
    ///    (draft-atwood-jmap-chat-00 §Message `burnOnRead`).
    /// 2. A deployment-provided scheduler when a message's
    ///    `senderExpiresAt` fires (draft-atwood-jmap-chat-00 §Message
    ///    `senderExpiresAt`). The kit does NOT run a scheduler;
    ///    consumers wire one in and call this method directly when a
    ///    timer fires.
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
    ///
    /// # Idempotency
    ///
    /// If the message no longer exists — because a previous
    /// `expire_message` call or an atomic `update_object`
    /// implementation has already deleted it — this method SHOULD
    /// return `Ok(())` rather than
    /// `BackendSetError::SetError(SetError::NotFound)`. Expiry events
    /// are inherently retry-friendly (a scheduler may re-fire if it
    /// crashed mid-batch) and the burn-on-read handler integration
    /// also treats a not-found backend response as a no-op.
    ///
    /// # Atomicity with `Message/set` update
    ///
    /// The reference burn-on-read handler integration calls
    /// `update_object` to apply the `readAt` patch and then calls
    /// `expire_message` as a separate step. A backend that wants
    /// strict atomicity between the `readAt` write and the hard-delete
    /// SHOULD override `update_object` to perform both inside a
    /// single transaction; in that case `expire_message` becomes
    /// idempotent for this code path (the message is already gone
    /// when the handler calls it).
    ///
    /// # Default implementation
    ///
    /// The default returns `Ok(())` without doing anything. This is
    /// appropriate for backends that have not yet implemented expiry
    /// or that handle the burn-on-read flow atomically inside their
    /// own `update_object`. Production backends that need expiry
    /// SHOULD override this method.
    fn expire_message(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _message_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send {
        async { Ok(()) }
    }
}
