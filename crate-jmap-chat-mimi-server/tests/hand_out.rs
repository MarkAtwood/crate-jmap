//! KeyPackage hand-out against draft-atwood-jmap-chat-mimi-00 §Handing Out
//! KeyPackages (jmap-chat-spec 6bcd9d8, L208-214). Each test quotes the
//! line it checks.

use std::collections::HashSet;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_chat_mimi_server::memory::MemoryBackend;
use jmap_chat_mimi_server::{
    KeyMaterialRequirements, KeyPackageFit, MimiBackend, handle_key_package_get,
    handle_key_package_set,
};
use jmap_chat_mimi_types::{Base64Url, ClientKeyMaterial, MimiCapability};
use jmap_types::Id;
use openmls::extensions::{AppDataDictionary, AppDataDictionaryExtension};
use openmls::prelude::{
    BasicCredential, Capabilities, Ciphersuite, CredentialType, CredentialWithKey, Extension,
    ExtensionType, Extensions, KeyPackage, KeyPackageIn, KeyPackageVerifyError, ProposalType,
    ProtocolVersion, RequiredCapabilitiesExtension,
};
use openmls::treesync::errors::LifetimeError;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{OpenMlsRustCrypto, RustCrypto};
use serde_json::{Value, json};
use tls_codec::{Deserialize as _, Serialize as _};

const ALICE: &str = "mimi://a.example/u/alice";
const SUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

/// The KeyPackage inside `key_package` of entry 0 of
/// `test-vectors/passive-client-welcome.json` in
/// github.com/mlswg/mls-implementations at 2034fe7, without its 4-octet
/// MLSMessage header (`version` 0x0001, `wire_format` 0x0005): a cipher
/// suite 1 KeyPackage whose leaf node lifetime ends at `not_after`
/// 1709378047 (2024-03-02T11:14:07Z).
const INTEROP_KEY_PACKAGE: &str = "000100012027eda0a6943bdaf78e8421903d921dff2833738853ff5ff7231839f81f62f05720afc80b9994962bda4bc1cfe02260a5b48a962e6fb78ef0848996a7dc7691746b202756a27055efed67e3b1e96910cd2be258fadde795c754c2253fc76fb5336e3300010641726e6f6c640200010e0001000200030004000500060007000002000101000000006401d67f0000000065e309ff004040986997da7096e69ba28a89d48738eb30bb173af40768c0bd5233889ef1ac429e6e00030e892d939719e0340c89d4eda0cb3d6a0a91330670e6841889708c050b0040406ecfea01c93873beb6695f010c40cb135e37ed5b2758a4f8c517ca4c03d6c12d32c48e50844797e1d28addaea3849b64ada646b080547fcabadf1e910a58f507";

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn alice_backend(max: u64) -> MemoryBackend<RustCrypto> {
    let backend = MemoryBackend::new(RustCrypto::default());
    let capability: MimiCapability = serde_json::from_value(json!({
        "userUri": ALICE,
        "roomIdPrefix": "mimi://a.example/r/",
        "hubSender": "AAEC",
        "cipherSuites": [1],
        "maxKeyPackages": max,
    }))
    .expect("capability");
    backend.register_account(&Id::from("a1"), capability);
    backend
}

/// A fresh cipher suite 1 KeyPackage for ALICE signed by `signer`; a
/// last-resort one carries the draft-ietf-mls-extensions-10 §6.4
/// `last_resort_key_package` component (id 0x0004) in its
/// `app_data_dictionary` extension.
fn key_package(signer: &SignatureKeyPair, last_resort: bool) -> Vec<u8> {
    key_package_listing(signer, last_resort, &[ExtensionType::AppDataDictionary])
}

/// As [`key_package`], with the leaf node capabilities listing exactly
/// `extensions`, no proposal types and the Basic credential type.
fn key_package_listing(
    signer: &SignatureKeyPair,
    last_resort: bool,
    extensions: &[ExtensionType],
) -> Vec<u8> {
    let provider = OpenMlsRustCrypto::default();
    let mut builder = KeyPackage::builder().leaf_node_capabilities(Capabilities::new(
        None,
        None,
        Some(extensions),
        Some(&[]),
        Some(&[CredentialType::Basic]),
    ));
    if last_resort {
        let mut dict = AppDataDictionary::new();
        dict.insert(0x0004, Vec::new());
        builder = builder.key_package_extensions(
            Extensions::single(Extension::AppDataDictionary(
                AppDataDictionaryExtension::new(dict),
            ))
            .expect("app_data_dictionary is a KeyPackage extension"),
        );
    }
    let cwk = CredentialWithKey {
        credential: BasicCredential::new(ALICE.as_bytes().to_vec()).into(),
        signature_key: signer.public().into(),
    };
    builder
        .build(SUITE, &provider, signer, cwk)
        .expect("build KeyPackage")
        .key_package()
        .tls_serialize_detached()
        .expect("serialize KeyPackage")
}

