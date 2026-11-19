//! Validation and normalization of the `emailPush` PushSubscription
//! property (draft-ietf-jmap-emailpush-05 §4).

use jmap_mail_types::{EMAIL_PUSH_PROPERTY, Email, EmailFilter, EmailPushConfig};
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::{
    BackendSetError, MAX_FILTER_DEPTH, SetError, SetErrorType, filter_nests_deeper_than,
};
use jmap_types::{Id, PushSubscription, Urgency};
use serde_json::{Map, Value};

use super::{EmailCondition, EmailPushAccount, EmailPushBackend};
use crate::filter::{EMAIL_PUSH_REQUIRED_CONDITIONS, conditions_used};
use crate::project::{check_email_property, is_email_get_property};

/// The Email/get property that object history adds
/// (draft-ietf-jmap-object-history-03 §3.1).
const OBJECT_HISTORY: &str = "objectHistory";

/// The properties an account without `urn:ietf:params:jmap:mail` "MUST
/// support" (-05 §4).
const REQUIRED_PROPERTIES: [&str; 8] = [
    "keywords",
    "from",
    "to",
    "cc",
    "subject",
    "preview",
    "receivedAt",
    "sentAt",
];

/// The properties ObjectID (RFC 8474) adds for an account without mail
/// (-05 §4: "it MUST also support").
const OBJECT_ID_PROPERTIES: [&str; 3] = ["id", "threadId", "mailboxIds"];

/// Which properties of one EmailPushConfig are invalid, as reported by
/// [`check_email_push_config`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEmailPushConfig {
    /// The invalid config properties (`filter`, `properties`, `urgency`),
    /// in that order. Empty when the config is not a JSON object.
    pub properties: Vec<&'static str>,
}

/// Checks one EmailPushConfig value for `account` without any backend
/// call, and returns it normalized.
///
/// The checks are those -05 §4 lists for `invalidProperties`, other than
/// the account check:
///
/// - `filter` decodes as a FilterOperator, FilterCondition or `null`,
///   nests no deeper than [`MAX_FILTER_DEPTH`], and uses only conditions
///   the account supports: every condition with `mail`; otherwise the ten
///   -05 §4 requires, `inMailbox` / `inMailboxOtherThan` with `object_ids`,
///   and the declared [`EmailPushAccount::filter_conditions`]. A
///   draft-ietf-jmap-metadata-05 condition also needs `metadata`, and a
///   `privateMetadata*` one its `supportsPrivate` (-05 §1.2.1: otherwise
///   those conditions "MUST be rejected").
/// - `properties` is a string array of properties the account supports:
///   with `mail`, every Email/get property except `bodyValues` (a
///   metadata property needs `metadata`); otherwise the eight -05 §4
///   requires, `id` / `threadId` / `mailboxIds` with `object_ids`, and the
///   declared [`EmailPushAccount::properties`]. `objectHistory` is
///   supported exactly when `mail` and `email_history`.
/// - `urgency` is an RFC 8030 §5.3 `urgency-option`.
///
/// `metadata` is the account's draft-ietf-jmap-metadata-05 information for
/// Email (`JmapAccounts::metadata_info::<Email>`), `None` when it has none.
///
/// The normalized config has every default filled (`filter` `null`,
/// `properties` `[]`, `urgency` `"normal"`) and no `objectHistory` in
/// `properties`, the other names keeping their order: EmailPush has no
/// `includeReplaced` / `includeDestroyed`, so `objectHistory` could never
/// be pushed, and dropping it keeps each pushed Email a prefix of the
/// stored list (-05 §3).
///
/// # Errors
///
/// [`InvalidEmailPushConfig`] naming every invalid property.
pub fn check_email_push_config(
    config: &Value,
    account: &EmailPushAccount,
    metadata: Option<&DataTypeMetadataInfo>,
) -> Result<EmailPushConfig, InvalidEmailPushConfig> {
    check_config(config, account, Metadata::Account(metadata))
}

/// Which draft-ietf-jmap-metadata-05 conditions a config check allows.
#[derive(Clone, Copy)]
pub(super) enum Metadata<'a> {
    /// Those the account's metadata information supports (`None`: none).
    Account(Option<&'a DataTypeMetadataInfo>),
    /// All of them: delivery has no caller to ask, and a metadata
    /// condition on an Email without metadata simply does not match.
    Any,
}

