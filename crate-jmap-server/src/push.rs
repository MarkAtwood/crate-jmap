//! RFC 8620 §7.2 push subscriptions: the caller-scoped storage trait, the
//! `PushSubscription/get` and `PushSubscription/set` handlers
//! (bd:JMAP-nbk3.2, bd:JMAP-nbk3.3), and the delivery gates
//! [`is_deliverable`] and [`state_change_for`].
//!
//! Push subscriptions are not tied to an account (RFC 8620 §7.2.1), so the
//! handlers here do not use `accountId`, [`JmapBackend::account_exists`],
//! or [`JmapBackend::get_state`]. Instead they scope every lookup to the
//! caller's credentials through [`PushSubscriptionBackend`].
//!
//! Sending the `PushVerification` and `StateChange` requests is the
//! consumer's job; this module only decides what may be sent.

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    future::Future,
    sync::Arc,
    time::SystemTime,
};

use jmap_types::{Id, JmapError, PushSubscription, SetObject, State, StateChange, UTCDate};
use serde_json::{Map, Value, json};
use subtle::ConstantTimeEq;

use crate::backend::{
    BackendSetError, JmapBackend, SetError, SetErrorType, backend_set_error_value,
};
use crate::handlers::{server_fail_from_backend, server_fail_value_from_backend};
use crate::helpers::{
    apply_patch_object, enforce_max_objects_in_get, enforce_max_objects_in_set, not_found_json,
    optional_arg, reject_server_set, serialize_value, signed_seconds_since_epoch,
    utc_date_from_epoch_seconds,
};
use crate::{
    CallContext, ClosureHandler, ConditionalBackend, Dispatcher, HandlerFuture, HandlerOutput,
    JmapHandler, RegistrationError, SetSupport,
};

/// The properties that `PushSubscription/get` never returns and rejects
/// with `forbidden` when requested (RFC 8620 §7.2.1).
const PRIVATE_PROPERTIES: [&str; 2] = ["url", "keys"];

/// The RFC 8620 §7.2 properties that `PushSubscription/get` may return.
const RFC_PROPERTIES: [&str; 5] = [
    "id",
    "deviceClientId",
    "verificationCode",
    "expires",
    "types",
];

/// The `ids` cap for `PushSubscription/get`.
///
/// ponytail: a fixed RFC 8620 §2 suggested minimum.
/// [`JmapBackend::max_objects_in_get`] is per account and this method has
/// no account, so a backend override of that method does not apply here.
/// The upgrade path is a caller-scoped limit method on
/// [`PushSubscriptionBackend`].
const MAX_OBJECTS_IN_GET: u64 = 500;

/// The `create` + `update` + `destroy` cap for `PushSubscription/set`.
///
/// ponytail: fixed, for the same reason as [`MAX_OBJECTS_IN_GET`], with the
/// same upgrade path.
const MAX_OBJECTS_IN_SET: u64 = 500;

/// The furthest in the future, in seconds from now, that
/// `PushSubscription/set` lets `expires` be: 7 days.
///
/// RFC 8620 §7.2.1 (rfc8620.txt:3711-3715): the server "SHOULD set an
/// expiry time for the push subscription if none is given and limit the
/// expiry time if set too far in the future. This maximum expiry time MUST
/// be at least 48 hours in the future and SHOULD be at least 7 days in the
/// future."
///
/// ponytail: one fixed limit for every caller. RFC 8620 (rfc8620.txt:
/// 3706-3709) says the server SHOULD NOT bound the expiry for session-bound
/// credentials, but MUST expire the subscription with the session; a backend
/// can only shorten `expires` further. The upgrade path is a caller-scoped
/// limit method on [`PushSubscriptionBackend`].
pub const MAX_PUSH_EXPIRY_SECS: i64 = 7 * 86_400;

/// A push subscription as the backend stores it.
///
/// `subscription` is the object as the client sees it, including the
/// private `url` and `keys`, which `PushSubscription/get` strips.
/// `subscription.verification_code` is the code the client echoed back, if
/// any. `expected_code` is the code the server sent in the
/// `PushVerification` object.
///
/// The `Debug` impl redacts `expected_code`.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredPushSubscription {
    /// The subscription, including the private `url` and `keys`.
    pub subscription: PushSubscription,
    /// The verification code the server sent to the push endpoint.
    pub expected_code: String,
}

