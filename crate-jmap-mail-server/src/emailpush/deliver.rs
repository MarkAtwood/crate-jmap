//! Building EmailPush objects for a delivery
//! (draft-ietf-jmap-emailpush-05 §3, §4).

use std::collections::BTreeMap;

use jmap_mail_types::{EMAIL_PUSH_PROPERTY, Email, EmailPush, EmailPushConfig, Keyword};
use jmap_server::{StoredPushSubscription, is_deliverable};
use jmap_types::{Id, State, Urgency};
use serde_json::{Map, Value};

use super::{EmailPushAccount, EmailPushError};
use crate::filter::{FilterError, email_matches};
use crate::project::project_email;

/// One message just delivered to an account, as the filter and the
/// projection see it.
///
/// `email` is the Email "as it is immediately after delivery, including any
/// mailboxes and keywords assigned by delivery-time processing such as
/// Sieve" (-05 §4). It carries `preview`, and decoded text `bodyValues`
/// when a `text` or `body` condition may run.
///
/// A deployment without ObjectID (RFC 8474) has no JMAP `id`, `blobId`,
/// `threadId` or `mailboxIds`. It fills those four required fields of
/// [`Email`] with any valid placeholder: for an account without mail and
/// without ObjectID, validation rejects those properties and the
/// `inMailbox` / `inMailboxOtherThan` conditions, and
/// [`email_pushes_for`] skips a stored config that uses them, so a
/// placeholder is never pushed or matched.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct DeliveredEmail<'a> {
    /// The delivered Email.
    pub email: &'a Email,
    /// The keywords of every Email in its Thread, this one included. A
    /// thread condition (`allInThreadHaveKeyword`, `someInThreadHaveKeyword`,
    /// `noneInThreadHaveKeyword`) needs them; for a mail account the
    /// deployment supplies them.
    pub thread_keywords: Option<&'a [&'a BTreeMap<Keyword, bool>]>,
}

impl<'a> DeliveredEmail<'a> {
    /// A delivered Email without its Thread's keywords.
    pub fn new(email: &'a Email) -> Self {
        Self {
            email,
            thread_keywords: None,
        }
    }

    /// Returns `self` with the keywords of every Email in its Thread.
    #[must_use]
    pub fn with_thread_keywords(mut self, keywords: &'a [&'a BTreeMap<Keyword, bool>]) -> Self {
        self.thread_keywords = Some(keywords);
        self
    }
}

/// The messages of one delivery batch to one account, and what the
/// EmailPush objects for them may hold.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct EmailPushBatch<'a> {
    /// The account the messages were delivered to.
    pub account_id: &'a Id,
    /// The account as [`EmailPushBackend::email_push_accounts`] returns it
    /// now for the subscription's owner; a revoked account has none, and
    /// gets no EmailPush (-05 §4: the server "MUST stop sending").
    ///
    /// [`EmailPushBackend::email_push_accounts`]: super::EmailPushBackend::email_push_accounts
    pub account: &'a EmailPushAccount,
    /// The delivered messages, in delivery order.
    pub emails: &'a [DeliveredEmail<'a>],
    /// The account's Email state after the whole batch (-05 §3: "made
    /// after all of the Emails in the "emails" array were created"). Pushed
    /// as `null` when the account has no mail capability, whatever is
    /// passed.
    pub state: Option<&'a State>,
    /// The limit on `serde_json::to_vec(&EmailPush).len()`, the serializer
    /// `jmap_web_push::Sender::send` uses. Web Push without padding allows
    /// `jmap_web_push::MAX_PLAINTEXT` = 3993 octets.
    pub max_bytes: usize,
}

impl<'a> EmailPushBatch<'a> {
    /// A batch of `emails` delivered to `account_id`.
    pub fn new(
        account_id: &'a Id,
        account: &'a EmailPushAccount,
        emails: &'a [DeliveredEmail<'a>],
        state: Option<&'a State>,
        max_bytes: usize,
    ) -> Self {
        Self {
            account_id,
            account,
            emails,
            state,
            max_bytes,
        }
    }
}

