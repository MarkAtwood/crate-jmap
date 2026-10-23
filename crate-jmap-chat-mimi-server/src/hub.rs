//! draft-atwood-jmap-chat-mimi-00 §KeyMaterial/fetch, §Room/update,
//! §Room/send, §Room/unbind and §GroupInfo/fetch: the methods that reach a
//! room's hub through [`MimiBackend::mimi_request`].

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_chat_mimi_types::{
    Base64Url, ClientKeyMaterial, GroupInfoCode, GroupInfoFetchResponse, KeyMaterialClientCode,
    KeyMaterialFetchResponse, KeyMaterialUserCode, RoomSendResponse, RoomUnbindResponse,
    RoomUpdateResponse, SubmitResponseCode, UpdateResponseCode,
};
use jmap_server::{HandlerOutput, extract_account_id, server_fail_from_backend};
use jmap_types::{Id, JmapError, UTCDate};
use serde_json::{Map, Value};

use crate::fanout::{parse_fanout_messages, utc_date_from_millis};
use crate::handout::KeyMaterialRequirements;
use crate::wire::{self, SubmitResponse, UpdateResponse};
use crate::{HubReply, MimiBackend, MimiEndpoint};

fn bad(description: &str) -> JmapError {
    JmapError::invalid_arguments(description)
}

fn b64(bytes: &[u8]) -> Base64Url {
    Base64Url::try_from(URL_SAFE_NO_PAD.encode(bytes))
        .expect("the base64url engine emits only base64url characters")
}

fn output(response: Result<Value, serde_json::Error>) -> HandlerOutput {
    HandlerOutput::new(response.expect("derive(Serialize) on plain data is infallible"))
}

/// `accountId` after checking that the account exists, and the other
/// arguments.
async fn account<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Id, Map<String, Value>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    Ok((account_id, args))
}

fn string_arg<'a>(args: &'a Map<String, Value>, name: &str) -> Result<&'a str, JmapError> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(&format!("{name} must be a string")))
}

fn octets_arg(args: &Map<String, Value>, name: &str) -> Result<Vec<u8>, JmapError> {
    args.get(name)
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Base64Url>().ok())
        .and_then(|s| URL_SAFE_NO_PAD.decode(s.as_str()).ok())
        .ok_or_else(|| bad(&format!("{name} must be an unpadded base64url string")))
}

/// Reject unless a Chat in the account is bound to `room_id`.
async fn require_bound<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    room_id: &str,
) -> Result<(), JmapError> {
    if backend
        .room_bound(caller, account_id, room_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        Ok(())
    } else {
        Err(bad("no Chat in the account is bound to roomId"))
    }
}

async fn send<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    room_id: &str,
    endpoint: MimiEndpoint,
    body: &[u8],
) -> Result<HubReply, JmapError> {
    backend
        .mimi_request(caller, account_id, room_id, &endpoint, body)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
        .map_err(|_| JmapError::server_unavailable())
}

/// A hub response that does not parse is the hub's fault, not the
/// client's.
fn malformed_reply(what: &str) -> impl FnOnce(String) -> JmapError + '_ {
    move |e| JmapError::server_fail(format!("the {what} does not parse: {e}"))
}

fn utc_date(ms: u64) -> Result<UTCDate, JmapError> {
    utc_date_from_millis(ms)
        .ok_or_else(|| JmapError::server_fail("accepted_timestamp is past year 9999"))
}

/// Store the FanoutMessage `timestamp`, `message`, `tail` in every Chat of
/// a local account bound to `room_id`, from the hub `hub_uri`.
async fn store_own<B: MimiBackend>(
    backend: &B,
    room_id: &str,
    hub_uri: &str,
    timestamp: u64,
    message: &[u8],
    tail: &[u8],
) -> Result<(), JmapError> {
    let body = [&timestamp.to_be_bytes()[..], message, tail].concat();
    for fanout in parse_fanout_messages(&body)
        .map_err(|e| JmapError::server_fail(format!("FanoutMessage: {e}")))?
    {
        backend
            .store_fanout(room_id, hub_uri, &fanout)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
    }
    Ok(())
}