impl fmt::Debug for StoredPushSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredPushSubscription")
            .field("subscription", &self.subscription)
            .field("expected_code", &"[REDACTED]")
            .finish()
    }
}

/// Storage for RFC 8620 §7.2 push subscriptions, scoped to the caller's
/// credentials rather than to an account.
///
/// The `PushSubscription/set` handler validates the request before it calls
/// the create and update methods: `url` begins with `"https://"`, a create
/// carries no `id` or `verificationCode`, an update changes no immutable
/// property, a new `verificationCode` matches
/// [`StoredPushSubscription::expected_code`], and `expires` is at most
/// [`MAX_PUSH_EXPIRY_SECS`] in the future. A backend may reject a request
/// for its own policy reasons by returning [`BackendSetError::SetError`],
/// which the handler places in the response verbatim.
///
/// # Consumer MUSTs
///
/// These RFC 8620 requirements need I/O, clocks or credential state that
/// this crate does not have. The consumer that implements this trait and
/// sends the pushes MUST meet them:
///
/// - "The server MUST ensure the URL is externally resolvable to avoid
///   server-side request forgery" (rfc8620.txt:4151-4153).
/// - "The rate of creation MUST also be limited" (rfc8620.txt:4175-4177).
/// - "Should these credentials expire or be revoked, the push subscription
///   MUST be destroyed by the JMAP server" (rfc8620.txt:3691-3693).
/// - "When a push subscription is destroyed, the server MUST securely erase
///   the URL and encryption keys from memory and storage as soon as
///   possible" (rfc8620.txt:3725-3727).
/// - Send nothing but the `PushVerification` to a subscription until
///   [`is_deliverable`] is true, and filter each `StateChange` with
///   [`state_change_for`].
pub trait PushSubscriptionBackend: JmapBackend {
    /// The extension property names, beyond those of RFC 8620 §7.2, that
    /// `PushSubscription/get` accepts in its `properties` argument (e.g.
    /// `chatPush`, draft-atwood-jmap-chat-push-00).
    ///
    /// RFC 8620 §5.1 (rfc8620.txt:1609-1611): "If an invalid property is
    /// requested, the call MUST be rejected with an "invalidArguments"
    /// error." The handler rejects any name that is neither an RFC 8620
    /// §7.2 property nor listed here.
    const PUSH_SUBSCRIPTION_EXTENSION_PROPERTIES: &'static [&'static str] = &[];

    /// Extension properties that exist only for a request whose `using`
    /// holds a capability, as `(property, capability URI)` pairs (e.g.
    /// `("emailPush", "urn:ietf:params:jmap:emailpush")`,
    /// draft-ietf-jmap-emailpush-05 §4: "To get or set the emailPush
    /// property, the client MUST include" the capability in `using`).
    ///
    /// A gated property is a valid `PushSubscription/get` property name;
    /// it need not also be in
    /// [`PUSH_SUBSCRIPTION_EXTENSION_PROPERTIES`](Self::PUSH_SUBSCRIPTION_EXTENSION_PROPERTIES).
    /// Without its capability in `using`, `/get` leaves it out of every
    /// object and rejects it in `properties` with `invalidArguments`, and
    /// `/set` rejects a create that sends it, or a patch whose first path
    /// segment names it, with `invalidProperties`. With the capability, an
    /// absent gated property reads as `null` in `/get` and in `created`.
    const PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES: &'static [(&'static str, &'static str)] = &[];

    /// Return every push subscription created with the credentials that
    /// `caller` identifies, and no others.
    ///
    /// RFC 8620 §7.2.1: "The server MUST only return push subscriptions
    /// that were created using the same authentication credentials as for
    /// this "PushSubscription/get" request." The handler cannot tell
    /// credentials apart, so it relies on this method for that MUST.
    ///
    /// Extension properties tied to accounts are returned as they stand
    /// for the caller now. An EmailPush backend runs
    /// `jmap_mail_server::emailpush::prune_email_push` on each subscription
    /// it returns, so an account the caller has lost is not listed
    /// (draft-ietf-jmap-emailpush-05 §4); `/set` patches the listed object,
    /// so the next update persists the pruned map.
    fn list_push_subscriptions(
        &self,
        caller: &Self::CallerCtx,
    ) -> impl Future<Output = Result<Vec<StoredPushSubscription>, Self::Error>> + Send;

    /// Store `new` as a subscription owned by the credentials that `caller`
    /// identifies, and return what was stored.
    ///
    /// `new` has no `id` and no `verificationCode`, and its `expires` is
    /// already limited to [`MAX_PUSH_EXPIRY_SECS`] from now. The backend
    /// MUST:
    ///
    /// - assign `id`;
    /// - generate [`StoredPushSubscription::expected_code`] as a fixed-length
    ///   code carrying at least 128 bits from a CSPRNG. The code "MUST
    ///   contain sufficient entropy to avoid the client being able to guess
    ///   the code via brute force" (rfc8620.txt:3784-3786);
    /// - check its per-user subscription cap atomically with the insert and
    ///   return `overQuota` when it is reached. "The server MUST limit the
    ///   number of push subscriptions any one user may have"
    ///   (rfc8620.txt:4172-4175);
    /// - hand a `PushVerification` with the new id and code to the
    ///   consumer's delivery path.
    ///
    /// The backend may shorten `expires`. In `created` the handler echoes
    /// the returned `id` and `expires`, every property the client did not
    /// send, and every extension property stored with a value other than
    /// the one sent.
    ///
    /// # Extension properties
    ///
    /// The backend validates and normalizes the extension properties in
    /// `new.extra` before storing them, and returns a SetError
    /// ([`BackendSetError::SetError`]) to reject the create; the handler
    /// checks only the `using` gate
    /// ([`PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES`](Self::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES)).
    /// An EmailPush backend MUST call
    /// `jmap_mail_server::emailpush::check_email_push_change`; a chatPush
    /// backend calls `validate_chat_push`. Validating here, in the same call
    /// as the store, keeps it inside a conditional-02 scope.
    fn create_push_subscription(
        &self,
        caller: &Self::CallerCtx,
        new: PushSubscription,
    ) -> impl Future<Output = Result<StoredPushSubscription, BackendSetError<Self::Error>>> + Send;

    /// Replace the caller's subscription whose id is `sub.id` with `sub`,
    /// and return what was stored.
    ///
    /// `sub` is the stored subscription with the client's patch applied and
    /// validated. Return `notFound` if no subscription of the caller has
    /// that id. The backend may shorten `expires`. The handler reports, in
    /// `updated`, a returned `expires` that differs from the one the client
    /// asked for and every extension property stored with a value other
    /// than the patched one.
    ///
    /// Validate and normalize extension properties as for
    /// [`create_push_subscription`](Self::create_push_subscription).
    fn update_push_subscription(
        &self,
        caller: &Self::CallerCtx,
        sub: PushSubscription,
    ) -> impl Future<Output = Result<PushSubscription, BackendSetError<Self::Error>>> + Send;

    /// Delete the caller's subscription with this `id`, or return
    /// `notFound` if the caller has none.
    ///
    /// See the secure-erase MUST in the trait docs.
    fn destroy_push_subscription(
        &self,
        caller: &Self::CallerCtx,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;
}

