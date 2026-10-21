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

use std::fmt;

use openmls::ciphersuite::signature::SignContent;
use openmls::messages::group_info::VerifiableGroupInfo;
use openmls::prelude::{
    Ciphersuite, Credential, ExternalSender, HpkeCiphertext, OpenMlsCrypto, SignaturePublicKey,
    SignatureScheme,
};
use tls_codec::{Deserialize as _, Serialize as _, VLBytes};

/// `Protocol` `mls10(1)` (draft-ietf-mimi-protocol-06 §4).
const MLS10: u8 = 1;
/// `GroupInfoCode` `success(1)` (draft-ietf-mimi-protocol-06 §5.6).
const SUCCESS: u8 = 1;
/// The SignWithLabel label of the GroupInfoResponse signature.
const LABEL: &str = "GroupInfoResponseTBS";

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
/// signed it. Decrypt `encrypted_groupinfo_and_tree` and check that the
/// GroupInfo names the same hub with [`group_info_names_hub_sender`]
/// before joining.
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
    use openmls_rust_crypto::RustCrypto;

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
}
