//! KeyMaterial/fetch, Room/update, Room/send, Room/unbind and
//! GroupInfo/fetch against draft-atwood-jmap-chat-mimi-00 (jmap-chat-spec
//! 33702a5, L216-421) for a room whose hub is another provider. Each test
//! quotes the line it checks.
//!
//! MLS messages are published vectors from github.com/mlswg/mls-implementations
//! at 2034fe7 (entry 0 of `test-vectors/messages.json` and
//! `test-vectors/message-protection.json`). The MIMI requests and responses
//! are encoded here by hand from the TLS presentation language of
//! draft-ietf-mimi-protocol-06 §5.2-§5.6 and
//! draft-mahy-mls-ratchet-tree-options-04, independently of the crate's
//! decoders. The Chat is bound by a real Welcome, as in `tests/fanout.rs`.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_chat_mimi_server::memory::MemoryBackend;
use jmap_chat_mimi_server::{
    HubReply, HubUnavailable, KeyMaterialRequirements, MimiBackend, MimiEndpoint,
    handle_group_info_fetch, handle_key_material_fetch, handle_key_package_set, handle_room_send,
    handle_room_unbind, handle_room_update, parse_fanout_messages,
};
use jmap_chat_mimi_types::MimiCapability;
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
const BOB: &str = "mimi://a.example/u/bob";
const CAROL: &str = "mimi://a.example/u/carol";

/// `public_message_proposal` of messages.json entry 0.
const PUBLIC_PROPOSAL: &str = "\
    000100011057f89bad9b38b906d15100f720422e900000000000000000010000000003616164020001000100012014da\
    aedfec3b04ca1b49695697b85e1e278c2b5851d031afe35ff0611aaec9602024a768c73a100ccceb3e062790bca6f17d\
    95ae5567c0bfbdd9ba52bbd84e9a18203170de95c16a387572fe5bc68757c56b68537f336ece7262b192c8be238da980\
    000103426f62020001060001000200030200010c00010002000300040005000702000101000000006408842400000000\
    64775034004040f40844a8d907c710f5da6423d30b4a0161924242931cc213bb2aff6f953550887f16f12d72248690e1\
    cc15045d51b5796a1afc5f521e8f2cf9975ed38ffaa307004040b0ffdb7f714e1a040e43f0e493f08eac215269b52b32\
    1112edb66a4bd2bd2136c8bf55dd9adbf3fc2a27395647bc74760d5b5b36c8a19050ad4f7c377b0b1d0e404022d70b9e\
    b5620721c5962b1114270fc6d55fc3b082dff3498075bd5c581dd5834d71fd6aa32da7676b6aee108f0e4128f13da7ac\
    4ac2a1806e04ecbd6befba07206c278f9cb0f2bfc369623ed9bb6ed25eccd17037f9a9d4939a2e8c2960194259";
/// `public_message_commit` of messages.json entry 0.
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
/// `public_message_application` of messages.json entry 0.
const PUBLIC_APPLICATION: &str = "\
    000100011057f89bad9b38b906d15100f720422e90000000000000000101000000000361616401036d7367404069d176\
    1429c48259525c1a7bdda1e92837a8e5f124f37f008ed1f0d9927cc197a708569fd1daccdbe79b5b9fb2d3eeb0e2da0a\
    e7d3471cc8d85e7063dafda309206c278f9cb0f2bfc369623ed9bb6ed25eccd17037f9a9d4939a2e8c2960194259";
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
/// `mls_group_info` of messages.json entry 0, an MLSMessage: its first
/// four octets are `version` and `wire_format`, the rest the GroupInfo.
const GROUP_INFO_MESSAGE: &str = "\
    00010004000100011057f89bad9b38b906d15100f720422e900000000000000000204791deb51610ceeee4333b92eb02\
    dbaeff2f29809ba66bfa105233e478f0b707200000000000000000000000000000000000000000000000000000000000\
    0000000600030300000040d5000240ad40ab010120d64aa6e8d92e72e40f5ee1e4530c5f15e8b1088fdb27191807ea68\
    2dce8722232038a5e7481e20afe7e500cb3065a67ebe9c2c3ecd537d4479309159fe2215c4b5000105416c6963650200\
    01020001000002000101000000006408842400000000647750340040409126d4860e4813c5cfddc8943406bcf5a6052e\
    e28f48e41d1feebdd6f833cfc9cffcb3d25cb6306060f86273abce10b21d73c8b48f1868ca0d7311507899a700000421\
    2046f72a2aadd9061959785bde73e922df0de3bae3c6aefc29a1e2e7b0f4f4987d203466c4297e35375676e20ccc765b\
    1e46973e9468c1679da707be06be0971be8500000000404022f616969d2bc9bbf0aa710e211c4e695be3383a3485c318\
    57dcb7cadc40cb690bf12d9a3eb460134f3b414e8b7a0ebcdd37de2c1c52fb97bb8e376ade02de0b";