/// Register the `PushSubscription/get` (RFC 8620 §7.2.1) and
/// `PushSubscription/set` (RFC 8620 §7.2.2) handlers on `dispatcher`.
///
/// Like the `register_*_handlers` functions of the extension server crates,
/// this calls [`Dispatcher::register`], which silently replaces any handler
/// already registered under the same method name.
pub fn register_push_subscription_handlers<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: PushSubscriptionBackend + 'static,
{
    let handler: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
        Arc::clone(&backend),
        |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
            Box::pin(async move {
                handle_push_subscription_get(&*b, &cx.caller, cx.args, &cx.using).await
            }) as HandlerFuture
        },
    ));
    dispatcher.register("PushSubscription/get", handler);

    let handler: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
        backend,
        |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
            Box::pin(async move {
                handle_push_subscription_set(&*b, &cx.caller, cx.args, &cx.using).await
            }) as HandlerFuture
        },
    ));
    dispatcher.register("PushSubscription/set", handler);
}

/// Opt `PushSubscription/set` in to draft-ietf-jmap-conditional-02
/// (`ifUnchangedBy` and `atomic`), after
/// [`register_push_subscription_handlers`] (docs/design/DECISIONS.md
/// decisions 1, 16 and 17).
///
/// Attaches [`SetSupport`] for [`PushSubscription`] to the registered
/// `PushSubscription/set`; it does not re-register it, so extension
/// validation in the backend's create and update keeps running, inside the
/// scope `backend` opens ([`ConditionalBackend::begin_set`]). The scope has
/// no account ([`SetScopeRequest::account_id`](crate::SetScopeRequest) is
/// `None`): key it by the caller's credentials. The dispatcher reads
/// preconditions through `PushSubscription/get`, so a pointer to `url` or
/// `keys` is `forbidden` (conditional-02 §9.1), and `ifInState` stays
/// ignored (RFC 8620 §7.2.2; `/get` has no `state`).
///
/// # Errors
///
/// [`RegistrationError::NotRegistered`] when `PushSubscription/set` is
/// not registered yet.
pub fn register_push_subscription_conditional<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) -> Result<(), RegistrationError>
where
    B: PushSubscriptionBackend + ConditionalBackend + 'static,
{
    dispatcher.attach_set_support(
        "PushSubscription/set",
        SetSupport::new::<PushSubscription, B>(backend),
    )
}

