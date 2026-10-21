//! Room/create against draft-atwood-jmap-chat-mimi-00 (jmap-chat-spec
//! 6bcd9d8) §Room/create, L277-285: "The server MUST reject the call with
//! `invalidArguments` unless all of the following hold". One test per
//! check; each quotes its line.
//!
//! The MLS groups come from openmls `MlsGroup`. The MIMI components are
//! encoded here by hand from the TLS presentation language of
//! draft-ietf-mimi-protocol-06 §5.4.1, §7.5, §7.6 and
//! draft-ietf-mimi-room-policy-04 §5, independently of the crate's
//! decoders.
//!
//! The last tests check KeyMaterial/fetch (jmap-chat-spec 33702a5, L231)
//! in a room that Room/create made this server the hub of.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jmap_chat_mimi_server::handle_key_package_set;
use jmap_chat_mimi_server::handle_room_create;
use jmap_chat_mimi_server::memory::MemoryBackend;
use jmap_chat_mimi_server::{handle_key_material_fetch, MimiEndpoint};
use jmap_chat_mimi_types::MimiCapability;
use jmap_types::Id;
use openmls::extensions::{AppDataDictionary, AppDataDictionaryExtension};
use openmls::prelude::{
    BasicCredential, Capabilities, Ciphersuite, Credential, CredentialType, CredentialWithKey,
    Extension, ExtensionType, Extensions, ExternalSender, GroupId, KeyPackage, MlsGroup,
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{OpenMlsRustCrypto, RustCrypto};
use serde_json::{json, Value};
use tls_codec::Serialize as _;

const ALICE: &str = "mimi://a.example/u/alice";
const BOB: &str = "mimi://a.example/u/bob";
const HUB: &str = "mimi://a.example";
const PREFIX: &str = "mimi://a.example/r/";
/// The `roomId` example of jmap-chat-mimi-types' `chat_room_id`: 26
/// lowercase base32 characters whose last, `a`, has zero pad bits.
const ROOM: &str = "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";
const SUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// RFC 9420 §2.1.2 variable-length integer prefix for `len` < 2^14.
fn varint(len: usize) -> Vec<u8> {
    if len < 64 {
        vec![len as u8]
    } else {
        vec![0x40 | (len >> 8) as u8, len as u8]
    }
}

/// `opaque <name><V>`.
fn opaque(bytes: &[u8]) -> Vec<u8> {
    let mut out = varint(bytes.len());
    out.extend_from_slice(bytes);
    out
}

/// `struct { UserRolePair participants<V>; } ParticipantListData` with
/// `struct { opaque user<V>; uint32 role_index; } UserRolePair`
/// (draft-ietf-mimi-protocol-06 §7.5), every role index 1.
fn participant_list(users: &[&str]) -> Vec<u8> {
    let mut inner = Vec::new();
    for user in users {
        inner.extend(opaque(user.as_bytes()));
        inner.extend_from_slice(&1u32.to_be_bytes());
    }
    opaque(&inner)
}

/// `RoomMetaData` (draft-ietf-mimi-protocol-06 §7.6): `room_uri`,
/// `room_name`, an empty `room_descriptions`, and empty `room_avatar`,
/// `room_subject` and `room_mood`.
fn room_metadata(room_uri: &str, room_name: &str) -> Vec<u8> {
    let mut out = opaque(room_uri.as_bytes());
    out.extend(opaque(room_name.as_bytes()));
    out.extend([0, 0, 0, 0]);
    out
}

/// `FrankingAgentData` (draft-ietf-mimi-protocol-06 §5.4.1):
/// `SignaturePublicKey franking_signature_key` (`opaque<V>`, RFC 9420
/// §5.1.1) then a Basic `Credential` (RFC 9420 §5.3: `uint16
/// credential_type` 0x0001, `opaque identity<V>`).
fn franking_agent(signature_key: &[u8], identity: &str) -> Vec<u8> {
    let mut out = opaque(signature_key);
    out.extend([0x00, 0x01]);
    out.extend(opaque(identity.as_bytes()));
    out
}

/// `BaseRoomPolicy` (draft-ietf-mimi-room-policy-04 §5) with
/// `fixed_membership` set to `fixed_membership`: `parent_dependant` false,
/// empty `parent_room`, `multi_device` true, no `max_clients` or
/// `max_users`, `pseudonyms_allowed` false, `persistent_room` true,
/// `discoverable` false, empty `policy_component_ids`.
fn base_room_policy(fixed_membership: u8) -> Vec<u8> {
    vec![fixed_membership, 0, 0, 1, 0, 0, 0, 1, 0, 0]
}

fn new_signer() -> SignatureKeyPair {
    SignatureKeyPair::new(SUITE.signature_algorithm()).expect("signer")
}

fn credential_with_key(identity: &str, signer: &SignatureKeyPair) -> CredentialWithKey {
    CredentialWithKey {
        credential: BasicCredential::new(identity.as_bytes().to_vec()).into(),
        signature_key: signer.public().into(),
    }
}

/// What the client puts in the group; [`Group::valid`] passes every check.
struct Group {
    identity: &'static str,
    group_id: Vec<u8>,
    external_senders: Vec<ExternalSender>,
    participants: Option<Vec<&'static str>>,
    room_uri: Option<String>,
    franking: Option<Vec<u8>>,
    policy: Option<Vec<u8>>,
    add_bob: bool,
    /// Give the leaf an X.509 credential holding `identity` instead of a
    /// Basic one.
    x509: bool,
}

struct Hub {
    signer: SignatureKeyPair,
}

impl Hub {
    fn new() -> Self {
        Self {
            signer: new_signer(),
        }
    }

    fn sender(&self) -> ExternalSender {
        ExternalSender::new(
            self.signer.public().into(),
            BasicCredential::new(HUB.as_bytes().to_vec()).into(),
        )
    }

    fn franking(&self) -> Vec<u8> {
        franking_agent(self.signer.public(), HUB)
    }
}

impl Group {
    fn valid(hub: &Hub) -> Self {
        Self {
            identity: ALICE,
            group_id: vec![7; 16],
            external_senders: vec![hub.sender()],
            participants: Some(vec![ALICE]),
            room_uri: Some(ROOM.to_owned()),
            franking: Some(hub.franking()),
            policy: Some(base_room_policy(0)),
            add_bob: false,
            x509: false,
        }
    }

    fn credential_with_key(&self, signer: &SignatureKeyPair) -> CredentialWithKey {
        let mut cwk = credential_with_key(self.identity, signer);
        if self.x509 {
            cwk.credential =
                Credential::new(CredentialType::X509, self.identity.as_bytes().to_vec());
        }
        cwk
    }

    /// The `groupInfo` and `ratchetTree` arguments for this group, built
    /// by `signer`.
    fn build(&self, signer: &SignatureKeyPair) -> (String, String) {
        let provider = OpenMlsRustCrypto::default();
        let mut dict = AppDataDictionary::new();
        if let Some(users) = &self.participants {
            dict.insert(0x0022, participant_list(users));
        }
        if let Some(uri) = &self.room_uri {
            dict.insert(0x0023, room_metadata(uri, "Lunch"));
        }
        if let Some(franking) = &self.franking {
            dict.insert(0x0021, franking.clone());
        }
        if let Some(policy) = &self.policy {
            dict.insert(0x0027, policy.clone());
        }
        let mut extensions = vec![Extension::AppDataDictionary(
            AppDataDictionaryExtension::new(dict),
        )];
        if !self.external_senders.is_empty() {
            extensions.push(Extension::ExternalSenders(self.external_senders.clone()));
        }
        let mut group = MlsGroup::builder()
            .ciphersuite(SUITE)
            .with_group_id(GroupId::from_slice(&self.group_id))
            .with_capabilities(capabilities())
            .with_group_context_extensions(
                Extensions::from_vec(extensions).expect("GroupContext extensions"),
            )
            .build(&provider, signer, self.credential_with_key(signer))
            .expect("build group");
        if self.add_bob {
            let bob_signer = new_signer();
            let bob = KeyPackage::builder()
                .leaf_node_capabilities(capabilities())
                .build(
                    SUITE,
                    &provider,
                    &bob_signer,
                    credential_with_key(BOB, &bob_signer),
                )
                .expect("Bob's KeyPackage");
            group
                .add_members(&provider, signer, &[bob.key_package().clone()])
                .expect("add Bob");
            group
                .merge_pending_commit(&provider)
                .expect("merge add commit");
        }
        let group_info = group
            .export_group_info(&RustCrypto::default(), signer, false)
            .expect("export GroupInfo")
            .tls_serialize_detached()
            .expect("serialize GroupInfo");
        let tree = group
            .export_ratchet_tree()
            .tls_serialize_detached()
            .expect("serialize ratchet tree");
        (b64(&group_info), b64(&tree))
    }
}

fn capabilities() -> Capabilities {
    Capabilities::new(
        None,
        None,
        Some(&[ExtensionType::AppDataDictionary]),
        None,
        Some(&[CredentialType::Basic, CredentialType::X509]),
    )
}

fn capability(hub: &Hub, suites: &[u16]) -> MimiCapability {
    let hub_sender = hub.sender().tls_serialize_detached().expect("hubSender");
    serde_json::from_value(json!({
        "userUri": ALICE,
        "roomIdPrefix": PREFIX,
        "hubSender": b64(&hub_sender),
        "cipherSuites": suites,
        "maxKeyPackages": 10,
    }))
    .expect("capability")
}

/// A backend with account `a1` for Alice, whose `signer` is bound to it
/// by an accepted KeyPackage when `bind` is set.
async fn backend(hub: &Hub, signer: &SignatureKeyPair, bind: bool) -> MemoryBackend<RustCrypto> {
    let backend = MemoryBackend::new(RustCrypto::default());
    backend.register_account(&Id::from("a1"), capability(hub, &[1]));
    if bind {
        let provider = OpenMlsRustCrypto::default();
        let kp = KeyPackage::builder()
            .build(SUITE, &provider, signer, credential_with_key(ALICE, signer))
            .expect("Alice's KeyPackage")
            .key_package()
            .tls_serialize_detached()
            .expect("serialize KeyPackage");
        let out = handle_key_package_set(
            &backend,
            &(),
            json!({ "accountId": "a1", "create": { "k": { "keyPackage": b64(&kp) } } }),
        )
        .await
        .expect("KeyPackage/set");
        assert!(out.primary["created"]["k"].is_object(), "{}", out.primary);
    }
    backend
}

async fn create(
    backend: &MemoryBackend<RustCrypto>,
    (group_info, ratchet_tree): &(String, String),
) -> Result<Value, String> {
    handle_room_create(
        backend,
        &(),
        json!({ "accountId": "a1", "groupInfo": group_info, "ratchetTree": ratchet_tree }),
    )
    .await
    .map(|out| out.primary)
    .map_err(|e| e.error_type.to_string())
}

/// Assert that `args` is rejected with `invalidArguments`, naming `why`.
async fn assert_rejected(backend: &MemoryBackend<RustCrypto>, args: &(String, String), why: &str) {
    assert_eq!(
        create(backend, args).await,
        Err("invalidArguments".to_owned()),
        "{why}"
    );
}

/// L289: "On success the server creates a Chat bound to the room, with
/// `roomId` set to the room ID ({{chat-roomid}}), and returns: `accountId`
/// ... `roomId` ... `chatId`". L256: "A Chat with a `roomId` has kind
/// `"group"`" and "The server sets `name` to the `room_name` of the room
/// metadata"; L258: "`members` of a Chat with a `roomId` is the empty
/// list, whether `Room/create` ... or a Welcome ... created it."
#[tokio::test]
async fn room_create_creates_bound_chat() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    let out = create(&backend, &Group::valid(&hub).build(&alice))
        .await
        .expect("Room/create succeeds");
    assert_eq!(out["accountId"], "a1");
    assert_eq!(out["roomId"], ROOM);
    let chat_id = out["chatId"].as_str().expect("chatId");
    let chat = backend.room_chat(&Id::from(chat_id)).expect("bound Chat");
    assert_eq!(chat["roomId"], ROOM);
    assert_eq!(chat["kind"], "group");
    assert_eq!(chat["name"], "Lunch");
    assert_eq!(chat["members"], json!([]));
}

