//! KeyPackage/* and KeyMaterial/fetch bindings against
//! draft-atwood-jmap-chat-mimi-00 (jmap-chat-spec 33702a5) §Examples,
//! L450-499, and the RFC 8620 §5.1 and §5.3 argument shapes.
//!
//! The examples shorten payloads with "...". Where the remaining prefix is
//! valid unpadded base64url it is kept; otherwise these tests substitute
//! "AAEC" (octets 0x00 0x01 0x02, hand encoded) for a KeyPackage and the
//! draft's §Conventions KeyPackageRef example string for a KeyPackageRef.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use helpers::{assert_using, sent, serve};
use jmap_chat_mimi_types::{KeyMaterialClientCode, KeyMaterialUserCode, KeyPackageCreate};
use jmap_types::{Id, State};
use serde_json::json;
use wiremock::MockServer;

const KP: &str = "AAEC";
const KP_REF: &str = "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f";
const ROOM: &str = "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";

#[tokio::test]
async fn key_package_set_matches_draft_example() {
    // Oracle: draft L450-472, "A client publishes a KeyPackage".
    let server = serve(
        "KeyPackage/set",
        json!({
            "accountId": "u1",
            "oldState": "s7",
            "newState": "s8",
            "created": { "k1": { "id": "kp42", "keyPackageRef": KP_REF } }
        }),
    )
    .await;
    let create = BTreeMap::from([(
        "k1".to_owned(),
        KeyPackageCreate::new(KP.parse().unwrap()).with_last_resort(false),
    )]);
    let resp = helpers::make_client(&server, true)
        .key_package_set(&create, &[], None)
        .await
        .expect("KeyPackage/set");
    assert_eq!(resp.new_state.as_ref(), "s8");
    let created = resp.created.expect("created");
    assert_eq!(created["k1"].id.as_ref(), "kp42");
    assert_eq!(created["k1"].key_package_ref.as_str(), KP_REF);

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["KeyPackage/set", {
            "accountId": "u1",
            "create": { "k1": { "keyPackage": KP, "lastResort": false } }
        }, "r1"])
    );
}

#[tokio::test]
async fn key_package_set_destroy_only() {
    // Oracle: RFC 8620 §5.3: `destroy` is "Id[]|null", "A list of ids for
    // Foo objects to permanently delete"; the response lists them in
    // `destroyed`.
    let server = serve(
        "KeyPackage/set",
        json!({
            "accountId": "u1",
            "oldState": "s8",
            "newState": "s9",
            "destroyed": ["kp42"]
        }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .key_package_set(&BTreeMap::new(), &[Id::from("kp42")], None)
        .await
        .expect("KeyPackage/set");
    assert_eq!(resp.destroyed.expect("destroyed")[0].as_ref(), "kp42");

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["KeyPackage/set", { "accountId": "u1", "destroy": ["kp42"] }, "r1"])
    );
}

#[tokio::test]
async fn key_package_set_sends_if_in_state() {
    // Oracle: RFC 8620 §5.3: "ifInState: `String|null` ... If supplied,
    // the string must match the current state; otherwise, the method will
    // be aborted and a `stateMismatch` error returned."
    let server = serve(
        "KeyPackage/set",
        json!({
            "accountId": "u1",
            "oldState": "s8",
            "newState": "s9",
            "destroyed": ["kp42"]
        }),
    )
    .await;
    helpers::make_client(&server, true)
        .key_package_set(
            &BTreeMap::new(),
            &[Id::from("kp42")],
            Some(&State::from("s8")),
        )
        .await
        .expect("KeyPackage/set");

    let (_, call) = sent(&server).await;
    assert_eq!(
        call,
        json!([
            "KeyPackage/set",
            { "accountId": "u1", "ifInState": "s8", "destroy": ["kp42"] },
            "r1"
        ])
    );
}

#[tokio::test]
async fn key_package_set_over_quota_is_not_created() {
    // Oracle: draft L204: the server "MUST reject a create that would
    // exceed `maxKeyPackages` with `overQuota`", a per-record SetError
    // (RFC 8620 §5.3 `notCreated`), not a method error.
    let server = serve(
        "KeyPackage/set",
        json!({
            "accountId": "u1",
            "oldState": "s8",
            "newState": "s8",
            "notCreated": { "k1": { "type": "overQuota" } }
        }),
    )
    .await;
    let create = BTreeMap::from([("k1".to_owned(), KeyPackageCreate::new(KP.parse().unwrap()))]);
    let resp = helpers::make_client(&server, true)
        .key_package_set(&create, &[], None)
        .await
        .expect("a SetError is not a method error");
    assert!(resp.created.is_none());
    let not_created = resp.not_created.expect("notCreated");
    assert_eq!(
        serde_json::to_value(&not_created["k1"]).unwrap()["type"],
        "overQuota"
    );
}

