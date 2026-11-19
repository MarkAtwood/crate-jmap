//! PushSubscription and EmailPush for the memory backend
//! (RFC 8620 §7.2, draft-ietf-jmap-emailpush-05).
//!
//! Single owner (`CallerCtx = ()`): every subscription belongs to the one
//! caller, and [`MAX_PUSH_SUBSCRIPTIONS`] caps them all. Nothing is sent:
//! PushVerification objects queue in an outbox
//! ([`MemoryBackend::take_push_verifications`]) and
//! [`MemoryBackend::deliver_email`] returns the EmailPush objects a
//! delivery produces.

use std::collections::{BTreeMap, HashMap};

use jmap_mail_types::{Email, Keyword};
use jmap_server::{
    BackendSetError, JmapObject, PushSubscriptionBackend, SetError, SetErrorType,
    StoredPushSubscription,
};
use jmap_types::{Id, PushSubscription, PushVerification, State, UTCDate};
use serde::Deserialize;

use super::{MemoryBackend, MemoryError};
use crate::MailBackend;
use crate::emailpush::{
    DeliveredEmail, EmailPushAccount, EmailPushBackend, EmailPushBatch, OutboundEmailPush,
    check_email_push_change, email_pushes_for, prune_email_push,
};

/// The most push subscriptions the memory backend stores ("The server MUST
/// limit the number of push subscriptions any one user may have", RFC 8620
/// §8.6).
pub const MAX_PUSH_SUBSCRIPTIONS: usize = 50;

/// The EmailPush size limit [`MemoryBackend::deliver_email`] uses: 3993
/// octets, the largest Web Push plaintext without padding (RFC 8291 §4:
/// 4096 minus the 103-octet header and padding delimiter). Equal to
/// `jmap_web_push::MAX_PLAINTEXT`, which this crate does not depend on.
pub const PUSH_PLAINTEXT_BYTES: usize = 3993;

/// Push state, a part of the backend's scoped store.
#[derive(Clone)]
pub(super) struct PushState {
    subs: BTreeMap<Id, StoredPushSubscription>,
    verifications: Vec<PushVerification>,
    /// Per-account EmailPush overrides; `None` revokes the account.
    accounts: HashMap<Id, Option<EmailPushAccount>>,
    max_bytes: usize,
    /// Every EmailPush produced, in order, for tests.
    sent: Vec<(Id, OutboundEmailPush)>,
}

impl Default for PushState {
    fn default() -> Self {
        Self {
            subs: BTreeMap::new(),
            verifications: Vec::new(),
            accounts: HashMap::new(),
            max_bytes: PUSH_PLAINTEXT_BYTES,
            sent: Vec::new(),
        }
    }
}

impl MemoryBackend {
    /// Removes and returns the PushVerification objects queued by
    /// `PushSubscription/set` creates, oldest first. A client sets the
    /// code on its subscription to verify it (RFC 8620 §7.2.2).
    pub fn take_push_verifications(&self) -> Vec<PushVerification> {
        std::mem::take(&mut self.inner.lock_unscoped().push.verifications)
    }

