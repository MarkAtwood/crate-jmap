//! The client's GroupInfo checks and the last-client rule against
//! draft-atwood-jmap-chat-mimi-00 (jmap-chat-spec 6bcd9d8).
//!
//! L419: "The client SHOULD verify the response signature with
//! `hub_sender`, and SHOULD verify that the decrypted GroupInfo's
//! `external_senders` extension contains that `hub_sender`, before it
//! joins."
//!
//! L260: "The account leaves the room's MLS group when a commit removes
//! from the group every leaf whose credential identity is the account's
//! `userUri`" and "a client that processes a commit removing the last of
//! its account's clients SHOULD then call `Room/unbind`".
//!
//! The GroupInfoResponse fixtures come from `fixtures/group_info_response.py`
//! (pyca/cryptography 41.0.7), which encodes draft-ietf-mimi-protocol-06
//! §5.6 by hand and signs with RFC 9420 §5.1.2 SignWithLabel. Its suite 1
//! and suite 2 keys are the `sign_with_label` private keys of
//! mlswg/mls-implementations test-vectors/crypto-basics.json, whose public
//! keys appear below.

use jmap_chat_mimi_client::{
    GroupInfoResponseError, account_has_left, group_info_names_hub_sender,
    verify_group_info_response,
};
use openmls::messages::group_info::VerifiableGroupInfo;
use openmls::prelude::{
    BasicCredential, Ciphersuite, Credential, CredentialWithKey, Extension, Extensions,
    ExternalSender, LeafNodeIndex, Member, MlsGroup, MlsMessageBodyIn, MlsMessageIn,
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{OpenMlsRustCrypto, RustCrypto};
use tls_codec::{Deserialize as _, Serialize as _};

const ROOM: &[u8] = b"mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";
const HUB: &[u8] = b"mimi://a.example";
const ALICE: &str = "mimi://a.example/u/alice";

/// crypto-basics.json cipher_suite 1 `sign_with_label.pub`.
const SUITE1_PUB: &str = "85600e54e5c2919ccbd0742126e5d837cf7a2ba50d75a69b3f35dcfe4a50ffe2";
/// crypto-basics.json cipher_suite 2 `sign_with_label.pub`.
const SUITE2_PUB: &str = "04448971fc06de011d780cf68fd27e2570322d04079f529c3deb48a2015fdd828162c570264e051b5856e8111171fb0341907173aefa665682c2549af982a31483";

const SUITE1_RESPONSE: &str = "012d6d696d693a2f2f612e6578616d706c652f722f6b3571776d7a6c716d6673676b3374626e76737869326c6634610100012085600e54e5c2919ccbd0742126e5d837cf7a2ba50d75a69b3f35dcfe4a50ffe20001106d696d693a2f2f612e6578616d706c6520000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f0a636970686572746578744040b9c9c1d7ef0df17656f298347d55f94df9dc735e5c54f400c2611226675c2c167d45dcc432ea2b4d738d135228c3b6cccd04bb5f29e71ac462146be1da82280c";
const SUITE2_RESPONSE: &str = "012d6d696d693a2f2f612e6578616d706c652f722f6b3571776d7a6c716d6673676b3374626e76737869326c663461010002404104448971fc06de011d780cf68fd27e2570322d04079f529c3deb48a2015fdd828162c570264e051b5856e8111171fb0341907173aefa665682c2549af982a314830001106d696d693a2f2f612e6578616d706c6520000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f0a63697068657274657874404730450221008b2cbc8e5c6bce4c5742231847e9f50ae2804c1387a86a11d223efa005bd452d022047e86ab8d196f6f1c68f2936b479acde96172a112ebd9c4befcaa78b3840eaac";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn basic(identity: &[u8]) -> Credential {
    BasicCredential::new(identity.to_vec()).into()
}

/// The fixtures' `hub_sender`: the vector public key and a Basic
/// credential with identity `mimi://a.example`.
fn fixture_hub_sender(public: &str) -> ExternalSender {
    ExternalSender::new(hex(public).into(), basic(HUB))
}

#[test]
fn verifies_signed_responses() {
    for (response, suite, public) in [
        (
            SUITE1_RESPONSE,
            Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519,
            SUITE1_PUB,
        ),
        (
            SUITE2_RESPONSE,
            Ciphersuite::MLS_128_DHKEMP256_AES128GCM_SHA256_P256,
            SUITE2_PUB,
        ),
    ] {
        let verified =
            verify_group_info_response(&RustCrypto::default(), &hex(response)).expect("verifies");
        assert_eq!(verified.room_id, ROOM);
        assert_eq!(verified.cipher_suite, suite);
        assert_eq!(verified.hub_sender, fixture_hub_sender(public));
        assert_eq!(
            verified.encrypted_groupinfo_and_tree.kem_output.as_slice(),
            (0u8..32).collect::<Vec<_>>()
        );
        assert_eq!(
            verified.encrypted_groupinfo_and_tree.ciphertext.as_slice(),
            b"ciphertext"
        );
    }
}

#[test]
fn rejects_any_changed_octet() {
    // Every octet before the signature is in GroupInfoResponseTBS, and the
    // signature octets are the signature: changing any one must fail.
    // Changes that break the framing fail to parse instead.
    let original = hex(SUITE1_RESPONSE);
    for i in 0..original.len() {
        let mut changed = original.clone();
        changed[i] ^= 0x01;
        assert!(
            verify_group_info_response(&RustCrypto::default(), &changed).is_err(),
            "octet {i}"
        );
    }
}

#[test]
fn rejects_signature_by_another_key() {
    // Swap the suite 1 hub_sender key for the key 0x00..0x1f (same length,
    // so the framing holds): the signature no longer verifies.
    let mut changed = hex(SUITE1_RESPONSE);
    let at = changed
        .windows(32)
        .position(|w| w == hex(SUITE1_PUB))
        .expect("key in fixture");
    changed[at..at + 32].copy_from_slice(&(0u8..32).collect::<Vec<_>>());
    assert_eq!(
        verify_group_info_response(&RustCrypto::default(), &changed),
        Err(GroupInfoResponseError::BadSignature)
    );
}

#[test]
fn non_success_status_has_no_signature() {
    // draft-ietf-mimi-protocol-06 §5.6: `select (status) { case success:
    // ...; default: struct{}; }`, `notAuthorized(2)`, `noSuchRoom(3)`.
    for status in [2u8, 3] {
        let mut response = vec![0x01, ROOM.len() as u8];
        response.extend_from_slice(ROOM);
        response.push(status);
        assert_eq!(
            verify_group_info_response(&RustCrypto::default(), &response),
            Err(GroupInfoResponseError::NotSuccess(status))
        );
    }
}

#[test]
fn rejects_truncation_trailing_octets_and_unknown_suite() {
    let original = hex(SUITE1_RESPONSE);
    let crypto = RustCrypto::default();
    assert_eq!(
        verify_group_info_response(&crypto, &original[..original.len() - 1]),
        Err(GroupInfoResponseError::Malformed)
    );
    let mut trailing = original.clone();
    trailing.push(0);
    assert_eq!(
        verify_group_info_response(&crypto, &trailing),
        Err(GroupInfoResponseError::Malformed)
    );
    // `cipher_suite` follows version (1), room_id (1 + 45) and status (1).
    let mut unknown = original.clone();
    unknown[48..50].copy_from_slice(&[0xfe, 0xfe]);
    assert_eq!(
        verify_group_info_response(&crypto, &unknown),
        Err(GroupInfoResponseError::UnsupportedCipherSuite(0xfefe))
    );
    // `Protocol` defines only `mls10(1)`.
    let mut version = original;
    version[0] = 2;
    assert_eq!(
        verify_group_info_response(&crypto, &version),
        Err(GroupInfoResponseError::UnsupportedProtocol(2))
    );
}

/// A GroupInfo exported by an openmls group whose GroupContext carries
/// `external_senders` (none when empty).
fn group_info_with(external_senders: Vec<ExternalSender>) -> VerifiableGroupInfo {
    let suite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
    let provider = OpenMlsRustCrypto::default();
    let signer = SignatureKeyPair::new(suite.signature_algorithm()).expect("signer");
    let mut builder = MlsGroup::builder().ciphersuite(suite);
    if !external_senders.is_empty() {
        builder = builder.with_group_context_extensions(
            Extensions::from_vec(vec![Extension::ExternalSenders(external_senders)])
                .expect("extensions"),
        );
    }
    let group = builder
        .build(
            &provider,
            &signer,
            CredentialWithKey {
                credential: basic(ALICE.as_bytes()),
                signature_key: signer.public().into(),
            },
        )
        .expect("group");
    let bytes = group
        .export_group_info(&RustCrypto::default(), &signer, false)
        .expect("GroupInfo")
        .tls_serialize_detached()
        .expect("serialize");
    match MlsMessageIn::tls_deserialize_exact(bytes)
        .expect("MLSMessage")
        .extract()
    {
        MlsMessageBodyIn::GroupInfo(info) => info,
        _ => panic!("not a GroupInfo"),
    }
}

#[test]
fn group_info_names_hub_sender_when_external_senders_contains_it() {
    let hub = fixture_hub_sender(SUITE1_PUB);
    let other = ExternalSender::new(hex(SUITE2_PUB).into(), basic(b"mimi://b.example"));
    assert!(group_info_names_hub_sender(
        &group_info_with(vec![hub.clone()]),
        &hub
    ));
    // "contains", not "is exactly".
    assert!(group_info_names_hub_sender(
        &group_info_with(vec![other.clone(), hub.clone()]),
        &hub
    ));
    assert!(!group_info_names_hub_sender(
        &group_info_with(vec![other]),
        &hub
    ));
    assert!(!group_info_names_hub_sender(&group_info_with(vec![]), &hub));
    // Same key, different credential: a different ExternalSender.
    let renamed = ExternalSender::new(hex(SUITE1_PUB).into(), basic(b"mimi://b.example"));
    assert!(!group_info_names_hub_sender(
        &group_info_with(vec![renamed]),
        &hub
    ));
}

fn member(index: u32, credential: Credential) -> Member {
    Member::new(
        LeafNodeIndex::new(index),
        vec![index as u8; 32],
        vec![index as u8; 32],
        credential,
    )
}

#[test]
fn account_has_left_only_without_a_useruri_leaf() {
    let bob = || member(1, basic(b"mimi://b.example/u/bob"));
    assert!(!account_has_left(
        [member(0, basic(ALICE.as_bytes())), bob()],
        ALICE
    ));
    // Two of the account's clients: removing one leaves the other.
    assert!(!account_has_left(
        [member(2, basic(ALICE.as_bytes())), bob()],
        ALICE
    ));
    assert!(account_has_left([bob()], ALICE));
    assert!(account_has_left(Vec::<Member>::new(), ALICE));
    // A prefix of `userUri` is a different identity.
    assert!(account_has_left(
        [member(3, basic(b"mimi://a.example/u/ali"))],
        ALICE
    ));
}