/// [`check_email_push_config`] with the metadata rule chosen by the caller.
pub(super) fn check_config(
    config: &Value,
    account: &EmailPushAccount,
    metadata: Metadata<'_>,
) -> Result<EmailPushConfig, InvalidEmailPushConfig> {
    let Value::Object(map) = config else {
        return Err(InvalidEmailPushConfig {
            properties: Vec::new(),
        });
    };
    let mut bad = Vec::new();
    if !filter_ok(map.get("filter"), account, metadata) {
        bad.push("filter");
    }
    let metadata_properties = match metadata {
        Metadata::Account(info) => info.is_some(),
        Metadata::Any => true,
    };
    if !properties_ok(map.get("properties"), account, metadata_properties) {
        bad.push("properties");
    }
    if let Some(u) = map.get("urgency")
        && serde_json::from_value::<Urgency>(u.clone()).is_err()
    {
        bad.push("urgency");
    }
    if !bad.is_empty() {
        return Err(InvalidEmailPushConfig { properties: bad });
    }
    // Each property decoded on its own above, so the whole does too.
    let mut cfg: EmailPushConfig =
        serde_json::from_value(config.clone()).map_err(|_| InvalidEmailPushConfig {
            properties: Vec::new(),
        })?;
    cfg.properties.retain(|p| p != OBJECT_HISTORY);
    Ok(cfg)
}

fn filter_ok(filter: Option<&Value>, account: &EmailPushAccount, metadata: Metadata<'_>) -> bool {
    let Some(raw) = filter else {
        return true;
    };
    if filter_nests_deeper_than(raw, MAX_FILTER_DEPTH) {
        return false;
    }
    let Ok(filter) = serde_json::from_value::<Option<EmailFilter>>(raw.clone()) else {
        return false;
    };
    let Some(filter) = filter else {
        return true;
    };
    // RFC 8620 §1.4: `before` / `after` are UTCDates.
    if !crate::filter::filter_dates_valid(&filter) {
        return false;
    }
    conditions_used(&filter)
        .into_iter()
        .all(|c| condition_supported(c, account, metadata))
}

fn condition_supported(
    c: EmailCondition,
    account: &EmailPushAccount,
    metadata: Metadata<'_>,
) -> bool {
    use EmailCondition as C;
    let private = matches!(
        c,
        C::PrivateMetadataExists | C::PrivateMetadataTextContains | C::PrivateMetadataTextEquals
    );
    let allowed = match metadata {
        Metadata::Account(info) => info.is_some_and(|m| m.supports_private || !private),
        Metadata::Any => true,
    };
    if c.is_metadata() && !allowed {
        return false;
    }
    account.mail
        || EMAIL_PUSH_REQUIRED_CONDITIONS.contains(&c)
        || (account.object_ids
            && matches!(
                c,
                EmailCondition::InMailbox | EmailCondition::InMailboxOtherThan
            ))
        || account.filter_conditions.contains(&c)
}

fn properties_ok(properties: Option<&Value>, account: &EmailPushAccount, metadata: bool) -> bool {
    let Some(raw) = properties else {
        return true;
    };
    let Ok(names) = serde_json::from_value::<Vec<String>>(raw.clone()) else {
        return false;
    };
    names
        .iter()
        .all(|n| property_supported(n, account, metadata))
}

fn property_supported(name: &str, account: &EmailPushAccount, metadata: bool) -> bool {
    if name == "bodyValues" {
        // -05 §4: "except that the "bodyValues" property is not supported".
        return false;
    }
    if name == OBJECT_HISTORY {
        return account.mail && account.email_history;
    }
    if account.mail {
        return is_email_get_property(name, metadata) && check_email_property(name).is_ok();
    }
    REQUIRED_PROPERTIES.contains(&name)
        || (account.object_ids && OBJECT_ID_PROPERTIES.contains(&name))
        || account.properties.contains(name)
}