/// L279, check 1: "The GroupInfo signature verifies, and its cipher suite
/// is in `cipherSuites`."
#[tokio::test]
async fn check_1_signature_and_cipher_suite() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    let (group_info, tree) = Group::valid(&hub).build(&alice);

    // GroupInfo ends with `opaque signature<V>` (RFC 9420 §12.4.3): flip
    // its last octet.
    let mut bytes = URL_SAFE_NO_PAD.decode(&group_info).expect("b64");
    *bytes.last_mut().expect("non-empty") ^= 0x01;
    assert_rejected(&backend, &(b64(&bytes), tree.clone()), "bad signature").await;

    // Cipher suite 1 is the group's; the account now offers only suite 3.
    backend.register_account(&Id::from("a1"), capability(&hub, &[3]));
    assert_rejected(&backend, &(group_info, tree), "cipher suite not offered").await;
}

/// L280, check 2: "The `ratchetTree` matches the tree hash in the
/// GroupInfo's GroupContext and contains exactly one leaf, whose
/// credential identity is `userUri` and whose signature key is bound to
/// the account ({{keypackage}})."
#[tokio::test]
async fn check_2_tree_hash_one_leaf_identity_binding() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend_bound = backend(&hub, &alice, true).await;

    let (group_info, _) = Group::valid(&hub).build(&alice);
    let (_, other_tree) = Group::valid(&hub).build(&alice);
    assert_rejected(&backend_bound, &(group_info, other_tree), "tree hash").await;

    let two_leaves = Group {
        add_bob: true,
        ..Group::valid(&hub)
    };
    assert_rejected(&backend_bound, &two_leaves.build(&alice), "two leaves").await;

    let mallory = Group {
        identity: "mimi://a.example/u/mallory",
        ..Group::valid(&hub)
    };
    assert_rejected(&backend_bound, &mallory.build(&alice), "identity").await;

    let unbound = backend(&hub, &alice, false).await;
    assert_rejected(&unbound, &Group::valid(&hub).build(&alice), "unbound key").await;

    // Alice's signature key bound to account b1 (Bob) instead of a1.
    let elsewhere = backend(&hub, &alice, false).await;
    let mut bob_capability = capability(&hub, &[1]);
    bob_capability.user_uri = BOB.to_owned();
    elsewhere.register_account(&Id::from("b1"), bob_capability);
    let provider = OpenMlsRustCrypto::default();
    let kp = KeyPackage::builder()
        .build(SUITE, &provider, &alice, credential_with_key(BOB, &alice))
        .expect("KeyPackage")
        .key_package()
        .tls_serialize_detached()
        .expect("serialize KeyPackage");
    let out = handle_key_package_set(
        &elsewhere,
        &(),
        json!({ "accountId": "b1", "create": { "k": { "keyPackage": b64(&kp) } } }),
    )
    .await
    .expect("KeyPackage/set");
    assert!(out.primary["created"]["k"].is_object(), "{}", out.primary);
    assert_rejected(
        &elsewhere,
        &Group::valid(&hub).build(&alice),
        "key bound to another account",
    )
    .await;
}

