//! MimiBackend trait and supporting types for the MIMI provider handlers.
//!
//! Read-side operations (`get_objects`, `get_state`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. This module adds the MLS crypto
//! provider, the account-level capability object, the KeyPackage writes,
//! the KeyPackage hand-out, rooms and FanoutMessage storage.
//!
//! The `pub use` block re-exports the [`jmap_server`] backend types so a
//! downstream impl can `use jmap_chat_mimi_server::backend::*`; each
//! re-export pins the upstream type's major-version contract into this
//! crate.

pub use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

use jmap_chat_mimi_types::{Base64Url, ClientKeyMaterial, MimiCapability};
use jmap_types::JmapError;
use openmls::prelude::OpenMlsCrypto;

use crate::fanout::Fanout;
use crate::handout::KeyMaterialRequirements;

/// A KeyPackage that passed the per-KeyPackage checks of
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set, ready for
/// [`MimiBackend::create_key_package`].
///
/// Built by [`crate::check_key_package_create`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewKeyPackage {
    /// The client's `keyPackage`, verbatim (§Conventions and Definitions:
    /// "Servers MUST NOT re-encode these octets").
    pub key_package: Base64Url,
    /// The KeyPackageRef of `key_package` (RFC 9420 §5.2), with its
    /// length prefix.
    pub key_package_ref: Base64Url,
    /// The `lastResort` value derived from `key_package`.
    pub last_resort: bool,
    /// The leaf node's signature key, which "identifies the client and
    /// binds the KeyPackage to the account".
    pub signature_key: Vec<u8>,
}

/// A room that passed the checks of draft-atwood-jmap-chat-mimi-00
/// §Room/create that need no server state, ready for
/// [`MimiBackend::create_room`].
///
/// Built by [`crate::check_room_create`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRoom {
    /// The room ID: the `room_uri` of the room metadata.
    pub room_id: String,
    /// The 16-octet MLS group ID.
    pub group_id: Vec<u8>,
    /// The `room_name` of the room metadata, or the empty string when it
    /// is not UTF-8.
    pub room_name: String,
    /// The signature key of the group's one leaf.
    pub signature_key: Vec<u8>,
    /// The client's `groupInfo`, verbatim.
    pub group_info: Base64Url,
    /// The client's `ratchetTree`, verbatim.
    pub ratchet_tree: Base64Url,
}

/// The `invalidArguments` error for a `Room/create` whose leaf signature
/// key is not bound to the account.
///
/// draft-atwood-jmap-chat-mimi-00 §Room/create check 2: the one leaf's
/// "signature key is bound to the account".
pub fn room_signature_key_not_bound() -> JmapError {
    JmapError::invalid_arguments("the leaf signature key is not bound to the account")
}

/// The `invalidArguments` error for a `Room/create` whose room ID a room
/// this server hosts already has.
///
/// draft-atwood-jmap-chat-mimi-00 §Room/create check 5: "no room this
/// server hosts has that room ID".
pub fn room_id_in_use() -> JmapError {
    JmapError::invalid_arguments("a room this server hosts has that room ID")
}

/// The `invalidArguments` error for a `Room/create` whose group ID a room
/// this server hosts already uses.
///
/// draft-atwood-jmap-chat-mimi-00 §Room/create check 6: "no room this
/// server hosts uses it".
pub fn group_id_in_use() -> JmapError {
    JmapError::invalid_arguments("a room this server hosts uses that group ID")
}

/// The `invalidProperties` SetError for a KeyPackage whose signature key
/// is bound to another account.
///
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set: "The server SHOULD
/// reject, with `invalidProperties` naming `keyPackage`, a KeyPackage whose
/// signature key is bound to another account."
pub fn signature_key_bound_elsewhere() -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties(["keyPackage"])
        .with_description("The KeyPackage signature key is bound to another account.")
}

/// The `invalidProperties` SetError for a KeyPackage whose KeyPackageRef
/// is already in use.
///
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set: "The server MUST reject,
/// with `invalidProperties` naming `keyPackage`, a KeyPackage whose
/// KeyPackageRef equals that of a KeyPackage it stores or has handed out".
pub fn key_package_ref_in_use() -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties(["keyPackage"])
        .with_description("A KeyPackage with this KeyPackageRef is stored or was handed out.")
}