/// `PushSubscription/get` (RFC 8620 §7.2.1).
///
/// A standard `/get` without `accountId` in the arguments or the response,
/// and without `state` in the response. A null `ids` returns all of the
/// caller's subscriptions. `url` and `keys` are never returned, and
/// requesting either in `properties` fails the call with `forbidden`.
async fn handle_push_subscription_get<B: PushSubscriptionBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("arguments must be an object"));
    };
    enforce_max_objects_in_get(&args, MAX_OBJECTS_IN_GET)?;

    let ids: Option<Vec<Id>> = optional_arg(&mut args, "ids", || {
        JmapError::invalid_arguments("ids must be an Id array")
    })?;
    let properties: Option<Vec<String>> = optional_arg(&mut args, "properties", || {
        JmapError::invalid_arguments("properties must be a string array")
    })?;
    if properties
        .iter()
        .flatten()
        .any(|p| PRIVATE_PROPERTIES.contains(&p.as_str()))
    {
        return Err(JmapError::forbidden());
    }
    // RFC 8620 §5.1 (rfc8620.txt:1609-1611): "If an invalid property is
    // requested, the call MUST be rejected with an "invalidArguments" error."
    // A gated property without its capability is not a property of this
    // request (draft-ietf-jmap-emailpush-05 §4).
    if let Some(bad) = properties.iter().flatten().find(|p| {
        let p = p.as_str();
        gated_off::<B>(p, using)
            || !RFC_PROPERTIES.contains(&p)
                && !B::PUSH_SUBSCRIPTION_EXTENSION_PROPERTIES.contains(&p)
                && !is_gated::<B>(p)
    }) {
        return Err(JmapError::invalid_arguments(format!(
            "unknown PushSubscription property: {bad}"
        )));
    }

    let stored = backend
        .list_push_subscriptions(caller)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let mut subs: Vec<PushSubscription> = stored.into_iter().map(|s| s.subscription).collect();

    let mut not_found = Vec::new();
    if let Some(ids) = ids {
        let mut found = Vec::with_capacity(ids.len());
        let mut seen = HashSet::with_capacity(ids.len());
        for id in ids {
            // RFC 8620 §5.1 (rfc8620.txt:1648-1651): an id given more than
            // once appears only once in "list" or "notFound".
            if !seen.insert(id.clone()) {
                continue;
            }
            match subs.iter().find(|s| s.id.as_ref() == Some(&id)) {
                Some(s) => found.push(s.clone()),
                None => not_found.push(id),
            }
        }
        subs = found;
    }

    let list = subs
        .into_iter()
        .map(|mut sub| {
            sub.url = None;
            sub.keys = None;
            let mut value = serialize_value(&sub)?;
            if let Value::Object(obj) = &mut value {
                apply_gates::<B>(obj, using);
            }
            if let (Some(props), Value::Object(obj)) = (&properties, &mut value) {
                // RFC 8620 §5.1 (rfc8620.txt:1608-1609): "The id property of
                // the object is *always* returned, even if not explicitly
                // requested."
                obj.retain(|key, _| key == "id" || props.contains(key));
            }
            Ok(value)
        })
        .collect::<Result<Vec<Value>, JmapError>>()?;

    Ok(HandlerOutput::new(json!({
        "list": list,
        "notFound": not_found_json(&not_found),
    })))
}

