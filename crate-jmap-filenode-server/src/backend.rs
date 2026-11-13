//! `FileNodeBackend` trait and supporting types.
//!
//! Consumers implement [`FileNodeBackend`] for their storage system.  The
//! method handlers in [`crate::filenode`] call into the backend through this
//! trait.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait.  Only write operations and the
//! FileNode-specific structural queries live here.

use jmap_filenode_types::{FileNode, FilesRights};
use jmap_server::{
    BackendQueryError, BackendSetError, JmapBackend, JmapObject, QueryParams, SetObject,
};

// ---------------------------------------------------------------------------
// CaseFolding
// ---------------------------------------------------------------------------

/// Case-folding mode for [`FileNodeBackend::find_sibling_by_name`].
///
/// JMAP FileNode (draft-ietf-jmap-filenode-20 §3.2.3
/// `compareCaseInsensitively`) is a Boolean on the wire, but the backend
/// trait uses a typed enum so call sites are self-documenting and the
/// algorithm space is open for future variants without a SemVer break.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseFolding {
    /// Byte-for-byte (case-sensitive) comparison.
    Sensitive,
    /// Comparison under the `i;unicode-casemap` collation (RFC 5051).
    ///
    /// draft-ietf-jmap-filenode-20 §1.1: "Where this document compares
    /// names case-insensitively, the comparison uses the
    /// "i;unicode-casemap" collation". Two names are equal under it when
    /// their RFC 5051 §2 prepared forms are equal;
    /// `jmap_metadata_server::unicode_casemap_prepare` computes that form.
    Insensitive,
}

impl CaseFolding {
    /// Map the JMAP wire-format `compareCaseInsensitively` boolean to a
    /// `CaseFolding` value. Convenience for handler call sites.
    #[must_use]
    pub fn from_wire_bool(case_insensitive: bool) -> Self {
        if case_insensitive {
            Self::Insensitive
        } else {
            Self::Sensitive
        }
    }
}

