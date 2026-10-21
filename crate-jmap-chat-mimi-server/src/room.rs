//! draft-atwood-jmap-chat-mimi-00 §Room/create.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jmap_chat_mimi_types::{Base64Url, MimiCapability, RoomCreateResponse};
use jmap_server::{extract_account_id, server_fail_from_backend, HandlerOutput};
use jmap_types::JmapError;
use openmls::component::ComponentId;
use openmls::prelude::{
    BasicCredential, Credential, ExternalSender, MlsMessageBodyIn, MlsMessageIn, OpenMlsCrypto,
    ProposalStore, PublicGroup, RatchetTreeIn, SignaturePublicKey,
};
use openmls_memory_storage::MemoryStorage;
use serde_json::Value;
use tls_codec::{Deserialize as _, TlsDeserialize, TlsSize, VLBytes};

use crate::backend::{MimiBackend, NewRoom};

/// `franking_agent` component ID: draft-ietf-mimi-protocol-06 §IANA
/// Considerations registers 0x0021 under the name `franking_signature_key`
/// for the component §5.4.1 calls `franking_agent`.
const FRANKING_AGENT: ComponentId = 0x0021;
/// `participant_list` component ID (draft-ietf-mimi-protocol-06 §IANA
/// Considerations: "Value: 0x0022").
const PARTICIPANT_LIST: ComponentId = 0x0022;
/// `room_metadata` component ID (draft-ietf-mimi-protocol-06 §IANA
/// Considerations: "Value: 0x0023").
const ROOM_METADATA: ComponentId = 0x0023;
/// `base_room_policy` component ID. ponytail: draft-ietf-mimi-room-policy-04
/// §10.1.4 has only "TBD3 (suggested value 0x0027)"; this uses the
/// suggested value until IANA assigns one.
const BASE_ROOM_POLICY: ComponentId = 0x0027;

/// `struct { opaque user<V>; uint32 role_index; } UserRolePair`
/// (draft-ietf-mimi-protocol-06 §7.5).
#[derive(TlsDeserialize, TlsSize)]
struct UserRolePair {
    user: VLBytes,
    #[allow(dead_code, reason = "check 4 constrains only the user")]
    role_index: u32,
}

/// `struct { UserRolePair participants<V>; } ParticipantListData`
/// (draft-ietf-mimi-protocol-06 §7.5).
#[derive(TlsDeserialize, TlsSize)]
struct ParticipantListData {
    participants: Vec<UserRolePair>,
}

/// `RichDescription` (draft-ietf-mimi-protocol-06 §7.6), parsed only so
/// that `RoomMetaData` parses.
#[allow(dead_code, reason = "parsed to reach the fields after it")]
#[derive(TlsDeserialize, TlsSize)]
struct RichDescription {
    media_type: VLBytes,
    language_tag: VLBytes,
    description_content: VLBytes,
}

/// `RoomMetaData` (draft-ietf-mimi-protocol-06 §7.6). `Uri` and
/// `UTF8String` are each `struct { opaque <name><V>; }`, which encode as
/// one `opaque<V>`.
#[derive(TlsDeserialize, TlsSize)]
struct RoomMetaData {
    room_uri: VLBytes,
    room_name: VLBytes,
    #[allow(dead_code, reason = "parsed so that RoomMetaData parses")]
    room_descriptions: Vec<RichDescription>,
    #[allow(dead_code, reason = "parsed so that RoomMetaData parses")]
    room_avatar: VLBytes,
    #[allow(dead_code, reason = "parsed so that RoomMetaData parses")]
    room_subject: VLBytes,
    #[allow(dead_code, reason = "parsed so that RoomMetaData parses")]
    room_mood: VLBytes,
}

/// `struct { SignaturePublicKey franking_signature_key; Credential
/// credential; } FrankingAgentData` (draft-ietf-mimi-protocol-06 §5.4.1),
/// which "mirrors" the RFC 9420 §12.1.8.1 `ExternalSender`, so `hubSender`
/// decodes as one too.
#[derive(PartialEq, TlsDeserialize, TlsSize)]
struct KeyAndCredential {
    signature_key: SignaturePublicKey,
    credential: Credential,
}

/// `BaseRoomPolicy` (draft-ietf-mimi-room-policy-04 §5). Each `bool` is
/// `enum { false(0), true(1) }`, read as a `uint8` and checked by
/// [`BaseRoomPolicy::parses`]; `Uri parent_room<V>` is a vector of
/// `opaque<V>`; `Component` is the `uint16 ComponentID` of
/// draft-ietf-mls-extensions-10 §4.6.
#[allow(dead_code, reason = "check 7 requires only that it parses")]
#[derive(TlsDeserialize, TlsSize)]
struct BaseRoomPolicy {
    fixed_membership: u8,
    parent_dependant: u8,
    parent_room: Vec<VLBytes>,
    multi_device: u8,
    max_clients: Option<u32>,
    max_users: Option<u32>,
    pseudonyms_allowed: u8,
    persistent_room: u8,
    discoverable: u8,
    policy_component_ids: Vec<u16>,
}