/// `PushSubscription/set` (RFC 8620 §7.2.2).
///
/// RFC 8620 §7.2.2 (rfc8620.txt:3761-3764): "Standard /set method as
/// described in Section 5.3, except it does *not* take or return an
/// "accountId" argument, as push subscriptions are not tied to specific
/// accounts. It also does *not* take an "ifInState" argument or return
/// "oldState" or "newState" arguments." Those arguments are ignored if
/// sent.
///
/// Creates, then updates, then destroys, as RFC 8620 §5.3 orders them.
async fn handle_push_subscription_set<B: PushSubscriptionBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("arguments must be an object"));
    };
    enforce_max_objects_in_set(&args, MAX_OBJECTS_IN_SET)?;

    let create: Option<Map<String, Value>> = optional_arg(&mut args, "create", || {
        JmapError::invalid_arguments("create must be an object")
    })?;
    let update: Option<Map<String, Value>> = optional_arg(&mut args, "update", || {
        JmapError::invalid_arguments("update must be an object")
    })?;
    let destroy: Option<Vec<Id>> = optional_arg(&mut args, "destroy", || {
        JmapError::invalid_arguments("destroy must be an Id array")
    })?;

    // Read the stored subscriptions before any create is committed, so a
    // list failure returns a method-level serverFail with no state changed
    // (RFC 8620 §3.6.2, rfc8620.txt:1158-1160). An update cannot name a
    // same-call create: its server-assigned id is unknown to the client.
    // ponytail: one O(n) list per call instead of a lookup by id. The
    // upgrade path is a get_push_subscription method on the trait.
    let update_with_stored = match update.filter(|u| !u.is_empty()) {
        Some(update) => {
            let stored = backend
                .list_push_subscriptions(caller)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            Some((update, stored))
        }
        None => None,
    };

    let mut created = Map::new();
    let mut not_created = Map::new();
    for (creation_id, obj) in create.unwrap_or_default() {
        let sent: Map<String, Value> = obj.as_object().cloned().unwrap_or_default();
        if let Some(p) = sent.keys().find(|p| gated_off::<B>(p, using)) {
            not_created.insert(creation_id, invalid_property(p));
            continue;
        }
        let new = match validate_create(obj) {
            Ok(new) => new,
            Err(err) => {
                not_created.insert(creation_id, err);
                continue;
            }
        };
        match backend.create_push_subscription(caller, new).await {
            Ok(stored) => {
                let mut value = created_value(&stored.subscription, &sent);
                if let Value::Object(obj) = &mut value {
                    // Absent gated properties read as null (their default);
                    // a sent one is echoed only if stored differently.
                    for (p, _) in B::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES {
                        if !gated_off::<B>(p, using) && !sent.contains_key(*p) {
                            obj.entry(*p).or_insert(Value::Null);
                        }
                    }
                }
                created.insert(creation_id, value);
            }
            Err(e) => {
                not_created.insert(creation_id, backend_set_error_value(e));
            }
        }
    }

    let mut updated = Map::new();
    let mut not_updated = Map::new();
    if let Some((update, stored)) = update_with_stored {
        for (id, patch) in update {
            let current = stored
                .iter()
                .find(|s| s.subscription.id.as_ref().map(AsRef::as_ref) == Some(id.as_str()));
            match update_one::<B>(backend, caller, current, patch, using).await {
                Ok(changed) => {
                    updated.insert(id, changed);
                }
                Err(err) => {
                    not_updated.insert(id, err);
                }
            }
        }
    }

    let mut destroyed = Vec::new();
    let mut not_destroyed = Map::new();
    for id in destroy.unwrap_or_default() {
        match backend.destroy_push_subscription(caller, &id).await {
            Ok(()) => destroyed.push(Value::String(id.as_ref().to_owned())),
            Err(e) => {
                not_destroyed.insert(id.as_ref().to_owned(), backend_set_error_value(e));
            }
        }
    }

    let or_null = |m: Map<String, Value>| {
        if m.is_empty() {
            Value::Null
        } else {
            Value::Object(m)
        }
    };
    Ok(HandlerOutput::new(json!({
        "created": or_null(created),
        "updated": or_null(updated),
        "destroyed": if destroyed.is_empty() { Value::Null } else { Value::Array(destroyed) },
        "notCreated": or_null(not_created),
        "notUpdated": or_null(not_updated),
        "notDestroyed": or_null(not_destroyed),
    })))
}

