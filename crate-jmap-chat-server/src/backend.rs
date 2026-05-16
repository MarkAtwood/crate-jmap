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
///
/// `#[non_exhaustive]` preserves the ability to add fields without a
/// SemVer break (e.g. an audit-log handle, commit ordering hint,
/// permission-diagnostic annotation). External consumers MUST construct
/// instances via [`OpResult::ok`] or [`OpResult::err`] rather than
/// field-init syntax.
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
    pub fn ok(op_index: usize, id: Option<jmap_types::Id>) -> Self {
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
    pub fn err(op_index: usize, error: SetError) -> Self {
        Self {
            op_index,
            outcome: Err(error),
        }
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
    pub fn new(retry_after: jmap_types::UTCDate) -> Self {
        Self { retry_after }
    }
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
    /// 2. **Minimum 128 bits of CSPRNG entropy.** The default
    ///    implementation here emits 32 lowercase-hex characters
    ///    (16 bytes = 128 bits) which is the workspace floor.
    ///    Production backends MAY use more; less is forbidden.
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
    /// oracle for credential recovery. See bd:JMAP-sc1b.89.
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
    /// Result<String, Self::Error>`; see bd:JMAP-x2gd.36 follow-ups
    /// for the workspace decision tracking.
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
    ///   `updated`. This pins a regression that was historically
    ///   present in the reference impl — see
    ///   `bd:JMAP-g7wu.2.4.9` — where the channel-categoryId mutation
    ///   silently bypassed the `Chat/changes` log.
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
    /// # `patch_map` shape
    ///
    /// The handler builds `patch_map` by walking the wire patch and
    /// keeping only the keys in `METADATA_FIELDS` (`name`,
    /// `description`, `iconBlobId`, `isPublic`,
    /// `isPubliclyPreviewable`). Backends MAY validate the values
    /// further (e.g. reject `iconBlobId` referencing a non-existent
    /// blob) and surface those as [`SetError`] returns.
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
    /// referencing a non-existent blob, see the `patch_map` shape
    /// section above) SHOULD surface as
    /// `SetErrorType::InvalidProperties` with
    /// `properties: ["iconBlobId"]` and a description that names
    /// the offending blob id. The reference impl does not validate
    /// blobs today; production backends with a blob store SHOULD.
    ///
    /// History: this method landed in bd:JMAP-g7wu.2.4.13 to close the
    /// gate gap on the top-level metadata path. Before this method
    /// existed, `Space/set` `update` routed top-level metadata
    /// through the generic `update_object::<Space>`, which has no
    /// permission gate. A caller without `manage_space` could
    /// successfully mutate a Space's `name` / `description` /
    /// `isPublic` / etc.
    fn apply_space_metadata_patch(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        space_id: &jmap_types::Id,
        patch_map: serde_json::Map<String, serde_json::Value>,
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
    /// The 2026-05-12 design reversal dropped the `Space.ownerId` field
    /// (bd:JMAP-g7wu.2.4.12): "who controls a Space" is now fully
    /// implementation-defined / out-of-band per draft-atwood-jmap-chat-00.
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
    /// `MemoryBackend::set_protect_last_admin_for_test(true)`. See
    /// `bd:JMAP-g7wu.2.4.3`.
    ///
    /// The reference impl's projection is intentionally narrow: it
    /// covers `RemoveMember` only, and does NOT model `UpdateMember`
    /// role-strip, `UpdateRole` permission-strip, or `RemoveRole`
    /// paths to zero-admin state. Production backends with the full
    /// invariant requirement MUST extend the projection. See
    /// `crate-jmap-chat-server/src/memory.rs:2838-2845` for the
    /// reference-impl scoping note.
    ///
    /// # Atomicity
    ///
    /// The check fires inside [`Self::apply_space_patch`] atomically
    /// with the candidate mutation: the backend snapshots the Space's
    /// member/role state, projects the post-patch admin count across
    /// all ops in the patch (member adds/removes, role-id changes,
    /// permission edits), and rejects the whole update target with a
    /// `Forbidden` SetError if zero admins would remain.
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
