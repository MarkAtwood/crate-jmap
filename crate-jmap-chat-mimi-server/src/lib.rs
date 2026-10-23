//! JMAP Chat MIMI provider server rules (draft-atwood-jmap-chat-mimi-00).
//!
//! # Usage
//!
//! Implement [`MimiBackend`] for your storage layer, then call
//! [`register_mimi_handlers`] to wire the method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_chat_mimi_server::{MimiBackend, register_mimi_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: MimiBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_mimi_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # Scope
//!
//! `KeyPackage/get` and `KeyPackage/set` (draft §KeyPackages): the
//! per-KeyPackage create checks in [`check_key_package_create`], parsed and
//! verified with openmls, and the account-wide rules the backend applies
//! atomically in [`MimiBackend::create_key_package`].
//!
//! KeyPackage hand-out (draft §Handing Out KeyPackages):
//! [`MimiBackend::claim_key_packages`] claims KeyPackages atomically,
//! judged by [`KeyMaterialRequirements::fit`].
//!
//! `Room/create` (draft §Room/create): the seven checks, those that need
//! no server state in [`check_room_create`] and the rest in
//! [`MimiBackend::create_room`], which also creates the bound Chat.
//!
//! Room-bound Chats (draft §Chat.roomId, §message/mls):
//! [`may_update_room_chat`], [`may_create_room_message`] and
//! [`may_update_room_message`] are the rules a `jmap-chat-server`
//! `ChatBackend` returns from its `may_update_chat`, `may_create_message`
//! and `may_update_message` hooks.
//!
//! Body types: that `ChatBackend`'s `limits` lists
//! [`jmap_chat_mimi_types::MIMI_FANOUT_BODY_TYPE`] in
//! `ChatLimits::supported_body_types` (draft §Account-Level Capability
//! Object: the server "MUST include
//! `"application/mimi-fanout"` in `supportedBodyTypes`"), in
//! `ChatLimits::server_reserved_body_types` ("Only servers create such
//! Messages; the server MUST reject a `Message/set` create with this body
//! type with an `invalidProperties` SetError naming `bodyType`", draft
//! §application/mimi-fanout) and in `ChatLimits::encrypted_body_types`
//! ("It is an encrypted body type for the purposes of" JMAP Chat, draft
//! §MLS Messages). `jmap-chat-server` then refuses a
//! client's fanout create in every Chat, room-bound or not.
//!
//! FanoutMessages (draft §application/mimi-fanout, §Chat.roomId): a
//! `/notify` handler rejects a sender that [`notify_from_hub`] says is not
//! the room's hub, splits the body with [`parse_fanout_messages`] and
//! hands each [`Fanout`] to [`MimiBackend::store_fanout`], which routes
//! Welcomes, drops duplicates and attributes Messages to the hub's
//! ChatContact. [`MimiBackend::unbind_room`] ends a Chat's binding.
//!
//! `KeyMaterial/fetch`, `Room/update`, `Room/send`, `Room/unbind` and
//! `GroupInfo/fetch` (draft §KeyMaterial/fetch, §Rooms, §GroupInfo/fetch):
//! [`handle_key_material_fetch`] answers a local target in a room this
//! server hosts from [`MimiBackend::claim_key_packages`]; it and the
//! others reach the hub, or the target provider, through
//! [`MimiBackend::mimi_request`], where [`HubUnavailable`] is
//! `serverUnavailable`. `Room/update` and `Room/send` store the accepted
//! FanoutMessage with [`MimiBackend::store_fanout`].
//!
//! # Public dependency on openmls
//!
//! `openmls` 0.x types appear in this crate's public API
//! ([`MimiBackend::Crypto`] is an `OpenMlsCrypto`), so the crate re-exports
//! [`openmls`] and [`openmls_rust_crypto`]. Name them through this crate to
//! get the versions it was built with, and expect a major release of this
//! crate whenever it moves to a new `openmls` minor version:
//!
//! ```rust
//! use jmap_chat_mimi_server::openmls::prelude::OpenMlsCrypto;
//! use jmap_chat_mimi_server::openmls_rust_crypto::RustCrypto;
//!
//! fn provider<C: OpenMlsCrypto>(crypto: C) -> C {
//!     crypto
//! }
//! let _crypto = provider(RustCrypto::default());
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose `memory::MemoryBackend`, the
//! in-memory reference implementation of [`MimiBackend`] used by this
//! crate's tests. **Not production.** Its API may break in any minor
//! release while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, JmapHandler};

mod backend;
mod fanout;
mod handout;
mod hub;
mod keypackage;
/// In-memory reference implementation of [`MimiBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
mod restrict;
mod room;
mod wire;

pub use backend::{
    HubReply, HubUnavailable, MimiBackend, MimiEndpoint, NewKeyPackage, NewRoom, group_id_in_use,
    key_package_over_quota, key_package_ref_in_use, room_id_in_use, room_signature_key_not_bound,
    signature_key_bound_elsewhere,
};
pub use fanout::{
    Fanout, FanoutParseError, FanoutRecipients, notify_from_hub, parse_fanout_messages,
};
pub use handout::{KeyMaterialRequirements, KeyPackageFit};
pub use hub::{
    handle_group_info_fetch, handle_key_material_fetch, handle_room_send, handle_room_unbind,
    handle_room_update,
};
pub use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult,
    SetError, SetErrorType, SetObject,
};
pub use keypackage::{check_key_package_create, handle_key_package_get, handle_key_package_set};
pub use restrict::{may_create_room_message, may_update_room_chat, may_update_room_message};
pub use room::{check_room_create, handle_room_create};

/// Capability URI for `urn:ietf:params:jmap:chat:mimi`.
pub use jmap_chat_mimi_types::JMAP_CHAT_MIMI_URI;
pub use openmls;
pub use openmls_rust_crypto;

/// Register the `KeyPackage/get`, `KeyPackage/set`, `KeyMaterial/fetch`,
/// `Room/create`, `Room/update`, `Room/send`, `Room/unbind` and
/// `GroupInfo/fetch` handlers on `dispatcher`.
///
/// Advertise [`JMAP_CHAT_MIMI_URI`] in your Session capabilities, with the
/// same account-level object [`MimiBackend::mimi_capability`] returns.
pub fn register_mimi_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MimiBackend + 'static,
{
    macro_rules! reg {
        ($method:expr, $handler:ident) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        call_id: _ci,
                        args: a,
                        caller: ctx,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &ctx, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("KeyPackage/get", handle_key_package_get);
    reg!("KeyPackage/set", handle_key_package_set);
    reg!("KeyMaterial/fetch", handle_key_material_fetch);
    reg!("Room/create", handle_room_create);
    reg!("Room/update", handle_room_update);
    reg!("Room/send", handle_room_send);
    reg!("Room/unbind", handle_room_unbind);
    reg!("GroupInfo/fetch", handle_group_info_fetch);
}
