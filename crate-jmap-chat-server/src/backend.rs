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
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

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
}
