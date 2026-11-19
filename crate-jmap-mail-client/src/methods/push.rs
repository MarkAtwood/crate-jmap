//! Email Delivery Push (draft-ietf-jmap-emailpush-05), client side: a
//! PushSubscription create that asks for EmailPush objects, updates of its
//! `emailPush` map, and decoding of the push payloads a client receives.
//!
//! Gated by the `emailpush` feature.
//!
//! # What an EmailPush is good for
//!
//! "Delivery of push messages is not guaranteed: the push service may delay
//! or drop them. Clients MUST NOT rely on EmailPush objects as a complete
//! record of new messages" (-05 §3): resynchronize with `Email/changes` (or
//! IMAP) as usual. The pushed Emails hold a prefix of the config's
//! `properties` when one is too large, so clients are RECOMMENDED "to only
//! request the properties they need for the visible notification, plus the
//! "id" property if they want to then fetch further information, and to
//! list the most important properties first".

use std::collections::BTreeMap;

use jmap_base_client::{ClientError, PushSubscriptionSetResponse};
use jmap_mail_types::{EMAIL_PUSH_PROPERTY, EmailPush, EmailPushConfig, JMAP_EMAILPUSH_URI};
use jmap_types::{
    Id, PatchObject, PushKeys, PushSubscription, PushVerification, StateChange, UTCDate,
};
use serde_json::{Map, Value};

use super::SessionClient;

/// A PushSubscription create (RFC 8620 §7.2) that asks for EmailPush
/// objects for some accounts.
///
/// `keys` is required: EmailPush objects carry user data, so "the client
/// MUST specify encryption keys when establishing the PushSubscription"
/// (-05 §6), and a server rejects `emailPush` without them (-05 §4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EmailPushSubscriptionInput {
    /// `deviceClientId`: identifies the client and device.
    pub device_client_id: String,
    /// `url`: the push endpoint (`https://`).
    pub url: String,
    /// `keys`: the RFC 8291 encryption keys.
    pub keys: PushKeys,
    /// `expires`, or `None` for the server's default.
    pub expires: Option<UTCDate>,
    /// `types`: the StateChange types wanted; `Some(vec![])` for EmailPush
    /// objects only (-05 §4: "a client that only wants EmailPush objects
    /// can set "types" to an empty array"), `None` for all types.
    pub types: Option<Vec<String>>,
    /// `emailPush`: what to push for each account.
    pub email_push: BTreeMap<Id, EmailPushConfig>,
}

impl EmailPushSubscriptionInput {
    /// A create with these required fields, the server's default expiry
    /// and every StateChange type.
    pub fn new(
        device_client_id: impl Into<String>,
        url: impl Into<String>,
        keys: PushKeys,
        email_push: BTreeMap<Id, EmailPushConfig>,
    ) -> Self {
        Self {
            device_client_id: device_client_id.into(),
            url: url.into(),
            keys,
            expires: None,
            types: None,
            email_push,
        }
    }

    /// Returns `self` with `types` set.
    #[must_use]
    pub fn with_types(mut self, types: Option<Vec<String>>) -> Self {
        self.types = types;
        self
    }

    /// Returns `self` with `expires` set.
    #[must_use]
    pub fn with_expires(mut self, expires: Option<UTCDate>) -> Self {
        self.expires = expires;
        self
    }

    fn to_subscription(&self) -> Result<PushSubscription, ClientError> {
        let mut v = serde_json::json!({
            "deviceClientId": self.device_client_id,
            "url": self.url,
            "keys": self.keys,
        });
        if let Some(e) = &self.expires {
            v["expires"] = jmap_base_client::to_args(e)?;
        }
        if let Some(t) = &self.types {
            v["types"] = jmap_base_client::to_args(t)?;
        }
        let mut sub: PushSubscription =
            serde_json::from_value(v).map_err(ClientError::from_serialize)?;
        jmap_mail_types::set_email_push(&mut sub, Some(self.email_push.clone()));
        Ok(sub)
    }
}