// ---------------------------------------------------------------------------
// FileNodeBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP FileNode method handlers
/// (draft-ietf-jmap-filenode-20).
///
/// Implementors provide the actual data access; the handler module
/// translates between the JMAP wire protocol and these backend calls.
///
/// Read-side operations are defined on the [`JmapBackend`] supertrait.
/// Only write operations and FileNode-specific structural queries are here.
///
/// This trait is not object-safe by design (generic methods).  Use
/// `Arc<impl FileNodeBackend>` when sharing across tasks.
///
/// # Permission enforcement (backend canonical)
///
/// The backend is the canonical enforcement point. The handlers check
/// `myRights` in one place only, the `onExists` "replace" of
/// `FileNode/set` and `FileNode/copy`, whose check order
/// draft-ietf-jmap-filenode-20 §3.2.3 fixes (see Destroy below); elsewhere
/// [`files_rights`](Self::files_rights) only reports rights. The backend
/// MUST enforce the caller's rights inside the
/// same atomic boundary as the write in
/// [`create_object`](Self::create_object),
/// [`update_object`](Self::update_object) and
/// [`destroy_object`](Self::destroy_object); `FileNode/copy` writes through
/// the same three methods.
///
/// The caller is known only through [`JmapBackend::principal_id`]. A backend
/// whose `principal_id` returns `None` honors no per-user ACL and permits
/// every write (fail-open, bd:JMAP-bghk); that suits test fixtures and
/// single-user servers, not multi-user deployments.
///
/// draft-ietf-jmap-filenode-20 §3.1.2 names the rights in `myRights`, and
/// §5 derives them from the `shareWith` entries on a node and its
/// ancestors: a node's `myRights` is "the union of the rights granted to
/// the user by the shareWith entries on the node and on all its
/// ancestors". A write that "would violate an ACL or other permissions
/// policy" fails with `forbidden` (RFC 8620 §5.3), returned as
/// [`BackendSetError::SetError`]:
///
/// - Create: `mayAddChildren` on the parent, or the account's
///   `mayCreateTopLevelFileNode` when `parentId` is null
///   (draft-ietf-jmap-filenode-20 §5).
/// - Update: `mayRename` to change `name`, `parentId` (plus
///   `mayAddChildren` on a new parent) or `role` (draft-ietf-jmap-filenode-20
///   §3.1.4: "Changing role on an existing node requires mayRename on that
///   node"), `mayModifyContent` to change
///   `blobId`, `mediaType`, `target`, `created`, `modified`, `accessed` or
///   `executable` (-20 §3.1.2), and `mayShare` to change `shareWith`. A
///   move to the top level needs the account's `mayCreateTopLevelFileNode`
///   in place of `mayAddChildren` (-20 §5).
/// - Destroy: `mayDelete`. -20 §3.2.3 also makes `forbidden` "a destroy
///   that would remove nodes the user cannot discover", such as an
///   `onDestroyRemoveChildren` cascade over a hidden descendant; for a
///   plain destroy the handler sees only the descendants
///   [`get_descendant_ids`](Self::get_descendant_ids) returns, so the
///   backend refuses the destroy of their parent.
/// - `onExists` "replace": -20 §3.2.3 has the server check, in order,
///   that the existing node is discoverable (else `forbidden`, without
///   its id), that the user has `mayDelete` on it (else `forbidden`), and
///   only then its children (`nodeHasChildren`, or `forbidden` when a
///   cascade would remove an undiscoverable descendant). The handlers make
///   these checks before any destroy: a node or descendant that
///   [`JmapBackend::get_objects`] leaves out is undiscoverable, and the
///   rights are [`files_rights`](Self::files_rights) where it is `Some`,
///   else the stored `myRights` (a parent's, for a File or SymLink). When
///   neither states rights the handler proceeds, and
///   [`destroy_object`](Self::destroy_object) stays the atomic check.
/// - Copy: `mayRead` on the source in `fromAccountId` (-20 §3.2.4; a node
///   discoverable only through a descendant's `shareWith` has `mayRead`
///   false), and the copy is a create in `accountId`.
///
/// draft-ietf-jmap-filenode-20 §5: these rights are checked "in the rights
/// of the node itself; for a node with no myRights, these are its parent's
/// rights". Only a `Directory` has `myRights` of its own (-20 §3.1.2), so a
/// write to a `File` or `SymLink` is checked against its parent's.
///
/// # Directory roles
///
/// draft-ietf-jmap-filenode-20 §5: "A Directory with a role cannot be
/// destroyed, renamed or moved; the server rejects any attempt with
/// "forbidden"". `handle_filenode_set` and `handle_filenode_copy` check the
/// stored role before every rename, move, destroy, cascade and `onExists`
/// replacement; [`update_object`](Self::update_object) and
/// [`destroy_object`](Self::destroy_object) MUST re-check it atomically
/// with the write, as for the other structural invariants. This holds for
/// every caller, whatever its rights, and does not change the rights the
/// Directory reports in `myRights`. Server code that removes such a
/// Directory, such as a Space deletion in `jmap-chat-filenode-server`,
/// clears its role first through [`update_object`](Self::update_object),
/// as a client does, so a backend that refuses to clear a role it manages
/// itself also blocks that removal.
///
/// -20 §3.1.4: "At most one Directory in an account has a given role",
/// checked "against the result of all the creates, updates and destroys in
/// a FileNode/set together". The handler checks that before any write of
/// the call, since a backend that sees one write at a time cannot tell a
/// role that moves within the call from a duplicate. A backend that guards
/// the rule itself, such as with a unique index, checks it when the whole
/// call commits, not per write.
///
/// Reads apply §Access Control discoverability: [`JmapBackend::get_objects`]
/// and [`JmapBackend::query_objects`] leave out nodes the caller cannot
/// discover, which "MUST return notFound errors if fetched with
/// FileNode/get and MUST NOT be returned in response to FileNode/query".
///
/// `jmap-sharing-server` serves the RFC 9670 Principal and ShareNotification
/// methods and owns the `myRights` contract that this trait applies to
/// FileNodes; it enforces nothing on this crate's behalf.
pub trait FileNodeBackend: JmapBackend {
    /// Create a new FileNode.
    ///
    /// Returns `(assigned_id, created_object)` on success.  `create_id` is
    /// the client-side creation id from the `/set` request.
    ///
    /// # Authorisation contract
    ///
    /// This method is the canonical enforcement point for caller authorisation
    /// per workspace AGENTS.md "Caller identity (foundation seam)" and per-crate
    /// AGENTS.md "Permission enforcement: backend canonical". Implementors MUST
    /// check that the principal identified by
    /// [`JmapBackend::principal_id(caller)`](JmapBackend::principal_id) holds the draft-ietf-jmap-filenode-20 `myRights` the write needs, as listed under
    /// "Permission enforcement (backend canonical)" on [`FileNodeBackend`],
    /// inside the same atomic boundary as the write, and MUST return
    /// [`BackendSetError::SetError`] carrying
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. Handlers in this crate do NO
    /// permission checking; any handler-layer pre-check is defense-in-depth
    /// only, and a backend that trusts the handler is a bug. A backend whose
    /// `principal_id` returns `None` permits every write (fail-open,
    /// bd:JMAP-bghk) and CANNOT correctly implement FileNode ACLs or per-user visibility scoping.
    ///
    /// # Invariants the backend MUST re-verify atomically with the insert
    ///
    /// The handler in [`crate::handle_filenode_set`] does best-effort
    /// pre-checks before invoking `create_object`. These pre-checks are
    /// **not transactional** with the insert — a concurrent client may
    /// mutate the same parent between the pre-check and the insert. The
    /// backend is the canonical enforcement point for the following
    /// FileNode-specific invariants and MUST re-verify each one inside
    /// the same atomic boundary as the insert (e.g. inside a single SQL
    /// transaction with appropriate row locks, or under the same in-
    /// memory lock for non-SQL backends):
    ///
    /// - **Name uniqueness under `parentId`** (draft-ietf-jmap-filenode-20
    ///   §3.2.3): no two siblings may share the same `name` under the
    ///   same `parentId`. The backend MUST return
    ///   [`BackendSetError::SetError`] carrying `SetErrorType::AlreadyExists`
    ///   (or the FileNode-specific equivalent the handler maps to wire) if
    ///   the insert would violate this. The handler's `onExists` mode
    ///   (`reject` / `replace` / `rename`) decides the pre-check shape;
    ///   the backend's job is to refuse the actual collision.
    ///
    /// - **Blob existence when `blobId` is non-null** (§3.1, §3.2.3): if
    ///   the requested node is a file referencing a blob, the backend
    ///   MUST verify the blob exists at insert time. A pre-check via
    ///   [`Self::blob_exists`] is not transactional with the insert.
    ///
    /// - **Parent existence and node-type compatibility** (§3.1): the
    ///   parent referenced by `parentId` (when non-null) MUST exist at
    ///   insert time and MUST be of node-type `directory`.
    ///
    /// A backend that follows the docstring literally — "if create
    /// succeeds, return Ok" — and skips the atomic re-verification will
    /// silently corrupt the FileNode invariants under concurrent writes.
    /// MemoryBackend gets this right because the single in-memory lock
    /// covers both the read pre-check and the insert; production SQL
    /// backends MUST use explicit row locks or unique constraints.
    ///
    /// # Errors
    ///
    /// Reject a create that RFC 8620 §5.3 defines an error for with
    /// [`BackendSetError::SetError`] carrying the matching
    /// [`SetErrorType`](jmap_server::SetErrorType); the handler places it verbatim in `notCreated`.
    /// For create these are `forbidden`, `overQuota`, `tooLarge`,
    /// `rateLimit` and `invalidProperties`. An `invalidProperties` error
    /// SHOULD name in `properties` "*all* the properties that were
    /// invalid".
    ///
    /// Return [`BackendSetError::Other`] only for unexpected storage
    /// failures. The handler reports it as `serverFail` without the
    /// error's `Display` text, which tells the client to retry; a
    /// deterministic rejection returned as `Other` invites retries that
    /// cannot succeed.
    ///
    /// The `/set` handlers in this crate reject an unknown `account_id`
    /// with the method-level `accountNotFound` error before calling the
    /// backend.
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
    /// The write, the state advance and the change record MUST be atomic
    /// with respect to other callers: the reference `MemoryBackend` does
    /// all three under one mutex acquisition, and a database backend uses
    /// one transaction. A call that returns `Err` MUST leave the stored
    /// data, the state and the change log unchanged.
    ///
    /// draft-ietf-jmap-filenode-20 adds FileNode rules to that log:
    ///
    /// - `changed` is server-set (§3.1.1): the backend sets it on every
    ///   create and copy ("a new changed value", §3.2.4), and on an update
    ///   that changes a property "that has the same value for every user".
    ///   A change to a per-user property (`isSubscribed`,
    ///   `privateMetadata`) "need not update changed", and the server
    ///   "SHOULD report a per-user change in FileNode/changes only to the
    ///   users whose value changed". A metadata-only change does not update
    ///   it either (draft-ietf-jmap-metadata-05 §2.2, which overrides -20's
    ///   listing of `metadata`).
    /// - "A node's rights can change without any change to the node
    ///   itself: through a change to shareWith on an ancestor, or when the
    ///   node or an ancestor moves to a parent whose inherited rights
    ///   differ. The server MUST report every node whose rights for a user
    ///   changed as changed in FileNode/changes for that user" (§5), so an
    ///   update of `shareWith` or `parentId` records such descendants as
    ///   updated, without touching their `changed`.
    /// - "Otherwise, moving or renaming a node changes only that node": a
    ///   descendant whose rights stay the same is not reported.
    /// - "When a node stops being discoverable by the user, for example
    ///   because a shareWith entry was removed or the node was moved out of
    ///   a shared Directory, the server reports it as destroyed in
    ///   FileNode/changes" for that user.
    ///
    /// The reference `MemoryBackend` has no users: it sets `changed` as
    /// above, reports the whole subtree of a node whose `shareWith`
    /// changed, and has no inherited rights that a move could change.
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