/// L274: `groupInfo` is "An MLSMessage ({{RFC9420, Section 6}}) of wire
/// format `mls_group_info`"; L277: `ratchetTree` is "a `ratchet_tree`
/// vector of `optional<Node>` entries"; check 2 needs the leaf's
/// "credential identity", which RFC 9420 §5.3 gives only for a Basic
/// credential.
#[tokio::test]
async fn malformed_arguments_and_a_non_basic_credential() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    let (group_info, tree) = Group::valid(&hub).build(&alice);

    // RFC 9420 §6: MLSMessage is `version` 0x0001 (mls10), `wire_format`,
    // then the body; wire format 0x0005 is mls_key_package.
    let provider = OpenMlsRustCrypto::default();
    let kp = KeyPackage::builder()
        .build(SUITE, &provider, &alice, credential_with_key(ALICE, &alice))
        .expect("KeyPackage")
        .key_package()
        .tls_serialize_detached()
        .expect("serialize KeyPackage");
    let kp_message = [&[0x00, 0x01, 0x00, 0x05][..], &kp].concat();
    assert_rejected(
        &backend,
        &(b64(&kp_message), tree.clone()),
        "mls_key_package wire format",
    )
    .await;

    for (args, why) in [
        (("!!".to_owned(), tree.clone()), "groupInfo not base64url"),
        (
            (group_info.clone(), "!!".to_owned()),
            "ratchetTree not base64url",
        ),
        ((b64(&[0x00]), tree.clone()), "groupInfo does not parse"),
        (
            (group_info.clone(), b64(&[0xff])),
            "ratchetTree does not parse",
        ),
    ] {
        assert_rejected(&backend, &args, why).await;
    }

    let x509 = Group {
        x509: true,
        ..Group::valid(&hub)
    };
    assert_rejected(&backend, &x509.build(&alice), "X.509 leaf credential").await;
}

