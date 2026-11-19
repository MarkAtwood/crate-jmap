//! JMAP Email Delivery Push (draft-ietf-jmap-emailpush-05), server side.
//!
//! EmailPush adds an `emailPush` map (account id to
//! [`EmailPushConfig`](jmap_mail_types::EmailPushConfig)) to the RFC 8620
//! PushSubscription. When a message is delivered to an account in that map,
//! the server pushes an [`EmailPush`](jmap_mail_types::EmailPush) to the
//! subscription's URL, holding the matching Emails with the requested
//! properties.
//!
//! This module adds no methods and no `register_*` function: EmailPush
//! rides on `PushSubscription/get` and `/set`, registered with
//! [`jmap_server::register_push_subscription_handlers`]. A deployment
//! implements [`EmailPushBackend`] on top of its
//! [`PushSubscriptionBackend`], lists
//! `(EMAIL_PUSH_PROPERTY, JMAP_EMAILPUSH_URI)` in
//! [`PushSubscriptionBackend::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES`],
//! and owns transport and delivery.
//!
//! [`EmailPushBackend`] does not extend [`MailBackend`](crate::MailBackend):
//! -05 §2.1 lets an account support EmailPush without
//! `urn:ietf:params:jmap:mail`, so an IMAP-only deployment implements only
//! the push traits.
//!
//! Gated by the `emailpush` feature.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use jmap_server::PushSubscriptionBackend;
use jmap_types::Id;

mod validate;

pub use crate::filter::EmailCondition;
pub use validate::{InvalidEmailPushConfig, check_email_push_change, check_email_push_config};

/// What one account supports for EmailPush, as the deployment reports it
/// from [`EmailPushBackend::email_push_accounts`].
///
/// The flags decide which `filter` conditions and `properties` an
/// EmailPushConfig for the account may use (-05 §4):
///
/// - with [`mail`](Self::mail), every condition Email/query supports and
///   every property Email/get supports;
/// - without it, the ten conditions and eight properties -05 §4 lists,
///   plus `inMailbox` / `inMailboxOtherThan` and `id` / `threadId` /
///   `mailboxIds` with [`object_ids`](Self::object_ids), plus the declared
///   [`filter_conditions`](Self::filter_conditions) and
///   [`properties`](Self::properties).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmailPushAccount {
    /// The account supports `urn:ietf:params:jmap:mail`. Without it the
    /// pushed `state` is `null` (-05 §3).
    pub mail: bool,
    /// The deployment offers IMAP access to the account with the ObjectID
    /// extension (RFC 8474), so Email, Thread and Mailbox ids exist.
    pub object_ids: bool,
    /// The account's object-history capability lists `Email` in
    /// `supportedTypes`, so its Email/get accepts `objectHistory`
    /// (draft-ietf-jmap-object-history-03 §3.1). Then `objectHistory` is
    /// accepted in a config's `properties` and dropped from the stored list;
    /// otherwise it is an unsupported property.
    pub email_history: bool,
    /// Conditions supported beyond the -05 §4 minimum, for an account
    /// without [`mail`](Self::mail). Ignored with `mail`.
    pub filter_conditions: BTreeSet<EmailCondition>,
    /// Email property names supported beyond the -05 §4 minimum, for an
    /// account without [`mail`](Self::mail). Ignored with `mail`.
    pub properties: BTreeSet<String>,
}

impl EmailPushAccount {
    /// An account with the given `urn:ietf:params:jmap:mail` support,
    /// without ObjectID, without Email history and with no extra
    /// conditions or properties.
    pub fn new(mail: bool) -> Self {
        Self {
            mail,
            ..Self::default()
        }
    }

    /// Returns `self` with [`object_ids`](Self::object_ids) set.
    #[must_use]
    pub fn with_object_ids(mut self, object_ids: bool) -> Self {
        self.object_ids = object_ids;
        self
    }

