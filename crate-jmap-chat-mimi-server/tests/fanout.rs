//! FanoutMessage storage against draft-atwood-jmap-chat-mimi-00
//! §application/mimi-fanout and §Chat.roomId (jmap-chat-spec 6bcd9d8,
//! L249-260 and L433-444). Each test quotes the line it checks.
//!
//! Wire-format oracles are published vectors from
//! github.com/mlswg/mls-implementations at 2034fe7 (entry 0 of
//! `test-vectors/messages.json` and `test-vectors/message-protection.json`).
//! The expected `senderMsgId`, `sentAt` and KeyPackageRef values were
//! computed once by an independent Python script (hashlib SHA-256, a
//! hand-written Crockford Base32 encoder per the ULID spec, a hand-written
//! TLS/varint Welcome parser) and are hardcoded here.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_chat_mimi_server::memory::MemoryBackend;
use jmap_chat_mimi_server::{
    Fanout, FanoutRecipients, KeyMaterialRequirements, MimiBackend, handle_key_package_set,
    notify_from_hub, parse_fanout_messages,
};
use jmap_chat_mimi_types::{Base64Url, MimiCapability};
use jmap_types::Id;
use openmls::prelude::{
    BasicCredential, Capabilities, Ciphersuite, CredentialType, CredentialWithKey, ExtensionType,
    KeyPackage, KeyPackageIn, MlsGroup, MlsGroupCreateConfig, OpenMlsProvider as _,
    ProtocolVersion, RequiredCapabilitiesExtension,
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{OpenMlsRustCrypto, RustCrypto};
use serde_json::{Value, json};
use tls_codec::{Deserialize as _, Serialize as _};

const SUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
const HUB: &str = "https://hub.example";
const ROOM: &str = "mimi://hub.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";

/// `private_message` of messages.json entry 0: a PrivateMessage whose
/// cleartext `content_type` is proposal (2).
const PRIVATE_PROPOSAL: &str = "\
    000100021057f89bad9b38b906d15100f720422e90000000000000000002036161641c2faa013da0b943a55ef506081f\
    b419a69b3210d57c9eaaf3279a97fa419f1c5d62527b15ed33031cae6db5146ada19ce847ad0a381c9b13d36a78a34aa\
    0c7bd98c4d4a17e40a47cda275121a7ef5b0e6d16aad03937f4b0bc0a4f62527108c9995005e67c618e7d46bb82e78c5\
    ac528cd4f89dbba5f2565fac926f49a94c48e99ac0ff87cbb7de8289e37c5d77acafbe4ba140d6184fecd1a93ea9ac54\
    119b7ed513d7f5c65541d8d9e8bea13278a4ee97edafe026961063333d69269e636e14873b40f85f39c12ff44db701a6\
    51dd873d2361eaccf5ef231eab42270b0af14c9f1adf5b03db2cb20cac503ca030512df27c4acd828f9be579ee54fdd8\
    40b6953d73a149c347f116a0abd0dcb5bcd3e118f52e18cace30f5e3c233cf16cc4edab69ee98f739b806f48a158e508\
    a17a0d9275d926e9bcc1e88182d8ba14e1997e982b989422389485b47152a4a26313d90f7af326f7bb2d4b8f51d0190e\
    1e33411337df1758ce95e499c8908ccc5f888024a62449ffbb7a7052ba4a31328655ddd92630b271069bbc2d97a4959c\
    a4227f12ec95bad5c732ced7370a594eec366eabd6f5becdafbf1cd72e8883138a917544bce9d93c2c09cc4e591ed94c";
/// `application_priv` of message-protection.json entry 0: a PrivateMessage
/// whose `content_type` is application (1).
const APPLICATION: &str = "\
    000100022042e4c3a73738d838cb4f9dc550cb81406206943f9e6870ee150f2000ae8aa780000000000012121201001c\
    f18ad2e0ad4462390d72714d68b2a845ba336c3e8d398d21334f8d0f407dab47ce77c5fff3107b2f4e4bf99936c0ec68\
    0364333403663b7a955708a73d91e921a4fb3ffd50292a51ec31d4ec684231eb6f765cfbd5cca5b78fbe42bbf1b04dd1\
    682fc87678b3a4f1599770839b171caff15e5e264957f3d16c31ba3ca16416f4665115244291536ddb438528cdb8502d\
    e654be9ca093915d36b1ab";
/// `public_message_commit` of messages.json entry 0: a PublicMessage whose
/// `content_type` is commit (3).
const PUBLIC_COMMIT: &str = "\
    000100011057f89bad9b38b906d15100f720422e90000000000000000001000000000361616403220220667a257d4ee5\
    21993fefc37ec3b43c8dd5572503134330219943cfcfc1a5a95201207098e821eef3e503ce5f2a54bfce94cf78b1aac5\
    90be0fe685e8dcbc7696d4452038a5e7481e20afe7e500cb3065a67ebe9c2c3ecd537d4479309159fe2215c4b5000105\
    416c696365020001020001000002000103202e6efd75b8b34836db7d525c7eeb65dcc11a918f21b5a9724ba63de99ef3\
    b1820040404833e2b7ce5740d84e06dfb6e9bb8c2fa3cec86baf43b0ed6459e9e4910d6fcd1dda761fb4e43d2a3c70fc\
    f045f95ee86dc7bad9078501c774c0f3cf275d170f22209ee6d8a7d342c383b16379beb6b4f7d3ce871973b1a3cee77f\
    190fe11fbc394b0040402d8a2583d18875921992fc08a922ac9eb8fafbb5d7dc7550011d6885fd083f2bdb4fe08dc3b2\
    8489dce11aa0500b176cce5c43970a4a01e1163fb4a5362c390220fd07c4baa82036fe87fe89a47024628e3a1cf1a7f5\
    c76b4782f9ed5a353cc99b206c278f9cb0f2bfc369623ed9bb6ed25eccd17037f9a9d4939a2e8c2960194259";
/// `mls_welcome` of messages.json entry 0, naming one KeyPackageRef.
const WELCOME: &str = "\
    000100030001409820b476143a05c9998ec979b6238fee883a9f465ed4b6ea2c694b2258e51dba7d5f20e1410ed962d6\
    91c2df6625d4ce889f322ae9ebdeff7a20cca524968e78d41c0140544245da924c89675bbc415e713ba2daf036bdab20\
    0d1dad416de001089cbf99918fb57f26658f117df2e8925aeb60a009636d6f64fab5b66debf02aeefbd0a7e1927196f9\
    bfd2796a1706586b7d0bc39d11d47a0b4102117b239baf2f747e56ac7f8b2868966ef5507fff8dcf31aaca212734fd90\
    3b73f3b4eac307ba16d00afe4e8714ddb9af0e313287b8786df09ed99447bcedc8b8fdaa5fff19339f7a176801c1224a\
    0e0c1f701dcc7fe9183a632b88b5f91f2c5780be8f9c2a40ed9157dd57ebb729733cbdece565f6e00bb59406b531775c\
    66d6c52491de29dd1b64e293046d06c68477e0ecd1b22d704d1cdc4615b03f98bde98535ac9f0eee6b7c7a0ccb8ac391\
    57aa846be24dcffcb05a5f048b346f0479f83bcf3bfa23c6a3bb3b7652f565d52295598ccd4714fead52ca595a6d613b\
    907c322079325cd3af24eda3544457aa76d2930c43c1e40e22d290a57f8f3a9dd6121a7b";
/// `mls_key_package` of messages.json entry 0 (wire format 5).
const KEY_PACKAGE: &str = "\
    000100050001000120c749ea08b3d33784ecf799c150ed8a43580e1cc12a1a893822547ba3c021ac76201e44ee2a2754\
    0d5c65a3f1e07bcb2ba558959ad22168d4de7f180201a9b666222038a5e7481e20afe7e500cb3065a67ebe9c2c3ecd53\
    7d4479309159fe2215c4b5000105416c696365020001060001000200030200010c000100020003000400050007020001\
    01000000006408842400000000647750340040404cdfef1fdc8d04758353f3f07c2fe7282fa0215ddc72441a33124ef5\
    106df858f86f57aefe08f255450df1243b630b8dd3912cbf2d94af7612fa4a07420cb70d004040136da188e3b773e12e\
    76c9be654f7b51fc94bb8f7d53059e7ce2421d9226e8d1637209d3d65ed1ab0a198e1e63e2052dfdae85609f7d06951c\
    f62ef66a12b300";

/// 2025-10-18T17:02:11.482Z in milliseconds since the Unix epoch.
const T: u64 = 1_760_806_931_482;

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A FanoutMessage (draft-ietf-mimi-protocol-06 §5.5): `uint64 timestamp`,
/// the MLSMessage, then `tail`.
fn fanout_message(timestamp: u64, message: &[u8], tail: &[u8]) -> Vec<u8> {
    [&timestamp.to_be_bytes()[..], message, tail].concat()
}

/// The five vector FanoutMessages: a proposal with an empty
/// `moreProposals` (0x00), an application message without a frank (0x00)
/// and with one (0x01, `server_frank` 0x00..0x1f, cipher suite 0x0001, an
/// empty signature), a commit with an empty `externalProposals`, and a
/// Welcome with RatchetTreeOption `outOfBand` (3).
fn vector_fanouts() -> Vec<Vec<u8>> {
    let frank = [&[1u8][..], &(0u8..32).collect::<Vec<_>>(), &[0, 1, 0]].concat();
    vec![
        fanout_message(T, &hex(PRIVATE_PROPOSAL), &[0]),
        fanout_message(T, &hex(APPLICATION), &[0]),
        fanout_message(T, &hex(APPLICATION), &frank),
        fanout_message(T + 1, &hex(PUBLIC_COMMIT), &[0]),
        fanout_message(T + 2, &hex(WELCOME), &[3]),
    ]
}

/// draft-ietf-mimi-protocol-06 §5.5: "The hub can include multiple
/// concatenated FanoutMessage objects". mimi L441: "`senderMsgId` is the
/// ULID whose 48-bit time component is the FanoutMessage `timestamp` and
/// whose 80-bit random component is the first 10 octets of the SHA-256
/// hash of the FanoutMessage octets". L443: "`sentAt` is the FanoutMessage
/// `timestamp`". L435: "has as `body` one FanoutMessage".
#[test]
fn splits_a_notify_body_and_derives_sender_msg_id_and_sent_at() {
    let parts = vector_fanouts();
    let fanouts = parse_fanout_messages(&parts.concat()).expect("parse");
    let expected = [
        (
            "01K7W4B30TQ3AP4CDSPB1H7R91",
            "2025-10-18T17:02:11.482Z",
            PRIVATE_PROPOSAL,
        ),
        (
            "01K7W4B30T0R9HBT0F3DVVNFFC",
            "2025-10-18T17:02:11.482Z",
            APPLICATION,
        ),
        (
            "01K7W4B30T6JK3C60E5BTF5MAS",
            "2025-10-18T17:02:11.482Z",
            APPLICATION,
        ),
        (
            "01K7W4B30V5CHGYF39MDD6YZSC",
            "2025-10-18T17:02:11.483Z",
            PUBLIC_COMMIT,
        ),
        (
            "01K7W4B30WX95547Q9KQTEGDJC",
            "2025-10-18T17:02:11.484Z",
            WELCOME,
        ),
    ];
    assert_eq!(fanouts.len(), expected.len());
    for ((fanout, part), (ulid, sent_at, message)) in fanouts.iter().zip(&parts).zip(expected) {
        assert_eq!(fanout.sender_msg_id, ulid);
        assert_eq!(fanout.sent_at, sent_at);
        assert_eq!(fanout.body.as_str(), b64(part));
        assert_eq!(fanout.message, hex(message));
    }
}

/// mimi L443: "`sentAt` is the FanoutMessage `timestamp`". RFC 8620 §1.4:
/// "time-secfrac MUST always be omitted if zero". `T` rounded down to the
/// whole second is 2025-10-18T17:02:11Z.
#[test]
fn a_whole_second_timestamp_has_no_fraction_in_sent_at() {
    let part = fanout_message(T - 482, &hex(APPLICATION), &[0]);
    let fanouts = parse_fanout_messages(&part).expect("parse");
    assert_eq!(fanouts.len(), 1);
    assert_eq!(fanouts[0].sent_at, "2025-10-18T17:02:11Z");
}

/// draft-ietf-mimi-protocol-06 §5.5: "An MLS Welcome message is sent to the
/// providers and local users associated with the KeyPackageRef values in
/// the secrets array of the Welcome." mimi L437: "A FanoutMessage carrying
/// a Welcome is stored only in the account owning a KeyPackageRef named in
/// the Welcome".
#[test]
fn a_welcome_names_the_key_package_refs_of_its_secrets() {
    let fanouts = parse_fanout_messages(&vector_fanouts().concat()).expect("parse");
    let welcome_ref: Base64Url = "ILR2FDoFyZmOyXm2I4_uiDqfRl7UtuosaUsiWOUdun1f"
        .parse()
        .expect("base64url");
    let recipients: Vec<_> = fanouts.iter().map(|f| f.recipients.clone()).collect();
    assert_eq!(
        recipients,
        [
            FanoutRecipients::Room,
            FanoutRecipients::Room,
            FanoutRecipients::Room,
            FanoutRecipients::Room,
            FanoutRecipients::Welcome(vec![welcome_ref]),
        ]
    );
}

/// draft-mahy-mls-ratchet-tree-options-04 §3: `full(1)` carries
/// `ratchet_tree<V>`, `httpsUri(2)` an opaque URL, `outOfBand(3)` and
/// `distributionService(4)` nothing; 0 is reserved.
#[test]
fn every_ratchet_tree_option_after_a_welcome_parses_and_reserved_does_not() {
    let welcome = hex(WELCOME);
    let url = b"https://hub.example/tree";
    let https = [&[2u8, url.len() as u8][..], url].concat();
    for tail in [&[1u8, 0][..], &https, &[3], &[4]] {
        let fanouts = parse_fanout_messages(&fanout_message(T, &welcome, tail)).expect("parse");
        assert_eq!(fanouts.len(), 1);
    }
    assert!(parse_fanout_messages(&fanout_message(T, &welcome, &[0])).is_err());
}

/// RFC 9420 §2.1.2: a `<V>` vector's varint length prefix counts the octets
/// that follow it. A hub body whose `moreProposals`, `externalProposals`,
/// frank signature or `ratchet_tree<V>` prefix claims more octets than
/// remain, or that ends inside the MLSMessage, is malformed and is
/// rejected (not a panic).
#[test]
fn a_vector_longer_than_the_body_is_rejected() {
    let frank = [&[1u8][..], &(0u8..32).collect::<Vec<_>>(), &[0, 1, 5, 0]].concat();
    let welcome = hex(WELCOME);
    let commit = hex(PUBLIC_COMMIT);
    let bodies = [
        fanout_message(T, &hex(PRIVATE_PROPOSAL), &[5, 1, 2]),
        fanout_message(T, &commit, &[0x40, 0x10, 1]),
        fanout_message(T, &hex(APPLICATION), &frank),
        fanout_message(T, &welcome, &[1, 4, 0]),
        fanout_message(T, &welcome, &[2, 9]),
        fanout_message(T, &commit[..commit.len() / 2], &[]),
    ];
    for body in &bodies {
        assert!(
            parse_fanout_messages(body).is_err(),
            "{} octets",
            body.len()
        );
    }
}

/// draft-ietf-mimi-protocol-06 §5.5: a FanoutMessage carries a "PrivateMessage
/// containing an application message, a SemiPrivateMessage or PublicMessage
/// containing a proposal or commit, or a Welcome message". ULID spec §Overflow
/// Errors: timestamps past 2 ^ 48 - 1 "should be rejected".
#[test]
fn rejects_what_is_not_a_sequence_of_fanout_messages() {
    let commit = fanout_message(T, &hex(PUBLIC_COMMIT), &[0]);
    let trailing = [&commit[..], &[0]].concat();
    let truncated = &commit[..commit.len() - 1];
    let key_package = fanout_message(T, &hex(KEY_PACKAGE), &[]);
    let late = fanout_message(1 << 48, &hex(PUBLIC_COMMIT), &[0]);
    for body in [&[][..], &trailing, truncated, &key_package, &late] {
        assert!(
            parse_fanout_messages(body).is_err(),
            "{} octets",
            body.len()
        );
    }
}

fn capability(user_uri: &str) -> MimiCapability {
    serde_json::from_value(json!({
        "userUri": user_uri,
        "roomIdPrefix": "mimi://a.example/r/",
        "hubSender": "AAEC",
        "cipherSuites": [1],
        "maxKeyPackages": 10,
    }))
    .expect("capability")
}

/// Local accounts b1 (Bob), c1 (Carol) and d1 (Dave).
fn backend() -> MemoryBackend<RustCrypto> {
    let backend = MemoryBackend::new(RustCrypto::default());
    for (id, user) in [("b1", "bob"), ("c1", "carol"), ("d1", "dave")] {
        backend.register_account(
            &Id::from(id),
            capability(&format!("mimi://a.example/u/{user}")),
        );
    }
    backend
}

/// Publish a fresh KeyPackage for `account` and hand it out through
/// `claim_key_packages`, as KeyMaterial/fetch would.
async fn claimed_key_package(backend: &MemoryBackend<RustCrypto>, account: &str) -> KeyPackage {
    let user = backend
        .mimi_capability(&(), &Id::from(account))
        .await
        .expect("capability")
        .user_uri;
    let provider = OpenMlsRustCrypto::default();
    let signer = SignatureKeyPair::new(SUITE.signature_algorithm()).expect("signer");
    let cwk = CredentialWithKey {
        credential: BasicCredential::new(user.into_bytes()).into(),
        signature_key: signer.public().into(),
    };
    let kp = KeyPackage::builder()
        .leaf_node_capabilities(Capabilities::new(
            None,
            None,
            Some(&[ExtensionType::AppDataDictionary]),
            None,
            Some(&[CredentialType::Basic]),
        ))
        .build(SUITE, &provider, &signer, cwk)
        .expect("build KeyPackage")
        .key_package()
        .tls_serialize_detached()
        .expect("serialize KeyPackage");
    let out = handle_key_package_set(
        backend,
        &(),
        json!({ "accountId": account, "create": { "k": { "keyPackage": b64(&kp) } } }),
    )
    .await
    .expect("KeyPackage/set")
    .primary;
    assert!(out["created"]["k"].is_object(), "{out}");
    let requirements =
        KeyMaterialRequirements::new(vec![1], RequiredCapabilitiesExtension::new(&[], &[], &[]));
    let handed = backend
        .claim_key_packages(&(), &Id::from(account), &requirements)
        .await
        .expect("claim");
    let handed = handed
        .iter()
        .find_map(|c| c.key_package.as_ref())
        .expect("handed out");
    let bytes = URL_SAFE_NO_PAD.decode(handed.as_str()).expect("base64url");
    KeyPackageIn::tls_deserialize_exact(bytes)
        .expect("KeyPackage")
        .validate(provider.crypto(), ProtocolVersion::Mls10)
        .expect("valid KeyPackage")
}

/// A remote member's MLS group: its creator, signer and provider.
struct Group {
    group: MlsGroup,
    signer: SignatureKeyPair,
    provider: OpenMlsRustCrypto,
}

impl Group {
    fn new() -> Self {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(SUITE.signature_algorithm()).expect("signer");
        let cwk = CredentialWithKey {
            credential: BasicCredential::new(b"mimi://hub.example/u/alice".to_vec()).into(),
            signature_key: signer.public().into(),
        };
        let config = MlsGroupCreateConfig::builder().ciphersuite(SUITE).build();
        let group = MlsGroup::new(&provider, &signer, &config, cwk).expect("group");
        Self {
            group,
            signer,
            provider,
        }
    }

    /// Add `key_packages`; returns the Welcome MLSMessage octets.
    fn add(&mut self, key_packages: &[KeyPackage]) -> Vec<u8> {
        let (_commit, welcome, _) = self
            .group
            .add_members(&self.provider, &self.signer, key_packages)
            .expect("add");
        self.group
            .merge_pending_commit(&self.provider)
            .expect("merge");
        welcome.tls_serialize_detached().expect("welcome")
    }

    /// An application PrivateMessage carrying `text`.
    fn send(&mut self, text: &[u8]) -> Vec<u8> {
        self.group
            .create_message(&self.provider, &self.signer, text)
            .expect("message")
            .tls_serialize_detached()
            .expect("serialize")
    }
}

/// A Welcome FanoutMessage at `timestamp` (RatchetTreeOption `outOfBand`).
fn welcome_fanout(timestamp: u64, welcome: &[u8]) -> Vec<u8> {
    fanout_message(timestamp, welcome, &[3])
}

/// An application FanoutMessage at `timestamp` without a frank.
fn application_fanout(timestamp: u64, message: &[u8]) -> Vec<u8> {
    fanout_message(timestamp, message, &[0])
}

/// What a `/notify` handler does with a body from HUB for ROOM.
async fn notify(backend: &MemoryBackend<RustCrypto>, body: &[u8]) -> Vec<Fanout> {
    let fanouts = parse_fanout_messages(body).expect("parse");
    for fanout in &fanouts {
        backend
            .store_fanout(ROOM, HUB, fanout)
            .await
            .expect("store");
    }
    fanouts
}

fn messages(backend: &MemoryBackend<RustCrypto>, account: &str) -> Vec<Value> {
    backend
        .room_chat_id(&Id::from(account), ROOM)
        .map(|id| backend.room_messages(&id))
        .unwrap_or_default()
}

/// L437: "A FanoutMessage carrying a Welcome is stored only in the account
/// owning a KeyPackageRef named in the Welcome; if that account has no Chat
/// bound to the room, the server ... creates a Chat bound to the room."
/// L256: "A Chat with a `roomId` has kind `"group"`"; the server sets
/// `name` "to the empty string" when it does not know `room_name`. L258:
/// "`members` of a Chat with a `roomId` is the empty list, whether
/// `Room/create` or a Welcome created it."
#[tokio::test]
async fn a_welcome_creates_a_bound_chat_only_for_the_account_it_names() {
    let backend = backend();
    let bob = claimed_key_package(&backend, "b1").await;
    let mut group = Group::new();
    let welcome = group.add(&[bob]);
    let fanouts = notify(&backend, &welcome_fanout(T, &welcome)).await;

    let chat_id = backend
        .room_chat_id(&Id::from("b1"), ROOM)
        .expect("Bob's Chat");
    assert_eq!(
        backend.room_chat(&chat_id),
        Some(json!({ "id": chat_id, "kind": "group", "name": "", "roomId": ROOM, "members": [] }))
    );
    assert_eq!(messages(&backend, "b1").len(), 1);
    assert_eq!(
        messages(&backend, "b1")[0]["body"],
        fanouts[0].body.as_str()
    );
    assert_eq!(backend.room_chat_id(&Id::from("c1"), ROOM), None);
    assert_eq!(backend.room_chat_id(&Id::from("d1"), ROOM), None);
}

/// L437: "The server stores each FanoutMessage it receives ... for a room
/// as one Message, in the order received, in every Chat of a local account
/// bound to that room". L435: "has as `body` one FanoutMessage". L441-L443:
/// `senderMsgId` and `sentAt`. L444: "`senderId` is the id of the
/// ChatContact that represents the room's hub, whose `identity` and
/// `login` are the hub's provider URI ...; the server creates that
/// ChatContact ... if it does not exist."
#[tokio::test]
async fn a_room_fanout_is_stored_in_every_bound_chat_from_the_hub_contact() {
    let backend = backend();
    let bob = claimed_key_package(&backend, "b1").await;
    let carol = claimed_key_package(&backend, "c1").await;
    let mut group = Group::new();
    notify(&backend, &welcome_fanout(T, &group.add(&[bob, carol]))).await;
    let first = group.send(b"first");
    let second = group.send(b"second");
    let body = [
        application_fanout(T + 1, &first),
        application_fanout(T + 2, &second),
    ]
    .concat();
    let fanouts = notify(&backend, &body).await;

    for account in ["b1", "c1"] {
        let contact = backend
            .hub_contact(&Id::from(account), HUB)
            .expect("hub ChatContact");
        assert_eq!(contact["identity"], HUB);
        assert_eq!(contact["login"], HUB);
        let stored = messages(&backend, account);
        assert_eq!(stored.len(), 3, "{account}: Welcome, first, second");
        for (message, fanout) in stored[1..].iter().zip(&fanouts) {
            assert_eq!(message["bodyType"], "application/mimi-fanout");
            assert_eq!(message["body"], fanout.body.as_str());
            assert_eq!(message["senderMsgId"], fanout.sender_msg_id.as_str());
            assert_eq!(message["sentAt"], fanout.sent_at.as_ref());
            assert_eq!(message["senderId"], contact["id"]);
        }
        assert_eq!(stored[0]["senderId"], contact["id"]);
    }
    assert!(messages(&backend, "d1").is_empty());
}

/// L441: "The server MUST NOT store a FanoutMessage whose `senderMsgId` it
/// has already stored in the Chat, so a repeated `/notify` yields one
/// Message." L442: "The server MUST NOT store a FanoutMessage whose
/// `message` is byte-identical to the `message` of a FanoutMessage already
/// stored in the Chat."
#[tokio::test]
async fn a_repeated_notify_and_a_byte_identical_message_are_stored_once() {
    let backend = backend();
    let bob = claimed_key_package(&backend, "b1").await;
    let mut group = Group::new();
    notify(&backend, &welcome_fanout(T, &group.add(&[bob]))).await;
    let message = group.send(b"hello");
    let body = application_fanout(T + 1, &message);
    notify(&backend, &body).await;
    notify(&backend, &body).await;
    notify(&backend, &application_fanout(T + 5, &message)).await;
    assert_eq!(messages(&backend, "b1").len(), 2, "Welcome and one message");
}

/// L260: "An unbound Chat keeps its `roomId` and its Messages. The server
/// stores no further FanoutMessage in it ... A later Welcome for the
/// account binds the Chat again". L437: "the server first binds the
/// account's unbound Chat for the room again".
#[tokio::test]
async fn an_unbound_chat_stores_nothing_until_a_welcome_binds_it_again() {
    let backend = backend();
    let bob = claimed_key_package(&backend, "b1").await;
    let carol = claimed_key_package(&backend, "c1").await;
    let mut group = Group::new();
    notify(&backend, &welcome_fanout(T, &group.add(&[bob, carol]))).await;
    let chat_id = backend
        .room_chat_id(&Id::from("b1"), ROOM)
        .expect("Bob's Chat");

    assert!(
        backend
            .unbind_room(&(), &Id::from("b1"), ROOM)
            .await
            .expect("unbind")
    );
    assert!(
        !backend
            .unbind_room(&(), &Id::from("d1"), ROOM)
            .await
            .expect("unbind")
    );
    notify(
        &backend,
        &application_fanout(T + 1, &group.send(b"while unbound")),
    )
    .await;
    assert_eq!(messages(&backend, "b1").len(), 1, "only the first Welcome");
    assert_eq!(messages(&backend, "c1").len(), 2);
    assert_eq!(backend.room_chat(&chat_id).expect("kept")["roomId"], ROOM);

    let bob_again = claimed_key_package(&backend, "b1").await;
    let mut rejoin = Group::new();
    notify(&backend, &welcome_fanout(T + 2, &rejoin.add(&[bob_again]))).await;
    assert_eq!(backend.room_chat_id(&Id::from("b1"), ROOM), Some(chat_id));
    notify(
        &backend,
        &application_fanout(T + 3, &rejoin.send(b"bound again")),
    )
    .await;
    assert_eq!(messages(&backend, "b1").len(), 3);
}

/// L441: "The server MUST NOT store a FanoutMessage whose `senderMsgId` it
/// has already stored in the Chat, so a repeated `/notify` yields one
/// Message." L260: after `Room/unbind`, "The server stores no further
/// FanoutMessage in it"; only "A later Welcome for the account binds the
/// Chat again", and a hub retrying the same `/notify` is not a later one.
#[tokio::test]
async fn a_repeated_welcome_does_not_rebind_an_unbound_chat() {
    let backend = backend();
    let bob = claimed_key_package(&backend, "b1").await;
    let mut group = Group::new();
    let welcome = welcome_fanout(T, &group.add(&[bob]));
    notify(&backend, &welcome).await;
    assert!(
        backend
            .unbind_room(&(), &Id::from("b1"), ROOM)
            .await
            .expect("unbind")
    );

    notify(&backend, &welcome).await;
    assert!(
        !backend
            .room_bound(&(), &Id::from("b1"), ROOM)
            .await
            .expect("room_bound")
    );
    notify(
        &backend,
        &application_fanout(T + 1, &group.send(b"after leaving")),
    )
    .await;
    assert_eq!(messages(&backend, "b1").len(), 1, "only the first Welcome");
}

/// L444: "The `blocked` field of that ChatContact applies as JMAP-CHAT
/// defines it, so blocking the hub drops every FanoutMessage of the rooms
/// it hosts."
#[tokio::test]
async fn blocking_the_hub_drops_its_fanout_messages() {
    let backend = backend();
    let bob = claimed_key_package(&backend, "b1").await;
    let carol = claimed_key_package(&backend, "c1").await;
    let mut group = Group::new();
    notify(&backend, &welcome_fanout(T, &group.add(&[bob]))).await;
    backend.block_contact(&Id::from("b1"), HUB);
    backend.block_contact(&Id::from("c1"), HUB);
    notify(&backend, &welcome_fanout(T + 1, &group.add(&[carol]))).await;
    notify(
        &backend,
        &application_fanout(T + 2, &group.send(b"dropped")),
    )
    .await;
    assert_eq!(
        messages(&backend, "b1").len(),
        1,
        "only the Welcome before the block"
    );
    assert_eq!(backend.room_chat_id(&Id::from("c1"), ROOM), None);
}

/// jmap-chat-spec 29e9a41 L439: "A room's hub is the provider named by the
/// authority component of its room ID. The server MUST reject a `/notify`
/// request for a room, and MUST NOT store its FanoutMessages, unless the
/// MIMI transport authenticated the sender as that provider."
#[test]
fn notify_accepted_only_from_the_room_id_authority() {
    let room = "mimi://hub.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";
    for hub in [
        "mimi://hub.example",
        "https://hub.example",
        "https://hub.example/",
        "mimi://HUB.Example",
    ] {
        assert!(notify_from_hub(room, hub), "{hub}");
    }
    for other in [
        "mimi://other.example",
        "https://other.example",
        "https://hub.example.other.example",
        "https://evil@hub.example",
        "mimi://hub.example:8443",
        "hub.example",
        "",
    ] {
        assert!(!notify_from_hub(room, other), "{other}");
    }
    for bad_room in [
        "mimi:///r/k5qwmzlqmfsgk3tbnvsxi2lf4a",
        "https://hub.example/r/x",
        "",
    ] {
        assert!(
            !notify_from_hub(bad_room, "mimi://hub.example"),
            "{bad_room}"
        );
    }
}