impl SessionClient {
    /// Creates a PushSubscription that asks for EmailPush objects
    /// (`PushSubscription/set` create, with `urn:ietf:params:jmap:emailpush`
    /// in `using`, -05 §4).
    ///
    /// The server then pushes a PushVerification to `input.url`; set its
    /// code on the subscription with
    /// [`JmapClient::push_subscription_set`](jmap_base_client::JmapClient::push_subscription_set)
    /// before any EmailPush is sent. The `created` entry holds the stored
    /// `emailPush` when the server changed it (filled defaults, a dropped
    /// `objectHistory`).
    ///
    /// # Errors
    ///
    /// Transport, parse and method-level errors. A rejected config is the
    /// response's `not_created["c"]`, `invalidProperties` naming each
    /// invalid path.
    pub async fn email_push_subscription_create(
        &self,
        input: &EmailPushSubscriptionInput,
    ) -> Result<PushSubscriptionSetResponse, ClientError> {
        let create = BTreeMap::from([("c".to_owned(), input.to_subscription()?)]);
        self.client
            .push_subscription_set(
                &self.session,
                &create,
                &BTreeMap::new(),
                &[],
                &[JMAP_EMAILPUSH_URI],
            )
            .await
    }

    /// Replaces subscription `sub_id`'s whole `emailPush` map; `None` sets
    /// it to `null`, which stops every EmailPush (-05 §4).
    ///
    /// # Errors
    ///
    /// As [`email_push_subscription_create`](Self::email_push_subscription_create);
    /// a rejected update is the response's `not_updated[sub_id]`.
    pub async fn set_email_push(
        &self,
        sub_id: &Id,
        email_push: Option<BTreeMap<Id, EmailPushConfig>>,
    ) -> Result<PushSubscriptionSetResponse, ClientError> {
        let value = match email_push {
            None => Value::Null,
            Some(map) => jmap_base_client::to_args(&map)?,
        };
        self.patch(sub_id, EMAIL_PUSH_PROPERTY.to_owned(), value)
            .await
    }

    /// Sets the config for one account in subscription `sub_id`'s
    /// `emailPush` map, or removes the account with `None` (an
    /// `emailPush/{account}` patch). The map must already be an object.
    ///
    /// # Errors
    ///
    /// As [`set_email_push`](Self::set_email_push).
    pub async fn patch_email_push(
        &self,
        sub_id: &Id,
        account: &Id,
        config: Option<&EmailPushConfig>,
    ) -> Result<PushSubscriptionSetResponse, ClientError> {
        let value = match config {
            None => Value::Null,
            Some(c) => jmap_base_client::to_args(c)?,
        };
        // RFC 6901: "~" and "/" in a pointer segment are escaped.
        let segment = account.as_ref().replace('~', "~0").replace('/', "~1");
        self.patch(sub_id, format!("{EMAIL_PUSH_PROPERTY}/{segment}"), value)
            .await
    }

    async fn patch(
        &self,
        sub_id: &Id,
        path: String,
        value: Value,
    ) -> Result<PushSubscriptionSetResponse, ClientError> {
        let patch: PatchObject =
            serde_json::from_value(Value::Object(Map::from_iter([(path, value)])))
                .map_err(ClientError::from_serialize)?;
        self.client
            .push_subscription_set(
                &self.session,
                &BTreeMap::new(),
                &BTreeMap::from([(sub_id.clone(), patch)]),
                &[],
                &[JMAP_EMAILPUSH_URI],
            )
            .await
    }
}

/// One push message a client received at its push endpoint.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PushPayload {
    /// `"@type": "StateChange"` (RFC 8620 §7.1).
    StateChange(StateChange),
    /// `"@type": "PushVerification"` (RFC 8620 §7.2.2).
    PushVerification(PushVerification),
    /// `"@type": "EmailPush"` (draft-ietf-jmap-emailpush-05 §3).
    EmailPush(EmailPush),
    /// Any other `@type`, or none.
    Other(Value),
}

/// Decodes a push message by its `@type`.
///
/// `decrypted` is the plaintext after RFC 8291 decryption with the
/// subscription's keys. A client MUST "ignore any push notification
/// received that is not encrypted with those keys" (-05 §6), so only pass
/// what decrypted with them.
///
/// # Errors
///
/// When `decrypted` is not a JSON object, or an object of a known `@type`
/// does not decode as that type.
pub fn parse_push_payload(decrypted: &[u8]) -> Result<PushPayload, serde_json::Error> {
    let value: Value = serde_json::from_slice(decrypted)?;
    if !value.is_object() {
        return Err(serde::de::Error::custom(
            "push payload is not a JSON object",
        ));
    }
    Ok(match value.get("@type").and_then(Value::as_str) {
        Some("StateChange") => PushPayload::StateChange(serde_json::from_value(value)?),
        Some("PushVerification") => PushPayload::PushVerification(serde_json::from_value(value)?),
        Some("EmailPush") => PushPayload::EmailPush(serde_json::from_value(value)?),
        _ => PushPayload::Other(value),
    })
}
