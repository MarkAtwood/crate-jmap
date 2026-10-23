//! draft-atwood-jmap-chat-mimi-00 §GroupInfo/fetch — the client's checks
//! on a GroupInfoResponse before an external join.
//!
//! "The client SHOULD verify the response signature with `hub_sender`,
//! and SHOULD verify that the decrypted GroupInfo's `external_senders`
//! extension contains that `hub_sender`, before it joins."
//!
//! The GroupInfoResponse is the struct of draft-ietf-mimi-protocol-06
//! §5.6:
//!
//! ```text
//! struct {
//!   Protocol version;
//!   opaque room_id<V>;
//!   GroupInfoCode status;
//!   select (status) {
//!     case success:
//!       select (protocol) {
//!         case mls10:
//!           CipherSuite cipher_suite;
//!           ExternalSender hub_sender;
//!           HPKECiphertext encrypted_groupinfo_and_tree;
//!           /* SignWithLabel(hub_sender.signature_key,        */
//!           /*  "GroupInfoResponseTBS", GroupInfoResponseTBS) */
//!           opaque signature<V>;
//!       };
//!     default: struct{};
//!   };
//! } GroupInfoResponse;
//! ```
//!
//! GroupInfoResponseTBS is the same struct without `signature`, so for a
//! `success` response it is exactly the received octets in front of
//! `signature`. The signature is verified over those octets, never over a
//! re-encoding (draft §Conventions and Definitions).
//!
//! `encrypted_groupinfo_and_tree` decrypts to a GroupInfoRatchetTreeTBE
//! (draft-ietf-mimi-protocol-06 §5.6):
//!
//! ```text
//! struct {
//!     MLSMessage proposal;
//!     uint64 hub_accepted_time;
//! } PendingProposal;
//!
//! struct {
//!   GroupInfo groupInfo;   /* without embedded ratchet_tree */
//!   RatchetTreeOption ratchetTreeOption;
//!   PendingProposal pending_proposals<V>;
//! } GroupInfoRatchetTreeTBE;
//!
//! encrypted_groupinfo_and_tree = EncryptWithLabel(
//!   groupInfoPublicKey,
//!   "GroupInfo and ratchet_tree encryption",
//!   room_id,                         /* context */
//!   group_info_ratchet_tree_tbe)
//! ```

use std::fmt;

use openmls::ciphersuite::signature::SignContent;
use openmls::messages::group_info::VerifiableGroupInfo;
use openmls::prelude::{
    Ciphersuite, Credential, ExternalSender, HpkeCiphertext, MlsMessageIn, OpenMlsCrypto,
    RatchetTreeIn, SignaturePublicKey, SignatureScheme,
};
use tls_codec::{Deserialize as _, Serialize as _, VLBytes};

/// `Protocol` `mls10(1)` (draft-ietf-mimi-protocol-06 §4).
const MLS10: u8 = 1;
/// `GroupInfoCode` `success(1)` (draft-ietf-mimi-protocol-06 §5.6).
const SUCCESS: u8 = 1;
/// The SignWithLabel label of the GroupInfoResponse signature.
const LABEL: &str = "GroupInfoResponseTBS";
/// The EncryptWithLabel label of `encrypted_groupinfo_and_tree`.
const TBE_LABEL: &str = "GroupInfo and ratchet_tree encryption";

/// A `success` GroupInfoResponse whose signature verified with its
/// `hub_sender`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedGroupInfoResponse {
    /// The `room_id` octets.
    pub room_id: Vec<u8>,
    /// The `cipher_suite`, which names the signature algorithm.
    pub cipher_suite: Ciphersuite,
    /// The `hub_sender` whose signature key verified the response. Check
    /// the decrypted GroupInfo with [`group_info_names_hub_sender`].
    pub hub_sender: ExternalSender,
    /// `encrypted_groupinfo_and_tree`, encrypted to the request's
    /// `groupInfoPublicKey` with EncryptWithLabel label "GroupInfo and
    /// ratchet_tree encryption" and context `room_id`.
    pub encrypted_groupinfo_and_tree: HpkeCiphertext,
}

