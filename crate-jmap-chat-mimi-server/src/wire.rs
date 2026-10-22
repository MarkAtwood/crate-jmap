//! The MIMI protocol messages that `KeyMaterial/fetch`, `Room/update`,
//! `Room/send` and `GroupInfo/fetch` carry (draft-ietf-mimi-protocol-06
//! §5.2-§5.4, §5.6), in the TLS presentation language of RFC 8446 §3.
//!
//! Only `mls10(1)` is defined as `Protocol`, so every other value fails to
//! parse. A response code outside its enum also fails: the `select` that
//! follows it is undefined.
//!
//! Everything is read with the `std::io::Read`-based [`Deserialize`]:
//! tls_codec 0.5.0's `DeserializeBytes` for a truncated `opaque<V>`
//! trips a `debug_assert_eq!` instead of returning an error.

use openmls::messages::group_info::VerifiableGroupInfo;
use openmls::prelude::{
    Capabilities, ContentType, Credential, Extension, ExternalSender, KeyPackageIn,
    MlsMessageBodyIn, MlsMessageIn, RequiredCapabilitiesExtension, SignaturePublicKey, Welcome,
};
use tls_codec::{Deserialize, VLBytes};

use crate::fanout::Frank;

/// `Protocol` `mls10(1)` (draft-ietf-mimi-protocol-06 §5).
const MLS10: u8 = 1;

type Parsed<'a, T> = Result<(T, &'a [u8]), String>;

fn read<'a, T: Deserialize>(bytes: &'a [u8], what: &str) -> Parsed<'a, T> {
    let mut rest = bytes;
    let value = T::tls_deserialize(&mut rest).map_err(|e| format!("{what}: {e}"))?;
    Ok((value, rest))
}

/// Read a `T` and return the octets it took.
fn read_octets<'a, T: Deserialize>(bytes: &'a [u8], what: &str) -> Parsed<'a, &'a [u8]> {
    let (_, rest) = read::<T>(bytes, what)?;
    Ok((&bytes[..bytes.len() - rest.len()], rest))
}

fn protocol(bytes: &[u8]) -> Result<&[u8], String> {
    match read::<u8>(bytes, "protocol")? {
        (MLS10, rest) => Ok(rest),
        (other, _) => Err(format!("protocol {other} is not mls10")),
    }
}

fn end(rest: &[u8]) -> Result<(), String> {
    if rest.is_empty() {
        Ok(())
    } else {
        Err(format!("{} trailing octets", rest.len()))
    }
}

fn utf8(bytes: VLBytes, what: &str) -> Result<String, String> {
    String::from_utf8(bytes.as_slice().to_vec()).map_err(|_| format!("{what} is not UTF-8"))
}

/// The parts of a KeyMaterialRequest (§5.2) the server reads.
pub(crate) struct KeyMaterialRequest {
    pub(crate) requesting_user: Vec<u8>,
    pub(crate) target_user: Vec<u8>,
    pub(crate) room_id: Vec<u8>,
    pub(crate) acceptable_ciphersuites: Vec<u16>,
    pub(crate) required_capabilities: RequiredCapabilitiesExtension,
}

/// `KeyMaterialRequest`: `protocol`, `IdentifierUri requestingUser`,
/// `targetUser` and `roomId` (each `struct { opaque uri<V>; }`), then for
/// `mls10` `CipherSuite acceptableCiphersuites<V>`, `RequiredCapabilities`,
/// `SignaturePublicKey`, `Credential` and `opaque
/// key_material_request_signature<V>`.
pub(crate) fn key_material_request(bytes: &[u8]) -> Result<KeyMaterialRequest, String> {
    let rest = protocol(bytes)?;
    let (requesting_user, rest) = read::<VLBytes>(rest, "requestingUser")?;
    let (target_user, rest) = read::<VLBytes>(rest, "targetUser")?;
    let (room_id, rest) = read::<VLBytes>(rest, "roomId")?;
    let (acceptable_ciphersuites, rest) = read::<Vec<u16>>(rest, "acceptableCiphersuites")?;
    let (required_capabilities, rest) = read(rest, "requiredCapabilities")?;
    let (_, rest) = read::<SignaturePublicKey>(rest, "requesterSignatureKey")?;
    let (_, rest) = read::<Credential>(rest, "requesterCredential")?;
    let (_, rest) = read::<VLBytes>(rest, "key_material_request_signature")?;
    end(rest)?;
    Ok(KeyMaterialRequest {
        requesting_user: requesting_user.into(),
        target_user: target_user.into(),
        room_id: room_id.into(),
        acceptable_ciphersuites,
        required_capabilities,
    })
}