fn signer() -> SignatureKeyPair {
    SignatureKeyPair::new(SUITE.signature_algorithm()).expect("signer")
}

/// Requirements accepting `suites` with an empty RequiredCapabilities.
fn accepts(suites: &[u16]) -> KeyMaterialRequirements {
    KeyMaterialRequirements::new(
        suites.to_vec(),
        RequiredCapabilitiesExtension::new(&[], &[], &[]),
    )
}

async fn set(backend: &MemoryBackend<RustCrypto>, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!("a1");
    handle_key_package_set(backend, &(), args)
        .await
        .expect("KeyPackage/set")
        .primary
}

async fn create(backend: &MemoryBackend<RustCrypto>, kp: &[u8]) -> Value {
    set(
        backend,
        json!({ "create": { "k": { "keyPackage": b64(kp) } } }),
    )
    .await
}

async fn get_all(backend: &MemoryBackend<RustCrypto>) -> Value {
    handle_key_package_get(backend, &(), json!({ "accountId": "a1", "ids": null }))
        .await
        .expect("KeyPackage/get")
        .primary
}

async fn claim(
    backend: &MemoryBackend<RustCrypto>,
    requirements: &KeyMaterialRequirements,
) -> Vec<ClientKeyMaterial> {
    backend
        .claim_key_packages(&(), &Id::from("a1"), requirements)
        .await
        .expect("claim")
}

fn claimed(kp: &[u8]) -> Option<Base64Url> {
    Some(b64(kp).parse().expect("base64url"))
}

/// L210: "For each client it returns a KeyPackage compatible with the
/// request that is not last-resort and destroys that record in the same
/// transaction". L214: "A destroyed record appears in a new state for the
/// KeyPackage type".
#[tokio::test]
async fn hands_out_a_key_package_and_destroys_its_record() {
    let backend = alice_backend(10);
    let kp = key_package(&signer(), false);
    create(&backend, &kp).await;
    let before = get_all(&backend).await;
    assert_eq!(before["list"].as_array().map(Vec::len), Some(1));

    let clients = claim(&backend, &accepts(&[1])).await;
    assert_eq!(clients.len(), 1);
    let entry = serde_json::to_value(&clients[0]).expect("serialize");
    // draft-ietf-mimi-protocol-06 §5.2 ClientKeyMaterial, as the draft's
    // KeyMaterial/fetch `clients` field writes it (L245).
    assert_eq!(entry["clientStatus"], json!("success"));
    assert_eq!(entry["keyPackage"], json!(b64(&kp)));

    let after = get_all(&backend).await;
    assert_eq!(after["list"], json!([]));
    assert_ne!(after["state"], before["state"]);
}

/// L210: "When the client has no such KeyPackage, the server returns the
/// client's compatible last-resort KeyPackage, if any, and keeps it."
#[tokio::test]
async fn prefers_a_regular_key_package_then_keeps_returning_the_last_resort_one() {
    let backend = alice_backend(10);
    let s = signer();
    let regular = key_package(&s, false);
    let last_resort = key_package(&s, true);
    create(&backend, &last_resort).await;
    create(&backend, &regular).await;

    let first = claim(&backend, &accepts(&[1])).await;
    assert_eq!(first[0].key_package, claimed(&regular));

    let state = get_all(&backend).await["state"].clone();
    for _ in 0..2 {
        let next = claim(&backend, &accepts(&[1])).await;
        assert_eq!(next[0].key_package, claimed(&last_resort));
    }
    let after = get_all(&backend).await;
    assert_eq!(after["list"].as_array().map(Vec::len), Some(1));
    assert_eq!(after["list"][0]["lastResort"], json!(true));
    assert_eq!(after["state"], state);
}