/// `mls_key_package` of messages.json entry 0, an MLSMessage: its first
/// four octets are `version` and `wire_format`, the rest the KeyPackage.
const KEY_PACKAGE_MESSAGE: &str = "\
    000100050001000120c749ea08b3d33784ecf799c150ed8a43580e1cc12a1a893822547ba3c021ac76201e44ee2a2754\
    0d5c65a3f1e07bcb2ba558959ad22168d4de7f180201a9b666222038a5e7481e20afe7e500cb3065a67ebe9c2c3ecd53\
    7d4479309159fe2215c4b5000105416c696365020001060001000200030200010c000100020003000400050007020001\
    01000000006408842400000000647750340040404cdfef1fdc8d04758353f3f07c2fe7282fa0215ddc72441a33124ef5\
    106df858f86f57aefe08f255450df1243b630b8dd3912cbf2d94af7612fa4a07420cb70d004040136da188e3b773e12e\
    76c9be654f7b51fc94bb8f7d53059e7ce2421d9226e8d1637209d3d65ed1ab0a198e1e63e2052dfdae85609f7d06951c\
    f62ef66a12b300";

/// 2025-10-18T17:02:11.482Z in milliseconds since the Unix epoch.
const T: u64 = 1_760_806_931_482;
const T_UTC: &str = "2025-10-18T17:02:11.482Z";

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

/// `opaque <name><V>` with the RFC 9420 §2.1.2 prefix, for fewer than 2^14
/// octets.
fn opaque(bytes: &[u8]) -> Vec<u8> {
    let mut out = if bytes.len() < 64 {
        vec![bytes.len() as u8]
    } else {
        vec![0x40 | (bytes.len() >> 8) as u8, bytes.len() as u8]
    };
    out.extend_from_slice(bytes);
    out
}

/// A Basic `Credential` (RFC 9420 §5.3: `uint16 credential_type` 0x0001,
/// `opaque identity<V>`).
fn basic_credential(identity: &str) -> Vec<u8> {
    [&[0x00, 0x01][..], &opaque(identity.as_bytes())].concat()
}

/// A KeyMaterialRequest (draft-ietf-mimi-protocol-06 §5.2): `mls10(1)`,
/// the three `IdentifierUri`s, `acceptableCiphersuites` [0x0001], an empty
/// `RequiredCapabilities` (three empty vectors), a 32-octet
/// `SignaturePublicKey`, a Basic `Credential`, and a 64-octet signature.
/// The server does not verify the signature (mimi L231 lists no such
/// check), so it is zeros.
fn key_material_request(requesting: &str, target: &str, room: &str) -> Vec<u8> {
    [
        &[1u8][..],
        &opaque(requesting.as_bytes()),
        &opaque(target.as_bytes()),
        &opaque(room.as_bytes()),
        &[2, 0x00, 0x01],
        &[0, 0, 0],
        &opaque(&[7; 32]),
        &basic_credential(requesting),
        &opaque(&[0; 64]),
    ]
    .concat()
}

/// A GroupInfoRequest (draft-ietf-mimi-protocol-06 §5.6): `mls10(1)`,
/// `cipher_suite` 0x0001, a 32-octet `SignaturePublicKey`, a Basic
/// `Credential`, a 32-octet `HPKEPublicKey`, an empty `joiningCode`, and a
/// 64-octet signature.
fn group_info_request() -> Vec<u8> {
    [
        &[1u8, 0x00, 0x01][..],
        &opaque(&[7; 32]),
        &basic_credential(BOB),
        &opaque(&[9; 32]),
        &[0],
        &opaque(&[0; 64]),
    ]
    .concat()
}

