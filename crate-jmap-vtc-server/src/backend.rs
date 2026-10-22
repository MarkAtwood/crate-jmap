//! VtcBackend trait and supporting types for the JMAP VTC method handlers.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. This module adds the write
//! operations and the deployment facts the VTCCall handlers consult.
//!
//! The crate root re-exports this module's public items and the
//! [`jmap_server`] backend types a downstream impl needs.

use jmap_server::{
    BackendSetError, GetObject, JmapBackend, JmapObject, SetError, SetErrorType, SetObject,
};

use jmap_vtc_types::{
    CallState, LivestreamState, RecordingState, VtcCall, VtcCapability, VtcEvent, VtcLivestream,
    VtcRecording, VtcRecordingStateEvent,
};

/// A VTC record whose `state` follows a draft-atwood-jmap-vtc-00 state
/// machine: `VTCCall` (§State Machine), `VTCRecording` (§Pausing,
/// Resuming, and Stopping a Recording) and `VTCLivestream` (§Stopping a
/// Livestream). [`VtcBackend::update_object_if`] compares this state.
pub trait StatefulObject: SetObject {
    /// The record's `state` type.
    type State: Clone + PartialEq + Send + Sync;

    /// The record's current `state`.
    fn state(&self) -> &Self::State;
}

impl StatefulObject for VtcCall {
    type State = CallState;

    fn state(&self) -> &CallState {
        &self.state
    }
}

impl StatefulObject for VtcRecording {
    type State = RecordingState;

    fn state(&self) -> &RecordingState {
        &self.state
    }
}

impl StatefulObject for VtcLivestream {
    type State = LivestreamState;