/// The `created` entry for `sub`: every property the client did not send,
/// plus `id` and `expires`, which the server always sets, and every
/// extension property the backend stored with a value other than the one
/// sent (RFC 8620 §5.3: properties "set to a default by the server", and a
/// backend that normalizes, e.g. fills an EmailPush default, sets one).
///
/// RFC 8620 §5.3 (rfc8620.txt:2034-2039): "created" holds "any properties
/// of the created Foo object that were not sent by the client. This
/// includes all server-set properties (such as the "id" in most object
/// types) and any properties that were omitted by the client and thus set
/// to a default by the server." `url` is always sent, so the url is never
/// echoed, and `keys` is echoed only when it was omitted (and so is null).
fn created_value(sub: &PushSubscription, sent: &Map<String, Value>) -> Value {
    let mut props = Map::new();
    props.insert("id".to_owned(), json!(sub.id));
    props.insert("deviceClientId".to_owned(), json!(sub.device_client_id));
    props.insert("url".to_owned(), json!(sub.url));
    props.insert("keys".to_owned(), json!(sub.keys));
    props.insert("verificationCode".to_owned(), json!(sub.verification_code));
    props.insert("expires".to_owned(), json!(sub.expires));
    props.insert("types".to_owned(), json!(sub.types));
    props.retain(|k, _| k == "id" || k == "expires" || !sent.contains_key(k));
    for (k, v) in &sub.extra {
        if sent.get(k) != Some(v) {
            props.insert(k.clone(), v.clone());
        }
    }
    // A sent extension property the backend dropped is now null.
    for k in sent.keys() {
        if !RFC_PROPERTIES.contains(&k.as_str())
            && !PRIVATE_PROPERTIES.contains(&k.as_str())
            && k != "verificationCode"
            && !sub.extra.contains_key(k)
            && !sent[k].is_null()
        {
            props.insert(k.clone(), Value::Null);
        }
    }
    Value::Object(props)
}

/// Check one `create` entry and turn it into the subscription to store,
/// with `expires` limited to [`MAX_PUSH_EXPIRY_SECS`] from now.
///
/// Returns the `notCreated` SetError on failure.
fn validate_create(obj: Value) -> Result<PushSubscription, Value> {
    let Value::Object(fields) = &obj else {
        return Err(Value::from(
            &SetError::new(SetErrorType::InvalidProperties)
                .with_description("a create entry must be an object"),
        ));
    };
    // RFC 8620 §1.1 (rfc8620.txt:261-262): the client MUST NOT send a
    // server-set property when creating.
    if let Some(err) = reject_server_set::<PushSubscription>(fields) {
        return Err(Value::from(&err));
    }
    // RFC 8620 §7.2 (rfc8620.txt:3620-3621): "This MUST begin with
    // "https://"." draft-atwood-jmap-updates-00 §7.1: it is an RFC 3986
    // URI, so an IRI or a malformed URL is rejected too.
    if !fields
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|url| url.starts_with("https://") && jmap_types::uri::check_uri(url).is_ok())
    {
        return Err(invalid_property("url"));
    }
    // RFC 8620 §7.2 (rfc8620.txt:3649): "This MUST be null (or omitted) when
    // the subscription is created."
    if fields.get("verificationCode").is_some_and(|v| !v.is_null()) {
        return Err(invalid_property("verificationCode"));
    }
    let mut new: PushSubscription = serde_json::from_value(obj).map_err(|e| {
        Value::from(&SetError::new(SetErrorType::InvalidProperties).with_description(e.to_string()))
    })?;
    new.expires = Some(limit_expires(new.expires.as_ref())?);
    Ok(new)
}