    /// Simulates an SMTP delivery to `account_id`: stores `raw` as a blob,
    /// imports it into `mailbox_ids` with `keywords` and the current time
    /// as `receivedAt`, advances the account's `EmailDelivery` state, and
    /// builds the EmailPush objects for every stored subscription
    /// (draft-ietf-jmap-emailpush-05 §3: a message is delivered under the
    /// conditions that change EmailDelivery, RFC 8621 §1.5).
    ///
    /// Returns the new Email's id and each EmailPush with its
    /// subscription's id; nothing is sent. `Email/import` and `Email/set`
    /// never produce EmailPush objects. A subscription whose size limit
    /// cannot hold any EmailPush gets none.
    ///
    /// # Errors
    ///
    /// As `Email/import`: an unknown account, or a duplicate Message-ID.
    pub async fn deliver_email(
        &self,
        account_id: &Id,
        raw: &[u8],
        mailbox_ids: &[Id],
        keywords: &[Keyword],
    ) -> Result<(Id, Vec<(Id, OutboundEmailPush)>), BackendSetError<MemoryError>> {
        let blob_id = Id::from(uuid::Uuid::new_v4().to_string());
        self.put_blob(
            None,
            account_id,
            blob_id.clone(),
            raw.to_vec(),
            "message/rfc822",
        );
        let now = UTCDate::from(crate::helpers::now_utc_string().as_ref());
        let (email_id, email) = self
            .import_email(&(), account_id, &blob_id, mailbox_ids, keywords, Some(&now))
            .await?;
        let (state, thread): (State, Vec<BTreeMap<Keyword, bool>>) = {
            let mut inner = self.inner.lock().await;
            inner.bump_state("EmailDelivery", account_id.as_ref());
            let state = State::from(
                inner
                    .current_state(Email::TYPE_NAME, account_id.as_ref())
                    .to_string(),
            );
            let thread = inner
                .objects_ref(Email::TYPE_NAME, account_id.as_ref())
                .into_iter()
                .flat_map(|m| m.values())
                .filter_map(|v| Email::deserialize(v).ok())
                .filter(|e| e.thread_id == email.thread_id)
                .map(|e| e.keywords)
                .collect();
            (state, thread)
        };
        let accounts = self.email_push_accounts(&()).await.unwrap_or_default();
        let Some(account) = accounts.get(account_id) else {
            // A revoked account gets no EmailPush (-05 §4: "MUST stop").
            return Ok((email_id, Vec::new()));
        };
        let thread_refs: Vec<&BTreeMap<Keyword, bool>> = thread.iter().collect();
        let delivered = [DeliveredEmail::new(&email).with_thread_keywords(&thread_refs)];
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let mut inner = self.inner.lock().await;
        let push = &mut inner.push;
        let batch = EmailPushBatch::new(
            account_id,
            account,
            &delivered,
            Some(&state),
            push.max_bytes,
        );
        let mut out = Vec::new();
        for (id, stored) in &push.subs {
            // Thread keywords are supplied, so only LimitTooSmall can fail.
            if let Ok(pushes) = email_pushes_for(stored, now_secs, &batch) {
                out.extend(pushes.into_iter().map(|p| (id.clone(), p)));
            }
        }
        push.sent.extend(out.iter().cloned());
        Ok((email_id, out))
    }
}

#[cfg(feature = "test-helpers")]
impl MemoryBackend {
    /// Overrides what `account_id` supports for EmailPush; `None` revokes
    /// it (the user lost access, or the account lost the capability).
    /// Registered accounts default to mail and ObjectID, with Email history
    /// when the object-history capability lists Email.
    pub fn set_email_push_account_for_test(
        &self,
        account_id: &Id,
        account: Option<EmailPushAccount>,
    ) {
        self.inner
            .lock_unscoped()
            .push
            .accounts
            .insert(account_id.clone(), account);
    }

    /// Sets the EmailPush size limit of [`deliver_email`](Self::deliver_email)
    /// (default [`PUSH_PLAINTEXT_BYTES`]).
    pub fn set_push_max_bytes_for_test(&self, max_bytes: usize) {
        self.inner.lock_unscoped().push.max_bytes = max_bytes;
    }

    /// Removes and returns every EmailPush produced so far, with its
    /// subscription's id.
    pub fn take_email_pushes_for_test(&self) -> Vec<(Id, OutboundEmailPush)> {
        std::mem::take(&mut self.inner.lock_unscoped().push.sent)
    }
}

