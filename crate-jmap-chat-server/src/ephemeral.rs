//! Ephemeral-stream subscription state (draft-atwood-jmap-chat-wss-00
//! §ChatStreamEnable).
//!
//! A WebSocket transport keeps one [`EphemeralSubscription`] per
//! connection, starting from [`EphemeralSubscription::default`] (nothing
//! subscribed). Each `ChatStreamEnable` frame goes through
//! [`EphemeralSubscription::replace_with`], which validates the frame,
//! normalizes it, and replaces the whole subscription. A `ChatStreamDisable`
//! frame resets it to the default.

use jmap_chat_types::ChatStreamEnable;
use jmap_types::{Id, JmapError};

/// Which ids a subscribed data type covers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdScope {
    /// Every id: the frame sent `null` or left the field out.
    All,
    /// Only the listed ids, in the order the frame sent them.
    Only(Vec<Id>),
}

impl From<Option<&Vec<Id>>> for IdScope {
    /// `None` (absent or JSON `null`) maps to [`IdScope::All`]: "An absent
    /// `chatIds` or `contactIds` is equivalent to `null`"
    /// (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable).
    fn from(ids: Option<&Vec<Id>>) -> Self {
        ids.map_or(Self::All, |ids| Self::Only(ids.clone()))
    }
}

/// The ephemeral subscription of one WebSocket connection, normalized
/// from a `ChatStreamEnable` frame.
///
/// The default value subscribes to nothing, which is the state before the
/// first `ChatStreamEnable` and after a `ChatStreamDisable`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EphemeralSubscription {
    typing: Option<IdScope>,
    presence: Option<IdScope>,
}

impl EphemeralSubscription {
    /// Validates and normalizes a `ChatStreamEnable` frame.
    ///
    /// Only `"typing"` and `"presence"` in `dataTypes` take effect; other
    /// values are ignored. `chatIds` applies only when `"typing"` is
    /// present and `contactIds` only when `"presence"` is present.
    ///
    /// # Errors
    ///
    /// Returns an `invalidArguments` [`JmapError`] when `dataTypes` is
    /// empty or holds neither `"typing"` nor `"presence"`. The transport
    /// sends it to the client as an RFC 8887 §4.3.4 `RequestError` frame.
    pub fn from_enable(enable: &ChatStreamEnable) -> Result<Self, JmapError> {
        let has = |tag: &str| enable.data_types.iter().any(|t| t == tag);
        let subscription = Self {
            typing: has("typing").then(|| IdScope::from(enable.chat_ids.as_ref())),
            presence: has("presence").then(|| IdScope::from(enable.contact_ids.as_ref())),
        };
        if subscription == Self::default() {
            return Err(JmapError::invalid_arguments(
                "dataTypes must contain \"typing\" or \"presence\"",
            ));
        }
        Ok(subscription)
    }

    /// Replaces this subscription with the one in `enable`.
    ///
    /// The new frame replaces the old subscription in full; nothing is
    /// merged from it (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable).
    ///
    /// # Errors
    ///
    /// Returns the same error as [`Self::from_enable`]. On error `self` is
    /// unchanged, as the draft requires.
    pub fn replace_with(&mut self, enable: &ChatStreamEnable) -> Result<(), JmapError> {
        *self = Self::from_enable(enable)?;
        Ok(())
    }

    /// The Chats covered for typing events, or `None` when typing is not
    /// subscribed. [`IdScope::All`] means every Chat of which the owner is
    /// a current member.
    pub fn typing(&self) -> Option<&IdScope> {
        self.typing.as_ref()
    }

    /// The ChatContacts covered for presence events, or `None` when
    /// presence is not subscribed. [`IdScope::All`] means every known
    /// ChatContact.
    pub fn presence(&self) -> Option<&IdScope> {
        self.presence.as_ref()
    }
}
