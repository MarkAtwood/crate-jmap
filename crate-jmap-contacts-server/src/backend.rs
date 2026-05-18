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
//! `jmap-contacts-types` respectively; they are re-exported here for convenience.

pub use jmap_contacts_types::backend::{AddressBookProperty, ContactCardProperty};
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

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
/// [`SetError`] / [`BackendSetError`].
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
    /// # Sentinel fields the backend MUST replace
    ///
    /// The method handlers in this crate pass partially-constructed objects
    /// with a sentinel value that the backend MUST replace with a real value
    /// before storing:
    ///
    /// - **`id`**: The `id` field in the input object is always set to
    ///   `"placeholder"` so the typed `Deserialize` on `O` (AddressBook /
    ///   ContactCard) succeeds. The backend MUST replace it with a real,
    ///   unique, account-scoped id, and return that id as the first element
    ///   of the result tuple AND on the returned `O` (the handler stores the
    ///   returned `O` verbatim in the `/set` `created` response per RFC 8620
    ///   §5.3).
    ///
    /// Failing to replace the sentinel produces a record reachable by the
    /// real id but carrying `id == "placeholder"` in its serialized form;
    /// every subsequent `/get` / `/set` lookup against the real id will
    /// return data that, when re-serialized, fails to round-trip.
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
    /// Mirrors the canonical jmap-mail-server sentinel-fields contract
    /// (bd:JMAP-qz9v.29). Contacts has no `blob_id` / `size` sentinels —
    /// only `id`.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing AddressBook or ContactCard.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim. Per-request auth context
    /// is available via the `caller` parameter, which the
    /// `register_contacts_handlers` closures forward unchanged from
    /// [`jmap_server::Dispatcher::dispatch`].
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an AddressBook or ContactCard by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend implementation supports the given
    /// JMAP object type.
    ///
    /// The answer is **global per backend instance** — the signature has
    /// no `caller` or `account_id` parameter, so a backend cannot make
    /// the answer depend on the calling principal or target account.
    /// Backends that support all types unconditionally can return `true`
    /// always.
    ///
    /// Consumers needing per-account or per-caller feature gating
    /// (multi-tenant SaaS with paid contacts tiers, etc.) must implement
    /// the gating at the session-capability-builder layer outside this
    /// trait, or wrap the backend in per-tenant instances and dispatch
    /// per request — neither is supported by the current trait surface.
    /// The workspace-design discussion for adding `caller` / `account_id`
    /// parameters is tracked at bd:JMAP-qz9v.30 follow-up.
    ///
    /// Called by the server consumer (e.g. the session capability
    /// builder) — NOT called internally by the handler library.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Copy a ContactCard from one account to another.
    ///
    /// Called by `ContactCard/copy`. The card has already been fetched from
    /// `from_account_id`; the backend must store it in `to_account_id` and
    /// return the newly assigned `(id, card)`.
    fn copy_contact_card(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        card: jmap_contacts_types::ContactCard,
    ) -> impl std::future::Future<
        Output = Result<
            (jmap_types::Id, jmap_contacts_types::ContactCard),
            BackendSetError<Self::Error>,
        >,
    > + Send;

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