/// One EmailPush to send to a subscription's URL, with the Web Push
/// urgency its config asks for.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundEmailPush {
    /// The EmailPush object.
    pub push: EmailPush,
    /// The RFC 8030 §5.3 `Urgency` to send it with
    /// (`jmap_web_push::PushOptions::urgency`).
    pub urgency: Urgency,
}

/// The EmailPush objects to send to `stored` for one delivery batch.
///
/// Returns no EmailPush when:
///
/// - the subscription is not verified, or has expired
///   ([`jmap_server::is_deliverable`]; -05 §4: "MUST NOT send any EmailPush
///   objects until the PushSubscription has been verified");
/// - it has no `keys` (-05 §6: the payload holds user data and must be
///   encrypted);
/// - its `emailPush` has no valid config for `batch.account_id`;
/// - no delivered Email matches the config's `filter`.
///
/// It ignores the subscription's `types` (-05 §4: "regardless of the value
/// of the "types" property"). Each matching Email holds the config's
/// `properties` as Email/get would return them, `id` only when listed and
/// never `objectHistory`.
///
/// # Errors
///
/// [`EmailPushError::MissingThreadKeywords`] when the filter has a thread
/// condition and a delivered Email has no thread keywords.
pub fn email_pushes_for(
    stored: &StoredPushSubscription,
    now_secs: i64,
    batch: &EmailPushBatch<'_>,
) -> Result<Vec<OutboundEmailPush>, EmailPushError> {
    let sub = &stored.subscription;
    if !is_deliverable(stored, now_secs) || sub.keys.is_none() {
        return Ok(Vec::new());
    }
    let Some(cfg) = config_for(sub.extra.get(EMAIL_PUSH_PROPERTY), batch.account_id) else {
        return Ok(Vec::new());
    };
    let mut emails = Vec::new();
    for delivered in batch.emails {
        let matched = match &cfg.filter {
            None => true,
            Some(filter) => {
                match email_matches(delivered.email, delivered.thread_keywords, filter) {
                    Ok(m) => m,
                    Err(FilterError::MissingThreadKeywords) => {
                        return Err(EmailPushError::MissingThreadKeywords);
                    }
                    // A decoded filter holds only known operators.
                    Err(FilterError::Unsupported) => return Ok(Vec::new()),
                }
            }
        };
        if matched {
            emails.push(project(delivered.email, &cfg.properties));
        }
    }
    if emails.is_empty() {
        return Ok(Vec::new());
    }
    let state = if batch.account.mail {
        batch.state.cloned()
    } else {
        None
    };
    // TODO(bd JMAP-r44n.2): split across pushes within batch.max_bytes,
    // then truncate a lone oversized Email to a prefix of its properties.
    Ok(vec![OutboundEmailPush {
        push: EmailPush::new(batch.account_id.clone(), emails, state),
        urgency: cfg.urgency,
    }])
}

/// The decoded config for `account_id` in an `emailPush` value.
fn config_for(email_push: Option<&Value>, account_id: &Id) -> Option<EmailPushConfig> {
    let config = email_push?.as_object()?.get(account_id.as_ref())?;
    serde_json::from_value(config.clone()).ok()
}

/// `email` projected onto `properties`, without `objectHistory`, which a
/// config stored without validation may still list (-05 §3: the pushed
/// properties are "always a prefix of the requested list").
fn project(email: &Email, properties: &[String]) -> Map<String, Value> {
    let properties: Vec<String> = properties
        .iter()
        .filter(|p| *p != "objectHistory")
        .cloned()
        .collect();
    // Projection fails only if an Email or address does not serialize,
    // which plain data cannot; `{}` is still a prefix.
    project_email(email, &properties).unwrap_or_default()
}