/// L281, check 3: "The `external_senders` extension contains exactly one
/// ExternalSender, equal to `hubSender`."
#[tokio::test]
async fn check_3_external_senders() {
    let hub = Hub::new();
    let other = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    for (senders, why) in [
        (vec![], "no external_senders"),
        (vec![other.sender()], "another sender"),
        (vec![hub.sender(), other.sender()], "two senders"),
    ] {
        let group = Group {
            external_senders: senders,
            ..Group::valid(&hub)
        };
        assert_rejected(&backend, &group.build(&alice), why).await;
    }
}

/// L282, check 4: "The participant list ({{MIMI-PROTOCOL, Section 7.5}})
/// contains exactly one participant, `userUri`."
#[tokio::test]
async fn check_4_participant_list() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    for (participants, why) in [
        (None, "no participant list"),
        (Some(vec![ALICE, BOB]), "two participants"),
        (Some(vec![BOB]), "another participant"),
    ] {
        let group = Group {
            participants,
            ..Group::valid(&hub)
        };
        assert_rejected(&backend, &group.build(&alice), why).await;
    }
}

/// L283, check 5: "The `room_uri` of the room metadata ({{MIMI-PROTOCOL,
/// Section 7.6}}) is `roomIdPrefix` followed by the unpadded lowercase
/// base32 encoding ({{RFC4648, Section 6}}) of 16 octets, and no room this
/// server hosts has that room ID."
#[tokio::test]
async fn check_5_room_id() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    for (uri, why) in [
        (None, "no room metadata"),
        (
            Some("mimi://b.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a"),
            "another prefix",
        ),
        (
            Some("mimi://a.example/r/K5QWMZLQMFSGK3TBNVSXI2LF4A"),
            "uppercase",
        ),
        (
            Some("mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a======"),
            "padded",
        ),
        // 16 octets are 128 bits: 26 characters, the last with two zero
        // pad bits. `b` (1) sets a pad bit.
        (
            Some("mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4b"),
            "non-zero pad bits",
        ),
        // 20 characters encode 12 octets (RFC 4648 §6: 5 octets per 8).
        (Some("mimi://a.example/r/k5qwmzlqmfsgk3tbnvsx"), "12 octets"),
    ] {
        let group = Group {
            room_uri: uri.map(str::to_owned),
            ..Group::valid(&hub)
        };
        assert_rejected(&backend, &group.build(&alice), why).await;
    }

    create(&backend, &Group::valid(&hub).build(&alice))
        .await
        .expect("first room");
    let same_room = Group {
        group_id: vec![8; 16],
        ..Group::valid(&hub)
    };
    assert_rejected(&backend, &same_room.build(&alice), "room ID in use").await;
}

