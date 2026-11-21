//! JMAP Email Delivery Push wire types (draft-ietf-jmap-emailpush-05).
//!
//! - [`JMAP_EMAILPUSH_URI`] and the Session values [`EmailPushCapability`]
//!   and [`EmailPushAccountCapability`] (§2.1).
//! - [`EmailPush`], the object pushed when a message is delivered (§3).
//! - [`EmailPushConfig`], one value of the `emailPush` map that §4 adds to
//!   the RFC 8620 [`PushSubscription`], and the accessors [`email_push`] and
//!   [`set_email_push`] that read and write that map in
//!   [`PushSubscription::extra`].
//!
//! All items are gated by the `emailpush` feature.

use std::collections::BTreeMap;

use jmap_types::{Id, PushSubscription, State, Urgency};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::EmailFilter;

/// Capability URI for JMAP Email Delivery Push (draft-ietf-jmap-emailpush-05
/// §2.1, §7).
pub const JMAP_EMAILPUSH_URI: &str = "urn:ietf:params:jmap:emailpush";

/// The [`PushSubscription`] property that draft-ietf-jmap-emailpush-05 §4
/// adds: `emailPush: Id[EmailPushConfig]|null (default: null)`.
pub const EMAIL_PUSH_PROPERTY: &str = "emailPush";

/// `@type` value of an [`EmailPush`] (§3).
const EMAIL_PUSH_TYPE: &str = "EmailPush";

/// The Session `capabilities` value for [`JMAP_EMAILPUSH_URI`]: "an empty
/// object" (§2.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EmailPushCapability {
    /// Vendor / site / private extension fields, per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// The `accountCapabilities` value for [`JMAP_EMAILPUSH_URI`] (§2.1): "The
/// value is an empty object."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EmailPushAccountCapability {
    /// Vendor / site / private extension fields, per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// One value of a [`PushSubscription`]'s `emailPush` map (§4): which delivered
/// messages to push for one account, which of their properties, and with
/// what Web Push urgency.
///
/// # Normalized form
///
/// All three properties always serialize, with their defaults filled in:
/// `filter` is `null` when absent, `properties` is `[]` and `urgency` is
/// `"normal"`. -05 gives defaults for `properties` and `urgency` but not
/// for `filter`; this crate treats an absent `filter` as `null`, because
/// "If null, all new messages delivered to the account are pushed". So
/// `{}` decodes and re-serializes as
/// `{"filter":null,"properties":[],"urgency":"normal"}`, the form a server
/// stores, returns from `PushSubscription/get` and reports in `created`
/// (RFC 8620 §5.3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailPushConfig {
    /// `filter: FilterOperator|FilterCondition|null`. `None` pushes every
    /// message delivered to the account. The conditions are those of
    /// RFC 8621 §4.4.1.
    #[serde(default)]
    pub filter: Option<EmailFilter>,
    /// `properties: String[] (default: [])`: the Email properties to push,
    /// in priority order, named as in `Email/get` (so `header:*` forms and
    /// extension properties are allowed). Empty pushes `{}` per message.
    #[serde(default)]
    pub properties: Vec<String>,
    /// `urgency: String (default: "normal")`: an RFC 8030 §5.3
    /// `urgency-option`.
    #[serde(default = "default_urgency")]
    pub urgency: Urgency,
    /// Vendor / site / private extension fields, per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

fn default_urgency() -> Urgency {
    Urgency::Normal
}

impl Default for EmailPushConfig {
    /// The config `{}` decodes to: no filter, no properties, normal urgency.
    fn default() -> Self {
        Self {
            filter: None,
            properties: Vec::new(),
            urgency: default_urgency(),
            extra: Map::new(),
        }
    }
}

impl EmailPushConfig {
    /// A config with the given filter and properties, normal urgency and
    /// no extension fields.
    pub fn new(filter: Option<EmailFilter>, properties: Vec<String>) -> Self {
        Self {
            filter,
            properties,
            ..Self::default()
        }
    }

    /// Returns `self` with `urgency` set.
    #[must_use]
    pub fn with_urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = urgency;
        self
    }
}