#[tokio::test]
async fn key_package_get_by_ids() {
    // Oracle: RFC 8620 §5.1 request `ids` and response `accountId`,
    // `state`, `list`, `notFound`; draft L180-192 KeyPackage fields.
    let server = serve(
        "KeyPackage/get",
        json!({
            "accountId": "u1",
            "state": "s8",
            "list": [{
                "id": "kp42",
                "keyPackage": KP,
                "keyPackageRef": KP_REF,
                "lastResort": false
            }],
            "notFound": ["kp9"]
        }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .key_package_get(Some(&[Id::from("kp42"), Id::from("kp9")]))
        .await
        .expect("KeyPackage/get");
    assert_eq!(resp.state.as_ref(), "s8");
    assert_eq!(resp.list.len(), 1);
    assert_eq!(resp.list[0].key_package_ref.as_str(), KP_REF);
    assert!(!resp.list[0].last_resort);
    assert_eq!(resp.not_found[0].as_ref(), "kp9");

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["KeyPackage/get", { "accountId": "u1", "ids": ["kp42", "kp9"] }, "r1"])
    );
}

#[tokio::test]
async fn key_package_get_all_omits_ids() {
    // Oracle: RFC 8620 §5.1: "If null, then all records of the data type
    // are returned"; an absent `ids` is null.
    let server = serve(
        "KeyPackage/get",
        json!({ "accountId": "u1", "state": "s8", "list": [], "notFound": [] }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .key_package_get(None)
        .await
        .expect("KeyPackage/get");
    assert!(resp.list.is_empty());

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(call, json!(["KeyPackage/get", { "accountId": "u1" }, "r1"]));
}

#[tokio::test]
async fn key_material_fetch_matches_draft_example() {
    // Oracle: draft L474-499, "Alice's client fetches KeyPackages for Bob,
    // who has two clients". "AAEYbWltaTov" and "AAEAAkBBy2c" are the
    // example prefixes with "..." dropped.
    let server = serve(
        "KeyMaterial/fetch",
        json!({
            "accountId": "u1",
            "userStatus": "partialSuccess",
            "userUri": "mimi://b.example/u/bob",
            "clients": [
                { "clientStatus": "success",
                  "clientUri": "mimi://b.example/d/ClientB1",
                  "keyPackage": "AAEAAkBBy2c" },
                { "clientStatus": "keyMaterialExhausted",
                  "clientUri": "mimi://b.example/d/ClientB2" }
            ]
        }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .key_material_fetch(ROOM, "AAEYbWltaTov".parse().unwrap())
        .await
        .expect("KeyMaterial/fetch");
    assert_eq!(resp.user_status, KeyMaterialUserCode::PartialSuccess);
    assert_eq!(resp.user_uri, "mimi://b.example/u/bob");
    assert_eq!(resp.clients.len(), 2);
    assert_eq!(
        resp.clients[0].client_status,
        KeyMaterialClientCode::Success
    );
    assert_eq!(
        resp.clients[0].key_package.as_ref().map(|k| k.as_str()),
        Some("AAEAAkBBy2c")
    );
    assert_eq!(
        resp.clients[1].client_status,
        KeyMaterialClientCode::KeyMaterialExhausted
    );
    assert_eq!(resp.clients[1].key_package, None);

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["KeyMaterial/fetch", {
            "accountId": "u1",
            "roomId": ROOM,
            "request": "AAEYbWltaTov"
        }, "r1"])
    );
}

#[tokio::test]
async fn key_material_fetch_server_unavailable_is_a_method_error() {
    // Oracle: draft L231: "If the provider it asks cannot be reached, the
    // server returns `serverUnavailable`", an RFC 8620 §3.6.2 method-level
    // error.
    let server = serve("error", json!({ "type": "serverUnavailable" })).await;
    let err = helpers::make_client(&server, true)
        .key_material_fetch(ROOM, "AAEYbWltaTov".parse().unwrap())
        .await
        .expect_err("serverUnavailable");
    assert!(
        matches!(
            &err,
            jmap_base_client::ClientError::MethodError(e)
                if e.error_type.to_string() == "serverUnavailable"
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn key_package_methods_need_mimi_account() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server, false);
    let err = sc.key_package_get(None).await.expect_err("no MIMI account");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidSession(_)),
        "{err:?}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}