/// L284, check 6: "The group ID is 16 octets and no room this server hosts
/// uses it."
#[tokio::test]
async fn check_6_group_id() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    let short = Group {
        group_id: vec![7; 15],
        ..Group::valid(&hub)
    };
    assert_rejected(&backend, &short.build(&alice), "15-octet group ID").await;

    create(&backend, &Group::valid(&hub).build(&alice))
        .await
        .expect("first room");
    let same_group = Group {
        room_uri: Some(format!("{PREFIX}{}", "a".repeat(26))),
        ..Group::valid(&hub)
    };
    assert_rejected(&backend, &same_group.build(&alice), "group ID in use").await;
}

/// L285, check 7: "The group's `app_data_dictionary` extension
/// ({{MLS-EXTENSIONS, Section 4.6}}) contains a `franking_agent` component
/// ({{MIMI-PROTOCOL, Section 5.4.1}}) whose `franking_signature_key` and
/// `credential` are those of `hubSender`, and a `base_room_policy`
/// component ({{MIMI-ROOM-POLICY, Section 5}}) that parses."
#[tokio::test]
async fn check_7_franking_agent_and_policy() {
    let hub = Hub::new();
    let other = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    for (franking, why) in [
        (None, "no franking_agent"),
        (Some(other.franking()), "another key"),
        (
            Some(franking_agent(hub.signer.public(), "mimi://b.example")),
            "another credential",
        ),
    ] {
        let group = Group {
            franking,
            ..Group::valid(&hub)
        };
        assert_rejected(&backend, &group.build(&alice), why).await;
    }
    // room-policy-04 §5: `enum { false(0), true(1) } bool`, so 2 is not a
    // bool; and a policy cut short does not parse.
    for (policy, why) in [
        (None, "no base_room_policy"),
        (Some(base_room_policy(2)), "bool 2"),
        (Some(vec![0, 0]), "truncated"),
    ] {
        let group = Group {
            policy,
            ..Group::valid(&hub)
        };
        assert_rejected(&backend, &group.build(&alice), why).await;
    }
}