const USER_CODES: [KeyMaterialUserCode; 8] = [
    KeyMaterialUserCode::Success,
    KeyMaterialUserCode::PartialSuccess,
    KeyMaterialUserCode::IncompatibleProtocol,
    KeyMaterialUserCode::NoCompatibleMaterial,
    KeyMaterialUserCode::UserUnknown,
    KeyMaterialUserCode::NoConsent,
    KeyMaterialUserCode::NoConsentForThisRoom,
    KeyMaterialUserCode::UserDeleted,
];

const CLIENT_CODES: [KeyMaterialClientCode; 3] = [
    KeyMaterialClientCode::Success,
    KeyMaterialClientCode::KeyMaterialExhausted,
    KeyMaterialClientCode::NothingCompatible,
];

/// The `userStatus` of a KeyMaterialResponse this server answers itself:
/// `success` "for every client of the target user", `partialSuccess` "for
/// at least one client", and otherwise `noCompatibleMaterial`, the one
/// failure code that may list the clients (draft-ietf-mimi-protocol-06
/// §5.2), whose `clientStatus` says which are exhausted.
fn local_user_status(clients: &[ClientKeyMaterial]) -> KeyMaterialUserCode {
    let success = |c: &ClientKeyMaterial| c.client_status == KeyMaterialClientCode::Success;
    if !clients.is_empty() && clients.iter().all(success) {
        KeyMaterialUserCode::Success
    } else if clients.iter().any(success) {
        KeyMaterialUserCode::PartialSuccess
    } else {
        KeyMaterialUserCode::NoCompatibleMaterial
    }
}

/// Handle `KeyMaterial/fetch` (draft-atwood-jmap-chat-mimi-00
/// §KeyMaterial/fetch).
///
/// "The server MUST reject the call with `invalidArguments` if `request`
/// does not parse as a KeyMaterialRequest, if its `requestingUser` is not
/// `userUri`, if its `roomId` is not `roomId`, or if no Chat in the
/// account is bound to that `roomId`." When
/// [`MimiBackend::local_key_material_account`] names the target's account,
/// the answer comes from [`MimiBackend::claim_key_packages`]; otherwise
/// the request goes out with [`MimiBackend::mimi_request`], and an
/// unreachable provider is `serverUnavailable`.
pub async fn handle_key_material_fetch<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = account(backend, caller, args).await?;
    let room_id = string_arg(&args, "roomId")?;
    let body = octets_arg(&args, "request")?;
    let request = wire::key_material_request(&body).map_err(|e| {
        bad(&format!(
            "request does not parse as a KeyMaterialRequest: {e}"
        ))
    })?;
    let capability = backend
        .mimi_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if request.requesting_user != capability.user_uri.as_bytes() {
        return Err(bad("the request's requestingUser is not userUri"));
    }
    if request.room_id != room_id.as_bytes() {
        return Err(bad("the request's roomId is not roomId"));
    }
    require_bound(backend, caller, &account_id, room_id).await?;
    let target_user = String::from_utf8(request.target_user)
        .map_err(|_| bad("the request's targetUser is not UTF-8"))?;

    let local = backend
        .local_key_material_account(caller, room_id, &target_user)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let response = if let Some(target_account) = local {
        let requirements = KeyMaterialRequirements::new(
            request.acceptable_ciphersuites,
            request.required_capabilities,
        );
        let clients = backend
            .claim_key_packages(caller, &target_account, &requirements)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        let status = local_user_status(&clients);
        KeyMaterialFetchResponse::new(account_id, status, target_user, clients)
    } else {
        let endpoint = MimiEndpoint::KeyMaterial { target_user };
        let reply = send(backend, caller, &account_id, room_id, endpoint, &body).await?;
        let decoded = wire::key_material_response(&reply.body)
            .map_err(malformed_reply("KeyMaterialResponse"))?;
        let clients = decoded
            .clients
            .into_iter()
            .map(|c| {
                let mut client = ClientKeyMaterial::new(
                    CLIENT_CODES[usize::from(c.status)].clone(),
                    c.client_uri,
                );
                client.key_package = c.key_package.as_deref().map(b64);
                client.client_capabilities = c.capabilities.as_deref().map(b64);
                client
            })
            .collect();
        KeyMaterialFetchResponse::new(
            account_id,
            USER_CODES[usize::from(decoded.user_status)].clone(),
            decoded.user_uri,
            clients,
        )
    };
    Ok(output(serde_json::to_value(response)))
}