impl BaseRoomPolicy {
    /// Whether every `bool` is 0 or 1.
    fn parses(&self) -> bool {
        [
            self.fixed_membership,
            self.parent_dependant,
            self.multi_device,
            self.pseudonyms_allowed,
            self.persistent_room,
            self.discoverable,
        ]
        .iter()
        .all(|b| *b <= 1)
    }
}

/// Whether `uri` is `prefix` followed by "the unpadded lowercase base32
/// encoding (RFC 4648 §6) of 16 octets": 26 characters of `a`-`z` and
/// `2`-`7`, the last carrying three data bits and two zero pad bits.
fn is_room_id(prefix: &str, uri: &str) -> bool {
    let Some(suffix) = uri.strip_prefix(prefix) else {
        return false;
    };
    let value = |b: u8| match b {
        b'a'..=b'z' => Some(b - b'a'),
        b'2'..=b'7' => Some(b - b'2' + 26),
        _ => None,
    };
    suffix.len() == 26
        && suffix.bytes().all(|b| value(b).is_some())
        && suffix
            .bytes()
            .last()
            .and_then(value)
            .is_some_and(|v| v & 0b11 == 0)
}

fn bad(description: &str) -> JmapError {
    JmapError::invalid_arguments(description)
}

fn decode(value: &Base64Url, name: &str) -> Result<Vec<u8>, JmapError> {
    URL_SAFE_NO_PAD
        .decode(value.as_str())
        .map_err(|_| bad(&format!("{name} is not unpadded base64url")))
}