/// RFC 8620 §3.6.2: an unknown `accountId` is `accountNotFound`.
#[tokio::test]
async fn unknown_account_is_account_not_found() {
    let hub = Hub::new();
    let alice = new_signer();
    let backend = backend(&hub, &alice, true).await;
    let (group_info, tree) = Group::valid(&hub).build(&alice);
    let err = handle_room_create(
        &backend,
        &(),
        json!({ "accountId": "nope", "groupInfo": group_info, "ratchetTree": tree }),
    )
    .await
    .expect_err("unknown account");
    assert_eq!(err.error_type.to_string(), "accountNotFound");
}

/// A KeyMaterialRequest (draft-ietf-mimi-protocol-06 §5.2): `mls10(1)`,
/// `requestingUser`, `targetUser` and `roomId`, `acceptableCiphersuites`
/// [0x0001], an empty `RequiredCapabilities` (three empty vectors), a
/// 32-octet `SignaturePublicKey`, a Basic `Credential` (RFC 9420 §5.3),
/// and a zero 64-octet signature, which mimi L231 does not ask the server
/// to verify.
fn key_material_request(target: &str) -> Vec<u8> {
    [
        &[1u8][..],
        &opaque(ALICE.as_bytes()),
        &opaque(target.as_bytes()),
        &opaque(ROOM.as_bytes()),
        &[2, 0x00, 0x01],
        &[0, 0, 0],
        &opaque(&[7; 32]),
        &[0x00, 0x01],
        &opaque(ALICE.as_bytes()),
        &opaque(&[0; 64]),
    ]
    .concat()
}