/// Why [`verify_group_info_response`] refused a GroupInfoResponse.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupInfoResponseError {
    /// The octets are not a GroupInfoResponse: truncated, a malformed
    /// field, or trailing octets after `signature`.
    Malformed,
    /// `version` is not `mls10`, the only Protocol defined.
    UnsupportedProtocol(u8),
    /// `status` is not `success`, so the response carries no GroupInfo
    /// and no signature. The value is the GroupInfoCode.
    NotSuccess(u8),
    /// `cipher_suite` is not one this version of openmls knows.
    UnsupportedCipherSuite(u16),
    /// The signature does not verify with `hub_sender`'s signature key.
    BadSignature,
    /// `encrypted_groupinfo_and_tree` does not decrypt with the given
    /// `groupInfoPublicKey` private key.
    DecryptionFailed,
    /// The decrypted plaintext is not a GroupInfoRatchetTreeTBE.
    MalformedGroupInfoAndTree,
}

impl fmt::Display for GroupInfoResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => f.write_str("not a GroupInfoResponse"),
            Self::UnsupportedProtocol(v) => write!(f, "unsupported GroupInfoResponse version {v}"),
            Self::NotSuccess(status) => {
                write!(f, "GroupInfoResponse status {status} is not success")
            }
            Self::UnsupportedCipherSuite(suite) => {
                write!(f, "unsupported GroupInfoResponse cipher suite {suite:#06x}")
            }
            Self::BadSignature => f.write_str("GroupInfoResponse signature does not verify"),
            Self::DecryptionFailed => {
                f.write_str("encrypted_groupinfo_and_tree does not decrypt with this key")
            }
            Self::MalformedGroupInfoAndTree => {
                f.write_str("decrypted plaintext is not a GroupInfoRatchetTreeTBE")
            }
        }
    }
}

impl std::error::Error for GroupInfoResponseError {}

impl From<tls_codec::Error> for GroupInfoResponseError {
    fn from(_: tls_codec::Error) -> Self {
        Self::Malformed
    }
}

/// Parse a GroupInfoResponse and verify its signature with its
/// `hub_sender` (draft-atwood-jmap-chat-mimi-00 §GroupInfo/fetch: "The
/// client SHOULD verify the response signature with `hub_sender`").
///
/// `response` is the decoded octets of the `GroupInfo/fetch` response's
/// `response` argument. `crypto` is the client's MLS crypto provider.
///
/// A verified response proves only that the holder of `hub_sender`'s key
/// signed it. Decrypt `encrypted_groupinfo_and_tree` with
/// [`decrypt_group_info_and_tree`] and check that the GroupInfo names the
/// same hub with [`group_info_names_hub_sender`] before joining.
///
/// # Errors
///
/// [`GroupInfoResponseError`]: the octets do not parse, the status is not
/// `success`, the protocol or cipher suite is unknown, or the signature
/// does not verify.
pub fn verify_group_info_response(
    crypto: &impl OpenMlsCrypto,
    response: &[u8],
) -> Result<VerifiedGroupInfoResponse, GroupInfoResponseError> {
    let mut rest = response;
    let version = u8::tls_deserialize(&mut rest)?;
    let room_id = VLBytes::tls_deserialize(&mut rest)?;
    let status = u8::tls_deserialize(&mut rest)?;
    if status != SUCCESS {
        return Err(GroupInfoResponseError::NotSuccess(status));
    }
    if version != MLS10 {
        return Err(GroupInfoResponseError::UnsupportedProtocol(version));
    }
    let suite = u16::tls_deserialize(&mut rest)?;
    // ExternalSender (RFC 9420 §12.1.8.1): `SignaturePublicKey
    // signature_key; Credential credential;`. openmls keeps its accessors
    // crate-private, so read the two fields here.
    let signature_key = VLBytes::tls_deserialize(&mut rest)?;
    let credential = Credential::tls_deserialize(&mut rest)?;
    let encrypted_groupinfo_and_tree = HpkeCiphertext::tls_deserialize(&mut rest)?;
    let tbs = &response[..response.len() - rest.len()];
    let signature = VLBytes::tls_deserialize_exact(rest)?;

    let cipher_suite = Ciphersuite::try_from(suite)
        .map_err(|_| GroupInfoResponseError::UnsupportedCipherSuite(suite))?;
    if !verify_with_label(
        crypto,
        cipher_suite.signature_algorithm(),
        signature_key.as_slice(),
        LABEL,
        tbs,
        signature.as_slice(),
    ) {
        return Err(GroupInfoResponseError::BadSignature);
    }
    Ok(VerifiedGroupInfoResponse {
        room_id: room_id.into(),
        cipher_suite,
        hub_sender: ExternalSender::new(
            SignaturePublicKey::from(signature_key.as_slice()),
            credential,
        ),
        encrypted_groupinfo_and_tree,
    })
}

