//! Generic `PushSubscription/get` and `PushSubscription/set` bindings
//! (RFC 8620 §7.2.1, §7.2.2).
//!
//! [`PushSubscription`] is not account-scoped: neither method takes or returns
//! `accountId`, `/get` returns no `state`, and `/set` takes no `ifInState`
//! and returns no `oldState` / `newState`. Extension properties (such as
//! `emailPush`, draft-ietf-jmap-emailpush-05 §4) travel in
//! [`PushSubscription::extra`], with their capability URI in `extra_using`.

use std::collections::BTreeMap;

use jmap_types::{Id, PartialObject, PatchObject, PushSubscription, SetError};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::{ClientError, JmapClient, JmapRequestBuilder, Session, extract_response, to_args};

/// A `PushSubscription/get` response (RFC 8620 §7.2.1): no `accountId` and
/// no `state`.
///
/// Each object holds the requested properties, so it is a
/// [`PartialObject`]: with `properties` null the server returns every
/// property except `url` and `keys`, and `deviceClientId` only when asked
/// for otherwise.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscriptionGetResponse {
    /// The subscriptions found.
    pub list: Vec<PartialObject>,
    /// Requested ids not found; `null` decodes as empty.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub not_found: Vec<Id>,
    /// Vendor / site / private extension fields, per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

/// A `PushSubscription/set` response (RFC 8620 §7.2.2): no `accountId`,
/// `oldState` or `newState`.
///
/// `created` holds each new subscription's server-set properties (`id`,
/// `expires`, and any property set or changed by the server, such as a
/// normalized `emailPush`); `updated` holds, per id, the properties the
/// server changed in a way the patch did not ask for, or `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscriptionSetResponse {
    /// Created subscriptions, by creation id.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub created: BTreeMap<String, PartialObject>,
    /// Updated subscriptions, by id.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub updated: BTreeMap<Id, Option<PartialObject>>,
    /// Destroyed ids.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub destroyed: Vec<Id>,
    /// Failed creates, by creation id.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub not_created: BTreeMap<String, SetError>,
    /// Failed updates, by id.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub not_updated: BTreeMap<Id, SetError>,
    /// Failed destroys, by id.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub not_destroyed: BTreeMap<Id, SetError>,
    /// Vendor / site / private extension fields, per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

/// RFC 8620 types these as "null if none": `null` decodes as empty.
fn null_as_empty<'de, D, C>(d: D) -> Result<C, D::Error>
where
    D: Deserializer<'de>,
    C: Deserialize<'de> + Default,
{
    Ok(Option::<C>::deserialize(d)?.unwrap_or_default())
}

/// `using`: core, then each of `extra_using` once.
fn using<'a>(extra_using: &[&'a str]) -> Vec<&'a str> {
    let mut using = vec!["urn:ietf:params:jmap:core"];
    for uri in extra_using {
        if !using.contains(uri) {
            using.push(uri);
        }
    }
    using
}

impl JmapClient {
    /// `PushSubscription/get` (RFC 8620 §7.2.1) to `session`'s API URL.
    ///
    /// `ids` `None` fetches every subscription of these credentials;
    /// `properties` `None` asks for every property but `url` and `keys`,
    /// which the server never returns ("forbidden" if requested).
    /// `extra_using` adds capability URIs to `using`, as an extension
    /// property needs (draft-ietf-jmap-emailpush-05 §4: "To get or set the
    /// "emailPush" property, the client MUST include" its capability).
    ///
    /// # Errors
    ///
    /// Transport and parse errors, and [`ClientError::MethodError`] for a
    /// method-level error.
    pub async fn push_subscription_get(
        &self,
        session: &Session,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        extra_using: &[&str],
    ) -> Result<PushSubscriptionGetResponse, ClientError> {
        let mut args = Map::new();
        args.insert("ids".to_owned(), to_args(&ids)?);
        if let Some(p) = properties {
            args.insert("properties".to_owned(), to_args(&p)?);
        }
        self.push_subscription_call(session, "PushSubscription/get", args, extra_using)
            .await
    }

    /// `PushSubscription/set` (RFC 8620 §7.2.2) to `session`'s API URL.
    ///
    /// Empty `create`, `update` and `destroy` are left out. A create needs
    /// `deviceClientId` and `url`; the server pushes a `PushVerification` to
    /// `url`, whose code the client then sets with an update. `url` and
    /// `keys` are immutable. `extra_using` adds capability URIs to `using`
    /// for extension properties.
    ///
    /// There are no conditional arguments here: send `ifUnchangedBy` or
    /// `atomic` (draft-ietf-jmap-conditional-02) through
    /// [`JmapClient::set_call`] with a [`SetCall`](crate::SetCall) that has
    /// no `account_id`.
    ///
    /// # Errors
    ///
    /// Transport and parse errors, and [`ClientError::MethodError`] for a
    /// method-level error. Per-object failures are in the response's
    /// `not_*` maps.
    pub async fn push_subscription_set(
        &self,
        session: &Session,
        create: &BTreeMap<String, PushSubscription>,
        update: &BTreeMap<Id, PatchObject>,
        destroy: &[Id],
        extra_using: &[&str],
    ) -> Result<PushSubscriptionSetResponse, ClientError> {
        let mut args = Map::new();
        if !create.is_empty() {
            args.insert("create".to_owned(), to_args(create)?);
        }
        if !update.is_empty() {
            args.insert("update".to_owned(), to_args(update)?);
        }
        if !destroy.is_empty() {
            args.insert("destroy".to_owned(), to_args(&destroy)?);
        }
        self.push_subscription_call(session, "PushSubscription/set", args, extra_using)
            .await
    }

    async fn push_subscription_call<R: serde::de::DeserializeOwned>(
        &self,
        session: &Session,
        method: &str,
        args: Map<String, Value>,
        extra_using: &[&str],
    ) -> Result<R, ClientError> {
        let mut builder = JmapRequestBuilder::new(&using(extra_using));
        builder.add_call(method, Value::Object(args), "0")?;
        let response = self.call_session(session, &builder.build()?).await?;
        extract_response(&response, "0")
    }
}