/// L210: "it returns at most one KeyPackage for each of the owner's
/// clients". draft-ietf-mimi-protocol-06 §5.2: "`keyMaterialExhausted`
/// indicates that there was no keying material available for the
/// specified client."
#[tokio::test]
async fn a_client_with_nothing_left_is_key_material_exhausted() {
    let backend = alice_backend(10);
    let s = signer();
    create(&backend, &key_package(&s, false)).await;
    create(&backend, &key_package(&s, false)).await;

    for _ in 0..2 {
        let clients = claim(&backend, &accepts(&[1])).await;
        assert_eq!(clients.len(), 1);
        assert_eq!(
            serde_json::to_value(&clients[0]).unwrap()["clientStatus"],
            json!("success")
        );
    }
    let clients = claim(&backend, &accepts(&[1])).await;
    let entry = serde_json::to_value(&clients[0]).expect("serialize");
    assert_eq!(entry["clientStatus"], json!("keyMaterialExhausted"));
    assert!(entry.get("keyPackage").is_none());
}

/// L210: "it returns a KeyPackage compatible with the request".
/// draft-ietf-mimi-protocol-06 §5.2: "`nothingCompatible` indicates that
/// the specified clients had no key material compatible with the
/// `requiredCapabilities` field in the request", and the request carries
/// "a non-empty list of acceptable MLS ciphersuites".
#[tokio::test]
async fn an_incompatible_cipher_suite_is_nothing_compatible_and_kept() {
    let backend = alice_backend(10);
    create(&backend, &key_package(&signer(), false)).await;

    let clients = claim(&backend, &accepts(&[2])).await;
    let entry = serde_json::to_value(&clients[0]).expect("serialize");
    assert_eq!(entry["clientStatus"], json!("nothingCompatible"));
    assert!(entry.get("keyPackage").is_none());
    assert_eq!(
        get_all(&backend).await["list"].as_array().map(Vec::len),
        Some(1)
    );
}

/// RFC 9420 §11.1: "The \"default\" proposal and extension types defined
/// in this document are assumed to be implemented by all clients, and
/// need not be listed in RequiredCapabilities in order to be safely used.
/// Note that this is not true for credential types." §7.2 lists
/// external_senders (0x0005) as a default extension type; last_resort
/// (0x000a) is not one, and the test KeyPackages do not list it.
/// §7.2 makes proposal types 0x0001 to 0x0007 and extension types 0x0001
/// to 0x0005 the defaults. The KeyPackage leaves list no proposal type,
/// and the second lists no extension type, so the first value past each
/// default range (0x0008 and 0x0006, assigned by
/// draft-ietf-mls-extensions) must be listed to be compatible.
#[test]
fn required_capabilities_are_checked_against_the_leaf_capabilities() {
    let crypto = RustCrypto::default();
    let kp: Base64Url = b64(&key_package(&signer(), false)).parse().unwrap();
    let requires_of = |kp: &Base64Url,
                       ext: &[ExtensionType],
                       prop: &[ProposalType],
                       cred: &[CredentialType]| {
        KeyMaterialRequirements::new(vec![1], RequiredCapabilitiesExtension::new(ext, prop, cred))
            .fit(&crypto, kp)
    };
    let requires =
        |ext: &[ExtensionType], cred: &[CredentialType]| requires_of(&kp, ext, &[], cred);
    assert_eq!(requires(&[], &[]), KeyPackageFit::Compatible);
    assert_eq!(
        requires(&[ExtensionType::ExternalSenders], &[]),
        KeyPackageFit::Compatible
    );
    assert_eq!(
        requires(&[ExtensionType::AppDataDictionary], &[]),
        KeyPackageFit::Compatible
    );
    assert_eq!(
        requires(&[ExtensionType::LastResort], &[]),
        KeyPackageFit::Incompatible
    );
    assert_eq!(
        requires(&[], &[CredentialType::Basic]),
        KeyPackageFit::Compatible
    );
    assert_eq!(
        requires(&[], &[CredentialType::X509]),
        KeyPackageFit::Incompatible
    );

    assert_eq!(
        requires_of(
            &kp,
            &[],
            &[ProposalType::Add, ProposalType::GroupContextExtensions],
            &[]
        ),
        KeyPackageFit::Compatible
    );
    assert_eq!(
        requires_of(&kp, &[], &[ProposalType::from(0x0008)], &[]),
        KeyPackageFit::Incompatible
    );

    let bare: Base64Url = b64(&key_package_listing(&signer(), false, &[]))
        .parse()
        .unwrap();
    assert_eq!(
        requires_of(&bare, &[ExtensionType::ExternalSenders], &[], &[]),
        KeyPackageFit::Compatible
    );
    assert_eq!(
        requires_of(&bare, &[ExtensionType::from(0x0006)], &[], &[]),
        KeyPackageFit::Incompatible
    );
}

