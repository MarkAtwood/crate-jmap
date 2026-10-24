//! ContactsBackend trait and supporting types for JMAP Contacts method handlers.
//!
//! Consumers implement [`ContactsBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Write operations and contacts-specific operations are here.
//!
//! Marker traits and property selector enums live in `jmap-types` and
//! `jmap-contacts-types` respectively; the crate root re-exports the ones an
//! impl needs.

use jmap_server::{BackendSetError, JmapBackend, JmapObject, SetObject};

// ---------------------------------------------------------------------------
// ContactsBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP Contacts method handlers (RFC 9610).
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are defined on the [`JmapBackend`]
/// supertrait. Only write operations and contacts-specific logic are here.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl ContactsBackend>` when sharing across tasks.
///
/// # Caller identity (foundation seam)
///
/// Per the workspace AGENTS.md "Caller identity (foundation seam)" section,
/// the canonical (and only) way for a method to learn who is asking is
/// [`JmapBackend::principal_id`] on the `caller: &Self::CallerCtx`
/// parameter. `ContactsBackend` extends [`JmapBackend`], so the seam is
/// inherited; no contacts-specific identity method exists or should
/// exist.
///
/// Backends that have not wired identity (test fixtures, single-user dev
/// servers — including the in-crate `memory::MemoryBackend`, gated behind
/// `feature = "memory"`) return `None` from `principal_id`. Such
/// backends CANNOT correctly
/// implement AddressBook ACLs or any other identity-sensitive contacts
/// surface; the workspace AGENTS.md per-extension implication for
/// contacts is recorded in `crate-jmap-contacts-server/AGENTS.md`
/// "Permission enforcement: backend canonical":
///
/// > every AddressBook / ContactCard mutation must be authorized
/// > against the caller's effective rights on the AddressBook (RFC 9670
/// > myRights semantics, propagated through `jmap-sharing-server` when
/// > present).
///
/// `ContactsBackend` itself has no method that consumes
/// `principal_id` today — RFC 9610 does not define an identity-sensitive
/// contacts method by name, and the in-crate handlers (
/// `handle_address_book_*`, `handle_contact_card_*`) compute the
/// candidate mutation without reading identity. Authorization is the
/// production backend's responsibility: the implementor reads
/// `principal_id(caller)`, resolves it against the deployment's
/// permission model (RFC 9670 `myRights`, `shareWith`,
/// `isSubscribed`, per-user $seen-style state, vendor ACLs), and
/// rejects the operation atomically with the storage write via
/// [`SetError`](jmap_server::SetError) / [`BackendSetError`].
///
/// The canonical consumer pattern is `ChatBackend::apply_space_patch`
/// in jmap-chat-server (bd:JMAP-g7wu.2.4). Future AddressBook ACL
/// enforcement work in contacts SHOULD mirror that pattern.
/// bd:JMAP-qz9v.19 tracks the trait-doc-vs-implementation gap.
pub trait ContactsBackend: JmapBackend {
    /// Create a new AddressBook or ContactCard.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request. Per-request auth
    /// context is available via the `caller` parameter, which the
    /// `register_contacts_handlers` closures forward unchanged from
    /// [`jmap_server::Dispatcher::dispatch`].
    ///
    /// # Authorisation contract
    ///
    /// This method is the canonical enforcement point for caller authorisation
    /// per workspace AGENTS.md "Caller identity (foundation seam)" and per-crate
    /// AGENTS.md "Permission enforcement: backend canonical". Implementors MUST
    /// check that the principal identified by
    /// [`JmapBackend::principal_id(caller)`](JmapBackend::principal_id) has write access to the target
    /// account / AddressBook / ContactCard
    /// inside the same atomic boundary as the write, and MUST return
    /// [`BackendSetError::SetError`] carrying
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. Handlers in this crate do NO
    /// permission checking; any handler-layer pre-check is defense-in-depth
    /// only, and a backend that trusts the handler is a bug. A backend whose
    /// `principal_id` returns `None` permits every write (fail-open,
    /// bd:JMAP-bghk) and CANNOT correctly implement AddressBook ACLs or per-user scoping.
    ///
    /// # Id assignment
    ///
    /// The backend mints the new object's id, calls `build(&id)` exactly
    /// once to obtain the complete object, and stores that object under
    /// `id` in the same atomic write. `build` sets the object's `id` field
    /// to the id it is given. The handler stores the returned `O` verbatim
    /// in the `/set` `created` response (RFC 8620 §5.3), so the returned
    /// `O` MUST be the object as stored.
    ///
    /// # Server-set fields beyond `id`
    ///
    /// Other server-set fields (timestamps, computed flags, etc.) MAY be
    /// added to the returned `O` and the handler will surface them on the
    /// wire per RFC 8620 §5.3 server-set-field echo. For example, when
    /// `AddressBook/set { create: { c0: { isDefault: true } } }` would
    /// demote a previously-default AddressBook, the backend MAY return the
    /// new `AddressBook` reflecting any server-set computed fields beyond
    /// what the client requested.
    ///
    /// Mirrors the canonical jmap-mail-server `create_object` contract
    /// (bd:JMAP-qz9v.29).
    ///
    /// # Errors
    ///
    /// Reject a create that RFC 8620 §5.3 defines an error for with
    /// [`BackendSetError::SetError`] carrying the matching
    /// [`SetErrorType`](jmap_server::SetErrorType); the handler places it verbatim in `notCreated`.
    /// For create these are `forbidden`, `overQuota`, `tooLarge`,
    /// `rateLimit` and `invalidProperties`. An `invalidProperties` error
    /// SHOULD name in `properties` "*all* the properties that were
    /// invalid", for example `addressBookIds` when a ContactCard names an
    /// AddressBook that does not exist.
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

