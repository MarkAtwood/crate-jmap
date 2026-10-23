//! The optional presence methods of draft-atwood-jmap-chat-federation-00:
//! §Peer/subscribePresence, §Peer/unsubscribePresence and §Peer/presence.
//!
//! All three are optional ("Servers MAY implement this method; callers
//! receive `unknownMethod` ... from servers that do not"). A deployment
//! that implements them implements [`FederationPresenceBackend`] and
//! registers the handlers; one that does not leaves them unregistered, and
//! its dispatcher answers `unknownMethod`.

use jmap_chat_federation_types::{
    PeerAck, PeerPresenceArgs, PeerSubscribePresenceArgs, PeerSubscribePresenceResponse,
    PeerUnsubscribePresenceArgs,
};
use jmap_chat_types::{ChatContact, ChatPresenceEvent, Clearable, Presence, PresenceStatus};
use jmap_server::{HandlerOutput, serialize_value, server_fail_from_backend};
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{Map, Value, json};

use crate::FederationBackend;
use crate::deliver::{Identities, Who, authenticate, check_account, set_failure};

/// What the presence methods need beyond [`FederationBackend`]: the
/// subscription store and the owner's client push.
///
/// None of these methods has a default: a deployment that cannot keep
/// subscriptions or reach its clients does not implement the optional
/// methods at all.
pub trait FederationPresenceBackend: FederationBackend {
    /// Record or renew `subscriber_id`'s subscription to the presence of
    /// the owner of `account_id`, and return the granted lifetime in
    /// seconds.
    ///
    /// §Peer/subscribePresence: "Servers MAY cap or adjust this value. If
    /// absent, the server assigns an implementation-defined default TTL.
    /// Subscriptions that are not renewed before expiry SHOULD be silently
    /// dropped by the server." and "Calling `Peer/subscribePresence` again
    /// before the TTL expires renews the subscription and SHOULD reset the
    /// TTL to the newly granted value."
    ///
    /// Pushing `Peer/presence` to subscribers, and the per-subscriber rate
    /// limit on it ("Servers SHOULD rate-limit outbound `Peer/presence`
    /// calls per subscriber"), belong to the deployment's sending side.
    fn subscribe_presence(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        subscriber_id: &str,
        ttl: Option<u64>,
    ) -> impl std::future::Future<Output = Result<u64, Self::Error>> + Send;

    /// Cancel `subscriber_id`'s subscription to the owner of `account_id`.
    ///
    /// §Peer/unsubscribePresence: "Servers MUST succeed if no active
    /// subscription exists for the given `subscriberId`", so having
    /// nothing to cancel is `Ok(())`.
    fn unsubscribe_presence(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        subscriber_id: &str,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;

    /// Send `event` to the connected clients of the owner of `account_id`.
    ///
    /// §Peer/presence "Receiver Behavior": "The server MUST then fire a
    /// local presence push event (as defined in {{JMAP-CHAT}}) to the
    /// owner's connected clients." [`handle_peer_presence`] calls this
    /// after updating the ChatContact. The event is not stored.
    fn forward_presence(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        event: &ChatPresenceEvent,
    ) -> impl std::future::Future<Output = ()> + Send;
}

/// Handle a `Peer/subscribePresence` method call.
///
/// - `subscriberId` must be the peer identity, else `forbidden` ("MUST
///   match the peer identity; the remote server MUST reject the request
///   if they differ").
/// - The response carries the owner's PresenceStatus `presence`,
///   `statusText` and `statusEmoji`, with `"invisible"` reported as
///   `"offline"` (§Peer/presence: "`"invisible"` is never sent to a
///   peer"). An account with no PresenceStatus reports `"offline"`.
///   PresenceStatus has no `lastActiveAt`, so the response has none.
/// - `ttl` is what [`FederationPresenceBackend::subscribe_presence`]
///   granted.
///
/// # Errors
///
/// `invalidArguments`, `forbidden` and `serverFail`; `accountNotFound`
/// for an unknown `accountId`.
pub async fn handle_peer_subscribe_presence<B: FederationPresenceBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerSubscribePresenceArgs = serde_json::from_value(args).map_err(|e| {
        JmapError::invalid_arguments(format!("Peer/subscribePresence arguments: {e}"))
    })?;
    authenticate(backend, caller, &args.subscriber_id)?;
    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;

    let (statuses, _) = backend
        .get_objects::<PresenceStatus>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let status = statuses.into_iter().next();
    let ttl = backend
        .subscribe_presence(caller, account_id, &args.subscriber_id, args.ttl)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let response = match status {
        Some(s) => {
            let presence = match s.presence {
                Presence::Invisible => Presence::Offline,
                p => p,
            };
            let mut r = PeerSubscribePresenceResponse::new(account_id.clone(), presence, ttl);
            r.status_text = s.status_text;
            r.status_emoji = s.status_emoji;
            r
        }
        None => PeerSubscribePresenceResponse::new(account_id.clone(), Presence::Offline, ttl),
    };
    Ok(HandlerOutput::new(serialize_value(response)?))
}