/// Handle `Room/update` (draft-atwood-jmap-chat-mimi-00 §Room/update).
///
/// "The server MUST reject the call with `invalidArguments` if `request`
/// does not parse as an UpdateRequest or if no Chat in the account is
/// bound to `roomId`. Otherwise it processes the request as hub, or
/// submits it to the hub, and returns `serverUnavailable` if the hub
/// cannot be reached." On `success` it stores the FanoutMessage "with
/// `timestamp` set to `accepted_timestamp`, `message` set to
/// `proposalOrCommit`, and `moreProposals` copied from the request (or an
/// empty `externalProposals` for a commit)" through
/// [`MimiBackend::store_fanout`]. A hub response that does not parse is
/// `serverFail`.
pub async fn handle_room_update<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = account(backend, caller, args).await?;
    let room_id = string_arg(&args, "roomId")?;
    let body = octets_arg(&args, "request")?;
    let request = wire::update_request(&body)
        .map_err(|e| bad(&format!("request does not parse as an UpdateRequest: {e}")))?;
    require_bound(backend, caller, &account_id, room_id).await?;

    let reply = send(
        backend,
        caller,
        &account_id,
        room_id,
        MimiEndpoint::Update,
        &body,
    )
    .await?;
    let decoded =
        wire::update_response(&reply.body).map_err(malformed_reply("UpdateRoomResponse"))?;
    let response = match decoded {
        UpdateResponse::Success {
            error_description,
            accepted_timestamp,
        } => {
            let accepted_at = utc_date(accepted_timestamp)?;
            store_own(
                backend,
                room_id,
                &reply.provider_uri,
                accepted_timestamp,
                &request.proposal_or_commit,
                &request.fanout_tail,
            )
            .await?;
            let mut r =
                RoomUpdateResponse::new(account_id, UpdateResponseCode::Success, error_description);
            r.accepted_at = Some(accepted_at);
            r
        }
        UpdateResponse::WrongEpoch {
            error_description,
            current_epoch,
        } => {
            let mut r = RoomUpdateResponse::new(
                account_id,
                UpdateResponseCode::WrongEpoch,
                error_description,
            );
            r.current_epoch = Some(current_epoch);
            r
        }
        UpdateResponse::NotAllowed { error_description } => RoomUpdateResponse::new(
            account_id,
            UpdateResponseCode::NotAllowed,
            error_description,
        ),
        UpdateResponse::InvalidProposal {
            error_description,
            invalid_proposals,
        } => {
            let mut r = RoomUpdateResponse::new(
                account_id,
                UpdateResponseCode::InvalidProposal,
                error_description,
            );
            r.invalid_proposals = Some(invalid_proposals.iter().map(|p| b64(p)).collect());
            r
        }
    };
    Ok(output(serde_json::to_value(response)))
}

