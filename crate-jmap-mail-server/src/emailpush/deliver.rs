//! Building EmailPush objects for a delivery
//! (draft-ietf-jmap-emailpush-05 §3, §4).

use std::collections::BTreeMap;

use jmap_mail_types::{EMAIL_PUSH_PROPERTY, Email, EmailPush, EmailPushConfig, Keyword};
use jmap_server::{StoredPushSubscription, is_deliverable};
use jmap_types::{Id, State, Urgency};
use serde_json::{Map, Value};

use super::validate::{Metadata, check_config};
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
/// - its `emailPush` has no config for `batch.account_id`, or that config
///   fails [`check_email_push_config`](super::check_email_push_config) for
///   the live `batch.account` (a config written without
///   `check_email_push_change`, or one an account change has made
///   invalid, is never used);
/// - no delivered Email matches the config's `filter`.
///
/// It ignores the subscription's `types` (-05 §4: "regardless of the value
/// of the "types" property"). Each matching Email holds the config's
/// `properties` as Email/get would return them, `id` only when listed and
/// never `objectHistory`.
///
/// # Size limit
///
/// No EmailPush is longer than `batch.max_bytes` (-05 §3: "MUST NOT exceed
/// the size limit of the push service"). Emails are packed in delivery
/// order, starting a new EmailPush whenever the next would not fit ("MUST
/// split them across multiple EmailPush objects"); every part carries the
/// same `state`, the state after the whole batch. An Email too large to fit
/// even alone goes out alone, with its properties added in the config's
/// order until the next would not fit; that property and all later ones
/// are left out, so its properties are a prefix of the list. No other Email
/// is cut.
///
/// # Errors
///
/// - [`EmailPushError::MissingThreadKeywords`] when the filter has a
///   thread condition and a delivered Email has no thread keywords;
/// - [`EmailPushError::LimitTooSmall`] when an EmailPush holding one `{}`
///   does not fit in `batch.max_bytes`.
pub fn email_pushes_for(
    stored: &StoredPushSubscription,
    now_secs: i64,
    batch: &EmailPushBatch<'_>,
) -> Result<Vec<OutboundEmailPush>, EmailPushError> {
    let sub = &stored.subscription;
    if !is_deliverable(stored, now_secs) || sub.keys.is_none() {
        return Ok(Vec::new());
    }
    let Some(cfg) = config_for(
        sub.extra.get(EMAIL_PUSH_PROPERTY),
        batch.account_id,
        batch.account,
    ) else {
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
    let parts = split(batch, &cfg.properties, emails, state.as_ref())?;
    Ok(parts
        .into_iter()
        .map(|emails| OutboundEmailPush {
            push: EmailPush::new(batch.account_id.clone(), emails, state.clone()),
            urgency: cfg.urgency,
        })
        .collect())
}

/// Serialized length, as `jmap_web_push::Sender::send` measures it.
fn json_len<T: serde::Serialize>(value: &T) -> usize {
    // EmailPush and Email maps are plain JSON data, so they always
    // serialize; an impossible failure measures as too large.
    serde_json::to_vec(value).map_or(usize::MAX, |v| v.len())
}

/// Packs `emails` into the `emails` arrays of EmailPush objects of at most
/// `batch.max_bytes`, cutting an Email to a prefix of `properties` only when
/// it does not fit alone.
///
/// The compact serialization of an EmailPush is its length with `emails`
/// empty, plus each Email's length, plus a comma between Emails, so each
/// Email is serialized once to measure it.
fn split(
    batch: &EmailPushBatch<'_>,
    properties: &[String],
    emails: Vec<Map<String, Value>>,
    state: Option<&State>,
) -> Result<Vec<Vec<Map<String, Value>>>, EmailPushError> {
    let max = batch.max_bytes;
    let base = json_len(&EmailPush::new(
        batch.account_id.clone(),
        Vec::new(),
        state.cloned(),
    ));
    // `[{}]`: "{}" is 2 octets.
    if base.saturating_add(2) > max {
        return Err(EmailPushError::LimitTooSmall);
    }
    let mut parts = Vec::new();
    let mut current: Vec<Map<String, Value>> = Vec::new();
    let mut current_len = base;
    for email in emails {
        let len = json_len(&email);
        if base.saturating_add(len) > max {
            // Too large even alone: it goes out alone, cut to a prefix.
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
                current_len = base;
            }
            parts.push(vec![truncate(email, properties, max - base)]);
            continue;
        }
        let comma = usize::from(!current.is_empty());
        if current_len + comma + len > max {
            parts.push(std::mem::take(&mut current));
            current_len = base;
        }
        current_len += usize::from(!current.is_empty()) + len;
        current.push(email);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}

/// `email` cut to the longest prefix of `properties` whose serialization
/// is at most `room` octets: properties are added in list order, and the
/// first that does not fit is left out with every one after it (-05 §3).
/// A property the projection lacks (absent from the Email) is skipped.
fn truncate(
    mut email: Map<String, Value>,
    properties: &[String],
    room: usize,
) -> Map<String, Value> {
    let mut out = Map::new();
    for name in properties {
        let Some(value) = email.remove(name) else {
            continue;
        };
        out.insert(name.clone(), value);
        if json_len(&out) > room {
            out.remove(name);
            break;
        }
    }
    out
}

/// The config for `account_id` in an `emailPush` value, when it passes
/// the config check for `account` (decision 16), normalized.
fn config_for(
    email_push: Option<&Value>,
    account_id: &Id,
    account: &EmailPushAccount,
) -> Option<EmailPushConfig> {
    let config = email_push?.as_object()?.get(account_id.as_ref())?;
    check_config(config, account, Metadata::Any).ok()
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