    /// Store `obj`, a record read from `from_account_id`, as a new record in
    /// `to_account_id`, for `/copy` (RFC 8620 §5.4).
    ///
    /// Called by `FileNode/copy` after the handler fetched the node from
    /// `from_account_id`, applied the request's overrides, including any
    /// `metadata` and `privateMetadata` it supplies
    /// (draft-ietf-jmap-metadata-05 §3.4), and resolved `onExists`.
    /// `from_account_id` may equal `to_account_id`
    /// (draft-ietf-jmap-filenode-20 §3.2.4).
    ///
    /// The handler has already made `obj` the copy -20 §3.2.4 describes: no
    /// `changed`, `shareWith`, `role`, `isSubscribed`, `myRights` or
    /// `sha256` of the source ("Each copy is a new node, with a new id and a
    /// new changed value"), the source's `created`, `modified` and
    /// `accessed` unless overridden, and for a File the `blobId` that
    /// [`copy_blob`](Self::copy_blob) returned in `to_account_id`, with its
    /// `sha256`. "Copying a Directory copies its whole subtree": the handler
    /// calls this once for the copied root and then once per descendant,
    /// parents first, each with `parentId` set to the stored copy of its
    /// parent and `create` an empty object. If one fails it destroys the
    /// copies already stored for that entry with
    /// [`destroy_object`](Self::destroy_object), so the subtree "succeeds or
    /// fails as a whole"; a backend that must not show a partial subtree
    /// even briefly needs its own transaction around the entry.
    ///
    /// `create` is the request's create object for this copy (RFC 8620
    /// §5.4). -05 §3.4 says its
    /// `metadata` and `privateMetadata` follow "the same rules as a create
    /// in Foo/set", so the backend validates them as
    /// [`create_object`](Self::create_object) does (-05 §3.2 rules 1 to 4,
    /// `maxDepth` only on these supplied values) and leaves the copied
    /// values unvalidated ("Copied values are not revalidated against the
    /// destination account"). Every other rule of
    /// [`create_object`](Self::create_object) applies to the copy,
    /// including the invariants it re-verifies atomically with the insert.
    /// The backend assigns the new `id`, stores the record in
    /// `to_account_id` and records the create in the state and change log
    /// as [`create_object`](Self::create_object) does. Returns the new id
    /// and the stored record.
    ///
    /// # Authorisation contract
    ///
    /// draft-ietf-jmap-filenode-20 §3.2.4: "Copying a node requires mayRead
    /// on it and mayAddChildren on the destination parent (or
    /// mayCreateTopLevelFileNode)". `obj.id` is still the id of the source
    /// node in `from_account_id`; the backend assigns the copy its own.
    /// Implementors MUST verify `mayRead` on that source node and the
    /// destination right on the
    /// destination parent in `to_account_id` before storing the
    /// copy, and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object): a
    /// [`BackendSetError::SetError`] is reported for that copy.
    fn copy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        obj: O,
        create: &serde_json::Value,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
    + Send;