/// draft-ietf-mimi-protocol-06 §5.2: "The target provider MUST NOT
/// provide expired keying material (ex: an MLS KeyPackage containing a
/// LeafNode with a `notAfter` time past the current date and time)." The
/// interop KeyPackage's `not_after` is 2024-03-02. openmls rejects it for
/// its lifetime alone, so `Unavailable` is not a decode failure.
#[test]
fn an_expired_key_package_is_unavailable() {
    let bytes = hex(INTEROP_KEY_PACKAGE);
    assert!(matches!(
        KeyPackageIn::tls_deserialize_exact(&bytes)
            .expect("the vector parses")
            .validate(&RustCrypto::default(), ProtocolVersion::Mls10),
        Err(KeyPackageVerifyError::LifetimeError(
            LifetimeError::Expired {
                not_after: 1_709_378_047,
                ..
            }
        ))
    ));
    let kp: Base64Url = b64(&bytes).parse().unwrap();
    assert_eq!(
        accepts(&[1]).fit(&RustCrypto::default(), &kp),
        KeyPackageFit::Unavailable
    );
}

/// L212: "the server MUST return the same `clientUri` in every response
/// for a given signature key, and different `clientUri` values for
/// different signature keys."
#[tokio::test]
async fn client_uri_is_stable_per_signature_key_and_unique() {
    let backend = alice_backend(10);
    let (s1, s2) = (signer(), signer());
    let mut uploads: Vec<(usize, Vec<u8>)> = Vec::new();
    for (client, s) in [(1, &s1), (2, &s2)] {
        for _ in 0..2 {
            let kp = key_package(s, false);
            create(&backend, &kp).await;
            uploads.push((client, kp));
        }
    }
    let client_of = |kp: &Option<Base64Url>| {
        uploads
            .iter()
            .find(|(_, bytes)| claimed(bytes) == *kp)
            .map(|(c, _)| *c)
            .expect("an uploaded KeyPackage")
    };

    let mut uris = [None::<String>, None];
    for _ in 0..2 {
        let clients = claim(&backend, &accepts(&[1])).await;
        assert_eq!(clients.len(), 2);
        for entry in clients {
            let slot = &mut uris[client_of(&entry.key_package) - 1];
            match slot {
                Some(uri) => assert_eq!(*uri, entry.client_uri),
                None => *slot = Some(entry.client_uri),
            }
        }
    }
    assert_ne!(uris[0], uris[1]);
}

/// L214: "The server MUST keep the KeyPackageRef of every KeyPackage it
/// hands out, with the account it belongs to, until a Welcome naming it
/// has been delivered or its leaf node lifetime has passed." L204: "The
/// server MUST reject, with `invalidProperties` naming `keyPackage`, a
/// KeyPackage whose KeyPackageRef equals that of a KeyPackage it stores
/// or has handed out".
#[tokio::test]
async fn a_handed_out_key_package_cannot_be_uploaded_again() {
    let backend = alice_backend(10);
    let kp = key_package(&signer(), false);
    create(&backend, &kp).await;
    claim(&backend, &accepts(&[1])).await;
    assert_eq!(get_all(&backend).await["list"], json!([]));

    let out = create(&backend, &kp).await;
    assert_eq!(out["notCreated"]["k"]["type"], json!("invalidProperties"));
    assert_eq!(out["notCreated"]["k"]["properties"], json!(["keyPackage"]));
}

/// L210: the server "destroys that record in the same transaction, so
/// that no KeyPackage is handed out twice."
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_claims_never_hand_out_the_same_key_package() {
    const UPLOADED: usize = 24;
    let backend = alice_backend(100);
    let s = signer();
    let mut uploaded = HashSet::new();
    for _ in 0..UPLOADED {
        let kp = key_package(&s, false);
        create(&backend, &kp).await;
        uploaded.insert(claimed(&kp).expect("some"));
    }

    let tasks: Vec<_> = (0..64)
        .map(|_| {
            let backend = backend.clone();
            tokio::spawn(async move { claim(&backend, &accepts(&[1])).await })
        })
        .collect();
    let mut handed = Vec::new();
    for task in tasks {
        handed.extend(
            task.await
                .expect("join")
                .into_iter()
                .filter_map(|c| c.key_package),
        );
    }

    let distinct: HashSet<_> = handed.iter().cloned().collect();
    assert_eq!(
        handed.len(),
        distinct.len(),
        "a KeyPackage was handed out twice"
    );
    assert_eq!(distinct, uploaded);
}