/// Handle `Room/send` (draft-atwood-jmap-chat-mimi-00 §Room/send).
///
/// "The server MUST reject the call with `invalidArguments` if `message`
/// does not parse as such an MLSMessage or if no Chat in the account is
/// bound to `roomId`. Otherwise it submits the message to the hub as a
/// SubmitMessageRequest with `sendingUri` set to `userUri`, or processes
/// it as hub, and returns `serverUnavailable` if the hub cannot be
/// reached." When the hub accepts, it stores the FanoutMessage "with
/// `timestamp` set to `accepted_timestamp`, `message` set to `message`,
/// and `frank` copied from the response when present" through
/// [`MimiBackend::store_fanout`]. A hub response that does not parse is
/// `serverFail`.
pub async fn handle_room_send<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = account(backend, caller, args).await?;
    let room_id = string_arg(&args, "roomId")?;
    let message = octets_arg(&args, "message")?;
    wire::application_message(&message).map_err(|e| {
        bad(&format!(
            "message is not an application PrivateMessage MLSMessage: {e}"
        ))
    })?;
    require_bound(backend, caller, &account_id, room_id).await?;
    let capability = backend
        .mimi_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let body = wire::submit_message_request(&message, &capability.user_uri);
    let reply = send(
        backend,
        caller,
        &account_id,
        room_id,
        MimiEndpoint::SubmitMessage,
        &body,
    )
    .await?;
    let decoded = wire::submit_message_response(&reply.body)
        .map_err(malformed_reply("SubmitMessageResponse"))?;
    let response = match decoded {
        SubmitResponse::Accepted {
            accepted_timestamp,
            frank,
        } => {
            let accepted_at = utc_date(accepted_timestamp)?;
            store_own(
                backend,
                room_id,
                &reply.provider_uri,
                accepted_timestamp,
                &message,
                &frank,
            )
            .await?;
            let mut r = RoomSendResponse::new(account_id, SubmitResponseCode::Accepted);
            r.accepted_at = Some(accepted_at);
            r
        }
        SubmitResponse::NotAllowed => {
            RoomSendResponse::new(account_id, SubmitResponseCode::NotAllowed)
        }
        SubmitResponse::EpochTooOld { current_epoch } => {
            let mut r = RoomSendResponse::new(account_id, SubmitResponseCode::EpochTooOld);
            r.current_epoch = Some(current_epoch);
            r
        }
    };
    Ok(output(serde_json::to_value(response)))
}

/// Handle `Room/unbind` (draft-atwood-jmap-chat-mimi-00 §Room/unbind).
///
/// "The server MUST reject the call with `invalidArguments` if no Chat in
/// the account has that `roomId`. Otherwise it unbinds the Chat, if it is
/// bound", through [`MimiBackend::unbind_room`].
pub async fn handle_room_unbind<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = account(backend, caller, args).await?;
    let room_id = string_arg(&args, "roomId")?;
    if !backend
        .unbind_room(caller, &account_id, room_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(bad("no Chat in the account has that roomId"));
    }
    Ok(output(serde_json::to_value(RoomUnbindResponse::new(
        account_id,
    ))))
}

/// Handle `GroupInfo/fetch` (draft-atwood-jmap-chat-mimi-00
/// §GroupInfo/fetch).
///
/// "The server MUST reject the call with `invalidArguments` if `request`
/// does not parse as a GroupInfoRequest or if no Chat in the account is
/// bound to `roomId`. Otherwise it answers as hub, or forwards the request
/// to the hub, and returns `serverUnavailable` if the hub cannot be
/// reached." The response carries the GroupInfoResponse `status` and "The
/// whole GroupInfoResponse, unchanged". A hub response that does not parse
/// is `serverFail`.
pub async fn handle_group_info_fetch<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = account(backend, caller, args).await?;
    let room_id = string_arg(&args, "roomId")?;
    let body = octets_arg(&args, "request")?;
    wire::group_info_request(&body).map_err(|e| {
        bad(&format!(
            "request does not parse as a GroupInfoRequest: {e}"
        ))
    })?;
    require_bound(backend, caller, &account_id, room_id).await?;

    let reply = send(
        backend,
        caller,
        &account_id,
        room_id,
        MimiEndpoint::GroupInfo,
        &body,
    )
    .await?;
    let status = match wire::group_info_response_status(&reply.body)
        .map_err(malformed_reply("GroupInfoResponse"))?
    {
        1 => GroupInfoCode::Success,
        2 => GroupInfoCode::NotAuthorized,
        _ => GroupInfoCode::NoSuchRoom,
    };
    Ok(output(serde_json::to_value(GroupInfoFetchResponse::new(
        account_id,
        status,
        b64(&reply.body),
    ))))
}