    /// Apply a partial update (patch) to an existing FileNode.
    ///
    /// Returns `Some(updated_object)` if the backend modified properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller write-access to the target object before
    /// applying the patch and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    ///
    /// # Invariants the backend MUST re-verify atomically with the update
    ///
    /// Like [`Self::create_object`], the handler's pre-checks are
    /// best-effort and the backend is the canonical enforcement point.
    /// When the patch mutates `parentId` and/or `name`, the backend MUST
    /// re-verify the following invariants inside the same atomic
    /// boundary as the write:
    ///
    /// - **Cycle prevention when `parentId` changes**
    ///   (draft-ietf-jmap-filenode-20 §3.2.3: "An attempt to move a node
    ///   beneath itself is rejected"): the new `parentId` MUST NOT be the
    ///   node itself nor any of its descendants. A pre-check via
    ///   [`Self::get_descendant_ids`] is not transactional with the
    ///   update — a concurrent move could change the tree shape between
    ///   the pre-check and the patch.
    ///
    /// - **Name uniqueness under the new (or unchanged) `parentId`**
    ///   when `name` changes: same shape as the create-side invariant.
    ///
    /// - **Blob existence when `blobId` changes** to a non-null value:
    ///   same shape as the create-side invariant.
    ///
    /// - **Immutable-property prevention**: the server-set properties
    ///   (`id`, `changed`, `myRights`, `size`, `sha256`) and the immutable
    ///   ones (`@type`, `spaceId`) cannot be patched. The handler rejects
    ///   patches that touch them with `invalidProperties` before reaching
    ///   this method; the backend MAY assume the patch does not contain
    ///   those keys. `created` is not among them: draft-ietf-jmap-filenode-20
    ///   §3.1.2 lets a client set it under `mayModifyContent`.
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

    /// Destroy a FileNode by id.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller write-access to the target object before
    /// destroying it and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    ///
    /// # Invariants the backend MUST re-verify atomically with the destroy
    ///
    /// - **`nodeHasChildren` guard**: when the FileNode has descendants
    ///   AND the handler's `onDestroyRemoveChildren` flag is `false`,
    ///   the destroy MUST be refused. The handler does a pre-check via
    ///   [`Self::get_descendant_ids`] and returns the wire-level
    ///   `nodeHasChildren` error before invoking this method, but the
    ///   pre-check is not transactional. A backend that supports
    ///   concurrent writers MUST re-verify atomically — either via
    ///   referential-integrity constraints (FOREIGN KEY ... ON DELETE
    ///   RESTRICT) or via an explicit re-check under the same lock as
    ///   the row removal.
    ///
    /// - **Cascade destroy ordering**: when the handler invokes
    ///   `destroy_object` as part of a cascade (handler has called
    ///   [`Self::get_descendant_ids`] and is destroying descendants
    ///   first, then the parent), the backend MUST NOT reject the
    ///   parent destroy on `nodeHasChildren` grounds even if the
    ///   descendants are not yet fully removed — the handler is
    ///   coordinating the ordering. A backend that enforces
    ///   `nodeHasChildren` purely via FK constraint will refuse the
    ///   parent removal until the descendants are gone; this is
    ///   acceptable because the handler destroys descendants first.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object), plus these RFC 8620
    /// §5.3 rules for destroy: an unknown id, including one already
    /// destroyed, is `notFound`, and a destroy that "would violate an ACL
    /// or other permissions policy" is `forbidden`.
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

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    ///
    /// Every `FileNode/*` handler returns `accountNotSupportedByMethod`
    /// when this is `false` for the request's `accountId`.
    ///
    /// # Per-account semantics
    ///
    /// Capabilities are per account (RFC 8620 §1.6.2, §2
    /// `accountCapabilities`), so the answer may vary with `caller` and
    /// `account_id`. Backends that support every type in this trait for
    /// every account can ignore both arguments and return `true`.
    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool;