/// Whether the GroupInfo's `external_senders` extension contains
/// `hub_sender` (draft-atwood-jmap-chat-mimi-00 §GroupInfo/fetch: the
/// client "SHOULD verify that the decrypted GroupInfo's
/// `external_senders` extension contains that `hub_sender`, before it
/// joins").
///
/// `external_senders` is a GroupContext extension (RFC 9420 §12.1.8.1).
/// `group_info` is the decrypted GroupInfo, not yet verified: openmls
/// verifies its signature when the client joins with it by external
/// commit. Pass the `hub_sender` of the [`VerifiedGroupInfoResponse`].
pub fn group_info_names_hub_sender(
    group_info: &VerifiableGroupInfo,
    hub_sender: &ExternalSender,
) -> bool {
    group_info
        .group_context()
        .extensions()
        .external_senders()
        .is_some_and(|senders| senders.contains(hub_sender))
}

/// A RatchetTreeOption (draft-mahy-mls-ratchet-tree-options-04 §3): how
/// the joiner gets the ratchet tree that the GroupInfo leaves out.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum RatchetTreeOption {
    /// `full(1)`: the complete `ratchet_tree` extension (RFC 9420
    /// §12.4.3.3), as openmls takes it for a join.
    Full(RatchetTreeIn),
    /// `httpsUrl(2)`: an HTTPS URL from which to download the tree.
    HttpsUrl(String),
    /// `outOfBand(3)`: an unspecified application protocol provides it.
    OutOfBand,
    /// `distributionService(4)`: the DS reconstructs it from the group's
    /// handshake messages.
    DistributionService,
}

/// A PendingProposal (draft-ietf-mimi-protocol-06 §5.6): a proposal the
/// hub accepted during the current epoch.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct PendingProposal {
    /// The `proposal` MLSMessage.
    pub proposal: MlsMessageIn,
    /// `hub_accepted_time`, as the hub sent it.
    pub hub_accepted_time: u64,
}

/// A decrypted GroupInfoRatchetTreeTBE (draft-ietf-mimi-protocol-06 §5.6).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct GroupInfoAndTree {
    /// The GroupInfo, without a `ratchet_tree` extension and not yet
    /// verified. Check it with [`group_info_names_hub_sender`] before
    /// joining.
    pub group_info: VerifiableGroupInfo,
    /// How to get the ratchet tree.
    pub ratchet_tree_option: RatchetTreeOption,
    /// The proposals the hub accepted during the current epoch, in the
    /// order sent.
    pub pending_proposals: Vec<PendingProposal>,
}

/// Decrypt `encrypted_groupinfo_and_tree` with the private key of the
/// request's `groupInfoPublicKey` and parse the GroupInfoRatchetTreeTBE
/// inside it (draft-ietf-mimi-protocol-06 §5.6).
///
/// Decryption is RFC 9420 §5.1.3 DecryptWithLabel with label "GroupInfo
/// and ratchet_tree encryption" and context `room_id`, using the HPKE
/// algorithms of the response's `cipher_suite`: the draft names no other
/// cipher suite for it, and the group's own suite is inside the
/// ciphertext. `group_info_private_key` is the HPKE private key in the
/// encoding that `crypto` takes for that KEM.
///
/// # Errors
///
/// [`GroupInfoResponseError::DecryptionFailed`] when the ciphertext does
/// not open with the key, and
/// [`GroupInfoResponseError::MalformedGroupInfoAndTree`] when the
/// plaintext does not parse or has trailing octets.
pub fn decrypt_group_info_and_tree(
    crypto: &impl OpenMlsCrypto,
    response: &VerifiedGroupInfoResponse,
    group_info_private_key: &[u8],
) -> Result<GroupInfoAndTree, GroupInfoResponseError> {
    let plaintext = decrypt_with_label(
        crypto,
        response.cipher_suite,
        group_info_private_key,
        TBE_LABEL,
        &response.room_id,
        &response.encrypted_groupinfo_and_tree,
    )
    .ok_or(GroupInfoResponseError::DecryptionFailed)?;
    group_info_and_tree(&plaintext).ok_or(GroupInfoResponseError::MalformedGroupInfoAndTree)
}