/// The Frank of draft-ietf-mimi-protocol-06 §5.4 as `optional Frank`
/// present: 0x01, `server_frank` 0x00..0x1f, cipher suite 0x0001, an
/// empty signature.
fn frank() -> Vec<u8> {
    [&[1u8][..], &(0u8..32).collect::<Vec<_>>(), &[0, 1, 0]].concat()
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

/// Publish a fresh KeyPackage for `account` and hand it out through
/// `claim_key_packages`, as the hub's key material request would.
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

/// Local accounts b1 (Bob), c1 (Carol) and d1 (Dave). A remote member of
/// the room hosted by HUB adds Bob and Carol, so b1 and c1 each have a
/// Chat bound to ROOM holding the Welcome; d1 has no Chat for it.
async fn backend() -> MemoryBackend<RustCrypto> {
    let backend = MemoryBackend::new(RustCrypto::default());
    for (id, user) in [
        ("b1", BOB),
        ("c1", CAROL),
        ("d1", "mimi://a.example/u/dave"),
    ] {
        backend.register_account(&Id::from(id), capability(user));
    }
    let bob = claimed_key_package(&backend, "b1").await;
    let carol = claimed_key_package(&backend, "c1").await;
    let provider = OpenMlsRustCrypto::default();
    let signer = SignatureKeyPair::new(SUITE.signature_algorithm()).expect("signer");
    let cwk = CredentialWithKey {
        credential: BasicCredential::new(b"mimi://hub.example/u/alice".to_vec()).into(),
        signature_key: signer.public().into(),
    };
    let config = MlsGroupCreateConfig::builder().ciphersuite(SUITE).build();
    let mut group = MlsGroup::new(&provider, &signer, &config, cwk).expect("group");
    let (_, welcome, _) = group
        .add_members(&provider, &signer, &[bob, carol])
        .expect("add");
    let welcome = welcome.tls_serialize_detached().expect("welcome");
    // FanoutMessage (draft-ietf-mimi-protocol-06 §5.5): timestamp, the
    // Welcome, RatchetTreeOption outOfBand(3).
    let body = [&(T - 1000).to_be_bytes()[..], &welcome, &[3]].concat();
    for fanout in parse_fanout_messages(&body).expect("parse") {
        backend
            .store_fanout(ROOM, HUB, &fanout)
            .await
            .expect("store");
    }
    assert_eq!(messages(&backend, "b1").len(), 1);
    assert_eq!(messages(&backend, "c1").len(), 1);
    backend
}

fn messages(backend: &MemoryBackend<RustCrypto>, account: &str) -> Vec<Value> {
    backend
        .room_chat_id(&Id::from(account), ROOM)
        .map(|id| backend.room_messages(&id))
        .unwrap_or_default()
}

fn reply(body: Vec<u8>) -> Result<HubReply, HubUnavailable> {
    Ok(HubReply::new(HUB, body))
}

async fn update(
    backend: &MemoryBackend<RustCrypto>,
    account: &str,
    request: &[u8],
) -> Result<Value, String> {
    handle_room_update(
        backend,
        &(),
        json!({ "accountId": account, "roomId": ROOM, "request": b64(request) }),
    )
    .await
    .map(|o| o.primary)
    .map_err(|e| e.error_type.to_string())
}

async fn send(
    backend: &MemoryBackend<RustCrypto>,
    account: &str,
    message: &[u8],
) -> Result<Value, String> {
    handle_room_send(
        backend,
        &(),
        json!({ "accountId": account, "roomId": ROOM, "message": b64(message) }),
    )
    .await
    .map(|o| o.primary)
    .map_err(|e| e.error_type.to_string())
}

async fn group_info(
    backend: &MemoryBackend<RustCrypto>,
    account: &str,
    request: &[u8],
) -> Result<Value, String> {
    handle_group_info_fetch(
        backend,
        &(),
        json!({ "accountId": account, "roomId": ROOM, "request": b64(request) }),
    )
    .await
    .map(|o| o.primary)
    .map_err(|e| e.error_type.to_string())
}

async fn key_material(
    backend: &MemoryBackend<RustCrypto>,
    account: &str,
    request: &[u8],
) -> Result<Value, String> {
    handle_key_material_fetch(
        backend,
        &(),
        json!({ "accountId": account, "roomId": ROOM, "request": b64(request) }),
    )
    .await
    .map(|o| o.primary)
    .map_err(|e| e.error_type.to_string())
}

async fn unbind(backend: &MemoryBackend<RustCrypto>, account: &str) -> Result<Value, String> {
    handle_room_unbind(
        backend,
        &(),
        json!({ "accountId": account, "roomId": ROOM }),
    )
    .await
    .map(|o| o.primary)
    .map_err(|e| e.error_type.to_string())
}

/// An UpdateRequest for a proposal: the HandshakeBundle's MLSMessage and an
/// empty `moreProposals<V>`.
fn proposal_update() -> Vec<u8> {
    [&hex(PUBLIC_PROPOSAL)[..], &[0]].concat()
}

/// An UpdateRequest for a commit: no Welcome (0x00), GroupInfoOption
/// `partial(2)` (PartialGroupInfo: `no_ratchet_tree(0)`, no extensions,
/// an empty signature), RatchetTreeOption `outOfBand(3)`.
fn commit_update() -> Vec<u8> {
    [&hex(PUBLIC_COMMIT)[..], &[0], &[2, 0, 0, 0], &[3]].concat()
}

/// UpdateRoomResponse `success(0)`, empty `errorDescription`,
/// `accepted_timestamp` T.
fn update_success() -> Vec<u8> {
    [&[0u8, 0][..], &T.to_be_bytes()].concat()
}

/// L339: "On success the server MUST store, in every Chat of a local
/// account bound to the room, a Message whose body is the FanoutMessage
/// with `timestamp` set to `accepted_timestamp`, `message` set to
/// `proposalOrCommit`, and `moreProposals` copied from the request".
/// L330: "`acceptedAt` ...: the hub's `accepted_timestamp`". L444: the
/// `senderId` is the hub's ChatContact "also for the Messages that
/// `Room/send` and `Room/update` store".
#[tokio::test]
async fn room_update_proposal_success_stores_the_fanout_message_in_every_bound_chat() {
    let backend = backend().await;
    backend.queue_hub_reply(reply(update_success()));
    let out = update(&backend, "b1", &proposal_update())
        .await
        .expect("Room/update");
    assert_eq!(
        out,
        json!({ "accountId": "b1", "responseCode": "success", "errorDescription": "", "acceptedAt": T_UTC })
    );
    let requests = backend.hub_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].endpoint, MimiEndpoint::Update);
    assert_eq!(requests[0].room_id, ROOM);
    assert_eq!(
        requests[0].body,
        proposal_update(),
        "the UpdateRequest, unchanged"
    );

    let fanout = [&T.to_be_bytes()[..], &hex(PUBLIC_PROPOSAL), &[0]].concat();
    for account in ["b1", "c1"] {
        let stored = messages(&backend, account);
        assert_eq!(stored.len(), 2, "{account}: Welcome and the proposal");
        assert_eq!(stored[1]["body"], b64(&fanout));
        assert_eq!(stored[1]["sentAt"], T_UTC);
        let contact = backend
            .hub_contact(&Id::from(account), HUB)
            .expect("hub contact");
        assert_eq!(stored[1]["senderId"], contact["id"]);
    }
}