/// One `ClientKeyMaterial` of a KeyMaterialResponse (§5.2).
pub(crate) struct ClientMaterial {
    /// `KeyMaterialClientCode`: `success(0)`, `keyMaterialExhausted(1)`,
    /// `nothingCompatible(2)`.
    pub(crate) status: u8,
    pub(crate) client_uri: String,
    /// The `KeyPackage` octets, for `success`.
    pub(crate) key_package: Option<Vec<u8>>,
    /// The `Capabilities` octets, for `nothingCompatible` when present.
    pub(crate) capabilities: Option<Vec<u8>>,
}

/// The parts of a KeyMaterialResponse (§5.2).
pub(crate) struct KeyMaterialResponse {
    /// `KeyMaterialUserCode`, `success(0)` to `userDeleted(7)`.
    pub(crate) user_status: u8,
    pub(crate) user_uri: String,
    pub(crate) clients: Vec<ClientMaterial>,
}

/// `KeyMaterialResponse`: `protocol`, `KeyMaterialUserCode userStatus`,
/// `IdentifierUri userUri`, `ClientKeyMaterial clients<V>`.
pub(crate) fn key_material_response(bytes: &[u8]) -> Result<KeyMaterialResponse, String> {
    let rest = protocol(bytes)?;
    let (user_status, rest) = read::<u8>(rest, "userStatus")?;
    if user_status > 7 {
        return Err(format!("userStatus {user_status} is not defined"));
    }
    let (user_uri, rest) = read::<VLBytes>(rest, "userUri")?;
    let (list, rest) = read::<VLBytes>(rest, "clients")?;
    end(rest)?;
    let mut clients = Vec::new();
    let mut rest = list.as_slice();
    while !rest.is_empty() {
        let (client, tail) = client_key_material(rest)?;
        clients.push(client);
        rest = tail;
    }
    Ok(KeyMaterialResponse {
        user_status,
        user_uri: utf8(user_uri, "userUri")?,
        clients,
    })
}

fn client_key_material(bytes: &[u8]) -> Parsed<'_, ClientMaterial> {
    let (status, rest) = read::<u8>(bytes, "clientStatus")?;
    let (client_uri, rest) = read::<VLBytes>(rest, "clientUri")?;
    let (key_package, capabilities, rest) = match status {
        0 => {
            let (octets, rest) = read_octets::<KeyPackageIn>(rest, "keyPackage")?;
            (Some(octets.to_vec()), None, rest)
        }
        1 => (None, None, rest),
        2 => match read::<u8>(rest, "clientCapabilities")? {
            (0, rest) => (None, None, rest),
            (1, rest) => {
                let (octets, rest) = read_octets::<Capabilities>(rest, "clientCapabilities")?;
                (None, Some(octets.to_vec()), rest)
            }
            (other, _) => return Err(format!("optional presence {other} is not 0 or 1")),
        },
        other => return Err(format!("clientStatus {other} is not defined")),
    };
    let client = ClientMaterial {
        status,
        client_uri: utf8(client_uri, "clientUri")?,
        key_package,
        capabilities,
    };
    Ok((client, rest))
}

/// An UpdateRequest (§5.3) reduced to what its FanoutMessage takes.
pub(crate) struct UpdateRequest {
    /// The `proposalOrCommit` MLSMessage octets.
    pub(crate) proposal_or_commit: Vec<u8>,
    /// What follows the MLSMessage in the FanoutMessage (§5.5): the
    /// request's `moreProposals<V>` for a proposal, an empty
    /// `externalProposals<V>` for a commit.
    pub(crate) fanout_tail: Vec<u8>,
}

/// `UpdateRequest`: for `mls10`, a `HandshakeBundle`: `MLSMessage
/// proposalOrCommit`, then for a commit `optional<Welcome> welcome`,
/// `GroupInfoOption` and `RatchetTreeOption`, and for a proposal
/// `MLSMessage moreProposals<V>`, kept as opaque octets.
// ponytail: a SemiPrivateMessage (draft-mahy-mls-semiprivatemessage)
// handshake does not parse, because openmls 0.9.2 does not decode that
// wire format; supporting it needs an openmls that does.
pub(crate) fn update_request(bytes: &[u8]) -> Result<UpdateRequest, String> {
    let (message, rest) = read::<MlsMessageIn>(bytes, "proposalOrCommit")?;
    let proposal_or_commit = bytes[..bytes.len() - rest.len()].to_vec();
    let MlsMessageBodyIn::PublicMessage(message) = message.extract() else {
        return Err("proposalOrCommit is not a PublicMessage".into());
    };
    let (fanout_tail, rest) = match message.content_type() {
        ContentType::Proposal => {
            let (octets, rest) = read_octets::<VLBytes>(rest, "moreProposals")?;
            (octets.to_vec(), rest)
        }
        ContentType::Commit => {
            let (_, rest) = read::<Option<Welcome>>(rest, "welcome")?;
            let rest = group_info_option(rest)?;
            let rest = ratchet_tree_option(rest)?;
            (vec![0], rest)
        }
        ContentType::Application => return Err("proposalOrCommit is an application message".into()),
    };
    end(rest)?;
    Ok(UpdateRequest {
        proposal_or_commit,
        fanout_tail,
    })
}

