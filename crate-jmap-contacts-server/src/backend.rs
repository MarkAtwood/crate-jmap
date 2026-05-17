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
pub trait ContactsBackend: JmapBackend {
    /// Create a new AddressBook or ContactCard.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request. Per-request auth
    /// context is available via the `caller` parameter, which the
    /// `register_contacts_handlers` closures forward unchanged from
    /// [`jmap_server::Dispatcher::dispatch`].
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

    /// Returns true if this account supports the given JMAP object type.
    ///
    /// Called by the server consumer (e.g. the session capability builder) —
    /// NOT called internally by the handler library. Backends that support all
    /// types unconditionally can return `true` always.
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

    /// Check whether an AddressBook has any ContactCards in it.
    ///
    /// Called by `AddressBook/set` destroy processing when
    /// `onDestroyRemoveContents` is false (the default). If this returns
    /// `Ok(true)`, the destroy is rejected with `addressBookHasContents`.
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