/// The `overQuota` SetError for a create beyond `maxKeyPackages`.
///
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set: the server "MUST reject
/// a create that would exceed `maxKeyPackages` with `overQuota`".
pub fn key_package_over_quota() -> SetError {
    SetError::new(SetErrorType::OverQuota)
        .with_description("The account already stores maxKeyPackages KeyPackages.")
}

/// Storage backend for the MIMI provider handlers
/// (draft-atwood-jmap-chat-mimi-00).
///
/// This trait is not object-safe by design (associated type, generic
/// supertrait methods). Use `Arc<impl MimiBackend>` when sharing across
/// tasks.
pub trait MimiBackend: JmapBackend {
    /// The MLS crypto provider that verifies KeyPackage signatures and
    /// computes KeyPackageRefs, for example `openmls_rust_crypto::RustCrypto`.
    type Crypto: OpenMlsCrypto;

    /// The crypto provider. It needs no key material: the handlers only
    /// verify and hash.
    fn mls_crypto(&self) -> &Self::Crypto;

    /// The account-level `urn:ietf:params:jmap:chat:mimi` capability object
    /// (draft-atwood-jmap-chat-mimi-00 §Account-Level Capability Object) the
    /// consumer advertises for `account_id` to this caller.
    ///
    /// `KeyPackage/set` create reads `userUri`, `cipherSuites` and
    /// `maxKeyPackages` from it. Return the value the Session object
    /// carries; a different value makes the handler contradict the
    /// advertised capability.
    fn mimi_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<MimiCapability, Self::Error>> + Send;

    /// Store a KeyPackage record.
    ///
    /// The handler has run the per-KeyPackage checks. The backend MUST, in
    /// one atomic step, applying the first rule that fails:
    ///
    /// - refuse with [`signature_key_bound_elsewhere`] when
    ///   `key_package.signature_key` is bound to an account other than
    ///   `account_id`; otherwise bind it to `account_id` ("the first
    ///   KeyPackage the server accepts with a given signature key binds
    ///   that signature key to the account"). The draft says SHOULD; a
    ///   deployment that skips this check documents why;
    /// - refuse with [`key_package_ref_in_use`] when
    ///   `key_package.key_package_ref` equals that of a KeyPackage it
    ///   stores, in any account, or has handed out;
    /// - when `key_package.last_resort` is `true`, destroy the stored
    ///   last-resort record with the same signature key in `account_id`,
    ///   if any ("Creating a last-resort KeyPackage for a client destroys
    ///   that client's previous last-resort KeyPackage"), recording a
    ///   `destroyed` change;
    /// - refuse with [`key_package_over_quota`] when the account would then
    ///   hold more than `max_key_packages` records, last-resort records
    ///   included; nothing is destroyed or bound in that case;
    /// - assign the record `id` and store the record.
    ///
    /// Returns the assigned id. The state advances only when the create
    /// commits, as for the canonical `jmap-mail-server` `create_object`.
    fn create_key_package(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        key_package: NewKeyPackage,
        max_key_packages: u64,
    ) -> impl std::future::Future<Output = Result<jmap_types::Id, BackendSetError<Self::Error>>> + Send;

    /// Hand out the owner's KeyPackages for one key material request
    /// (draft-atwood-jmap-chat-mimi-00 §Handing Out KeyPackages).
    ///
    /// The backend MUST, in one atomic step, return one
    /// [`ClientKeyMaterial`] for each signature key bound to `account_id`
    /// (each of the owner's clients), judging each stored record with
    /// [`KeyMaterialRequirements::fit`] over [`MimiBackend::mls_crypto`]:
    ///
    /// - `success` with a compatible record that is not last-resort,
    ///   destroying that record so that "no KeyPackage is handed out
    ///   twice", recording a `destroyed` change and advancing the state
    ///   ("A destroyed record appears in a new state for the KeyPackage
    ///   type");
    /// - otherwise `success` with the client's compatible last-resort
    ///   record, which it keeps;
    /// - otherwise `nothingCompatible` when the client has a record that
    ///   is [`KeyPackageFit::Incompatible`](crate::KeyPackageFit), and
    ///   `keyMaterialExhausted` when it has none.
    ///
    /// Concurrent calls never return the same non-last-resort KeyPackage.
    ///
    /// `clientUri` is the backend's choice but "MUST be stable and unique
    /// per leaf": the same in every response for a given signature key and
    /// different for different signature keys.
    ///
    /// The backend MUST keep the KeyPackageRef of every KeyPackage it
    /// returns, with `account_id`, "until a Welcome naming it has been
    /// delivered or its leaf node lifetime has passed", and
    /// [`MimiBackend::create_key_package`] refuses that KeyPackageRef
    /// while it is kept.
    fn claim_key_packages(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        requirements: &KeyMaterialRequirements,
    ) -> impl std::future::Future<Output = Result<Vec<ClientKeyMaterial>, Self::Error>> + Send;