    /// Returns the union of all ancestor nodes (immediate parent, grandparent,
    /// ..., up to the root) for every input id in `ids`.
    ///
    /// # Contract
    ///
    /// - **Union semantics.** The result is a single flat `Vec<FileNode>`
    ///   that is the union of every input id's ancestor chain. The input
    ///   ids themselves are NOT included.
    /// - **Deduplication.** When two input ids share an ancestor (e.g. a
    ///   common parent), that ancestor MUST appear at most once in the
    ///   result. Backends doing `SELECT ... WHERE id IN (parents)` must
    ///   add a DISTINCT/dedup step.
    /// - **Ordering is unspecified.** Callers MUST NOT depend on the order
    ///   of results. The reference `MemoryBackend` returns BFS order
    ///   (all immediate parents, then all grandparents, ...) but that is
    ///   not a contract; production backends may use any order. If a
    ///   caller needs the per-id chain (the path from a specific input id
    ///   up to root), it must reconstruct that walk itself using each
    ///   FileNode's `parentId` field.
    /// - **Empty input.** An empty `ids` slice returns `Ok(vec![])`.
    /// - **Unknown ids.** Ids in the input that do not exist in the account
    ///   are silently skipped (no error, no entry in the result). Detection
    ///   of unknown ids belongs to `get_objects` if needed.
    ///
    /// # Use cases
    ///
    /// - Cycle detection: if a proposed new `parentId` is in this set, the
    ///   move would create a cycle.
    /// - `fetchParents` expansion in `FileNode/get`: the result is dumped
    ///   into the flat `list` alongside the requested ids; ordering does
    ///   not matter to RFC 8620 §5.1.
    fn get_ancestors(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ids: &[jmap_types::Id],
    ) -> impl std::future::Future<Output = Result<Vec<FileNode>, Self::Error>> + Send;

