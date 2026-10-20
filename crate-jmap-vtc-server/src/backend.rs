//! VtcBackend trait and supporting types for the JMAP VTC method handlers.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. This module adds the write
//! operations and the deployment facts the VTCCall handlers consult.
//!
//! The `pub use` block re-exports the [`jmap_server`] backend types so a
//! downstream impl can `use jmap_vtc_server::backend::*`; each re-export
//! pins the upstream type's major-version contract into this crate.

pub use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};

use jmap_vtc_types::{VtcCall, VtcCapability};

/// The `invalidProperties` SetError that
/// draft-atwood-jmap-vtc-00 §VTCCall/set requires for a second active call
/// on one Chat.
///
/// "When creating a VTCCall with a `chatId` that is already referenced by
/// another VTCCall in `"active"` or `"ringing"` state, the server MUST
/// return an `invalidProperties` SetError naming `chatId`, with
/// description "A call is already active on this Chat.""
///
/// [`VtcBackend::create_call`] returns this. The check is a uniqueness
/// constraint over stored calls, so it belongs to the backend, atomically
/// with the insert; a handler-side read could race a concurrent create.
pub fn call_already_active_on_chat() -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties(["chatId"])
        .with_description("A call is already active on this Chat.")
}

/// Storage backend for the JMAP VTC method handlers
/// (draft-atwood-jmap-vtc-00).
///
/// # Permission enforcement (backend canonical)
///
/// Per the workspace AGENTS.md "Caller identity (foundation seam)" rule the
/// backend re-verifies every permission atomically with the mutation. The
/// handlers apply the draft's call-level gates (capability flags, the
/// moderator rule via [`Self::is_moderator`]) as defense in depth.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl VtcBackend>` when sharing across tasks.
pub trait VtcBackend: JmapBackend {
    /// The account-level `urn:ietf:params:jmap:vtc` capability object
    /// (draft-atwood-jmap-vtc-00 §Account-Level Capability Object) the
    /// consumer advertises for `account_id` to this caller.
    ///
    /// `VTCCall/set` create reads `mayCreateCall`, `supportsRingCalls`,
    /// `supportsRoomCalls`, `supportsScheduledCalls` and
    /// `supportedMediaTypes` from it. Return the value the Session object
    /// carries; a different value makes the handler contradict the
    /// advertised capability.
    fn vtc_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<VtcCapability, Self::Error>> + Send;

    /// `true` if the consumer advertises the capability `uri` (for example
    /// `urn:ietf:params:jmap:chat`) for `account_id`.
    ///
    /// The draft makes several VTCCall rules depend on a companion
    /// capability: a `chatId` on create without
    /// `urn:ietf:params:jmap:chat` is `invalidProperties`, and the
    /// `chatId` / `spaceId` query filters are `unsupportedFilter`.
    fn has_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        uri: &str,
    ) -> bool;

    /// The caller's userId: "The authenticated user identity string"
    /// (draft-atwood-jmap-vtc-00 §VTCParticipant `userId`).
    ///
    /// `VTCCall/set` create stores it as `initiatorId` ("The userId of the
    /// participant who created the call"), and ending a ring call compares
    /// it with `initiatorId`.
    fn caller_user_id(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<String, Self::Error>> + Send;

    /// Create a call.
    ///
    /// The handler has validated `call` against draft-atwood-jmap-vtc-00
    /// §VTCCall/set and filled the server-set properties except two: its
    /// `id` is a placeholder and its `joinUri` is empty. The backend
    /// MUST, in one atomic step:
    ///
    /// - assign a ULID `id` ("VTCCall IDs are ULIDs ... assigned by the
    ///   server at the time the call is created") and a `joinUri` ("The
    ///   server generates `joinUri` pointing to the deployment's media
    ///   stack");
    /// - when `call.chat_id` names a Chat already referenced by a call in
    ///   state `"active"` or `"ringing"`, refuse with
    ///   [`call_already_active_on_chat`];
    /// - for a ring call (`call.state` is `"creating"`), create a
    ///   VTCParticipant record for each of `ring_targets` (`joinedAt`
    ///   `null`, `callResponse` `"pending"`) and one for the initiator
    ///   with `role` `"moderator"` ("The call initiator receives
    ///   `"moderator"` by default"), dispatch the ring notifications, and
    ///   store the call in state `"ringing"` ("Server transitions
    ///   automatically when the call is created and ring notifications
    ///   have been dispatched"). `ring_targets` is empty for other call
    ///   types;
    /// - for a breakout room (`call.parent_call_id` set), append the new
    ///   id to the parent's `breakoutRoomIds` ("The server adds the new
    ///   call's id to the parent's `breakoutRoomIds`") and advance the
    ///   parent's state.
    ///
    /// Returns the stored call. Error mapping, state-counter and change-log
    /// rules are those of the canonical `jmap-mail-server`
    /// `create_object`: the state advances only when the create commits.
    fn create_call(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        call: VtcCall,
        ring_targets: &[String],
    ) -> impl std::future::Future<Output = Result<VtcCall, BackendSetError<Self::Error>>> + Send;

    /// Apply an RFC 8620 §5.3 PatchObject to an existing object.
    ///
    /// `Ok(Some(o))` echoes server-changed properties; `Ok(None)` means the
    /// patch applied verbatim. Both MUST advance the state and record an
    /// `updated` change. Unknown `id` is `notFound`; a malformed patch is
    /// `invalidPatch`; a result that no longer decodes is
    /// `invalidProperties`. Apply the whole patch or none of it.
    ///
    /// When a `VTCCall` patch moves a breakout room (`parentCallId` set) to
    /// `"ended"`, the backend MUST also remove its id from the parent's
    /// `breakoutRoomIds` in the same atomic step ("The server removes its
    /// id from the parent's `breakoutRoomIds`").
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id. Unknown `id` is `notFound`; the state
    /// advances only when the destroy commits.
    ///
    /// The handler has already applied the permission check. When the
    /// destroyed `VTCCall` is a breakout room (`parentCallId` set), the
    /// backend MUST also remove its id from the parent's `breakoutRoomIds`
    /// in the same atomic step, as for closing one via `update_object`.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// `true` if the caller is a moderator on `call`
    /// (draft-atwood-jmap-vtc-00 §Moderator Actions on VTCCall): its
    /// VTCParticipant record has `role` `"moderator"`, or the deployment
    /// grants moderator status by policy.
    ///
    /// `VTCCall/set` returns `forbidden` to a non-moderator who ends a
    /// call, changes `lobbyEnabled`, `policy` or `e2eeEnabled`, or creates
    /// a breakout room under this call.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(true)`: every caller is a moderator. This is the
    /// workspace's fail-open posture for permission hooks (bd:JMAP-bghk);
    /// production backends override it.
    fn is_moderator(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        call: &VtcCall,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, call);
        async { Ok(true) }
    }

    /// Per-backend capability probe: `true` if this backend supports the
    /// JMAP object type `O` for `account_id`. Not called by the handlers;
    /// consumers use it when assembling their Session capabilities.
    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool;
}