/// Handle a `Peer/unsubscribePresence` method call.
///
/// `subscriberId` must be the peer identity, else `forbidden` ("MUST
/// match the peer identity"). The subscription is then cancelled through
/// [`FederationPresenceBackend::unsubscribe_presence`]; the call succeeds
/// whether or not one existed.
///
/// # Errors
///
/// `invalidArguments`, `forbidden` and `serverFail`; `accountNotFound`
/// for an unknown `accountId`.
pub async fn handle_peer_unsubscribe_presence<B: FederationPresenceBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerUnsubscribePresenceArgs = serde_json::from_value(args).map_err(|e| {
        JmapError::invalid_arguments(format!("Peer/unsubscribePresence arguments: {e}"))
    })?;
    authenticate(backend, caller, &args.subscriber_id)?;
    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;
    backend
        .unsubscribe_presence(caller, account_id, &args.subscriber_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    Ok(HandlerOutput::new(serialize_value(PeerAck::new(
        account_id.clone(),
    ))?))
}

/// Handle a `Peer/presence` method call (§Peer/presence "Receiver
/// Behavior").
///
/// - `identity` must be the peer identity, else `forbidden` ("MUST match
///   the peer identity; the receiver MUST reject the request if they
///   differ").
/// - `presence` must be `"online"`, `"away"`, `"busy"` or `"offline"`,
///   else `invalidArguments` ("One of `"online"`, `"away"`, `"busy"`, or
///   `"offline"`").
/// - A `blocked` ChatContact with that identity is dropped silently:
///   success, nothing updated, no event.
/// - With no ChatContact of that identity there is no record to update:
///   success, nothing created, no event.
/// - Otherwise the ChatContact's `presence` is set, `lastActiveAt` when
///   sent, and `statusText` and `statusEmoji` when sent (`null` clears
///   them; absent leaves them unchanged). Then
///   [`FederationPresenceBackend::forward_presence`] gets a presence
///   event with the contact's id and its updated values, `null` for a
///   status field it does not have (draft-atwood-jmap-chat-00 §Presence
///   Events: "they are `null` when the contact has no active custom
///   status").
///
/// # Errors
///
/// `invalidArguments`, `forbidden` and `serverFail`; `accountNotFound`
/// for an unknown `accountId`.
pub async fn handle_peer_presence<B: FederationPresenceBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerPresenceArgs = serde_json::from_value(args)
        .map_err(|e| JmapError::invalid_arguments(format!("Peer/presence arguments: {e}")))?;
    authenticate(backend, caller, &args.identity)?;
    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;
    if !matches!(
        args.presence,
        Presence::Online | Presence::Away | Presence::Busy | Presence::Offline
    ) {
        return Err(JmapError::invalid_arguments(
            "presence must be \"online\", \"away\", \"busy\" or \"offline\"",
        ));
    }
    let ack = HandlerOutput::new(serialize_value(PeerAck::new(account_id.clone()))?);

    let identities = Identities::load(backend, caller, account_id).await?;
    let Who::Contact(contact) = identities.lookup(&args.identity) else {
        return Ok(ack);
    };
    if contact.blocked {
        return Ok(ack);
    }

    let mut patch = Map::new();
    patch.insert("presence".into(), json!(args.presence));
    if let Some(t) = &args.last_active_at {
        patch.insert("lastActiveAt".into(), json!(t));
    }
    for (key, value) in [
        ("statusText", &args.status_text),
        ("statusEmoji", &args.status_emoji),
    ] {
        match value {
            Some(Clearable::Set(v)) => patch.insert(key.into(), json!(v)),
            Some(Clearable::Clear) => patch.insert(key.into(), Value::Null),
            None => None,
        };
    }
    backend
        .update_object::<ChatContact>(
            caller,
            account_id,
            &contact.id,
            PatchObject::from_map(patch),
        )
        .await
        .map_err(set_failure)?;
    let (contacts, _) = backend
        .get_objects::<ChatContact>(
            caller,
            account_id,
            Some(std::slice::from_ref(&contact.id)),
            None,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let Some(updated) = contacts.into_iter().find(|c| c.id == contact.id) else {
        return Ok(ack);
    };

    let event: ChatPresenceEvent = serde_json::from_value(json!({
        "accountId": account_id,
        "contactId": updated.id,
        "presence": updated.presence,
        "lastActiveAt": updated.last_active_at,
        "statusText": updated.status_text,
        "statusEmoji": updated.status_emoji,
    }))
    .map_err(|e| JmapError::server_fail(format!("presence event: {e}")))?;
    backend.forward_presence(caller, account_id, &event).await;
    Ok(ack)
}