/// L339: for a commit, the FanoutMessage carries "an empty
/// `externalProposals`". draft-ietf-mimi-protocol-06 §5.3: a commit's
/// HandshakeBundle carries `optional<Welcome> welcome`, `GroupInfoOption`
/// (`full(1)` a GroupInfo, `partial(2)` a PartialGroupInfo) and
/// `RatchetTreeOption`.
#[tokio::test]
async fn room_update_commit_stores_an_empty_external_proposals_and_parses_both_group_info_options()
{
    let backend = backend().await;
    backend.queue_hub_reply(reply(update_success()));
    update(&backend, "b1", &commit_update())
        .await
        .expect("Room/update");
    let fanout = [&T.to_be_bytes()[..], &hex(PUBLIC_COMMIT), &[0]].concat();
    assert_eq!(messages(&backend, "b1")[1]["body"], b64(&fanout));

    let full = [
        &hex(PUBLIC_COMMIT)[..],
        &[0],
        &[1],
        &hex(GROUP_INFO_MESSAGE)[4..],
        &[3],
    ]
    .concat();
    // `wrongEpoch(1)`, errorDescription "", currentEpoch 7.
    backend.queue_hub_reply(reply([&[1u8, 0][..], &7u64.to_be_bytes()].concat()));
    let out = update(&backend, "b1", &full).await.expect("Room/update");
    assert_eq!(out["responseCode"], "wrongEpoch");
}