/// Parse a GroupInfoRatchetTreeTBE; `None` when it does not parse or has
/// trailing octets.
fn group_info_and_tree(mut rest: &[u8]) -> Option<GroupInfoAndTree> {
    let group_info = VerifiableGroupInfo::tls_deserialize(&mut rest).ok()?;
    let ratchet_tree_option = match u8::tls_deserialize(&mut rest).ok()? {
        1 => RatchetTreeOption::Full(RatchetTreeIn::tls_deserialize(&mut rest).ok()?),
        2 => RatchetTreeOption::HttpsUrl(
            String::from_utf8(VLBytes::tls_deserialize(&mut rest).ok()?.into()).ok()?,
        ),
        3 => RatchetTreeOption::OutOfBand,
        4 => RatchetTreeOption::DistributionService,
        _ => return None,
    };
    let proposals = VLBytes::tls_deserialize_exact(rest).ok()?;
    let mut proposals = proposals.as_slice();
    let mut pending_proposals = Vec::new();
    while !proposals.is_empty() {
        pending_proposals.push(PendingProposal {
            proposal: MlsMessageIn::tls_deserialize(&mut proposals).ok()?,
            hub_accepted_time: u64::tls_deserialize(&mut proposals).ok()?,
        });
    }
    Some(GroupInfoAndTree {
        group_info,
        ratchet_tree_option,
        pending_proposals,
    })
}

/// RFC 9420 §5.1.3 DecryptWithLabel: HPKE OpenBase with info
/// `EncryptContext { "MLS 1.0 " + label, context }` and empty aad.
fn decrypt_with_label(
    crypto: &impl OpenMlsCrypto,
    suite: Ciphersuite,
    private_key: &[u8],
    label: &str,
    context: &[u8],
    ciphertext: &HpkeCiphertext,
) -> Option<Vec<u8>> {
    let mut info = VLBytes::from(format!("MLS 1.0 {label}").into_bytes())
        .tls_serialize_detached()
        .ok()?;
    info.extend(VLBytes::from(context).tls_serialize_detached().ok()?);
    crypto
        .hpke_open(suite.hpke_config(), ciphertext, private_key, &info, &[])
        .ok()
}