    fn state(&self) -> &LivestreamState {
        &self.state
    }
}

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
/// # Per-call access
///
/// The handlers read every VTC record through `get_objects`, so the
/// backend's read path is the access check. For each caller,
/// `get_objects`, `query_objects` and `query_changes` MUST treat a
/// `VTCCall` the caller has no access to, and the `VTCParticipant`,
/// `VTCRecording` and `VTCLivestream` records of that call, as absent: a
/// requested id goes to the not-found list and a full fetch or query
/// leaves it out. Every method then answers `notFound`, as
/// draft-atwood-jmap-vtc-00 requires: "Non-members SHOULD receive
/// `notFound` for `VTCCall/get` and `VTCParticipant/get` requests on the
/// call" (§Chat-Bound Calls), "Non-participants receive `notFound` for
/// calls they cannot access" (§Standalone Calls), "The server SHOULD
/// return `notFound` if the `callId` does not exist or the caller does
/// not have access to it" (§Joining a Room or Scheduled Call), and "The
/// query SHOULD return only calls the authenticated user has access to"
/// (§VTCCall/query). Who has access is the deployment's: a Chat binding
/// applies the bound Chat's membership, `"view"` permission and the
/// initiator's blocks; a standalone call admits its participants and
/// initiator. A backend with no rule grants access (bd:JMAP-bghk).
/// [`Self::get_objects_unscoped`] is the one read exempt from this rule.
///
/// For a call with `e2eeEnabled: true` the same reads MUST also hide its
/// `VTCParticipant` records from a user with no record in it: "For such
/// users the server SHOULD return `notFound` for `VTCParticipant/get` and
/// exclude the records from `VTCParticipant/query`" (§Participant
/// Visibility). `get_changes` is unaffected: "`VTCParticipant/changes`
/// keeps reporting changes for the account as a whole".
///
/// The backend is also the only writer of `speakerTimeMs` after a join.
/// For an E2EE call it SHOULD NOT map media streams to participants and
/// then keeps the value at `0`: "it sets `speakerTimeMs` to `0` as that
/// field's definition requires" (§Active Speaker and Speaker Time).
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
    /// The handler has validated the call against draft-atwood-jmap-vtc-00
    /// §VTCCall/set and filled the server-set properties except two: `id`,
    /// which `build` sets to the id it is given, and `joinUri`, which is
    /// empty. The backend MUST, in one atomic step:
    ///
    /// - mint a ULID `id` ("VTCCall IDs are ULIDs ... assigned by the
    ///   server at the time the call is created"), call `build(&id)` once
    ///   for the call carrying it, and assign a `joinUri` ("The server
    ///   generates `joinUri` pointing to the deployment's media stack");
    /// - when `call.chat_id` names a Chat already referenced by a call in
    ///   state `"active"` or `"ringing"`, refuse with
    ///   [`call_already_active_on_chat`];
    /// - for a ring call (`call.state` is `"creating"`), create a
    ///   VTCParticipant record for each of `ring_targets` (`joinedAt`
    ///   `null`, `callResponse` `"pending"`) and one for the initiator
    ///   with `role` `"moderator"` ("The call initiator receives
    ///   `"moderator"` by default") and `joinedAt` equal to
    ///   `call.created_at` ("with `joinedAt` set to the creation time,
    ///   since the initiator is in the call from the start"), set the
    ///   call's `activeParticipantCount` to 1, dispatch the ring
    ///   notifications, and
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
    fn create_call<F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        build: F,
        ring_targets: &[String],
    ) -> impl std::future::Future<Output = Result<VtcCall, BackendSetError<Self::Error>>> + Send
    where
        F: FnOnce(&jmap_types::Id) -> VtcCall + Send;

    /// Create a `VTCRecording`, `VTCLivestream` or `VTCParticipant`.
    ///
    /// The handler has validated the create against draft-atwood-jmap-vtc-00
    /// §Starting a Recording, §Starting a Livestream, §Joining a Room or
    /// Scheduled Call or §Gateway Dial-Out and filled its server-set
    /// properties other than `id`. The backend MUST, in one atomic step,
    /// mint the `id` ("Opaque server-assigned identifier"), call
    /// `build(&id)` once for the object carrying it, and store that
    /// object. In the same step:
    ///
    /// - for a recording or livestream, refuse with `invalidProperties`
    ///   naming `callId` when the object's `callId` names no call in state
    ///   `"active"` ("The server MUST return `invalidProperties` naming
    ///   `callId` when the call is not in state `"active"`", rechecked here
    ///   because the call can end after the handler read it). A livestream
    ///   backend then starts the outbound stream;
    /// - for a participant with a non-null `userId`, refuse with
    ///   `alreadyExists` whose `existingId` names that user's record when
    ///   the call already has one ("A `create` for a user who already has a
    ///   record in the call fails with an `alreadyExists` SetError ...
    ///   whose `existingId` names that record", §Reconnecting);
    /// - for a participant, keep the call's `activeParticipantCount` equal
    ///   to its participants with `joinedAt` set and `leftAt` `null`, and
    ///   refuse with `overQuota` a record that would raise it above
    ///   `maxParticipantsPerCall`, as for [`Self::update_object`];
    /// - for a participant whose `displayName` is its `userId`, the
    ///   handler's placeholder, the backend MAY substitute the name "from
    ///   the user profile or `ChatContact` record";
    /// - for a dial-out participant (`userId` `null`, `joinedAt` `null`,
    ///   `gatewayData` the outbound addressing), initiate the outbound call
    ///   through the gateway of its `joinMethod`. Later, "When the external
    ///   party answers, the server sets `joinedAt`. If the outbound call
    ///   fails or is not answered, the server sets `leftAt` and records the
    ///   gateway-specific failure reason in `gatewayData`".
    ///
    /// Returns the assigned id and the stored object. `create_id` is the
    /// client's creation id. Error mapping, state-counter and change-log
    /// rules are those of the canonical `jmap-mail-server` `create_object`:
    /// the state advances only when the create commits.
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

    /// Deliver `event` to every participant in `event.call_id`
    /// (draft-atwood-jmap-vtc-00 §VTCRecordingStateEvent: "Delivered to all
    /// participants when recording state changes. This is the consent
    /// signal").
    ///
    /// `VTCRecording/set` calls this after each create, pause, resume and
    /// stop has committed. §Recording Consent says "The server SHOULD
    /// notify all participants when recording state changes" and
    /// "deployments subject to all-party consent laws rely on this signal".
    /// There is no default, so that a backend skips the consent signal only
    /// by choosing to, never by inheriting a silent no-op. Delivery
    /// failures are the backend's to log or retry; the recording change has
    /// already committed.
    fn deliver_recording_state_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        event: VtcRecordingStateEvent,
    ) -> impl std::future::Future<Output = ()> + Send;

    /// Apply an RFC 8620 §5.3 PatchObject to an existing object.
    ///
    /// `Ok(Some(o))` echoes server-changed properties; `Ok(None)` means the
    /// patch applied verbatim. Both MUST advance the state and record an
    /// `updated` change. Unknown `id` is `notFound`; a malformed patch is
    /// `invalidPatch`; a result that no longer decodes is
    /// `invalidProperties`. Apply the whole patch or none of it.
    ///
    /// When a `VTCCall` patch moves a breakout room (`parentCallId` set) to
    /// `"ended"`, the backend MUST also, in the same atomic step, remove its
    /// id from the parent's `breakoutRoomIds` and reset to `null` every
    /// `breakoutCallId` naming it on the parent call's participants ("The
    /// server removes its id from the parent's `breakoutRoomIds` and resets
    /// to `null` any `breakoutCallId` on the parent call's participants
    /// that names it").
    ///
    /// For a `VTCParticipant` patch the backend MUST, in the same atomic
    /// step, keep the call's `activeParticipantCount` equal to "Number of
    /// participants currently in the call (joined and not yet left)" and
    /// refuse with `overQuota` a patch that would raise it above
    /// `maxParticipantsPerCall` ("If the call's `activeParticipantCount` has
    /// reached `maxParticipantsPerCall`, the server MUST return
    /// `overQuota`").
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// [`Self::update_object`] with a compare-and-set precondition: apply
    /// `patch` only if the stored object's `state` is one of `expected`,
    /// checked in the same atomic step as the write.
    ///
    /// The handlers send every `state` change of a `VTCCall`,
    /// `VTCRecording` or `VTCLivestream` through this, with `expected` the
    /// states from which the draft lists the transition. A check against an
    /// earlier read would let two concurrent updates both pass, and the
    /// second would then apply a transition the draft forbids: "The server
    /// MUST NOT transition a call out of `"ended"`" (§State Machine), "The
    /// server MUST return `invalidProperties` naming `state` for any state
    /// transition not listed above" (§Pausing, Resuming, and Stopping a
    /// Recording).
    ///
    /// Returns `Ok(Ok(_))` with [`Self::update_object`]'s result when the
    /// patch applied, and `Ok(Err(state))` with the stored state, changing
    /// nothing and advancing no state, when that state is not in
    /// `expected`. Every other outcome, and every side effect of the patch,
    /// is [`Self::update_object`]'s.
    #[allow(clippy::type_complexity)]
    fn update_object_if<O: StatefulObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        expected: &[O::State],
        patch: O::Patch,
    ) -> impl std::future::Future<
        Output = Result<Result<Option<O>, O::State>, BackendSetError<Self::Error>>,
    > + Send;

    /// Destroy an object by id. Unknown `id` is `notFound`; the state
    /// advances only when the destroy commits.
    ///
    /// The handler has already applied the permission check. For a
    /// `VTCLivestream` the backend MUST stop the outbound stream first: "If
    /// the livestream is in state `"starting"` or `"live"`, the server first
    /// stops it, so that no orphaned stream keeps running after the metadata
    /// is removed" (§Destroying a Livestream). When the
    /// destroyed `VTCCall` is a breakout room (`parentCallId` set), the
    /// backend MUST also remove its id from the parent's `breakoutRoomIds`
    /// and reset the `breakoutCallId`s naming it in the same atomic step,
    /// as for closing one via `update_object`.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Deliver an event that `VTCParticipant/set` produces, after the
    /// change has committed:
    ///
    /// - [`VtcEvent::UnmuteRequest`] to the target participant's WebSocket
    ///   connections ("The server delivers a `VTCUnmuteRequestEvent` ... to
    ///   the target participant's WebSocket connections", §Participant-Level
    ///   Moderator Actions);
    /// - [`VtcEvent::GatewaySignal`] with `direction` `"outbound"` to the
    ///   gateway carrying the participant ("The server extracts the
    ///   signal, delivers it to the gateway, and removes the key",
    ///   §VTCGatewaySignal). The backend relays `signal` and `data` without
    ///   interpreting them (§Gateway Signal Injection);
    /// - [`VtcEvent::CallEnd`] with `endReason` `"answered_elsewhere"`,
    ///   after a target answers a ring, to that user's other devices that
    ///   received the ring ("The first device to answer wins: the server
    ///   accepts the first `VTCParticipant/set` update (setting
    ///   `joinMethod`), then dispatches `VTCCallEndEvent` with `endReason:
    ///   "answered_elsewhere"` to all other devices of that participant",
    ///   §Multi-Device Forking). `caller` is the answering request's
    ///   context, which identifies the device that answered;
    /// - [`VtcEvent::CallEnd`] with `endReason` `"declined"`, after a
    ///   target declines a ring, to that user's other devices that
    ///   received the ring ("When a ring call is answered or declined on
    ///   one device, the server SHOULD deliver a silent push notification
    ///   ... to all other devices that received the original
    ///   `VTCCallPush`", §VTCCallEndEvent). `caller` is the declining
    ///   request's context, which identifies the device that declined.
    ///
    /// [`crate::call::expire_ring`] also delivers [`VtcEvent::CallEnd`],
    /// with `endReason` `"timeout"`, to the devices of the call's targets
    /// whose `callResponse` is still `"pending"` (§Ring Timeout). A
    /// ringing call that ends `"cancelled"`, through `VTCCall/set`,
    /// [`crate::call::end_call`] or [`crate::participant::after_leave`],
    /// delivers [`VtcEvent::CallEnd`] with `endReason` `"cancelled"`, to
    /// the devices of the targets whose `callResponse` is still
    /// `"pending"` ("When a ringing call is answered, declined, cancelled,
    /// or times out, the server delivers a `VTCCallEndEvent` to all
    /// devices that received the ring", §VTCCallEndEvent).
    ///
    /// There is no default: each delivery is the effect the update
    /// requests. Delivery failures are the backend's to log or retry.
    fn deliver_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        event: VtcEvent,
    ) -> impl std::future::Future<Output = ()> + Send;

    /// `true` if the caller is a moderator on `call`
    /// (draft-atwood-jmap-vtc-00 §Moderator Actions on VTCCall): its
    /// VTCParticipant record has `role` `"moderator"`, or the deployment
    /// grants moderator status by policy.
    ///
    /// `VTCCall/set` returns `forbidden` to a non-moderator who ends a
    /// call, changes `lobbyEnabled`, `policy` or `e2eeEnabled`, or creates
    /// a breakout room under this call. `VTCRecording/set` and
    /// `VTCLivestream/set` return `forbidden` to a non-moderator for every
    /// create, update and destroy, and `VTCLivestream/get` returns
    /// `streamKey` and `streamUri` as `""` to a non-moderator.
    /// `VTCParticipant/set` gives a joining moderator `role` `"moderator"`
    /// and no lobby, and holds the participant-level moderator actions and
    /// gateway dial-out to it.
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

    /// The number of calls in `account_id` in state `"active"`, for
    /// `maxConcurrentCalls`: "If the account already has
    /// `maxConcurrentCalls` active calls, the server MUST return
    /// `overQuota`" (draft-atwood-jmap-vtc-00 §Joining a Room or Scheduled
    /// Call).
    ///
    /// The quota is the account's, so §Per-call access does not apply:
    /// the count MUST include the calls the caller cannot access, or a
    /// caller denied an active call could activate one beyond the limit.
    fn active_call_count(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<u64, Self::Error>> + Send;

    /// [`JmapBackend::get_objects`] without §Per-call access: the `O`
    /// records of `ids`, or every `O` record of `account_id` when `ids` is
    /// `None`, whether or not the caller may access their call. The second
    /// list holds the requested ids that are not stored.
    ///
    /// Ending a call reads its breakout rooms, and the recordings and
    /// livestreams of each ended call, through this: "when a parent call
    /// ends, its child breakout-room VTCCalls MUST also be ended"
    /// (jmap-vtc-implementer-guide §Breakout room cleanup), and "the server
    /// MUST stop every recording for that call" (jmap-vtc-recording-guide
    /// §Auto-stop on call end), including a room the ending moderator
    /// cannot access. The handlers return none of these records to a
    /// client.
    fn get_objects_unscoped<O: GetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ids: Option<&[jmap_types::Id]>,
    ) -> impl std::future::Future<Output = Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error>> + Send;
    /// `true` if `user_id` is blocked by the caller: the caller's
    /// ChatContact for `user_id` has `blocked` set to `true` (when
    /// `urn:ietf:params:jmap:chat` is present), or the user is on an
    /// equivalent deployment-defined block list
    /// (draft-atwood-jmap-vtc-wss-00 §Blocked-Sender Suppression).
    ///
    /// [`crate::ephemeral::should_deliver`] asks this at delivery time,
    /// with the recipient as the caller, to suppress ring and in-call
    /// events from a blocked user, and only when the recipient's own
    /// account advertises `urn:ietf:params:jmap:chat`
    /// ([`VtcBackend::has_capability`]). That gate deliberately narrows
    /// the VTC SHOULD, which also covers a deployment-defined block list
    /// without Chat (L344-348, L358-361 at 29e9a41; bd:JMAP-skk9.4): a
    /// deployment list answered here has no effect for a recipient whose
    /// account lacks Chat. The caller identifies whose
    /// contact list to read: "the recipient's contact list"
    /// (draft-atwood-jmap-vtc-wss-00 at 29e9a41, L347, L360), never the
    /// call's hosting account.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(false)`: no one is blocked. This is the workspace's
    /// fail-open posture for policy hooks (bd:JMAP-bghk); backends with a
    /// contact or block list override it.
    fn is_user_blocked(
        &self,
        caller: &Self::CallerCtx,
        user_id: &str,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, user_id);
        async { Ok(false) }
    }

    /// The deployment-defined ring timeout period
    /// (draft-atwood-jmap-vtc-00 §Ring Timeout), counted from a ring
    /// call's `createdAt`.
    ///
    /// [`crate::call::expire_ring`] enforces it on the call. After it,
    /// `VTCParticipant/set` returns `invalidProperties` naming `joinMethod`
    /// to a still-`"pending"` target's answer and naming `callResponse` to
    /// its decline: each target "may still answer (and join) or decline
    /// until the ring timeout expires" (§Multi-Device Forking).
    ///
    /// # Default implementation
    ///
    /// Returns [`crate::RING_TIMEOUT`], the RECOMMENDED 30 seconds.
    fn ring_timeout(&self) -> std::time::Duration {
        crate::RING_TIMEOUT
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