/// L325-L337: `responseCode` "written as its name", `errorDescription`
/// "The hub's `errorDescription`", `currentEpoch` "When `responseCode` is
/// `"wrongEpoch"`", `invalidProposals` "When `responseCode` is
/// `"invalidProposal"`: the ProposalRefs". L112/L119: a ProposalRef
/// "begins with the vector's variable-length length prefix"; the one whose
/// hash is 0x00 through 0x1f is "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f".
/// Only success stores a Message (L339).
#[tokio::test]
async fn room_update_decodes_each_failure_response_and_stores_nothing() {
    let backend = backend().await;
    let proposal_ref = opaque(&(0u8..32).collect::<Vec<_>>());
    let cases = [
        (
            [&[1u8, 0][..], &7u64.to_be_bytes()].concat(),
            json!({ "accountId": "b1", "responseCode": "wrongEpoch", "errorDescription": "", "currentEpoch": 7 }),
        ),
        (
            [&[2u8][..], &opaque(b"policy")].concat(),
            json!({ "accountId": "b1", "responseCode": "notAllowed", "errorDescription": "policy" }),
        ),
        (
            [&[3u8, 0][..], &opaque(&proposal_ref)].concat(),
            json!({
                "accountId": "b1",
                "responseCode": "invalidProposal",
                "errorDescription": "",
                "invalidProposals": ["IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f"],
            }),
        ),
    ];
    for (body, expected) in cases {
        backend.queue_hub_reply(reply(body));
        assert_eq!(
            update(&backend, "b1", &proposal_update()).await,
            Ok(expected)
        );
    }
    assert_eq!(messages(&backend, "b1").len(), 1, "only the Welcome");
}

/// L317: "The server MUST reject the call with `invalidArguments` if
/// `request` does not parse as an UpdateRequest". draft-ietf-mimi-protocol-06
/// §5.3: `proposalOrCommit` is "A Proposal or Commit". A request that is
/// refused never reaches the hub.
#[tokio::test]
async fn room_update_rejects_what_is_not_an_update_request() {
    let backend = backend().await;
    let trailing = [&proposal_update()[..], &[0]].concat();
    let application = [&hex(PUBLIC_APPLICATION)[..], &[0]].concat();
    let private = [&hex(PRIVATE_PROPOSAL)[..], &[0]].concat();
    let bad_option = [&hex(PUBLIC_COMMIT)[..], &[0], &[3], &[3]].concat();
    let no_tail = hex(PUBLIC_PROPOSAL);
    // RatchetTreeOption full(1) whose ratchet_tree<V> says 5 octets but
    // carries 2.
    let truncated_tree = [&hex(PUBLIC_COMMIT)[..], &[0], &[2, 0, 0, 0], &[1, 5, 0, 0]].concat();
    for request in [
        &trailing,
        &application,
        &private,
        &bad_option,
        &no_tail,
        &truncated_tree,
    ] {
        assert_eq!(
            update(&backend, "b1", request).await,
            Err("invalidArguments".to_owned()),
            "{} octets",
            request.len()
        );
    }
    assert!(backend.hub_requests().is_empty());
}