/// Check one `Room/create` against draft-atwood-jmap-chat-mimi-00
/// §Room/create.
///
/// "The server MUST reject the call with `invalidArguments` unless all of
/// the following hold", checks 1 to 7. This function applies every part
/// that needs no server state; [`MimiBackend::create_room`] applies the
/// rest atomically with the insert: that the leaf's signature key "is
/// bound to the account" (check 2), "no room this server hosts has that
/// room ID" (check 5) and "no room this server hosts uses" the group ID
/// (check 6).
///
/// Checks 1 and 2 (signature, tree hash) are those of openmls
/// `PublicGroup::from_external` (RFC 9420 §12.4.3.1), which also rejects
/// a ratchet tree whose leaf nodes are invalid. A `groupInfo` or
/// `ratchetTree` that does not parse, a credential that is not a Basic
/// credential, and a missing component are `invalidArguments` too. A
/// `hubSender` in `capability` that is not an ExternalSender is
/// `serverFail`.
///
/// The returned [`NewRoom::room_name`] is the `room_name` of the room
/// metadata, or the empty string when it is not UTF-8 (§Chat.roomId: "The
/// server sets `name` to the `room_name` of the room metadata ... when it
/// knows it and to the empty string otherwise").
// JmapError is the type every method-level refusal takes.
#[allow(clippy::result_large_err)]
pub fn check_room_create<C: OpenMlsCrypto>(
    crypto: &C,
    capability: &MimiCapability,
    group_info: &Base64Url,
    ratchet_tree: &Base64Url,
) -> Result<NewRoom, JmapError> {
    let hub_bytes = URL_SAFE_NO_PAD
        .decode(capability.hub_sender.as_str())
        .map_err(|_| JmapError::server_fail("hubSender is not base64url"))?;
    let hub_sender = ExternalSender::tls_deserialize_exact(&hub_bytes)
        .map_err(|_| JmapError::server_fail("hubSender is not an ExternalSender"))?;
    let hub_key = KeyAndCredential::tls_deserialize_exact(&hub_bytes)
        .map_err(|_| JmapError::server_fail("hubSender is not an ExternalSender"))?;

    let message = MlsMessageIn::tls_deserialize_exact(decode(group_info, "groupInfo")?)
        .map_err(|_| bad("groupInfo does not parse as an MLSMessage"))?;
    let MlsMessageBodyIn::GroupInfo(verifiable) = message.extract() else {
        return Err(bad("groupInfo is not of wire format mls_group_info"));
    };
    let tree = RatchetTreeIn::tls_deserialize_exact(decode(ratchet_tree, "ratchetTree")?)
        .map_err(|_| bad("ratchetTree does not parse as a ratchet_tree"))?;

    // Check 1: "its cipher suite is in `cipherSuites`".
    if !capability
        .cipher_suites
        .contains(&u16::from(verifiable.ciphersuite()))
    {
        return Err(bad("GroupInfo cipher suite is not in cipherSuites"));
    }
    // Checks 1 and 2: "The GroupInfo signature verifies" and "The
    // `ratchetTree` matches the tree hash in the GroupInfo's GroupContext".
    let (public_group, info) = PublicGroup::from_external(
        crypto,
        &MemoryStorage::default(),
        tree,
        verifiable,
        ProposalStore::new(),
    )
    .map_err(|e| bad(&format!("GroupInfo and ratchetTree do not verify: {e}")))?;

    // Check 2: "contains exactly one leaf, whose credential identity is
    // `userUri`".
    let mut members = public_group.members();
    let (Some(leaf), None) = (members.next(), members.next()) else {
        return Err(bad("ratchetTree does not contain exactly one leaf"));
    };
    let identity_ok = BasicCredential::try_from(leaf.credential)
        .is_ok_and(|c| c.identity() == capability.user_uri.as_bytes());
    if !identity_ok {
        return Err(bad("the leaf credential identity is not userUri"));
    }

    let context = info.group_context();
    let extensions = context.extensions();
    // Check 3: "The `external_senders` extension contains exactly one
    // ExternalSender, equal to `hubSender`."
    if !matches!(extensions.external_senders(), Some(s) if s.as_slice() == [hub_sender]) {
        return Err(bad("external_senders is not exactly hubSender"));
    }

    let dictionary = extensions.app_data_dictionary().map(|d| d.dictionary());
    let component = |id: ComponentId| dictionary.and_then(|d| d.get(&id));

    // Check 4: "The participant list ... contains exactly one participant,
    // `userUri`."
    let participants = component(PARTICIPANT_LIST)
        .and_then(|data| ParticipantListData::tls_deserialize_exact(data).ok())
        .ok_or_else(|| bad("participant_list is missing or does not parse"))?
        .participants;
    if !matches!(participants.as_slice(), [only] if only.user.as_slice() == capability.user_uri.as_bytes())
    {
        return Err(bad("participant list is not exactly userUri"));
    }

    // Check 5: "The `room_uri` of the room metadata is `roomIdPrefix`
    // followed by the unpadded lowercase base32 encoding of 16 octets".
    let metadata = component(ROOM_METADATA)
        .and_then(|data| RoomMetaData::tls_deserialize_exact(data).ok())
        .ok_or_else(|| bad("room_metadata is missing or does not parse"))?;
    let room_id = String::from_utf8(metadata.room_uri.as_slice().to_vec())
        .ok()
        .filter(|uri| is_room_id(&capability.room_id_prefix, uri))
        .ok_or_else(|| bad("room_uri is not roomIdPrefix and a 16-octet base32 room ID"))?;

    // Check 6: "The group ID is 16 octets".
    let group_id = context.group_id().as_slice();
    if group_id.len() != 16 {
        return Err(bad("the group ID is not 16 octets"));
    }

    // Check 7: "a `franking_agent` component whose
    // `franking_signature_key` and `credential` are those of `hubSender`,
    // and a `base_room_policy` component that parses".
    let franking_ok = component(FRANKING_AGENT)
        .and_then(|data| KeyAndCredential::tls_deserialize_exact(data).ok())
        .is_some_and(|agent| agent == hub_key);
    if !franking_ok {
        return Err(bad("franking_agent is missing or is not hubSender"));
    }
    let policy_ok = component(BASE_ROOM_POLICY)
        .and_then(|data| BaseRoomPolicy::tls_deserialize_exact(data).ok())
        .is_some_and(|policy| policy.parses());
    if !policy_ok {
        return Err(bad("base_room_policy is missing or does not parse"));
    }

    Ok(NewRoom {
        room_id,
        group_id: group_id.to_vec(),
        room_name: String::from_utf8(metadata.room_name.as_slice().to_vec()).unwrap_or_default(),
        signature_key: leaf.signature_key,
        group_info: group_info.clone(),
        ratchet_tree: ratchet_tree.clone(),
    })
}

/// Handle `Room/create` (draft-atwood-jmap-chat-mimi-00 §Room/create).
///
/// Runs [`check_room_create`] against [`MimiBackend::mimi_capability`],
/// then [`MimiBackend::create_room`], and returns `accountId`, `roomId`
/// and `chatId`. Every refusal is the method-level error "reject the call
/// with `invalidArguments`"; an unknown `accountId` is `accountNotFound`
/// (RFC 8620 §3.6.2).
pub async fn handle_room_create<B: MimiBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    let arg = |name: &str| {
        args.get(name)
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Base64Url>().ok())
            .ok_or_else(|| bad(&format!("{name} must be an unpadded base64url string")))
    };
    let (group_info, ratchet_tree) = (arg("groupInfo")?, arg("ratchetTree")?);

    let capability = backend
        .mimi_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let room = check_room_create(
        backend.mls_crypto(),
        &capability,
        &group_info,
        &ratchet_tree,
    )?;
    let room_id = room.room_id.clone();
    let chat_id = backend
        .create_room(caller, &account_id, room)
        .await
        .map_err(|e| server_fail_from_backend(&e))??;

    let response = RoomCreateResponse::new(account_id, room_id, chat_id);
    Ok(HandlerOutput::new(
        serde_json::to_value(response).expect("derive(Serialize) on plain data is infallible"),
    ))
}