impl PushSubscriptionBackend for MemoryBackend {
    const PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES: &'static [(&'static str, &'static str)] = &[(
        jmap_mail_types::EMAIL_PUSH_PROPERTY,
        jmap_mail_types::JMAP_EMAILPUSH_URI,
    )];

    /// Every stored subscription, with accounts the caller no longer has
    /// pruned from `emailPush`.
    async fn list_push_subscriptions(
        &self,
        caller: &(),
    ) -> Result<Vec<StoredPushSubscription>, MemoryError> {
        let accounts = self.email_push_accounts(caller).await?;
        let mut subs: Vec<StoredPushSubscription> = self
            .inner
            .lock()
            .await
            .push
            .subs
            .values()
            .cloned()
            .collect();
        for s in &mut subs {
            prune_email_push(&mut s.subscription, &accounts);
        }
        Ok(subs)
    }

    /// Validates `emailPush`, then stores `new` under a fresh id with a
    /// 128-bit verification code and queues its PushVerification, or
    /// returns `overQuota` at [`MAX_PUSH_SUBSCRIPTIONS`].
    async fn create_push_subscription(
        &self,
        caller: &(),
        mut new: PushSubscription,
    ) -> Result<StoredPushSubscription, BackendSetError<MemoryError>> {
        check_email_push_change(self, caller, None, &mut new).await?;
        let expected_code = random_code();
        let mut inner = self.inner.lock().await;
        let push = &mut inner.push;
        if push.subs.len() >= MAX_PUSH_SUBSCRIPTIONS {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::OverQuota).with_description(format!(
                    "at most {MAX_PUSH_SUBSCRIPTIONS} push subscriptions per user"
                )),
            )));
        }
        let id = Id::from(uuid::Uuid::new_v4().to_string());
        new.id = Some(id.clone());
        let stored = StoredPushSubscription {
            subscription: new,
            expected_code: expected_code.clone(),
        };
        push.subs.insert(id.clone(), stored.clone());
        push.verifications
            .push(PushVerification::new(id, expected_code));
        Ok(stored)
    }

    /// Validates `emailPush` against the stored subscription, then replaces
    /// it.
    async fn update_push_subscription(
        &self,
        caller: &(),
        mut sub: PushSubscription,
    ) -> Result<PushSubscription, BackendSetError<MemoryError>> {
        let not_found =
            || BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)));
        let id = sub.id.clone().ok_or_else(not_found)?;
        let old = self
            .inner
            .lock()
            .await
            .push
            .subs
            .get(&id)
            .map(|s| s.subscription.clone())
            .ok_or_else(not_found)?;
        check_email_push_change(self, caller, Some(&old), &mut sub).await?;
        let mut inner = self.inner.lock().await;
        let stored = inner.push.subs.get_mut(&id).ok_or_else(not_found)?;
        stored.subscription = sub.clone();
        Ok(sub)
    }

    async fn destroy_push_subscription(
        &self,
        _caller: &(),
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        self.inner
            .lock()
            .await
            .push
            .subs
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| {
                BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
            })
    }
}

impl EmailPushBackend for MemoryBackend {
    /// Every registered account, as mail with ObjectID, with Email history
    /// when its object-history capability lists Email, unless a test
    /// override changes or revokes it.
    async fn email_push_accounts(
        &self,
        _caller: &(),
    ) -> Result<BTreeMap<Id, EmailPushAccount>, MemoryError> {
        let inner = self.inner.lock().await;
        // Email history exactly when the object-history capability lists
        // Email, so `objectHistory` is accepted in a config where Email/get
        // accepts it (decision 10).
        let email_history = inner
            .history
            .as_ref()
            .is_some_and(|h| h.capability().supports::<Email>());
        Ok(inner
            .known_accounts
            .iter()
            .map(|a| Id::from(a.as_str()))
            .filter_map(|id| {
                let account = match inner.push.accounts.get(&id) {
                    None => Some(
                        EmailPushAccount::new(true)
                            .with_object_ids(true)
                            .with_email_history(email_history),
                    ),
                    Some(over) => over.clone(),
                };
                account.map(|a| (id, a))
            })
            .collect())
    }
}

/// A push verification code: 16 bytes from the OS CSPRNG (128 bits) as
/// lowercase hex. RFC 8620 §7.2.2: the code "MUST contain sufficient
/// entropy to avoid the client being able to guess the code via brute
/// force".
///
/// # Panics
///
/// If the OS CSPRNG is unavailable: a guessable code would let a client
/// verify a URL it does not control, so there is no safe fallback.
fn random_code() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available for push verification codes");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