/// `GroupInfoOption`: `full(1)` carries a `GroupInfo`, `partial(2)` a
/// `PartialGroupInfo` (draft-mahy-mls-ratchet-tree-options-04 §4:
/// `RatchetTreePresence` 0 to 3, `Extension group_info_extensions<V>`,
/// `opaque signature<V>`).
fn group_info_option(bytes: &[u8]) -> Result<&[u8], String> {
    match read::<u8>(bytes, "groupInfoOption")? {
        (1, rest) => Ok(read::<VerifiableGroupInfo>(rest, "groupInfo")?.1),
        (2, rest) => {
            let (presence, rest) = read::<u8>(rest, "ratchet_tree_presence")?;
            if presence > 3 {
                return Err(format!("ratchet_tree_presence {presence} is not defined"));
            }
            let (_, rest) = read::<Vec<Extension>>(rest, "group_info_extensions")?;
            Ok(read::<VLBytes>(rest, "signature")?.1)
        }
        (other, _) => Err(format!("GroupInfoRepresentation {other} is not defined")),
    }
}

/// Skip a RatchetTreeOption (draft-mahy-mls-ratchet-tree-options-04 §3):
/// `full(1)` carries `Node ratchet_tree<V>`, `httpsUrl(2)` `opaque
/// ratchet_tree_url<V>`, `outOfBand(3)` and `distributionService(4)`
/// nothing.
fn ratchet_tree_option(bytes: &[u8]) -> Result<&[u8], String> {
    match read::<u8>(bytes, "ratchetTreeOption")? {
        (1 | 2, rest) => Ok(read::<VLBytes>(rest, "ratchetTreeOption")?.1),
        (3 | 4, rest) => Ok(rest),
        (other, _) => Err(format!("RatchetTreeRepresentation {other} is not defined")),
    }
}

/// An UpdateRoomResponse (§5.3).
pub(crate) enum UpdateResponse {
    Success {
        error_description: String,
        accepted_timestamp: u64,
    },
    WrongEpoch {
        error_description: String,
        current_epoch: u64,
    },
    NotAllowed {
        error_description: String,
    },
    InvalidProposal {
        error_description: String,
        /// Each ProposalRef with its length prefix.
        invalid_proposals: Vec<Vec<u8>>,
    },
}

/// `UpdateRoomResponse`: `UpdateResponseCode responseCode`, `string
/// errorDescription` (read as UTF-8 `opaque<V>`), then `uint64
/// accepted_timestamp` for `success(0)`, `uint64 currentEpoch` for
/// `wrongEpoch(1)`, nothing for `notAllowed(2)`, and `ProposalRef
/// invalidProposals<V>` for `invalidProposal(3)`.
pub(crate) fn update_response(bytes: &[u8]) -> Result<UpdateResponse, String> {
    let (code, rest) = read::<u8>(bytes, "responseCode")?;
    let (description, rest) = read::<VLBytes>(rest, "errorDescription")?;
    let error_description = utf8(description, "errorDescription")?;
    let (response, rest) = match code {
        0 => {
            let (accepted_timestamp, rest) = read::<u64>(rest, "accepted_timestamp")?;
            let response = UpdateResponse::Success {
                error_description,
                accepted_timestamp,
            };
            (response, rest)
        }
        1 => {
            let (current_epoch, rest) = read::<u64>(rest, "currentEpoch")?;
            let response = UpdateResponse::WrongEpoch {
                error_description,
                current_epoch,
            };
            (response, rest)
        }
        2 => (UpdateResponse::NotAllowed { error_description }, rest),
        3 => {
            let (list, rest) = read::<VLBytes>(rest, "invalidProposals")?;
            let mut invalid_proposals = Vec::new();
            let mut refs = list.as_slice();
            while !refs.is_empty() {
                let (octets, tail) = read_octets::<VLBytes>(refs, "ProposalRef")?;
                invalid_proposals.push(octets.to_vec());
                refs = tail;
            }
            let response = UpdateResponse::InvalidProposal {
                error_description,
                invalid_proposals,
            };
            (response, rest)
        }
        other => return Err(format!("responseCode {other} is not defined")),
    };
    end(rest)?;
    Ok(response)
}

/// Check that `bytes` is an MLSMessage of wire format PrivateMessage and
/// content type application (draft-atwood-jmap-chat-mimi-00 §Room/send).
pub(crate) fn application_message(bytes: &[u8]) -> Result<(), String> {
    let (message, rest) = read::<MlsMessageIn>(bytes, "message")?;
    end(rest)?;
    match message.extract() {
        MlsMessageBodyIn::PrivateMessage(m) if m.content_type() == ContentType::Application => {
            Ok(())
        }
        _ => Err("message is not an application PrivateMessage".into()),
    }
}