    /// Returns `self` with [`email_history`](Self::email_history) set.
    #[must_use]
    pub fn with_email_history(mut self, email_history: bool) -> Self {
        self.email_history = email_history;
        self
    }

    /// Returns `self` with [`filter_conditions`](Self::filter_conditions)
    /// set.
    #[must_use]
    pub fn with_filter_conditions(
        mut self,
        conditions: impl IntoIterator<Item = EmailCondition>,
    ) -> Self {
        self.filter_conditions = conditions.into_iter().collect();
        self
    }

    /// Returns `self` with [`properties`](Self::properties) set.
    #[must_use]
    pub fn with_properties<S: Into<String>>(
        mut self,
        properties: impl IntoIterator<Item = S>,
    ) -> Self {
        self.properties = properties.into_iter().map(Into::into).collect();
        self
    }
}

/// Why EmailPush objects could not be built for a delivery.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailPushError {
    /// An EmailPush holding one empty Email does not fit in the size limit,
    /// so no EmailPush can be sent without exceeding it (-05 §3: "The server
    /// MUST NOT exceed the size limit of the push service").
    LimitTooSmall,
    /// The filter has a thread condition (`allInThreadHaveKeyword`,
    /// `someInThreadHaveKeyword`, `noneInThreadHaveKeyword`) and the
    /// delivered Email came without its Thread's keywords.
    MissingThreadKeywords,
}

impl std::fmt::Display for EmailPushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::LimitTooSmall => "the push size limit cannot hold an EmailPush",
            Self::MissingThreadKeywords => "a thread filter condition needs the Thread's keywords",
        })
    }
}

impl std::error::Error for EmailPushError {}

/// A [`PushSubscriptionBackend`] whose accounts support EmailPush.
///
/// # Consumer MUSTs
///
/// - `create_push_subscription` and `update_push_subscription` MUST call
///   [`check_email_push_change`] before storing, and store the normalized
///   result. PushSubscriptionBackend has no validation hook, so the backend
///   does this itself, in the same call as the store.
/// - Each delivered Email is built from a full
///   [`Email`](jmap_mail_types::Email), after delivery-time processing such
///   as Sieve (-05 §4: the filter "is evaluated against the message as it is
///   immediately after delivery"). A deployment without ObjectID (RFC 8474)
///   has no JMAP `id`, `blobId`, `threadId` or `mailboxIds`; it fills them
///   with any valid placeholder. For such an account (`mail` and
///   `object_ids` both false) those properties and `inMailbox` /
///   `inMailboxOtherThan` are rejected, so a placeholder is never pushed or
///   matched.
/// - EmailPush objects are built only for SMTP/LMTP-style deliveries (-05
///   §3: not for an Email "created as the result of a JMAP method call or
///   the equivalent in another protocol"), with the live
///   [`EmailPushAccount`].
/// - Each EmailPush is sent only to the subscription's URL, never over an
///   EventSource or WebSocket push channel (-05 §4), and within the push
///   service's size limit (Web Push without padding allows
///   `jmap_web_push::MAX_PLAINTEXT` = 3993 octets).
/// - When the user loses access to an account, or the account loses the
///   emailpush capability, the backend MUST stop sending EmailPush objects
///   for it and remove it from every affected `emailPush` map (-05 §4).
pub trait EmailPushBackend: PushSubscriptionBackend {
    /// The accounts `caller` can access whose `accountCapabilities` include
    /// `urn:ietf:params:jmap:emailpush`, with what each supports.
    ///
    /// MUST return exactly the accounts the Session advertises with the
    /// emailpush account capability (-05 §2.1): a key in a config's
    /// `emailPush` map that is not one of them is rejected (-05 §4).
    fn email_push_accounts(
        &self,
        caller: &Self::CallerCtx,
    ) -> impl Future<Output = Result<BTreeMap<Id, EmailPushAccount>, Self::Error>> + Send;
}