    /// Apply a partial update (patch) to an existing AddressBook or ContactCard.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim. Per-request auth context
    /// is available via the `caller` parameter, which the
    /// `register_contacts_handlers` closures forward unchanged from
    /// [`jmap_server::Dispatcher::dispatch`].
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller write-access to the target object before
    /// applying the patch and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
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

    /// Destroy an AddressBook or ContactCard by id.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller write-access to the target object before
    /// destroying it and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object), plus these RFC 8620
    /// §5.3 rules for destroy: an unknown id, including one already
    /// destroyed, is `notFound`, and a destroy that "would violate an ACL
    /// or other permissions policy" is `forbidden`.
    ///
    /// # Cascades
    ///
    /// The handlers perform the cross-record effects the client chooses,
    /// through explicit backend calls, and the backend does not repeat
    /// them. `AddressBook/set` (RFC 9610 §2.3) rejects a non-empty
    /// AddressBook with `addressBookHasContents`, or with
    /// `onDestroyRemoveContents` destroys the ContactCards only in that
    /// AddressBook and removes it from the other cards'
    /// `addressBookIds`, before it calls
    /// `destroy_object::<AddressBook>`.
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
    /// # Types in scope
    ///
    /// Implementors should answer for the JMAP object types this trait
    /// covers (RFC 9610): `AddressBook` and `ContactCard`. Backends MAY
    /// answer for object types from sibling extensions (`Email`,
    /// `Calendar`, etc.) if they also implement those traits, but the
    /// typical pattern is one backend impl per extension family. A backend
    /// that does not recognise `O` SHOULD return `false`.
    ///
    /// # Callers
    ///
    /// The contacts handler library itself does NOT consult
    /// `supports_type` — every method in
    /// [`register_contacts_handlers`](crate::register_contacts_handlers)
    /// is registered unconditionally. The intended caller is the server
    /// consumer's session-capability builder, which uses
    /// `supports_type::<AddressBook>(caller, account_id)` / `supports_type::<ContactCard>(caller, account_id)`
    /// to decide which capability URIs to advertise. This is a deliberate
    /// divergence from canonical `jmap-mail-server`, where the handler
    /// library DOES short-circuit optional methods (notably
    /// `handle_search_snippet_get` via `supports_type::<SearchSnippet>`).
    /// A backend impl shared across both extension families should answer
    /// truthfully per-type, not paper over the difference.
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

    /// Store `obj`, a record read from `from_account_id`, as a new record in
    /// `to_account_id`, for `/copy` (RFC 8620 §5.4).
    ///
    /// Called by `ContactCard/copy` after the handler fetched the card from
    /// `from_account_id` and applied the request's patch, including any
    /// `metadata` and `privateMetadata` it supplies
    /// (draft-ietf-jmap-metadata-04 §3.4). `create` is the request's create
    /// object for this copy (RFC 8620 §5.4). -04 §3.4 says its `metadata`
    /// and `privateMetadata` follow "the same rules as a create in
    /// Foo/set", so the backend validates them as
    /// [`create_object`](Self::create_object) does (-04 §3.2 rules 1 to 4,
    /// `maxDepth` only on these supplied values) and leaves the copied
    /// values unvalidated. The backend assigns the new `id`, stores the record in `to_account_id` and
    /// records the create in the state and change log as
    /// [`create_object`](Self::create_object) does. Returns the new id and
    /// the stored record.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller read access to `obj` in
    /// `from_account_id` and write access to `to_account_id` before storing the
    /// copy, and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    fn copy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        obj: O,
        create: &serde_json::Value,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
    + Send;

    /// Return `Ok(true)` if `address_book_id` (in `account_id`)
    /// currently references one or more ContactCards.
    ///
    /// "References" is defined by `ContactCard.addressBookIds[id] =
    /// true` per RFC 9610 §3 (a JMAP addition over the bare RFC 9553
    /// JSContact schema). Cards that reference this AddressBook in
    /// addition to others still count — emptiness is a property of
    /// the AddressBook, not of card ownership.
    ///
    /// Backends SHOULD NOT inspect the caller's permissions here.
    /// Visibility and authorization checks happen in
    /// [`Self::destroy_object`] (and earlier in
    /// [`Self::update_object`] for the patch-out-of-addressBookIds
    /// path); this method is a pure storage-state query.
    ///
    /// The handler that calls this method is `AddressBook/set` destroy
    /// processing when `onDestroyRemoveContents` is false (the default);
    /// see [`crate::addressbook::handle_address_book_set`] for the full
    /// RFC 9610 §2.3 destroy semantics. The wire-format consequence of
    /// returning `Ok(true)` is an `addressBookHasContents` SetError in
    /// the response — that mapping is the handler's responsibility,
    /// not the backend's.
    ///
    /// # Error contract
    ///
    /// Returning `Err(Self::Error)` signals that the backend could not
    /// determine whether the AddressBook has contents — typically a
    /// transient storage failure (DB unreachable, replica timeout, etc.).
    /// The handler maps `Err` to a method-level `serverFail` rather than
    /// proceeding with the destroy. Backends MUST NOT fail open by
    /// returning `Ok(false)` when the underlying storage is degraded —
    /// returning `Ok(false)` is a positive claim that the AddressBook is
    /// empty, and the destroy will proceed unconditionally, silently
    /// discarding any ContactCards the storage layer was unable to
    /// enumerate. Surface the storage error via `Err` instead.
    fn address_book_has_contents(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        address_book_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;
}