/// `SubmitMessageRequest`: `protocol` `mls10`, `MLSMessage appMessage`,
/// `IdentifierURI sendingUri`.
pub(crate) fn submit_message_request(app_message: &[u8], sending_uri: &str) -> Vec<u8> {
    let uri = VLBytes::new(sending_uri.as_bytes().to_vec());
    let uri = tls_codec::Serialize::tls_serialize_detached(&uri)
        .expect("a URI shorter than 2^30 octets serializes");
    [&[MLS10][..], app_message, &uri].concat()
}

/// A SubmitMessageResponse (§5.4).
pub(crate) enum SubmitResponse {
    Accepted {
        accepted_timestamp: u64,
        /// The `optional Frank frank` octets, presence octet included.
        frank: Vec<u8>,
    },
    NotAllowed,
    EpochTooOld {
        current_epoch: u64,
    },
}

/// `SubmitMessageResponse`: `protocol`, `SubmitResponseCode statusCode`,
/// then `uint64 accepted_timestamp` and `optional Frank frank` for
/// `accepted(0)`, nothing for `notAllowed(1)`, and `uint64 currentEpoch`
/// for `epochTooOld(2)`.
pub(crate) fn submit_message_response(bytes: &[u8]) -> Result<SubmitResponse, String> {
    let rest = protocol(bytes)?;
    let (response, rest) = match read::<u8>(rest, "statusCode")? {
        (0, rest) => {
            let (accepted_timestamp, rest) = read::<u64>(rest, "accepted_timestamp")?;
            let (frank, rest) = read_octets::<Option<Frank>>(rest, "frank")?;
            let response = SubmitResponse::Accepted {
                accepted_timestamp,
                frank: frank.to_vec(),
            };
            (response, rest)
        }
        (1, rest) => (SubmitResponse::NotAllowed, rest),
        (2, rest) => {
            let (current_epoch, rest) = read::<u64>(rest, "currentEpoch")?;
            (SubmitResponse::EpochTooOld { current_epoch }, rest)
        }
        (other, _) => return Err(format!("statusCode {other} is not defined")),
    };
    end(rest)?;
    Ok(response)
}

/// `GroupInfoRequest`: `protocol`, then for `mls10` `CipherSuite
/// cipher_suite`, `SignaturePublicKey requestingSignatureKey`, `Credential
/// requestingCredential`, `HPKEPublicKey groupInfoPublicKey`, `opaque
/// joiningCode<V>`, `opaque signature<V>`.
pub(crate) fn group_info_request(bytes: &[u8]) -> Result<(), String> {
    let rest = protocol(bytes)?;
    let (_, rest) = read::<u16>(rest, "cipher_suite")?;
    let (_, rest) = read::<SignaturePublicKey>(rest, "requestingSignatureKey")?;
    let (_, rest) = read::<Credential>(rest, "requestingCredential")?;
    let (_, rest) = read::<VLBytes>(rest, "groupInfoPublicKey")?;
    let (_, rest) = read::<VLBytes>(rest, "joiningCode")?;
    let (_, rest) = read::<VLBytes>(rest, "signature")?;
    end(rest)
}

/// The `GroupInfoCode status` of a `GroupInfoResponse`: `protocol`,
/// `opaque room_id<V>`, `status`, then for `success(1)` `CipherSuite
/// cipher_suite`, `ExternalSender hub_sender`, `HPKECiphertext
/// encrypted_groupinfo_and_tree` (RFC 9420 §5.1.3: `opaque
/// kem_output<V>`, `opaque ciphertext<V>`) and `opaque signature<V>`, and
/// nothing for `notAuthorized(2)` and `noSuchRoom(3)`.
pub(crate) fn group_info_response_status(bytes: &[u8]) -> Result<u8, String> {
    let rest = protocol(bytes)?;
    let (_, rest) = read::<VLBytes>(rest, "room_id")?;
    let (status, rest) = match read::<u8>(rest, "status")? {
        (1, rest) => {
            let (_, rest) = read::<u16>(rest, "cipher_suite")?;
            let (_, rest) = read::<ExternalSender>(rest, "hub_sender")?;
            let (_, rest) = read::<VLBytes>(rest, "kem_output")?;
            let (_, rest) = read::<VLBytes>(rest, "ciphertext")?;
            let (_, rest) = read::<VLBytes>(rest, "signature")?;
            (1, rest)
        }
        (status @ (2 | 3), rest) => (status, rest),
        (other, _) => return Err(format!("GroupInfoCode {other} is not defined")),
    };
    end(rest)?;
    Ok(status)
}