    /// Returns all IDs that are descendants of the given node (children,
    /// grandchildren, etc.).
    ///
    /// Used for: (1) cycle detection — if proposed new `parentId` is in the
    /// descendant set, the move would create a cycle; (2) `nodeHasChildren`
    /// guard — if result is non-empty, the node has children.
    fn get_descendant_ids(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_types::Id>, Self::Error>> + Send;

    /// Returns whether a blob exists in the given account.
    ///
    /// Used by `FileNode/set` to validate `blobId` fields before creating or
    /// updating a file node with `@type` `"File"`, and by `FileNode/copy`
    /// to check that a copied `File` node's blob exists in the destination
    /// account.
    ///
    /// # Three-way result
    ///
    /// The return type is `Result<bool, Self::Error>` to distinguish three
    /// states that callers actually need to tell apart:
    ///
    /// - `Ok(true)` — the blob is definitely present and reachable.
    /// - `Ok(false)` — the blob is definitely absent. The handler maps this
    ///   to `invalidProperties` ("blob not found") on a create.
    /// - `Err(_)` — connectivity/transient failure. The handler maps this
    ///   to `serverFail` so the client knows to retry. Returning `Ok(false)`
    ///   for a transient backend failure is a bug: the client receives a
    ///   deterministic-looking error and will not retry.
    ///
    /// Mirrors the canonical `MailBackend::blob_exists` contract.
    fn blob_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Copy the content of blob `blob_id` of `from_account_id` into
    /// `to_account_id` for `FileNode/copy`, and return the `blobId` it has
    /// there, or `None` if it cannot be copied.
    ///
    /// draft-ietf-jmap-filenode-20 §3.2.4: "For a File, the server copies
    /// the content of its blob into the destination account; the blob need
    /// not already exist there, and the new blobId may differ."
    /// `handle_filenode_copy` calls this for every File it copies, the
    /// descendants of a copied Directory included, before storing any of
    /// them, and stores the copy with the returned `blobId`. On `Ok(None)`
    /// the copy fails with `invalidProperties` naming `blobId`, and on
    /// `Err(e)` with `serverFail`; nothing is stored for that entry, though
    /// blobs copied for it before the failure stay, unreferenced.
    ///
    /// The backend checks the caller may read the blob in
    /// `from_account_id`, as for the node itself (see
    /// [`copy_object`](Self::copy_object)).
    ///
    /// # Default implementation
    ///
    /// Copies nothing: returns `Some(blob_id)` when
    /// [`blob_exists`](Self::blob_exists) finds the blob in
    /// `to_account_id`, which holds within one account, and `None`
    /// otherwise, so a copy between accounts that do not share the blob
    /// fails. Backends that keep blobs per account override this to copy
    /// the content.
    fn copy_blob(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send {
        let _ = from_account_id;
        async move {
            Ok(self
                .blob_exists(caller, to_account_id, blob_id)
                .await?
                .then(|| blob_id.clone()))
        }
    }

    /// SHA-256 digest of a stored blob, for the `sha256` FileNode
    /// property of draft-atwood-jmap-cid-00 §FileNode Extension.
    ///
    /// The draft defines the property only "When a server advertises both
    /// `urn:ietf:params:jmap:cid` and the {{JMAP-FILENODE}} capability",
    /// and says: "When a `FileNode/set` ({{JMAP-FILENODE}}) create or
    /// update changes the associated blob, the server MUST update `sha256`
    /// to reflect the new blob content before returning the response."
    ///
    /// # When the handler calls this
    ///
    /// `handle_filenode_set` calls it for a `File` create with a `blobId`
    /// (after [`Self::blob_exists`]) and for an update whose patch sets
    /// `blobId`, and `handle_filenode_copy` calls it (in the destination
    /// account) for each copied `File` with a `blobId`; each stores the
    /// result as the node's `sha256`. The property is server-set: a
    /// client-supplied or source `sha256` is dropped on create and copy, and an update patch may carry it only at its current value
    /// (RFC 8620 §5.3), otherwise the update fails with `invalidProperties`.
    ///
    /// # Return mapping
    ///
    /// - `Ok(Some(digest))`: the server advertises `urn:ietf:params:jmap:cid`
    ///   for this account; the node's `sha256` becomes `digest`.
    /// - `Ok(None)`: the server does not advertise it; the node has no
    ///   `sha256`.
    /// - `Err(e)`: that create or update fails with `serverFail`. A backend
    ///   that advertises the capability returns `Err` rather than `None`
    ///   when it cannot compute a digest, since the draft says a server
    ///   "MUST NOT return `null` for any file FileNode created or updated
    ///   while this capability is active".
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(None)`: no `urn:ietf:params:jmap:cid` support. Backends
    /// that advertise the capability MUST override this method.
    fn blob_sha256(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<jmap_cid_types::Sha256>, Self::Error>> + Send
    {
        async { Ok(None) }
    }

    /// Whether the server advertises `urn:ietf:params:jmap:cid` for this
    /// account, so that every FileNode carries the `sha256` property.
    ///
    /// draft-atwood-jmap-cid-00 §FileNode Extension: when both capabilities
    /// are advertised, "FileNode objects MUST include" `sha256`, which "MUST
    /// be `null` for directory FileNodes". A stored node without a digest
    /// (a directory, or a file created before the capability was enabled)
    /// has no `sha256` field, so `handle_filenode_get`,
    /// `handle_filenode_set` and `handle_filenode_copy` ask this method
    /// whether to emit `"sha256": null` for it.
    ///
    /// # Return mapping
    ///
    /// - `Ok(true)`: `FileNode/get` emits `"sha256": null` for every node
    ///   without a digest (unless `properties` excludes `sha256`), and
    ///   `FileNode/set` and `FileNode/copy` do the same for each `created`
    ///   entry (for `/copy`, asked of the destination account).
    /// - `Ok(false)`: nodes without a digest have no `sha256` property.
    /// - `Err(e)`: the `FileNode/get` call, or a `FileNode/set` or
    ///   `FileNode/copy` call with `create`, fails with `serverFail` (before
    ///   any create is applied).
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(false)`, matching the default [`Self::blob_sha256`].
    /// A backend that overrides `blob_sha256` to advertise the capability
    /// MUST override this method too.
    fn cid_advertised(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(false) }
    }

    /// The `maxFileNodeDepth` this account advertises, or `None` for `null`.
    ///
    /// draft-ietf-jmap-filenode-20 §2.1: "The maximum number of levels in
    /// the FileNode hierarchy, or null for no limit. A FileNode may have at
    /// most one fewer ancestors than this value, so 1 allows only top-level
    /// nodes." Return the value the session advertises for `account_id`.
    ///
    /// With `Some(max)`, `FileNode/set` create rejects a node that would be
    /// deeper than `max`, and a `FileNode/set` update of `parentId` and a
    /// `FileNode/copy` reject one that would put any node of the moved or
    /// copied subtree deeper than `max`. -20 §3.2.3 makes this
    /// "invalidProperties" ("a create or move that would give the node, or
    /// any of its descendants, more ancestors than maxFileNodeDepth
    /// allows"); the error names `parentId`, as
    /// brong/draft-gondwana-jmap-filenode#16 proposed.
    ///
    /// The handlers count depth with [`Self::get_ancestors`] and
    /// [`Self::get_descendant_ids`], so a backend that leaves nodes the
    /// caller cannot discover out of those results undercounts; such a
    /// backend enforces the exact limit in its writes.
    ///
    /// # Default implementation
    ///
    /// Returns `None`: no limit (fail-open, bd:JMAP-bghk).
    fn max_file_node_depth(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> Option<u64> {
        let _ = (caller, account_id);
        None
    }

    /// The `myRights` the caller holds on the stored `node`, when this
    /// backend derives them from a policy outside the stored node.
    ///
    /// draft-ietf-jmap-filenode-20 §3.1.2 makes `myRights` "server-set":
    /// "The set of rights (ACLs) the user has in relation to this node". It
    /// is defined only on nodes whose `@type` permits children, so the
    /// handlers ask this method only about `Directory` nodes: -20 §5 "A
    /// node whose @type does not permit children has no myRights. The
    /// user's rights on it are those in its parent's myRights". -20 §5 also
    /// makes a node's `myRights` "the union of the rights granted to the
    /// user by the shareWith entries on the node and on all its ancestors";
    /// an override that derives rights from `shareWith` computes that union
    /// itself (it can read the ancestors with [`Self::get_ancestors`]).
    ///
    /// `handle_filenode_get` calls this method for each Directory in its
    /// `list`, including the ancestors `fetchParents` appends, unless
    /// `properties` excludes `myRights`. `handle_filenode_set` and
    /// `handle_filenode_copy` call it for each Directory they create, after
    /// [`Self::create_object`] stores it, since RFC 8620 §5.3 puts "all
    /// server-set properties" in `created`. A deployment offering
    /// draft-atwood-jmap-chat-filenode-00 returns the result of
    /// `jmap_chat_filenode_server::space_files_rights` here.
    ///
    /// The handler only reports these rights. Backends enforce them in
    /// [`Self::create_object`], [`Self::update_object`] and
    /// [`Self::destroy_object`], atomically with the write, and leave
    /// undiscoverable nodes out of their reads (draft-ietf-jmap-filenode-20
    /// §Access Control), since discoverability depends on a node's
    /// ancestors and descendants and not on its own rights alone.
    ///
    /// # Return mapping
    ///
    /// - `Ok(Some(rights))`: `FileNode/get` reports `rights` as the node's
    ///   `myRights`, and `FileNode/set` and `FileNode/copy` report them in
    ///   the node's `created` entry.
    /// - `Ok(None)`: no policy applies to this node; `FileNode/get` reports
    ///   the `myRights` that [`JmapBackend::get_objects`] returned, and
    ///   `created` reports the `myRights` that [`Self::create_object`]
    ///   returned.
    /// - `Err(e)`: the `FileNode/get` call fails with `serverFail`. After a
    ///   `FileNode/set` or `FileNode/copy` create the node is already
    ///   stored, so the create stands and `created` reports the `myRights`
    ///   that [`Self::create_object`] returned, as for `Ok(None)`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(None)` for every node: the stored `myRights` stand and
    /// nothing is restricted (fail-open, bd:JMAP-bghk). Backends with an
    /// access policy override this method.
    fn files_rights(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _node: &FileNode,
    ) -> impl std::future::Future<Output = Result<Option<FilesRights>, Self::Error>> + Send {
        async { Ok(None) }
    }

    /// Returns the id of any sibling node that already has the given name, or
    /// `None` if the name is unique within that parent.
    ///
    /// `parent_id` is `None` for the top level. `folding` controls the
    /// comparison.
    ///
    /// Used by `FileNode/set` to enforce the `alreadyExists` constraint.
    ///
    /// # Case folding
    ///
    /// With [`CaseFolding::Insensitive`], names compare under the
    /// `i;unicode-casemap` collation (draft-ietf-jmap-filenode-20 §1.1,
    /// RFC 5051): equal when their RFC 5051 §2 prepared forms are equal,
    /// as `jmap_metadata_server::unicode_casemap_prepare` computes them.
    /// A backend whose account advertises `caseInsensitiveNames` compares
    /// this way even when `folding` is [`CaseFolding::Sensitive`]: -20
    /// §2.1 makes that server treat names "as case-insensitive for all
    /// name collision checks". A storage layer whose
    /// native folding differs (ASCII-only, Unicode full case folding,
    /// locale-dependent folding, normalization-insensitive file systems)
    /// can disagree with the collation on some pairs, such as "ß" and
    /// "ss" or a dotless "ı" and "I", and then reports collisions the
    /// draft does not, or misses ones it requires.
    fn find_sibling_by_name(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        parent_id: Option<&jmap_types::Id>,
        name: &str,
        folding: CaseFolding,
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send;

    /// Returns a name of the form `{base_name}-{n}` (`n >= 1`) that no
    /// sibling under `parent_id` has under `folding`, or `None` if the
    /// backend gives up.
    ///
    /// `FileNode/set` and `FileNode/copy` call this for `onExists`
    /// `"rename"`. `draft-ietf-jmap-filenode-20` §3.2.3 leaves the algorithm
    /// to the server ("using an algorithm of its choice"); on `None` the
    /// handler reports the create as failed.
    ///
    /// # Default implementation
    ///
    /// Tries `n = 1..=100` in order, one [`Self::find_sibling_by_name`] call
    /// each, and returns `None` if all 100 collide. Backends that can list a
    /// directory's names in one query SHOULD override this to avoid up to
    /// 100 sequential round-trips per create.
    fn find_unique_name(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        parent_id: Option<&jmap_types::Id>,
        base_name: &str,
        folding: CaseFolding,
    ) -> impl std::future::Future<Output = Result<Option<String>, Self::Error>> + Send {
        async move {
            for suffix in 1_u32..=100 {
                let candidate = format!("{base_name}-{suffix}");
                if self
                    .find_sibling_by_name(caller, account_id, parent_id, &candidate, folding)
                    .await?
                    .is_none()
                {
                    return Ok(Some(candidate));
                }
            }
            Ok(None)
        }
    }

    /// Return all FileNode IDs in the subtree rooted at any node in `root_ids`,
    /// up to `max_depth` levels deep.
    ///
    /// # `max_depth` semantics
    ///
    /// `max_depth` is the **number of levels below each root to include**.
    /// This is not the draft-ietf-jmap-filenode-20 §3.2.5 `FileNode/query`
    /// `depth` argument, which counts from the top level of the account and
    /// which `handle_filenode_query` applies itself.
    ///
    /// - `0`: returns an empty `Vec`.
    /// - `1`: include direct children of every `root_id` (one level of
    ///   descent).
    /// - `N`: include up to `N` levels of descendants.
    /// - `u64::MAX`: practical "full subtree". The default impl terminates
    ///   when `frontier` becomes empty (i.e. the deepest reachable level
    ///   has been visited), well before the counter could wrap. A
    ///   pathological tree deep enough to exhaust `u64::MAX` levels is
    ///   not reachable in practice.
    ///
    /// The `root_ids` themselves are NOT included in the result.
    ///
    /// # Default impl
    ///
    /// The default implementation calls `query_objects` with a `parentId` filter
    /// once per level — O(max_depth) backend calls.  Backends with a nested-sets
    /// model, closure table, or recursive CTE SHOULD override this to a single query.
    ///
    /// Returned IDs are deduplicated; ordering is unspecified.
    ///
    /// Errors from the per-level `query_objects` calls are propagated. The
    /// default impl does NOT silently truncate the subtree on a transient
    /// backend error — workspace policy treats silent-drop in a query
    /// result as a server-side correctness bug (see workspace AGENTS.md,
    /// Filter algebra exclusion §1). Callers that want best-effort
    /// behaviour must override this method and document the partiality
    /// in the consumer's response shape.
    fn query_subtree(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        root_ids: &[jmap_types::Id],
        max_depth: u64,
    ) -> impl std::future::Future<
        Output = Result<Vec<jmap_types::Id>, BackendQueryError<Self::Error>>,
    > + Send {
        // Default: loop using query_objects with parentId filter per level.
        // This is correct but O(max_depth) in backend calls.
        let account_id = account_id.clone();
        let root_ids = root_ids.to_vec();
        async move {
            let mut all_ids: Vec<jmap_types::Id> = Vec::new();
            let mut seen: std::collections::HashSet<jmap_types::Id> =
                root_ids.iter().cloned().collect();
            let mut frontier: Vec<jmap_types::Id> = root_ids;
            let mut depth_remaining = max_depth;

            while !frontier.is_empty() && depth_remaining > 0 {
                depth_remaining = depth_remaining.saturating_sub(1);
                let mut next_frontier: Vec<jmap_types::Id> = Vec::new();
                for parent_id in frontier.drain(..) {
                    // FileNodeFilterCondition is #[non_exhaustive], so build
                    // via Default and field mutation (struct literal is not
                    // permitted outside the defining crate). This is
                    // infallible — no silent-drop window for filter
                    // construction.
                    let mut condition = jmap_filenode_types::FileNodeFilterCondition::default();
                    condition.parent_id = Some(parent_id.clone());
                    let child_filter = jmap_types::Filter::Condition(condition);
                    let result = self
                        .query_objects::<FileNode>(
                            caller,
                            &account_id,
                            QueryParams::new().filter(Some(&child_filter)),
                        )
                        .await?;
                    for id in result.ids {
                        if seen.insert(id.clone()) {
                            all_ids.push(id.clone());
                            next_frontier.push(id);
                        }
                    }
                }
                frontier = next_frontier;
            }
            Ok(all_ids)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CaseFolding;

    #[test]
    fn case_folding_from_wire_bool_maps_true_to_insensitive() {
        // Oracle: bd:JMAP-510h.54 — the wire bool true must round-trip
        // into CaseFolding::Insensitive at the handler boundary.
        assert_eq!(CaseFolding::from_wire_bool(true), CaseFolding::Insensitive);
        assert_eq!(CaseFolding::from_wire_bool(false), CaseFolding::Sensitive);
    }
}
