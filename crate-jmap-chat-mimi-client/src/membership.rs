//! draft-atwood-jmap-chat-mimi-00 §Chat.roomId — when the account has
//! left a room's MLS group.

use openmls::prelude::{BasicCredential, Member};

/// Whether the account has left the room's MLS group: `true` when no leaf
/// among `members` has a Basic credential whose identity is `user_uri`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Chat.roomId: "The account leaves
/// the room's MLS group when a commit removes from the group every leaf
/// whose credential identity is the account's `userUri`", and "a client
/// that processes a commit removing the last of its account's clients
/// SHOULD then call `Room/unbind`".
///
/// Pass the members of the epoch the commit creates, for example
/// `group.members()` of an openmls `MlsGroup` after merging it, and the
/// `userUri` of the account's [`MimiCapability`](crate::MimiCapability).
/// When this returns `true`, call
/// [`SessionClient::room_unbind`](crate::SessionClient::room_unbind).
/// A leaf with a non-Basic credential is not one of the account's clients:
/// the server accepts only Basic credentials in the account's
/// KeyPackages (draft §KeyPackage/set).
pub fn account_has_left(members: impl IntoIterator<Item = Member>, user_uri: &str) -> bool {
    !members.into_iter().any(|member| {
        BasicCredential::try_from(member.credential)
            .is_ok_and(|credential| credential.identity() == user_uri.as_bytes())
    })
}