/// L356: "Otherwise it submits the message to the hub as a
/// SubmitMessageRequest with `sendingUri` set to `userUri`"
/// (draft-ietf-mimi-protocol-06 §5.4: `mls10(1)`, `appMessage`,
/// `sendingUri`). L372: "When the hub accepts the message, the server MUST
/// store, in every Chat of a local account bound to the room, a Message
/// whose body is the FanoutMessage with `timestamp` set to
/// `accepted_timestamp`, `message` set to `message`, and `frank` copied
/// from the response when present". L366: `acceptedAt`.
#[tokio::test]
async fn room_send_submits_and_stores_the_message_with_its_frank() {
    let backend = backend().await;
    // SubmitMessageResponse: mls10, accepted(0), accepted_timestamp T,
    // a present Frank.
    let response = [&[1u8, 0][..], &T.to_be_bytes(), &frank()].concat();
    backend.queue_hub_reply(reply(response));
    let out = send(&backend, "b1", &hex(APPLICATION))
        .await
        .expect("Room/send");
    assert_eq!(
        out,
        json!({ "accountId": "b1", "responseCode": "accepted", "acceptedAt": T_UTC })
    );
    let requests = backend.hub_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].endpoint, MimiEndpoint::SubmitMessage);
    assert_eq!(
        requests[0].body,
        [&[1u8][..], &hex(APPLICATION), &opaque(BOB.as_bytes())].concat()
    );
    let fanout = [&T.to_be_bytes()[..], &hex(APPLICATION), &frank()].concat();
    for account in ["b1", "c1"] {
        let stored = messages(&backend, account);
        assert_eq!(stored.len(), 2, "{account}");
        assert_eq!(stored[1]["body"], b64(&fanout));
        assert_eq!(stored[1]["sentAt"], T_UTC);
    }
}

/// L363-L370: `responseCode` is the `statusCode` "written as its name";
/// `currentEpoch` "When `responseCode` is `"epochTooOld"`". Only an
/// accepted message is stored (L372). draft-ietf-mimi-protocol-06 §5.4:
/// an accepted response without a frank carries an absent `optional
/// Frank` (0x00), and the FanoutMessage then carries 0x00 too.
#[tokio::test]
async fn room_send_decodes_each_status_code() {
    let backend = backend().await;
    backend.queue_hub_reply(reply(vec![1, 1]));
    assert_eq!(
        send(&backend, "b1", &hex(APPLICATION)).await,
        Ok(json!({ "accountId": "b1", "responseCode": "notAllowed" }))
    );
    backend.queue_hub_reply(reply([&[1u8, 2][..], &9u64.to_be_bytes()].concat()));
    assert_eq!(
        send(&backend, "b1", &hex(APPLICATION)).await,
        Ok(json!({ "accountId": "b1", "responseCode": "epochTooOld", "currentEpoch": 9 }))
    );
    assert_eq!(messages(&backend, "b1").len(), 1, "only the Welcome");

    backend.queue_hub_reply(reply([&[1u8, 0][..], &T.to_be_bytes(), &[0]].concat()));
    send(&backend, "b1", &hex(APPLICATION))
        .await
        .expect("Room/send");
    let fanout = [&T.to_be_bytes()[..], &hex(APPLICATION), &[0]].concat();
    assert_eq!(messages(&backend, "b1")[1]["body"], b64(&fanout));
}

/// L354/L356: `message` is "An MLSMessage of wire format PrivateMessage and
/// content type application"; "The server MUST reject the call with
/// `invalidArguments` if `message` does not parse as such an MLSMessage".
#[tokio::test]
async fn room_send_rejects_what_is_not_an_application_private_message() {
    let backend = backend().await;
    let trailing = [&hex(APPLICATION)[..], &[0]].concat();
    for message in [
        hex(PRIVATE_PROPOSAL),
        hex(PUBLIC_APPLICATION),
        hex(PUBLIC_COMMIT),
        trailing,
    ] {
        assert_eq!(
            send(&backend, "b1", &message).await,
            Err("invalidArguments".to_owned())
        );
    }
    assert!(backend.hub_requests().is_empty());
}

