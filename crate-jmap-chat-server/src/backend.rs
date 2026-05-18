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
    /// # Sentinel fields the backend MUST replace
    ///
    /// The method handlers in this crate pass partially-constructed objects
    /// with sentinel values that the backend MUST replace with real values
    /// before storing:
    ///
    /// - **`id`**: The `id` field in the input object is always set to
    ///   `"placeholder"`. The backend MUST replace it with a real, unique,
    ///   account-scoped ID and return that ID as the first element of the
    ///   result tuple.
    ///
    /// Failing to replace this sentinel will cause the client to receive an
    /// invalid wire value (`"placeholder"`) as the assigned id of every
    /// created `Space`, `Chat`, `Message`, `SpaceInvite`, `SpaceBan`,
    /// `CustomEmoji`, and `ReadPosition`.
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
    ///   `(account_id, chat_id)`. The handler at
    ///   [`crate::position::handle_position_set`] pre-checks via a
    ///   sequential scan and rejects sequential / intra-batch
    ///   duplicates with `alreadyExists`, but two concurrent
    ///   `ReadPosition/set` requests for the same `chatId` can both
    ///   pass the pre-check and then both reach `create_object`. The
    ///   backend MUST enforce a unique constraint on
    ///   `(account_id, chat_id)` at the storage layer (typically a
    ///   composite unique index in a database backend) and surface a
    ///   duplicate as `BackendSetError::SetError(SetError::new(
    ///   SetErrorType::AlreadyExists).with_existing_id(canonical))`.
    ///   See draft-atwood-jmap-chat-00 §ReadPosition.
    /// - **`Chat`** with `kind == Direct` — at most one direct chat
    ///   per `(account_id, contact_id)`. The handler at
    ///   [`crate::chat::handle_chat_set`] uses optimistic
    ///   create-then-validate with rollback; backends MAY enforce a
    ///   storage-level unique constraint to eliminate the rollback
    ///   path entirely, but it is not required because the
    ///   handler's rollback closes the race correctly. See
    ///   draft-atwood-jmap-chat-00 §Chat.
    ///
    /// The reference [`crate::memory::MemoryBackend`] does NOT
    /// enforce the `ReadPosition` `(account, chatId)` uniqueness
    /// constraint at the storage layer — it is single-Mutex
    /// per-call so concurrent racing cannot occur in practice, but a
    /// production backend that holds different locks per record (or
    /// no lock at all between read and write) MUST.
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
    ///
    /// # Why sync, not async
    ///
    /// Type-support is a startup-time deployment capability question,
    /// not a per-request decision. Backends that vary supported types
    /// per-tenant (e.g. "Pro tier exposes `CustomEmoji`") SHOULD load
    /// the tenant configuration once at startup and answer from the
    /// in-memory snapshot. See the trait-level "Async vs sync method
    /// convention" section above for the workspace rationale.
    ///
    /// # Known limitation: no args
    ///
    /// This method takes no `caller` / `account_id` arguments, so a
    /// multi-tenant backend cannot vary the answer per tenant from
    /// inside this method alone — it must funnel all per-tenant
    /// capability variation through `&self` state. Adding the
    /// arguments would be a non-breaking signature widening; that
    /// proposal is tracked separately.
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
    /// highest-position role — draft §Space/set lines 1096, 1102) MUST
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
    ///   channel (draft §Space/set line 1117: "Cascades to all
    ///   Messages in those channels."). The `Chat` and `Message` type
    ///   state tokens MUST both bump so `Chat/changes` and
    ///   `Message/changes` subscribers see the destruction.
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
    ///   §Space/set line 1099). Members embed directly in `Space`,
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
    /// | `AddRole` / `UpdateRole` at or above caller's highest role `position` | `Forbidden` | `"hierarchy"` or `"position"` |
    /// | Any op or combination of ops whose post-patch projection leaves the Space with zero `manage_members`-holding members (when [`Self::protect_last_admin`] returns `true`) | `Forbidden` | `"last-admin"` or `"manage_members"` |
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
    /// caller's effective permissions in the target Space:
    ///
    /// - If [`JmapBackend::principal_id`] returns `Some(caller_id)`,
    ///   verify the caller holds `manage_space` (resolved through the
    ///   Space's role hierarchy). Reject the patch with
    ///   [`SetErrorType::Forbidden`] if not.
    /// - If [`JmapBackend::principal_id`] returns `None` (single-user
    ///   mode — the backend has not wired identity), the gate is
    ///   skipped. This is consistent with the workspace AGENTS.md
    ///   "Caller identity (foundation seam)" section: a backend that
    ///   does not honor identity-dependent semantics opts out.
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
    /// [`SetError`] returns.
    ///
    /// # Return value
    ///
    /// `Ok(Some(updated_space))` if the backend modified any
    /// properties beyond what the client requested (RFC 8620 §5.3
    /// server-set field echo, e.g. a normalised `name` or a derived
    /// `iconBlobId`). `Ok(None)` if the patch was applied verbatim.
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

    /// Whether the backend should reject any [`Self::apply_space_patch`]
    /// that would leave the target Space with zero members holding
    /// either `manage_members` or `manage_space` permission.
    ///
    /// # Invariant (by outcome, not by op type)
    ///
    /// The protection is invariant on the *post-patch* state of the
    /// (member, role, permission) graph, not on any single op variant.
    /// A backend that returns `true` from this method MUST reject every
    /// patch whose post-application projection has zero members holding
    /// `manage_members` or `manage_space`, including (non-exhaustively):
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
    /// The reference impl's projection is intentionally narrow: it
    /// covers `RemoveMember` only, and does NOT model `UpdateMember`
    /// role-strip, `UpdateRole` permission-strip, or `RemoveRole`
    /// paths to zero-admin state. Production backends with the full
    /// invariant requirement MUST extend the projection.
    ///
    /// # Atomicity
    ///
    /// The check fires inside [`Self::apply_space_patch`] atomically
    /// with the candidate mutation: the backend snapshots the Space's
    /// member/role state, projects the post-patch admin count across
    /// all ops in the patch (member adds/removes, role-id changes,
    /// permission edits), and rejects the whole update target with a
    /// `Forbidden` SetError if zero admins would remain.
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

    /// Predicate consulted by typing/presence fan-out paths: is the
    /// caller (the sender of the ephemeral event) blocked by the
    /// recipient who owns `account_id`'s contact list?
    ///
    /// Per draft-atwood-jmap-chat-00 commit `d68b4e3` (2026-05-11,
    /// "close blocked-sender suppression gaps for typing/presence"):
    /// when the sender corresponds to a [`ChatContact`] whose
    /// `blocked` is `true` on the recipient's contact list, the
    /// server MUST silently suppress the ephemeral event for that
    /// recipient. The sender is NOT informed.
    ///
    /// [`ChatContact`]: jmap_chat_types::ChatContact
    ///
    /// # Argument semantics
    ///
    /// - `caller`: identity of the sender. Backends with identity
    ///   wired resolve this via
    ///   [`JmapBackend::principal_id`];
    ///   the resolved principal is matched against the
    ///   [`ChatContact`]'s implementation-defined principal binding
    ///   to determine which `ChatContact` (if any) in the recipient's
    ///   list represents the sender.
    /// - `account_id`: the **recipient's** account, i.e. the account
    ///   that owns the [`ChatContact`] list to consult. NOT the
    ///   sender's account.
    /// - `contact_id`: the candidate [`ChatContact`] id within the
    ///   recipient's account. The backend checks whether THIS
    ///   contact's `blocked` field is `true` AND whether THIS
    ///   contact corresponds to the sender (via the principal
    ///   binding resolved from `caller`).
    ///
    /// # Single-user mode (principal_id returns None)
    ///
    /// Backends that have not wired identity cannot match the sender
    /// against a `ChatContact` principal binding. Such backends
    /// SHOULD fail-safe by returning `Ok(false)` (the default impl
    /// does this), which preserves visibility — under-suppression in
    /// a single-user dev deployment is preferable to
    /// over-suppression that would mask a misconfigured backend.
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
    /// this predicate per recipient with that recipient's
    /// `account_id`.
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
    /// [`jmap_server::JmapBackend::principal_id`] on `caller` and
    /// compare against the Space's `members` list, ACLs, or whatever
    /// permission model the deployment uses. Backends whose
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
    /// implementation has already deleted it — this method MUST
    /// return `Ok(())`, AND MUST NOT bump the per-account `Message`
    /// state token in that case. The no-op case must be
    /// observationally indistinguishable from "the scheduler never
    /// fired" for `Message/changes` subscribers; a state bump on the
    /// retry path would surface a spurious empty change set on every
    /// re-fire and induce a poll storm. Expiry events are
    /// inherently retry-friendly (a scheduler may re-fire if it
    /// crashed mid-batch) and the burn-on-read handler integration
    /// also treats a not-found backend response as a no-op. The
    /// return type is `Result<(), Self::Error>` (not
    /// `BackendSetError`) precisely to prevent a `NotFound`
    /// SetError construction on this path: expiry is an internal
    /// scheduler hook, not a `/set`-shaped call, and there is no
    /// JMAP wire surface for per-message expiry rejection.
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