async fn key_material(backend: &MemoryBackend<RustCrypto>, target: &str) -> Result<Value, String> {
    handle_key_material_fetch(
        backend,
        &(),
        json!({ "accountId": "a1", "roomId": ROOM, "request": b64(&key_material_request(target)) }),
    )
    .await
    .map(|out| out.primary)
    .map_err(|e| e.error_type.to_string())
}

/// Alice's backend after a successful Room/create, so this server hosts
/// ROOM, with a local account `b1` for Bob that has published one
/// KeyPackage; returns that KeyPackage's octets.
async fn hosted_room_with_bob(hub: &Hub) -> (MemoryBackend<RustCrypto>, Vec<u8>) {
    let alice = new_signer();
    let backend = backend(hub, &alice, true).await;
    create(&backend, &Group::valid(hub).build(&alice))
        .await
        .expect("Room/create");
    let mut bob_capability = capability(hub, &[1]);
    bob_capability.user_uri = BOB.to_owned();
    backend.register_account(&Id::from("b1"), bob_capability);
    let bob = new_signer();
    let kp = KeyPackage::builder()
        .leaf_node_capabilities(capabilities())
        .build(
            SUITE,
            &OpenMlsRustCrypto::default(),
            &bob,
            credential_with_key(BOB, &bob),
        )
        .expect("Bob's KeyPackage")
        .key_package()
        .tls_serialize_detached()
        .expect("serialize KeyPackage");
    let out = handle_key_package_set(
        &backend,
        &(),
        json!({ "accountId": "b1", "create": { "k": { "keyPackage": b64(&kp) } } }),
    )
    .await
    .expect("KeyPackage/set");
    assert!(out.primary["created"]["k"].is_object(), "{}", out.primary);
    (backend, kp)
}

/// L231: "When this server is the room's hub, it answers from its own
/// KeyPackage store ({{keypackage-claim}}) if the target user is local".
/// L210: it "returns a KeyPackage compatible with the request that is not
/// last-resort and destroys that record". draft-ietf-mimi-protocol-06
/// §5.2: `success` "indicates that key material was provided for every
/// client of the target user"; for a client with none left,
/// `keyMaterialExhausted`, and of the user codes only
/// `noCompatibleMaterial` may "populate the clients list" when no client
/// succeeds. No request leaves the server.
#[tokio::test]
async fn key_material_fetch_answers_a_local_target_from_the_key_package_store() {
    let hub = Hub::new();
    let (backend, kp) = hosted_room_with_bob(&hub).await;

    let out = key_material(&backend, BOB)
        .await
        .expect("KeyMaterial/fetch");
    assert_eq!(out["accountId"], "a1");
    assert_eq!(out["userStatus"], "success");
    assert_eq!(out["userUri"], BOB);
    let clients = out["clients"].as_array().expect("clients");
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0]["clientStatus"], "success");
    assert_eq!(
        clients[0]["keyPackage"],
        b64(&kp),
        "the octets Bob published"
    );

    let out = key_material(&backend, BOB)
        .await
        .expect("KeyMaterial/fetch");
    assert_eq!(out["userStatus"], "noCompatibleMaterial");
    assert_eq!(out["clients"][0]["clientStatus"], "keyMaterialExhausted");
    assert_eq!(out["clients"][0]["clientUri"], clients[0]["clientUri"]);
    assert!(out["clients"][0].get("keyPackage").is_none());
    assert!(backend.hub_requests().is_empty());
}

/// L231: as hub, the server "asks the target provider otherwise", and "If
/// the provider it asks cannot be reached, the server returns
/// `serverUnavailable`".
#[tokio::test]
async fn key_material_fetch_as_hub_asks_the_provider_of_a_remote_target() {
    let hub = Hub::new();
    let (backend, _) = hosted_room_with_bob(&hub).await;
    let zed = "mimi://b.example/u/zed";
    assert_eq!(
        key_material(&backend, zed).await,
        Err("serverUnavailable".to_owned())
    );
    let requests = backend.hub_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].endpoint,
        MimiEndpoint::KeyMaterial {
            target_user: zed.to_owned()
        }
    );
    assert_eq!(requests[0].body, key_material_request(zed));
}
