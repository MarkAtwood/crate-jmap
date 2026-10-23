//! KeyPackage/get and KeyPackage/set against draft-atwood-jmap-chat-mimi-00
//! (jmap-chat-spec 6bcd9d8). Each test quotes the draft line it checks.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_chat_mimi_server::memory::MemoryBackend;
use jmap_chat_mimi_server::{handle_key_package_get, handle_key_package_set};
use jmap_chat_mimi_types::MimiCapability;
use jmap_types::Id;
use openmls::extensions::{AppDataDictionary, AppDataDictionaryExtension};
use openmls::prelude::{
    BasicCredential, Capabilities, Ciphersuite, Credential, CredentialType, CredentialWithKey,
    Extension, ExtensionType, Extensions, KeyPackage,
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::{OpenMlsRustCrypto, RustCrypto};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tls_codec::Serialize as _;

const ALICE: &str = "mimi://a.example/u/alice";
const SUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

/// `key_package` of entry 0 of `test-vectors/passive-client-welcome.json`
/// in github.com/mlswg/mls-implementations at 2034fe7: an RFC 9420
/// MLSMessage (`version` 0x0001, `wire_format` 0x0005 mls_key_package)
/// framing a cipher suite 1 KeyPackage whose Basic credential identity is
/// "Arnold" and whose leaf node lifetime ends at `not_after` 1709378047
/// (2024-03-02T11:14:07Z).
const INTEROP_KEY_PACKAGE_MESSAGE: &str = "00010005000100012027eda0a6943bdaf78e8421903d921dff2833738853ff5ff7231839f81f62f05720afc80b9994962bda4bc1cfe02260a5b48a962e6fb78ef0848996a7dc7691746b202756a27055efed67e3b1e96910cd2be258fadde795c754c2253fc76fb5336e3300010641726e6f6c640200010e0001000200030004000500060007000002000101000000006401d67f0000000065e309ff004040986997da7096e69ba28a89d48738eb30bb173af40768c0bd5233889ef1ac429e6e00030e892d939719e0340c89d4eda0cb3d6a0a91330670e6841889708c050b0040406ecfea01c93873beb6695f010c40cb135e37ed5b2758a4f8c517ca4c03d6c12d32c48e50844797e1d28addaea3849b64ada646b080547fcabadf1e910a58f507";

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn capability(user_uri: &str, suites: &[u16], max: u64) -> MimiCapability {
    serde_json::from_value(json!({
        "userUri": user_uri,
        "roomIdPrefix": "mimi://a.example/r/",
        "hubSender": "AAEC",
        "cipherSuites": suites,
        "maxKeyPackages": max,
    }))
    .expect("capability")
}

fn backend_with(accounts: &[(&str, MimiCapability)]) -> MemoryBackend<RustCrypto> {
    let backend = MemoryBackend::new(RustCrypto::default());
    for (id, cap) in accounts {
        backend.register_account(&Id::from(*id), cap.clone());
    }
    backend
}

fn alice_backend(max: u64) -> MemoryBackend<RustCrypto> {
    backend_with(&[("a1", capability(ALICE, &[1], max))])
}

/// A fresh KeyPackage from openmls for `credential`, signed by `signer`.
/// When `last_resort` is set its `app_data_dictionary` extension carries a
/// `last_resort_key_package` component (draft-ietf-mls-extensions-10 §6.4:
/// "a last_resort_key_package component contains the component_id with an
/// empty data field"; component id 0x0004, extension type 0x0006).
fn key_package(credential: Credential, signer: &SignatureKeyPair, last_resort: bool) -> Vec<u8> {
    let provider = OpenMlsRustCrypto::default();
    let mut builder = KeyPackage::builder().leaf_node_capabilities(Capabilities::new(
        None,
        None,
        Some(&[ExtensionType::AppDataDictionary]),
        None,
        Some(&[credential.credential_type()]),
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
        credential,
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

fn basic(identity: &str) -> Credential {
    BasicCredential::new(identity.as_bytes().to_vec()).into()
}

/// RFC 9420 §2.1.2 variable-length integer prefix for `len` < 2^14.
fn varint(len: usize) -> Vec<u8> {
    if len < 64 {
        vec![len as u8]
    } else {
        vec![0x40 | (len >> 8) as u8, len as u8]
    }
}

/// RFC 9420 §5.2, written out independently of openmls:
/// `MakeKeyPackageRef(value) = RefHash("MLS 1.0 KeyPackage Reference",
/// value)`, `RefHash(label, value) = Hash(RefHashInput)` with
/// `struct { opaque label<V>; opaque value<V>; } RefHashInput`, encoded as
/// the `opaque HashReference<V>` (cipher suite 1 hashes with SHA-256).
fn expected_ref(key_package: &[u8]) -> String {
    let label = b"MLS 1.0 KeyPackage Reference";
    let mut input = varint(label.len());
    input.extend_from_slice(label);
    input.extend(varint(key_package.len()));
    input.extend_from_slice(key_package);
    let mut out = vec![32u8];
    out.extend_from_slice(&Sha256::digest(&input));
    b64(&out)
}

async fn set(backend: &MemoryBackend<RustCrypto>, account: &str, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(account);
    handle_key_package_set(backend, &(), args)
        .await
        .expect("KeyPackage/set")
        .primary
}

async fn create(backend: &MemoryBackend<RustCrypto>, account: &str, kp: &[u8]) -> Value {
    set(
        backend,
        account,
        json!({ "create": { "k": { "keyPackage": b64(kp) } } }),
    )
    .await
}

async fn get_all(backend: &MemoryBackend<RustCrypto>, account: &str) -> Vec<Value> {
    let out = handle_key_package_get(backend, &(), json!({ "accountId": account, "ids": null }))
        .await
        .expect("KeyPackage/get")
        .primary;
    out["list"].as_array().expect("list").clone()
}

/// Assert `out` refused create `k` with `invalidProperties` naming exactly
/// `property`.
fn assert_invalid(out: &Value, property: &str) {
    let err = &out["notCreated"]["k"];
    assert_eq!(err["type"], "invalidProperties", "{out}");
    assert_eq!(err["properties"], json!([property]), "{out}");
    assert!(out["created"].is_null(), "{out}");
}

#[tokio::test]
async fn create_stores_key_package_verbatim_with_its_ref() {
    // L188-189: "`keyPackageRef` (String, immutable, server-set): The
    // KeyPackageRef of `keyPackage` (RFC 9420 Section 5.2)." L110:
    // "Servers MUST NOT re-encode these octets". L112: a KeyPackageRef
    // "begins with the vector's variable-length length prefix".
    let backend = alice_backend(10);
    let kp = key_package(basic(ALICE), &signer(), false);
    let out = create(&backend, "a1", &kp).await;
    let created = &out["created"]["k"];
    assert_eq!(created["keyPackageRef"], expected_ref(&kp), "{out}");
    // L192: "The server sets it from `keyPackage`"; the create omitted it,
    // so RFC 8620 §5.3 `created` carries it.
    assert_eq!(created["lastResort"], false, "{out}");
    assert_ne!(out["oldState"], out["newState"]);

    let list = get_all(&backend, "a1").await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], created["id"]);
    assert_eq!(list[0]["keyPackage"], b64(&kp));
    assert_eq!(list[0]["keyPackageRef"], expected_ref(&kp));
    assert_eq!(list[0]["lastResort"], false);
}

#[tokio::test]
async fn update_is_forbidden() {
    // L200: "`update` is not supported: the server MUST reject every
    // update with a SetError of type `forbidden`."
    let backend = alice_backend(10);
    let out = create(&backend, "a1", &key_package(basic(ALICE), &signer(), false)).await;
    let id = out["created"]["k"]["id"].as_str().expect("id").to_owned();
    let out = set(
        &backend,
        "a1",
        json!({ "update": { id.clone(): { "lastResort": true }, "nope": {} } }),
    )
    .await;
    assert_eq!(out["notUpdated"][&id]["type"], "forbidden", "{out}");
    assert_eq!(out["notUpdated"]["nope"]["type"], "forbidden", "{out}");
    assert!(out["updated"].is_null());
    assert_eq!(out["oldState"], out["newState"]);
}

#[tokio::test]
async fn rejects_bytes_that_do_not_parse_as_a_key_package() {
    // L202: "the server MUST reject the record with `invalidProperties`
    // naming `keyPackage` if `keyPackage` does not parse as a KeyPackage".
    let backend = backend_with(&[("a1", capability("Arnold", &[1], 10))]);
    let out = create(&backend, "a1", &[0x00, 0x01, 0x02]).await;
    assert_invalid(&out, "keyPackage");

    // L110 and L186: `keyPackage` is the octets of "The KeyPackage (RFC
    // 9420 Section 10)", so the interop vector still framed as an
    // MLSMessage does not parse as one.
    let out = create(&backend, "a1", &hex(INTEROP_KEY_PACKAGE_MESSAGE)).await;
    assert_invalid(&out, "keyPackage");
    assert!(get_all(&backend, "a1").await.is_empty());
}

#[tokio::test]
async fn rejects_a_key_package_whose_signature_does_not_verify() {
    // L202: "... if its signature does not verify". The last octet is part
    // of the KeyPackage signature (RFC 9420 Section 10: `signature` is the
    // final field).
    let backend = alice_backend(10);
    let mut kp = key_package(basic(ALICE), &signer(), false);
    *kp.last_mut().expect("non-empty") ^= 0x01;
    assert_invalid(&create(&backend, "a1", &kp).await, "keyPackage");
}

#[tokio::test]
async fn rejects_a_cipher_suite_not_in_cipher_suites() {
    // L202: "... if its cipher suite is not in `cipherSuites`". The
    // KeyPackage uses cipher suite 1; the account accepts only 2.
    let backend = backend_with(&[("a1", capability(ALICE, &[2], 10))]);
    let kp = key_package(basic(ALICE), &signer(), false);
    assert_invalid(&create(&backend, "a1", &kp).await, "keyPackage");
}

#[tokio::test]
async fn rejects_an_expired_interop_key_package() {
    // L202: "... if its leaf node lifetime has expired". The interop
    // vector's lifetime ended 2024-03-02; its cipher suite (1) and Basic
    // identity ("Arnold") match the account, so only the lifetime fails.
    let backend = backend_with(&[("a1", capability("Arnold", &[1], 10))]);
    let kp = hex(INTEROP_KEY_PACKAGE_MESSAGE);
    let out = create(&backend, "a1", &kp[4..]).await;
    assert_invalid(&out, "keyPackage");
}

#[tokio::test]
async fn rejects_a_credential_that_is_not_basic() {
    // L202: "... if its credential is not a Basic credential (RFC 9420
    // Section 5.3)".
    let backend = alice_backend(10);
    let x509 = Credential::new(CredentialType::X509, vec![0x00]);
    let kp = key_package(x509, &signer(), false);
    assert_invalid(&create(&backend, "a1", &kp).await, "keyPackage");
}

#[tokio::test]
async fn rejects_a_credential_identity_that_is_not_user_uri() {
    // L202: "... or if the credential identity is not `userUri`".
    let backend = alice_backend(10);
    let kp = key_package(basic("mimi://a.example/u/mallory"), &signer(), false);
    assert_invalid(&create(&backend, "a1", &kp).await, "keyPackage");
}

#[tokio::test]
async fn last_resort_is_derived_and_a_differing_claim_is_rejected() {
    // L192: "`true` when `keyPackage` is the client's last-resort
    // KeyPackage, which its `app_data_dictionary` extension marks with a
    // `last_resort_key_package` component ... A create MAY include it; the
    // server MUST reject a create whose `lastResort` differs from the
    // value derived from `keyPackage` with `invalidProperties` naming
    // `lastResort`."
    let backend = alice_backend(10);
    let alice = signer();
    let regular = key_package(basic(ALICE), &alice, false);
    let out = set(
        &backend,
        "a1",
        json!({ "create": { "k": { "keyPackage": b64(&regular), "lastResort": true } } }),
    )
    .await;
    assert_invalid(&out, "lastResort");

    let last = key_package(basic(ALICE), &alice, true);
    let out = set(
        &backend,
        "a1",
        json!({ "create": { "k": { "keyPackage": b64(&last), "lastResort": false } } }),
    )
    .await;
    assert_invalid(&out, "lastResort");

    // A matching claim is accepted; RFC 8620 §5.3 `created` then omits
    // the property the client sent.
    let out = set(
        &backend,
        "a1",
        json!({ "create": { "k": { "keyPackage": b64(&last), "lastResort": true } } }),
    )
    .await;
    assert!(out["created"]["k"].get("lastResort").is_none(), "{out}");
    let list = get_all(&backend, "a1").await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["lastResort"], true);
}

#[tokio::test]
async fn rejects_a_signature_key_bound_to_another_account() {
    // L202: "the first KeyPackage the server accepts with a given
    // signature key binds that signature key to the account. The server
    // SHOULD reject, with `invalidProperties` naming `keyPackage`, a
    // KeyPackage whose signature key is bound to another account."
    let backend = backend_with(&[
        ("a1", capability(ALICE, &[1], 10)),
        ("a2", capability(ALICE, &[1], 10)),
    ]);
    let alice = signer();
    let first = create(&backend, "a1", &key_package(basic(ALICE), &alice, false)).await;
    assert!(first["created"]["k"].is_object(), "{first}");
    let second = create(&backend, "a2", &key_package(basic(ALICE), &alice, false)).await;
    assert_invalid(&second, "keyPackage");

    // The same client's next KeyPackage on its own account is accepted.
    let again = create(&backend, "a1", &key_package(basic(ALICE), &alice, false)).await;
    assert!(again["created"]["k"].is_object(), "{again}");
}

#[tokio::test]
async fn rejects_a_key_package_ref_already_stored() {
    // L204: "The server MUST reject, with `invalidProperties` naming
    // `keyPackage`, a KeyPackage whose KeyPackageRef equals that of a
    // KeyPackage it stores or has handed out".
    let backend = alice_backend(10);
    let kp = key_package(basic(ALICE), &signer(), false);
    assert!(create(&backend, "a1", &kp).await["created"]["k"].is_object());
    assert_invalid(&create(&backend, "a1", &kp).await, "keyPackage");
    assert_eq!(get_all(&backend, "a1").await.len(), 1);
}

#[tokio::test]
async fn create_beyond_max_key_packages_is_over_quota() {
    // L204: "MUST reject a create that would exceed `maxKeyPackages` with
    // `overQuota`". L172: the maximum counts "KeyPackage records,
    // last-resort KeyPackages included".
    let backend = alice_backend(2);
    let alice = signer();
    let out = set(
        &backend,
        "a1",
        json!({ "create": {
            "k1": { "keyPackage": b64(&key_package(basic(ALICE), &alice, true)) },
            "k2": { "keyPackage": b64(&key_package(basic(ALICE), &alice, false)) },
        } }),
    )
    .await;
    assert_eq!(
        out["created"].as_object().map(|m| m.len()),
        Some(2),
        "{out}"
    );
    let out = create(&backend, "a1", &key_package(basic(ALICE), &alice, false)).await;
    assert_eq!(out["notCreated"]["k"]["type"], "overQuota", "{out}");
    assert_eq!(get_all(&backend, "a1").await.len(), 2);
}

#[tokio::test]
async fn a_new_last_resort_key_package_replaces_the_clients_previous_one() {
    // L206: "Each client has at most one last-resort KeyPackage. Creating
    // a last-resort KeyPackage for a client destroys that client's
    // previous last-resort KeyPackage." L202: "The leaf node's signature
    // key identifies the client".
    let backend = alice_backend(2);
    let (phone, laptop) = (signer(), signer());
    let old = create(&backend, "a1", &key_package(basic(ALICE), &phone, true)).await;
    let old_id = old["created"]["k"]["id"].clone();
    let other = create(&backend, "a1", &key_package(basic(ALICE), &laptop, true)).await;
    let other_id = other["created"]["k"]["id"].clone();

    // Both slots of `maxKeyPackages` are used, yet the replacement fits:
    // it destroys the record it replaces.
    let new = create(&backend, "a1", &key_package(basic(ALICE), &phone, true)).await;
    let new_id = new["created"]["k"]["id"].clone();
    assert!(new_id.is_string(), "{new}");

    let ids: Vec<Value> = get_all(&backend, "a1")
        .await
        .into_iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert!(ids.contains(&new_id));
    assert!(
        ids.contains(&other_id),
        "another client's last resort stays"
    );
    assert!(
        !ids.contains(&old_id),
        "the previous last resort is destroyed"
    );
}

#[tokio::test]
async fn destroy_removes_a_record() {
    // L200: KeyPackage/set is "Standard JMAP `/set` (RFC 8620 Section
    // 5.3)" apart from update, so destroy is supported.
    let backend = alice_backend(10);
    let out = create(&backend, "a1", &key_package(basic(ALICE), &signer(), false)).await;
    let id = out["created"]["k"]["id"].clone();
    let out = set(&backend, "a1", json!({ "destroy": [id.clone(), "nope"] })).await;
    assert_eq!(out["destroyed"], json!([id]), "{out}");
    assert_eq!(out["notDestroyed"]["nope"]["type"], "notFound", "{out}");
    assert!(get_all(&backend, "a1").await.is_empty());
}

#[tokio::test]
async fn create_naming_a_server_set_property_is_invalid() {
    // L188: `keyPackageRef` is "server-set"; RFC 8620 Section 5.3 refuses a
    // create that sets a server-set property with `invalidProperties`.
    let backend = alice_backend(10);
    let kp = key_package(basic(ALICE), &signer(), false);
    let out = set(
        &backend,
        "a1",
        json!({ "create": { "k": { "keyPackage": b64(&kp), "keyPackageRef": expected_ref(&kp) } } }),
    )
    .await;
    assert_invalid(&out, "keyPackageRef");
}

/// RFC 8620 §5.3 types `destroy` as "Id[]|null", so an object element is
/// "invalidArguments: One of the arguments is of the wrong type" (§3.6.2).
/// The description names only the JSON type: §3.6.1 reserves it for
/// "unstructured English text suitable for displaying to a developer", so
/// the client-supplied value is never echoed (bd JMAP-zegc).
#[tokio::test]
async fn set_non_string_destroy_element_does_not_echo_value() {
    let backend = alice_backend(10);
    let args = json!({ "accountId": "a1", "destroy": [{ "note": "CANARY-zegc" }] });
    let err = handle_key_package_set(&backend, &(), args)
        .await
        .expect_err("non-string destroy element must be a method error");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
    let desc = err.description.as_deref().unwrap_or_default();
    assert!(desc.contains("got object"), "{desc}");
    assert!(!desc.contains("CANARY"), "{desc}");
}