/// Validates and normalizes the `emailPush` property of one
/// PushSubscription create or update, as -05 §4 requires.
///
/// `old` is the stored subscription for an update and `None` for a
/// create; `new` is the subscription to store (for an update, `old` with
/// the client's patch applied). A deployment's
/// `create_push_subscription` and `update_push_subscription` MUST call
/// this before storing `new`, and store `new` as this leaves it. It has
/// no side effects.
///
/// The change "includes emailPush" when a create sends it or an update
/// changes it, compared with `old` pruned of revoked accounts as
/// [`prune_email_push`] lists it. Otherwise only the `keys` rule runs, so an
/// update of other properties succeeds even if a stored account has since
/// been revoked or changed.
/// When it does, `emailPush` is checked:
///
/// - a non-object value is invalid at `emailPush`;
/// - a non-null `emailPush` with null `keys` is invalid, naming `keys`;
/// - a key not among [`EmailPushBackend::email_push_accounts`], or a value
///   that is not an object, is invalid at `emailPush/{accountId}`;
/// - each config is checked by [`check_email_push_config`], its invalid
///   properties reported at `emailPush/{accountId}/{property}`.
///
/// On success every config is replaced by its normalized form, which the
/// PushSubscription/set handler echoes in `created` / `updated` when it
/// differs from what the client sent (RFC 8620 §5.3).
///
/// A metadata condition needs only
/// [`JmapAccounts::metadata_info`](jmap_server::JmapAccounts::metadata_info) for Email on the account, not the
/// metadata capability in `using`: the filter runs at delivery, outside
/// any request.
///
/// # Errors
///
/// - [`BackendSetError::SetError`] with `invalidProperties`, listing
///   `emailPush`, then every invalid path in map order, then `keys` for
///   the `keys` rule.
/// - [`BackendSetError::Other`] when `email_push_accounts` fails.
pub async fn check_email_push_change<B: EmailPushBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    old: Option<&PushSubscription>,
    new: &mut PushSubscription,
) -> Result<(), BackendSetError<B::Error>> {
    let value = new.extra.get(EMAIL_PUSH_PROPERTY);
    let mut touches = match old {
        None => value.is_some(),
        Some(old) => old.extra.get(EMAIL_PUSH_PROPERTY) != value,
    };
    let mut accounts = None;
    // `new` comes from the listed, pruned object (`list_push_subscriptions`
    // strips revoked accounts), `old` from storage: compare with `old` as
    // listed, so a stored revoked account alone is not a change.
    if touches
        && let Some(old) = old
        && old
            .extra
            .get(EMAIL_PUSH_PROPERTY)
            .is_some_and(Value::is_object)
    {
        let current = backend
            .email_push_accounts(caller)
            .await
            .map_err(BackendSetError::Other)?;
        let mut listed = old.clone();
        prune_email_push(&mut listed, &current);
        touches = listed.extra.get(EMAIL_PUSH_PROPERTY) != value;
        accounts = Some(current);
    }
    let non_null = value.is_some_and(|v| !v.is_null());
    // -05 §4: "The "emailPush" property is not null and the "keys"
    // property of the PushSubscription is null."
    let keys_bad = non_null && new.keys.is_none();
    let mut paths: Vec<String> = Vec::new();
    let mut normalized: Option<Map<String, Value>> = None;
    if touches && non_null {
        match value {
            Some(Value::Object(map)) => {
                let accounts = match accounts {
                    Some(a) => a,
                    None => backend
                        .email_push_accounts(caller)
                        .await
                        .map_err(BackendSetError::Other)?,
                };
                let mut out = Map::new();
                for (key, cfg) in map {
                    let account = Id::new_validated(key.clone())
                        .ok()
                        .and_then(|id| accounts.get(&id).map(|a| (id, a)));
                    let Some((id, account)) = account else {
                        paths.push(format!("{EMAIL_PUSH_PROPERTY}/{key}"));
                        continue;
                    };
                    let metadata = backend.metadata_info::<Email>(caller, &id);
                    match check_email_push_config(cfg, account, metadata.as_ref()) {
                        Ok(cfg) => {
                            out.insert(key.clone(), config_value(&cfg));
                        }
                        Err(e) if e.properties.is_empty() => {
                            paths.push(format!("{EMAIL_PUSH_PROPERTY}/{key}"));
                        }
                        Err(e) => paths.extend(
                            e.properties
                                .iter()
                                .map(|p| format!("{EMAIL_PUSH_PROPERTY}/{key}/{p}")),
                        ),
                    }
                }
                normalized = Some(out);
            }
            _ => paths.push(EMAIL_PUSH_PROPERTY.to_owned()),
        }
    }
    if !paths.is_empty() || keys_bad {
        let mut properties = vec![EMAIL_PUSH_PROPERTY.to_owned()];
        properties.extend(paths.into_iter().filter(|p| p != EMAIL_PUSH_PROPERTY));
        if keys_bad {
            properties.push("keys".to_owned());
        }
        return Err(BackendSetError::SetError(Box::new(
            SetError::new(SetErrorType::InvalidProperties).with_properties(properties),
        )));
    }
    if let Some(map) = normalized {
        new.extra
            .insert(EMAIL_PUSH_PROPERTY.to_owned(), Value::Object(map));
    }
    Ok(())
}

/// The normalized JSON form of `cfg`.
fn config_value(cfg: &EmailPushConfig) -> Value {
    // EmailPushConfig is plain data with string-keyed maps, so it always
    // serializes.
    serde_json::to_value(cfg).unwrap_or(Value::Null)
}

/// Removes from `sub`'s `emailPush` map every account not in `accounts`,
/// and returns whether anything was removed.
///
/// -05 §4: when the user loses access to an account, or the account no
/// longer supports emailpush, the server "MUST remove that account from the
/// "emailPush" map". A map whose every key goes stays `{}`, not `null`.
/// An absent, `null` or non-object `emailPush` is left alone.
///
/// Call it with the caller's current [`EmailPushBackend::email_push_accounts`]:
///
/// - from `list_push_subscriptions`, on what it returns (strip on read: the
///   next update, which patches the listed object, persists the result);
/// - from the deployment's revocation path, persisting the result at once.
pub fn prune_email_push(
    sub: &mut PushSubscription,
    accounts: &std::collections::BTreeMap<Id, EmailPushAccount>,
) -> bool {
    let Some(Value::Object(map)) = sub.extra.get_mut(EMAIL_PUSH_PROPERTY) else {
        return false;
    };
    let before = map.len();
    map.retain(|k, _| {
        Id::new_validated(k.clone())
            .ok()
            .is_some_and(|id| accounts.contains_key(&id))
    });
    map.len() != before
}