/// An [`EmailPush`] object (§3): messages newly delivered to one account,
/// pushed to a [`PushSubscription`]'s URL.
///
/// The wire form always carries `"@type": "EmailPush"` ("This MUST be the
/// string "EmailPush""): serialize always emits it; deserialize rejects any
/// other value, accepts its absence and never leaves it in `extra`. `state`
/// is always serialized, as `null` when `None`.
///
/// # `extra` equality is feature-flag-dependent
///
/// The derived `PartialEq` compares `extra` and the `emails` maps as
/// `serde_json::Map`s, which are order-insensitive under the workspace
/// default (BTreeMap-backed) and order-sensitive if any crate in the
/// dependency graph enables `serde_json/preserve_order`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct EmailPush {
    /// `accountId`: the account the Emails are in.
    pub account_id: Id,
    /// `emails: Email[]`: one partial Email per message, holding a prefix of
    /// the config's `properties` (§3), so a JSON map rather than
    /// [`Email`](crate::Email).
    pub emails: Vec<Map<String, Value>>,
    /// `state: String|null`: the account's Email state after all of
    /// `emails` were created; `None` when the account does not support
    /// `urn:ietf:params:jmap:mail`.
    pub state: Option<State>,
    /// Vendor / site / private extension fields, per workspace
    /// extras-preservation policy (see workspace AGENTS.md). Never
    /// contains `@type`.
    pub extra: Map<String, Value>,
}

impl EmailPush {
    /// An [`EmailPush`] with the given fields and no extension fields.
    pub fn new(account_id: Id, emails: Vec<Map<String, Value>>, state: Option<State>) -> Self {
        Self {
            account_id,
            emails,
            state,
            extra: Map::new(),
        }
    }
}

impl Serialize for EmailPush {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("@type", EMAIL_PUSH_TYPE)?;
        map.serialize_entry("accountId", &self.account_id)?;
        map.serialize_entry("emails", &self.emails)?;
        map.serialize_entry("state", &self.state)?;
        for (k, v) in &self.extra {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for EmailPush {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut extra = Map::deserialize(d)?;
        match extra.remove("@type") {
            None => {}
            Some(Value::String(s)) if s == EMAIL_PUSH_TYPE => {}
            // The value is not echoed: RFC 8620 §3.6.1 error descriptions
            // are for developers, and the value is the sender's.
            Some(_) => return Err(D::Error::custom("@type must be \"EmailPush\"")),
        }
        let account_id = take(&mut extra, "accountId")?;
        let emails = take(&mut extra, "emails")?;
        // "state" is required on the wire, but `null` and absence both mean
        // no mail state.
        let state = match extra.remove("state") {
            None => None,
            Some(v) => serde_json::from_value(v).map_err(D::Error::custom)?,
        };
        Ok(Self {
            account_id,
            emails,
            state,
            extra,
        })
    }
}

/// Removes the required property `key` from `map` and deserializes it.
fn take<T: serde::de::DeserializeOwned, E: serde::de::Error>(
    map: &mut Map<String, Value>,
    key: &'static str,
) -> Result<T, E> {
    let value = map.remove(key).ok_or_else(|| E::missing_field(key))?;
    serde_json::from_value(value).map_err(E::custom)
}

/// Reads the `emailPush` property (§4) from a [`PushSubscription`]'s `extra`.
///
/// Returns `Ok(None)` when the property is absent or `null`.
///
/// # Errors
///
/// Returns the decode error when `emailPush` is not a map of account ids
/// to valid [`EmailPushConfig`]s.
pub fn email_push(
    sub: &PushSubscription,
) -> Result<Option<BTreeMap<Id, EmailPushConfig>>, serde_json::Error> {
    match sub.extra.get(EMAIL_PUSH_PROPERTY) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some),
    }
}

/// Writes the `emailPush` property (§4) into a [`PushSubscription`]'s `extra`.
///
/// [`None`] stores `null`, which stops [`EmailPush`] objects for every account
/// ("set it to null to stop [`EmailPush`] objects from being sent"); each
/// config is stored in its normalized form.
pub fn set_email_push(sub: &mut PushSubscription, value: Option<BTreeMap<Id, EmailPushConfig>>) {
    let value = match value {
        None => Value::Null,
        Some(map) => Value::Object(
            map.into_iter()
                .map(|(id, cfg)| (id.to_string(), config_value(&cfg)))
                .collect(),
        ),
    };
    sub.extra.insert(EMAIL_PUSH_PROPERTY.to_owned(), value);
}

/// The normalized JSON form of `cfg`.
fn config_value(cfg: &EmailPushConfig) -> Value {
    // EmailPushConfig serializes through derived impls of plain data and a
    // string-keyed extras map, so it cannot fail.
    serde_json::to_value(cfg).expect("EmailPushConfig always serializes")
}