/// Apply one `update` entry to `current` and store the result.
///
/// Returns the `updated` value on success (`{"expires": ...}` when the
/// stored expiry differs from the one the client asked for, else null) and
/// the `notUpdated` SetError on failure.
async fn update_one<B: PushSubscriptionBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    current: Option<&StoredPushSubscription>,
    patch: Value,
    using: &[String],
) -> Result<Value, Value> {
    // draft-ietf-jmap-emailpush-05 §4: without its capability a gated
    // property cannot be set; checked before the other validation.
    if let Value::Object(patch) = &patch
        && let Some(p) = patch
            .keys()
            .map(|k| k.split('/').next().unwrap_or(k))
            .find(|p| gated_off::<B>(p, using))
    {
        return Err(invalid_property(p));
    }
    let Some(current) = current else {
        return Err(Value::from(&SetError::new(SetErrorType::NotFound)));
    };
    let Value::Object(patch) = patch else {
        return Err(Value::from(
            &SetError::new(SetErrorType::InvalidPatch)
                .with_description("a PatchObject must be an object"),
        ));
    };
    let Value::Object(mut target) =
        serialize_value(&current.subscription).map_err(|e| server_fail_value_from_backend(&e))?
    else {
        return Err(server_fail_value_from_backend(
            "PushSubscription did not serialize to an object",
        ));
    };
    let before = target.clone();
    apply_patch_object(&mut target, &patch).map_err(|e| {
        Value::from(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    // RFC 8620 §5.3 (rfc8620.txt:2141-2145): sending an immutable or
    // server-set property is an error only if the value differs.
    let changed_read_only = PushSubscription::SERVER_SET
        .iter()
        .chain(PushSubscription::IMMUTABLE)
        .find(|p| before.get(**p) != target.get(**p));
    let mut patched: PushSubscription =
        serde_json::from_value(Value::Object(target)).map_err(|e| {
            Value::from(
                &SetError::new(SetErrorType::InvalidProperties).with_description(e.to_string()),
            )
        })?;

    if let Some(property) = changed_read_only {
        return Err(invalid_property(property));
    }
    let old = &current.subscription;

    // RFC 8620 §7.2.2 (rfc8620.txt:3790-3792): "Attempts to update the
    // subscription with an invalid verification code MUST be rejected by
    // the server with an "invalidProperties" SetError."
    if patched.verification_code != old.verification_code
        && let Some(code) = &patched.verification_code
        && !codes_match(code, &current.expected_code)
    {
        return Err(invalid_property("verificationCode"));
    }

    // RFC 8620 §7.2.2 (rfc8620.txt:3794-3798): "The server MAY modify the
    // proposed new expiry time to enforce server-defined limits. Extending
    // the lifetime does not require the subscription to be verified again."
    let requested_expires = patched.expires.clone();
    let requested_extra = patched.extra.clone();
    if requested_expires != old.expires {
        patched.expires = Some(limit_expires(requested_expires.as_ref())?);
    }

    let stored = backend
        .update_push_subscription(caller, patched)
        .await
        .map_err(backend_set_error_value)?;
    // RFC 8620 §5.3 (rfc8620.txt:2048-2051): "updated" holds "any property
    // that changed in a way *not* explicitly requested by the PatchObject":
    // a shortened `expires`, and extension properties the backend stored
    // other than as patched.
    let mut changed = Map::new();
    if stored.expires != requested_expires {
        changed.insert("expires".to_owned(), json!(stored.expires));
    }
    for (k, v) in &stored.extra {
        if requested_extra.get(k) != Some(v) {
            changed.insert(k.clone(), v.clone());
        }
    }
    for k in requested_extra.keys() {
        if !stored.extra.contains_key(k) {
            changed.insert(k.clone(), Value::Null);
        }
    }
    Ok(if changed.is_empty() {
        Value::Null
    } else {
        Value::Object(changed)
    })
}

/// Whether `property` is gated on a capability
/// ([`PushSubscriptionBackend::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES`]).
fn is_gated<B: PushSubscriptionBackend>(property: &str) -> bool {
    B::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES
        .iter()
        .any(|(p, _)| *p == property)
}

/// Whether `property` is gated on a capability that `using` lacks, so it
/// does not exist for this request.
fn gated_off<B: PushSubscriptionBackend>(property: &str, using: &[String]) -> bool {
    B::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES
        .iter()
        .any(|(p, uri)| *p == property && !using.iter().any(|u| u == uri))
}

/// Remove the gated properties `using` lacks the capability for from a
/// `/get` object, and show the others as `null` when absent (their
/// default).
fn apply_gates<B: PushSubscriptionBackend>(obj: &mut Map<String, Value>, using: &[String]) {
    for (p, _) in B::PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES {
        if gated_off::<B>(p, using) {
            obj.remove(*p);
        } else {
            obj.entry(*p).or_insert(Value::Null);
        }
    }
}

/// `requested`, or `None`'s default, limited to [`MAX_PUSH_EXPIRY_SECS`]
/// from now.
///
/// Returns `invalidProperties` naming `expires` if `requested` is not a
/// valid UTCDate, and `serverFail` if the system clock cannot be read.
fn limit_expires(requested: Option<&UTCDate>) -> Result<UTCDate, Value> {
    let clock_fail = || server_fail_value_from_backend("system clock out of range");
    let (now, _) = signed_seconds_since_epoch(SystemTime::now()).ok_or_else(clock_fail)?;
    let max = now
        .checked_add(MAX_PUSH_EXPIRY_SECS)
        .ok_or_else(clock_fail)?;
    if let Some(requested) = requested {
        let secs = requested
            .to_epoch_seconds()
            .map_err(|_| invalid_property("expires"))?;
        if secs <= max {
            return Ok(requested.clone());
        }
    }
    utc_date_from_epoch_seconds(max).ok_or_else(clock_fail)
}

/// Whether a push may be sent to `stored` at `now_secs` seconds since the
/// Unix epoch: the subscription is verified and has not expired.
///
/// RFC 8620 §7.2 (rfc8620.txt:3578-3581): "The JMAP server MUST NOT make
/// any further requests to the URL until the client receives the push and
/// updates the subscription with the correct verification code."
///
/// RFC 8620 §7.2 (rfc8620.txt:3656-3658): "If specified, the JMAP server
/// MUST NOT make further requests to this resource after this time." A push
/// at exactly `expires` is allowed. An `expires` that is not a valid
/// UTCDate counts as expired.
///
/// This does not cover the `PushVerification` itself, which is sent before
/// the subscription is verified.
pub fn is_deliverable(stored: &StoredPushSubscription, now_secs: i64) -> bool {
    let sub = &stored.subscription;
    let verified = sub
        .verification_code
        .as_deref()
        .is_some_and(|code| codes_match(code, &stored.expected_code));
    let unexpired = match &sub.expires {
        None => true,
        Some(expires) => expires
            .to_epoch_seconds()
            .is_ok_and(|expires| now_secs <= expires),
    };
    verified && unexpired
}

/// The `StateChange` to push to `sub` for the `changed` map, or `None` if
/// none of its types changed.
///
/// RFC 8620 §7.2 (rfc8620.txt:3669-3672): "A StateChange notification will
/// only be sent if the data for one of these types changes. Other types are
/// omitted from the TypeState object. If null, changes will be pushed for
/// all types." Accounts left with no types are dropped.
///
/// This does not check which accounts the subscription's credentials may
/// see; the caller passes only those.
pub fn state_change_for(
    sub: &PushSubscription,
    changed: &BTreeMap<Id, BTreeMap<String, State>>,
) -> Option<StateChange> {
    let filtered: BTreeMap<Id, BTreeMap<String, State>> = changed
        .iter()
        .map(|(account, types)| {
            let kept: BTreeMap<String, State> = types
                .iter()
                .filter(|(t, _)| sub.types.as_ref().is_none_or(|wanted| wanted.contains(t)))
                .map(|(t, s)| (t.clone(), s.clone()))
                .collect();
            (account.clone(), kept)
        })
        .filter(|(_, kept)| !kept.is_empty())
        .collect();
    (!filtered.is_empty()).then(|| StateChange::new(filtered))
}

/// Compare a client-sent verification code with the expected one in
/// constant time for equal lengths. Codes are fixed-length (see
/// [`PushSubscriptionBackend::create_push_subscription`]), so a length
/// mismatch reveals nothing secret.
fn codes_match(sent: &str, expected: &str) -> bool {
    sent.as_bytes().ct_eq(expected.as_bytes()).into()
}

/// An `invalidProperties` SetError naming `property`.
fn invalid_property(property: &str) -> Value {
    Value::from(&SetError::new(SetErrorType::InvalidProperties).with_properties([property]))
}