/// L406: "Otherwise it answers as hub, or forwards the request to the
/// hub". L413-L417: `status` "written as its name", `response` "The whole
/// GroupInfoResponse, unchanged". draft-ietf-mimi-protocol-06 §5.6:
/// GroupInfoResponse is `version`, `room_id<V>`, `status`, then for
/// `success(1)` `cipher_suite`, `ExternalSender hub_sender`,
/// `HPKECiphertext` and `signature<V>`.
#[tokio::test]
async fn group_info_fetch_forwards_and_returns_the_response_unchanged() {
    let backend = backend().await;
    let success = [
        &[1u8][..],
        &opaque(ROOM.as_bytes()),
        &[1, 0x00, 0x01],
        &opaque(&[5; 32]),
        &basic_credential(HUB),
        &opaque(&[6; 32]),
        &opaque(&[8; 48]),
        &opaque(&[0; 64]),
    ]
    .concat();
    backend.queue_hub_reply(reply(success.clone()));
    assert_eq!(
        group_info(&backend, "b1", &group_info_request()).await,
        Ok(json!({ "accountId": "b1", "status": "success", "response": b64(&success) }))
    );
    let requests = backend.hub_requests();
    assert_eq!(requests[0].endpoint, MimiEndpoint::GroupInfo);
    assert_eq!(requests[0].body, group_info_request());

    let no_such_room = [&[1u8][..], &opaque(ROOM.as_bytes()), &[3]].concat();
    backend.queue_hub_reply(reply(no_such_room.clone()));
    assert_eq!(
        group_info(&backend, "b1", &group_info_request()).await,
        Ok(json!({ "accountId": "b1", "status": "noSuchRoom", "response": b64(&no_such_room) }))
    );
}

/// L406: "The server MUST reject the call with `invalidArguments` if
/// `request` does not parse as a GroupInfoRequest". A hub response that
/// does not parse (GroupInfoCode `reserved(0)`) is the hub's failure, not
/// the client's: `serverFail`.
#[tokio::test]
async fn group_info_fetch_rejects_a_bad_request_and_fails_on_a_bad_response() {
    let backend = backend().await;
    let request = group_info_request();
    let wrong_protocol = [&[2u8][..], &request[1..]].concat();
    for bad in [&request[..request.len() - 1], &wrong_protocol[..]] {
        assert_eq!(
            group_info(&backend, "b1", bad).await,
            Err("invalidArguments".to_owned())
        );
    }
    backend.queue_hub_reply(reply([&[1u8][..], &opaque(ROOM.as_bytes()), &[0]].concat()));
    assert_eq!(
        group_info(&backend, "b1", &request).await,
        Err("serverFail".to_owned())
    );
}

/// L231: "When this server is not the room's hub, it MUST send the request
/// to the hub, even if the target user is local". L238-L245: `userStatus`
/// and `clientStatus` are code names; `keyPackage` is "present when
/// `clientStatus` is `"success"`"; `clientCapabilities` is "present when
/// the provider included it". draft-ietf-mimi-protocol-06 §5.2:
/// KeyMaterialResponse is `protocol`, `userStatus`, `userUri`, `clients<V>`.
#[tokio::test]
async fn key_material_fetch_goes_to_the_hub_for_a_local_target_and_decodes_the_response() {
    let backend = backend().await;
    let key_package = hex(KEY_PACKAGE_MESSAGE)[4..].to_vec();
    // RFC 9420 §7.2 Capabilities: five empty vectors.
    let capabilities = [0u8, 0, 0, 0, 0];
    let clients = [
        &[0u8][..],
        &opaque(b"mimi://a.example/u/carol/c1"),
        &key_package,
        &[1],
        &opaque(b"mimi://a.example/u/carol/c2"),
        &[2],
        &opaque(b"mimi://a.example/u/carol/c3"),
        &[1],
        &capabilities,
    ]
    .concat();
    let response = [&[1u8, 1][..], &opaque(CAROL.as_bytes()), &opaque(&clients)].concat();
    backend.queue_hub_reply(reply(response));
    let request = key_material_request(BOB, CAROL, ROOM);
    let out = key_material(&backend, "b1", &request)
        .await
        .expect("KeyMaterial/fetch");
    assert_eq!(
        out,
        json!({
            "accountId": "b1",
            "userStatus": "partialSuccess",
            "userUri": CAROL,
            "clients": [
                { "clientStatus": "success", "clientUri": "mimi://a.example/u/carol/c1", "keyPackage": b64(&key_package) },
                { "clientStatus": "keyMaterialExhausted", "clientUri": "mimi://a.example/u/carol/c2" },
                { "clientStatus": "nothingCompatible", "clientUri": "mimi://a.example/u/carol/c3", "clientCapabilities": b64(&capabilities) },
            ],
        })
    );
    let requests = backend.hub_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].endpoint,
        MimiEndpoint::KeyMaterial {
            target_user: CAROL.to_owned()
        }
    );
    assert_eq!(requests[0].body, request);
}

