//! RFC 8620 §7.2 push subscriptions: the caller-scoped storage trait and
//! the `PushSubscription/get` handler (bd:JMAP-nbk3.2).
//!
//! Push subscriptions are not tied to an account (RFC 8620 §7.2.1), so the
//! handler here does not use `accountId`, [`JmapBackend::account_exists`],
//! or [`JmapBackend::get_state`]. Instead it scopes every lookup to the
//! caller's credentials through [`PushSubscriptionBackend`].

use std::{collections::HashSet, fmt, future::Future, sync::Arc};

use jmap_types::{Id, Invocation, JmapError, PushSubscription};
use serde_json::{json, Value};

use crate::backend::JmapBackend;
use crate::handlers::server_fail_from_backend;
use crate::helpers::{enforce_max_objects_in_get, not_found_json, optional_arg, serialize_value};
use crate::{ClosureHandler, Dispatcher, HandlerFuture, JmapHandler};

/// The properties that `PushSubscription/get` never returns and rejects
/// with `forbidden` when requested (RFC 8620 §7.2.1).
const PRIVATE_PROPERTIES: [&str; 2] = ["url", "keys"];

/// The `ids` cap for `PushSubscription/get`.
///
/// ponytail: a fixed RFC 8620 §2 suggested minimum.
/// [`JmapBackend::max_objects_in_get`] is per account and this method has
/// no account, so a backend override of that method does not apply here.
/// The upgrade path is a caller-scoped limit method on
/// [`PushSubscriptionBackend`].
const MAX_OBJECTS_IN_GET: u64 = 500;

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
pub trait PushSubscriptionBackend: JmapBackend {
    /// Return every push subscription created with the credentials that
    /// `caller` identifies, and no others.
    ///
    /// RFC 8620 §7.2.1: "The server MUST only return push subscriptions
    /// that were created using the same authentication credentials as for
    /// this "PushSubscription/get" request." The handler cannot tell
    /// credentials apart, so it relies on this method for that MUST.
    fn list_push_subscriptions(
        &self,
        caller: &Self::CallerCtx,
    ) -> impl Future<Output = Result<Vec<StoredPushSubscription>, Self::Error>> + Send;
}

/// Register the `PushSubscription/get` handler (RFC 8620 §7.2.1) on
/// `dispatcher`.
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
        backend,
        |b: Arc<B>, _call_id: String, args: Value, ctx: B::CallerCtx| {
            Box::pin(async move { handle_push_subscription_get(&*b, &ctx, args).await })
                as HandlerFuture
        },
    ));
    dispatcher.register("PushSubscription/get", handler);
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
) -> Result<(Value, Vec<Invocation>), JmapError> {
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
            if let (Some(props), Value::Object(obj)) = (&properties, &mut value) {
                // RFC 8620 §5.1 (rfc8620.txt:1608-1609): "The id property of
                // the object is *always* returned, even if not explicitly
                // requested."
                obj.retain(|key, _| key == "id" || props.contains(key));
            }
            Ok(value)
        })
        .collect::<Result<Vec<Value>, JmapError>>()?;

    Ok((
        json!({
            "list": list,
            "notFound": not_found_json(&not_found),
        }),
        vec![],
    ))
}