    /// Create a room with this server as hub and a Chat bound to it
    /// (draft-atwood-jmap-chat-mimi-00 §Room/create).
    ///
    /// The handler has run [`crate::check_room_create`]. The backend MUST,
    /// in one atomic step, applying the first rule that fails:
    ///
    /// - refuse with [`room_signature_key_not_bound`] unless
    ///   `room.signature_key` is bound to `account_id` by
    ///   [`MimiBackend::create_key_package`];
    /// - refuse with [`room_id_in_use`] when a room it hosts, in any
    ///   account, has `room.room_id`;
    /// - refuse with [`group_id_in_use`] when a room it hosts uses
    ///   `room.group_id`;
    /// - store the room and create a Chat in `account_id` "with `roomId`
    ///   set to the room ID" and returning its id. Per §Chat.roomId the
    ///   Chat "has kind `"group"`", its `name` is `room.room_name`, and its
    ///   `members` "is the empty list".
    ///
    /// The outer `Err` is a storage failure (`serverFail`).
    fn create_room(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        room: NewRoom,
    ) -> impl std::future::Future<Output = Result<Result<jmap_types::Id, JmapError>, Self::Error>> + Send;

    /// Store one FanoutMessage the server received or originated for
    /// `room_id` (draft-atwood-jmap-chat-mimi-00 §application/mimi-fanout).
    ///
    /// `hub_uri` is "the hub's provider URI as authenticated by the MIMI
    /// transport". Call it once per [`Fanout`] of
    /// [`crate::parse_fanout_messages`], in the order received. The
    /// backend MUST, in one atomic step:
    ///
    /// - choose the Chats: for [`FanoutRecipients::Room`], "every Chat of
    ///   a local account bound to that room"; for
    ///   [`FanoutRecipients::Welcome`], one Chat in each account that
    ///   [`MimiBackend::claim_key_packages`] handed out a named
    ///   KeyPackageRef for, binding that account's unbound Chat for the
    ///   room again or, "if it has none", creating a Chat bound to the
    ///   room, as [`MimiBackend::create_room`] does, with `name` the empty
    ///   string;
    /// - skip an account in which the ChatContact of the hub is
    ///   `blocked`: "blocking the hub drops every FanoutMessage of the
    ///   rooms it hosts", so nothing is stored, bound or created there;
    /// - skip a Chat that holds a Message with `fanout.sender_msg_id`
    ///   ("so a repeated `/notify` yields one Message") or one whose
    ///   FanoutMessage `message` octets equal `fanout.message`;
    /// - otherwise store a Message in the Chat with `bodyType`
    ///   `"application/mimi-fanout"`, `body` `fanout.body`, `senderMsgId`
    ///   `fanout.sender_msg_id`, `sentAt` `fanout.sent_at` and `senderId`
    ///   the id of the account's ChatContact "whose `identity` and `login`
    ///   are" `hub_uri`, creating that ChatContact if it does not exist.
    ///
    /// [`FanoutRecipients::Room`]: crate::FanoutRecipients::Room
    /// [`FanoutRecipients::Welcome`]: crate::FanoutRecipients::Welcome
    fn store_fanout(
        &self,
        room_id: &str,
        hub_uri: &str,
        fanout: &Fanout,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;

    /// Unbind the Chat in `account_id` whose `roomId` is `room_id`, if it
    /// is bound (draft-atwood-jmap-chat-mimi-00 §Chat.roomId).
    ///
    /// "An unbound Chat keeps its `roomId` and its Messages. The server
    /// stores no further FanoutMessage in it" until "a later Welcome for
    /// the account binds the Chat again". Returns `false` when no Chat in
    /// the account has that `roomId`.
    fn unbind_room(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        room_id: &str,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Destroy a KeyPackage record by id. Unknown `id` is `notFound`; the
    /// state advances only when the destroy commits. The signature-key
    /// binding stays.
    fn destroy_key_package(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;
}