/// L231: "The server MUST reject the call with `invalidArguments` if
/// `request` does not parse as a KeyMaterialRequest, if its
/// `requestingUser` is not `userUri`, if its `roomId` is not `roomId`".
#[tokio::test]
async fn key_material_fetch_rejects_a_bad_request() {
    let backend = backend().await;
    let good = key_material_request(BOB, CAROL, ROOM);
    for request in [
        key_material_request(CAROL, CAROL, ROOM),
        key_material_request(BOB, CAROL, "mimi://hub.example/r/other"),
        good[..good.len() - 1].to_vec(),
        [&good[..], &[0]].concat(),
    ] {
        assert_eq!(
            key_material(&backend, "b1", &request).await,
            Err("invalidArguments".to_owned())
        );
    }
    assert!(backend.hub_requests().is_empty());
}

/// L231, L317, L356, L406: "returns `serverUnavailable` if the hub cannot
/// be reached" (KeyMaterial/fetch: "If the provider it asks cannot be
/// reached"). Nothing is stored.
#[tokio::test]
async fn an_unreachable_hub_is_server_unavailable() {
    let backend = backend().await;
    let unavailable = Err("serverUnavailable".to_owned());
    assert_eq!(
        update(&backend, "b1", &proposal_update()).await,
        unavailable
    );
    assert_eq!(send(&backend, "b1", &hex(APPLICATION)).await, unavailable);
    assert_eq!(
        group_info(&backend, "b1", &group_info_request()).await,
        unavailable
    );
    assert_eq!(
        key_material(&backend, "b1", &key_material_request(BOB, CAROL, ROOM)).await,
        unavailable
    );
    assert_eq!(backend.hub_requests().len(), 4);
    assert_eq!(messages(&backend, "b1").len(), 1, "only the Welcome");
}

/// L386: "The server MUST reject the call with `invalidArguments` if no
/// Chat in the account has that `roomId`. Otherwise it unbinds the Chat,
/// if it is bound, and returns `accountId`". L260: an unbound Chat "keeps
/// its `roomId`", and "`KeyMaterial/fetch`, `Room/send`, `Room/update`,
/// and `GroupInfo/fetch` treat it as if no Chat in the account had that
/// `roomId`", which L231/L317/L356/L406 reject with `invalidArguments`.
#[tokio::test]
async fn room_unbind_then_the_room_methods_reject_the_room() {
    let backend = backend().await;
    assert_eq!(
        unbind(&backend, "d1").await,
        Err("invalidArguments".to_owned())
    );
    assert_eq!(
        unbind(&backend, "b1").await,
        Ok(json!({ "accountId": "b1" }))
    );
    assert_eq!(
        unbind(&backend, "b1").await,
        Ok(json!({ "accountId": "b1" })),
        "already unbound"
    );
    let chat_id = backend.room_chat_id(&Id::from("b1"), ROOM).expect("kept");
    assert_eq!(backend.room_chat(&chat_id).expect("Chat")["roomId"], ROOM);

    for account in ["b1", "d1"] {
        let invalid = Err("invalidArguments".to_owned());
        assert_eq!(update(&backend, account, &proposal_update()).await, invalid);
        assert_eq!(send(&backend, account, &hex(APPLICATION)).await, invalid);
        assert_eq!(
            group_info(&backend, account, &group_info_request()).await,
            invalid
        );
        let requesting = if account == "b1" {
            BOB
        } else {
            "mimi://a.example/u/dave"
        };
        assert_eq!(
            key_material(
                &backend,
                account,
                &key_material_request(requesting, CAROL, ROOM)
            )
            .await,
            invalid
        );
    }
    assert!(backend.hub_requests().is_empty());
}