/// RFC 9420 §5.1.2 VerifyWithLabel: verify `signature` over
/// `SignContent { "MLS 1.0 " + label, content }`.
fn verify_with_label(
    crypto: &impl OpenMlsCrypto,
    scheme: SignatureScheme,
    public_key: &[u8],
    label: &str,
    content: &[u8],
    signature: &[u8],
) -> bool {
    let Ok(sign_content) = SignContent::new(label, content.into()).tls_serialize_detached() else {
        return false;
    };
    crypto
        .verify_signature(scheme, &sign_content, public_key, signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmls::prelude::{
        BasicCredential, CredentialWithKey, LeafNodeParameters, MlsGroup, MlsMessageBodyIn,
        OpenMlsProvider as _,
    };
    use openmls_basic_credential::SignatureKeyPair;
    use openmls_rust_crypto::{OpenMlsRustCrypto, RustCrypto};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// `sign_with_label` of cipher suites 1 and 2 in
    /// mlswg/mls-implementations test-vectors/crypto-basics.json
    /// (commit 2034fe7): (scheme, pub, label, content, signature).
    const VECTORS: [(SignatureScheme, &str, &str, &str, &str); 2] = [
        (
            SignatureScheme::ED25519,
            "85600e54e5c2919ccbd0742126e5d837cf7a2ba50d75a69b3f35dcfe4a50ffe2",
            "SignWithLabel",
            "cd289cc7ba2869f64f3c32ffd133f500d17abace919a5ffe7faa974200d81932",
            "996bd223ddb4d55a2b57d85cb2944f21facc95696053ddf66d590060fdc719f4a26c6212ce605414e0d5e66a55921dd99d11122218c35bc23408b0076e8bc40b",
        ),
        (
            SignatureScheme::ECDSA_SECP256R1_SHA256,
            "04448971fc06de011d780cf68fd27e2570322d04079f529c3deb48a2015fdd828162c570264e051b5856e8111171fb0341907173aefa665682c2549af982a31483",
            "SignWithLabel",
            "a0dda617ce4685d764c762b11186b6d60ff8de85ff01eca2413bd4ecfd3b3a57",
            "304402206042e397e1bb78951709790e3446b13bf17f9c641aaed92fb1768b5bc99dd98402202465153d91dd79e808286088768d8ed150381f3675498d6e150ae713be43387b",
        ),
    ];

    #[test]
    fn verify_with_label_accepts_rfc9420_vectors() {
        for (scheme, public, label, content, signature) in VECTORS {
            assert!(
                verify_with_label(
                    &RustCrypto::default(),
                    scheme,
                    &hex(public),
                    label,
                    &hex(content),
                    &hex(signature),
                ),
                "{scheme:?}"
            );
        }
    }

    #[test]
    fn verify_with_label_rejects_other_label_or_content() {
        for (scheme, public, label, content, signature) in VECTORS {
            let crypto = RustCrypto::default();
            let mut changed = hex(content);
            changed[0] ^= 1;
            assert!(!verify_with_label(
                &crypto,
                scheme,
                &hex(public),
                label,
                &changed,
                &hex(signature)
            ));
            assert!(!verify_with_label(
                &crypto,
                scheme,
                &hex(public),
                LABEL,
                &hex(content),
                &hex(signature)
            ));
        }
    }

    /// `encrypt_with_label` of cipher suites 1 and 2 in
    /// mlswg/mls-implementations test-vectors/crypto-basics.json
    /// (commit 2034fe7): (suite, priv, label, context, kem_output,
    /// ciphertext, plaintext).
    const ENCRYPT_VECTORS: [(Ciphersuite, &str, &str, &str, &str, &str, &str); 2] = [
        (
            Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519,
            "fb1ade7939987ff12a9d620772b1f9f7caeba26f8a3ecea9617d9402cd862444",
            "EncryptWithLabel",
            "26347dd7f218d1de8673d6a66646ce06ac5fd3aa8d5c33f65d86aeefdcf4a31e",
            "0a144e8fbf2d6dcf6fe9d2e2b8aeca5461ff5b0ea9c0ede1040c3dc7ed1dfd1c",
            "15c80ea2bc37db221baa530ef5aea88650f0ce0f262803d6f78f3a1392f7ccd960eff94ca081ee54efa4c3acfa0eb591",
            "8f55dd30f03d64335c22b53ea7670bb1becf49b04021f706368fe93eeb358f46",
        ),
        (
            Ciphersuite::MLS_128_DHKEMP256_AES128GCM_SHA256_P256,
            "ff21771424dadd640e05c67983aafe19b4d8df50783a0c2decc17d0c7ca4cc17",
            "EncryptWithLabel",
            "e27e3b0104990cca866751732c82787af4dcf265c893f77e31bcfd679370e24e",
            "041fae8a8173cad50c0cc6d55f148ff8edda63083b3673cf6dce6a0ae6ee3f0b61505470309dade87ac5ccdb581da3e9ddf6726949d5a92b65dcad6c8679c7313e",
            "98b7d4edc79f4a90f65434050f75e31a3bc6501c584d41abd6ed5fe3c2db9de22d25ef40c45cce7d0fcc881d7d27af2f",
            "38a6b327573639d654b5b729336cf74d01728cf4fa9af81a0ef1814ffc1d492f",
        ),
    ];

    #[test]
    fn decrypt_with_label_matches_rfc9420_vectors() {
        for (suite, private, label, context, kem_output, ciphertext, plaintext) in ENCRYPT_VECTORS {
            let crypto = RustCrypto::default();
            let ct = HpkeCiphertext {
                kem_output: hex(kem_output).into(),
                ciphertext: hex(ciphertext).into(),
            };
            assert_eq!(
                decrypt_with_label(&crypto, suite, &hex(private), label, &hex(context), &ct),
                Some(hex(plaintext)),
                "{suite:?}"
            );
            // The label and the context are bound into the HPKE info.
            assert_eq!(
                decrypt_with_label(&crypto, suite, &hex(private), TBE_LABEL, &hex(context), &ct),
                None
            );
            let mut other_context = hex(context);
            other_context[0] ^= 1;
            assert_eq!(
                decrypt_with_label(&crypto, suite, &hex(private), label, &other_context, &ct),
                None
            );
        }
    }

    const ROOM: &[u8] = b"mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";
    const SUITE1: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
    /// crypto-basics.json cipher_suite 1 `encrypt_with_label` `pub` and
    /// `priv`, used as the request's `groupInfoPublicKey`.
    const GI_PUB: &str = "ecea6564da58d6c6cff6c733bd4ae0815b1f60bb911b73e4ef1d06263ec4ce58";
    const GI_PRIV: &str = "fb1ade7939987ff12a9d620772b1f9f7caeba26f8a3ecea9617d9402cd862444";

    /// opaque <V> (RFC 9420 §2.1.2) for fewer than 2^14 octets.
    fn opaque(data: &[u8]) -> Vec<u8> {
        let n = data.len();
        let mut out = if n < 0x40 {
            vec![n as u8]
        } else {
            vec![0x40 | (n >> 8) as u8, n as u8]
        };
        out.extend_from_slice(data);
        out
    }

    /// EncryptWithLabel(GI_PUB, "GroupInfo and ratchet_tree encryption",
    /// `room_id`, `plaintext`) with the RFC 9420 §5.1.3 EncryptContext
    /// written out by hand, sealed by the HPKE library directly.
    fn response_with(room_id: &[u8], plaintext: &[u8]) -> VerifiedGroupInfoResponse {
        let info = [
            opaque(b"MLS 1.0 GroupInfo and ratchet_tree encryption"),
            opaque(room_id),
        ]
        .concat();
        let encrypted_groupinfo_and_tree = RustCrypto::default()
            .hpke_seal(SUITE1.hpke_config(), &hex(GI_PUB), &info, &[], plaintext)
            .expect("seal");
        VerifiedGroupInfoResponse {
            room_id: room_id.to_vec(),
            cipher_suite: SUITE1,
            hub_sender: ExternalSender::new(
                SignaturePublicKey::from(&[0u8; 32][..]),
                BasicCredential::new(b"mimi://a.example".to_vec()).into(),
            ),
            encrypted_groupinfo_and_tree,
        }
    }

    /// The GroupInfo octets, the ratchet tree and a self-update proposal
    /// MLSMessage of a one-member openmls group.
    fn group_parts() -> (Vec<u8>, RatchetTreeIn, Vec<u8>) {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(SUITE1.signature_algorithm()).expect("signer");
        let mut group = MlsGroup::builder()
            .ciphersuite(SUITE1)
            .build(
                &provider,
                &signer,
                CredentialWithKey {
                    credential: BasicCredential::new(b"mimi://a.example/u/alice".to_vec()).into(),
                    signature_key: signer.public().into(),
                },
            )
            .expect("group");
        let message = group
            .export_group_info(provider.crypto(), &signer, false)
            .expect("GroupInfo")
            .tls_serialize_detached()
            .expect("serialize");
        let MlsMessageBodyIn::GroupInfo(info) = MlsMessageIn::tls_deserialize_exact(message)
            .expect("MLSMessage")
            .extract()
        else {
            panic!("not a GroupInfo");
        };
        let tree = RatchetTreeIn::from(group.export_ratchet_tree());
        let (proposal, _) = group
            .propose_self_update(&provider, &signer, LeafNodeParameters::default())
            .expect("proposal");
        (
            info.tls_serialize_detached().expect("GroupInfo"),
            tree,
            proposal.tls_serialize_detached().expect("proposal"),
        )
    }

    /// A GroupInfoRatchetTreeTBE (draft-ietf-mimi-protocol-06 §5.6).
    fn tbe(group_info: &[u8], option: &[u8], pending: &[u8]) -> Vec<u8> {
        [group_info, option, &opaque(pending)].concat()
    }

    #[test]
    fn decrypts_and_parses_group_info_and_tree() {
        let crypto = RustCrypto::default();
        let (group_info, tree, proposal) = group_parts();
        let expected_info = VerifiableGroupInfo::tls_deserialize_exact(&group_info).unwrap();
        let expected_proposal = MlsMessageIn::tls_deserialize_exact(&proposal).unwrap();

        // full(1) with one PendingProposal accepted at 0x0102030405060708.
        let option = [&[1u8][..], &tree.tls_serialize_detached().unwrap()].concat();
        let pending = [&proposal[..], &[1, 2, 3, 4, 5, 6, 7, 8]].concat();
        let response = response_with(ROOM, &tbe(&group_info, &option, &pending));
        let decrypted =
            decrypt_group_info_and_tree(&crypto, &response, &hex(GI_PRIV)).expect("decrypts");
        assert_eq!(decrypted.group_info, expected_info);
        assert_eq!(decrypted.ratchet_tree_option, RatchetTreeOption::Full(tree));
        assert_eq!(
            decrypted.pending_proposals,
            vec![PendingProposal {
                proposal: expected_proposal,
                hub_accepted_time: 0x0102_0304_0506_0708,
            }]
        );

        // httpsUrl(2), outOfBand(3) and distributionService(4), no proposals.
        let url = [&[2u8][..], &opaque(b"https://a.example/tree")].concat();
        for (option, expected) in [
            (
                url,
                RatchetTreeOption::HttpsUrl("https://a.example/tree".into()),
            ),
            (vec![3], RatchetTreeOption::OutOfBand),
            (vec![4], RatchetTreeOption::DistributionService),
        ] {
            let response = response_with(ROOM, &tbe(&group_info, &option, &[]));
            let decrypted =
                decrypt_group_info_and_tree(&crypto, &response, &hex(GI_PRIV)).expect("decrypts");
            assert_eq!(decrypted.ratchet_tree_option, expected);
            assert!(decrypted.pending_proposals.is_empty());
        }
    }

    #[test]
    fn refuses_other_key_room_or_malformed_plaintext() {
        let crypto = RustCrypto::default();
        let (group_info, _, _) = group_parts();
        let good = tbe(&group_info, &[3], &[]);

        // crypto-basics.json cipher_suite 1 `sign_with_label` `priv`: a
        // valid X25519 scalar that is not GI_PRIV.
        let other_key = hex("a2f640dd5005fcad6adb8e9bd8b60d70946bb802e1e788307929fdac81e1ec74");
        assert_eq!(
            decrypt_group_info_and_tree(&crypto, &response_with(ROOM, &good), &other_key),
            Err(GroupInfoResponseError::DecryptionFailed)
        );
        // The context is the room_id the response carries.
        let mut response = response_with(b"mimi://a.example/r/other", &good);
        response.room_id = ROOM.to_vec();
        assert_eq!(
            decrypt_group_info_and_tree(&crypto, &response, &hex(GI_PRIV)),
            Err(GroupInfoResponseError::DecryptionFailed)
        );

        for plaintext in [
            tbe(&group_info, &[5], &[]),
            tbe(&group_info, &[2, 1, 0xff], &[]),
            [&good[..], &[0]].concat(),
            tbe(&group_info, &[3], &[0; 7]),
            group_info[..group_info.len() - 1].to_vec(),
        ] {
            assert_eq!(
                decrypt_group_info_and_tree(
                    &crypto,
                    &response_with(ROOM, &plaintext),
                    &hex(GI_PRIV)
                ),
                Err(GroupInfoResponseError::MalformedGroupInfoAndTree)
            );
        }
    }
}
