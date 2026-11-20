//! MiPush delivery against a local stub.
//!
//! Oracles: jmap-push-platform-guide §Xiaomi Push (MiPush);
//! jmap-chat-push-platform-guide §Payload encoding, MiPush; Xiaomi's
//! "Server API Address" (pId=1696: `Authorization: key=`, Table 1-1
//! parameters, the response format) and the Java server SDK guide
//! (pId=1695) §4.7 Message receipt.

mod common;

use common::{form_value, header, state_change};
use jmap_platform_push::mipush::{MiPusher, invalid_reg_ids};
use jmap_platform_push::url::Url;
use jmap_platform_push::{Outcome, PlatformPusher, PushError};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pusher(server: &MockServer) -> MiPusher {
    MiPusher::new("dummy-app-secret", "com.example.app")
        .unwrap()
        .with_endpoint(&Url::parse(&server.uri()).unwrap())
}

async fn reply(server: &MockServer, status: u16, body: Value) {
    Mock::given(method("POST"))
        .and(path("/v3/message/regid"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

/// Request: `Authorization: key=<secret>`, form-encoded
/// `registration_id`, `restricted_package_name`, `payload` (the JSON,
/// URL-encoded), `pass_through=1`, `time_to_live` in milliseconds.
/// Response: `"result": "ok"`, `data.id`.
#[tokio::test]
async fn pass_through_message_is_form_encoded() {
    let server = MockServer::start().await;
    reply(
        &server,
        200,
        json!({
            "result": "ok", "trace_id": "Xcm55b35473343960213KW", "code": 0,
            "data": { "id": "scm55b35473343960213KW" }, "description": "Success", "info": "Received push messages for 1 REGID"
        }),
    )
    .await;

    let outcome = pusher(&server).push("reg-id-1", &state_change()).await;
    let Outcome::Delivered(receipt) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(
        receipt.message_id.as_deref(),
        Some("scm55b35473343960213KW")
    );

    let req = &server.received_requests().await.unwrap()[0];
    assert_eq!(header(req, "authorization"), Some("key=dummy-app-secret"));
    assert_eq!(
        header(req, "content-type"),
        Some("application/x-www-form-urlencoded")
    );
    let raw = std::str::from_utf8(&req.body).unwrap();
    assert!(
        raw.contains("payload=%7B%22%40type%22%3A%22StateChange%22"),
        "{raw}"
    );
    assert_eq!(
        form_value(req, "registration_id").as_deref(),
        Some("reg-id-1")
    );
    assert_eq!(
        form_value(req, "restricted_package_name").as_deref(),
        Some("com.example.app")
    );
    assert_eq!(form_value(req, "pass_through").as_deref(), Some("1"));
    assert_eq!(form_value(req, "time_to_live").as_deref(), Some("3600000"));
    let payload: Value = serde_json::from_str(&form_value(req, "payload").unwrap()).unwrap();
    assert_eq!(
        payload,
        json!({ "@type": "StateChange", "changed": { "u1": { "Email": "d35ecb040aab" } } })
    );
    assert_eq!(form_value(req, "extra.callback"), None);
}

/// §4.7: `extra.callback` is the receipt URL and `extra.callback.type` 16
/// asks for invalid-target receipts.
#[tokio::test]
async fn callback_asks_for_invalid_target_receipts() {
    let server = MockServer::start().await;
    reply(
        &server,
        200,
        json!({ "result": "ok", "code": 0, "data": { "id": "m" } }),
    )
    .await;
    let pusher =
        pusher(&server).with_callback(Url::parse("https://jmap.example.com/mipush").unwrap());
    assert!(matches!(
        pusher.push("r", &state_change()).await,
        Outcome::Delivered(_)
    ));
    let req = &server.received_requests().await.unwrap()[0];
    assert_eq!(
        form_value(req, "extra.callback").as_deref(),
        Some("https://jmap.example.com/mipush")
    );
    assert_eq!(
        form_value(req, "extra.callback.type").as_deref(),
        Some("16")
    );
}

/// "Failed responses include `"result": "error"`, a `"reason"`,
/// `"description"`, and a non-zero `"code"`" (the pId=1696 example is code
/// 10016, missing parameter). A failed send never destroys; HTTP 503 is
/// retried.
#[tokio::test]
async fn error_result_fails_and_never_destroys() {
    let server = MockServer::start().await;
    reply(
        &server,
        200,
        json!({ "result": "error", "reason": "No valid targets!", "code": 10016, "description": "missing required parameter" }),
    )
    .await;
    let outcome = pusher(&server).push("r", &state_change()).await;
    let Outcome::Failed(PushError::Platform(e)) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(e.reason.as_deref(), Some("10016"));
    assert_eq!(e.description.as_deref(), Some("No valid targets!"));

    let server = MockServer::start().await;
    reply(&server, 503, json!({ "result": "error", "code": 1 })).await;
    assert!(matches!(
        pusher(&server).push("r", &state_change()).await,
        Outcome::Retry { .. }
    ));
}

/// §4.7 receipt format: message ID to `{type, targets, errorCode}`; type
/// 16 with `errorCode` 1 is "invalid RegID", 2 "invalid alias"; type 1 is
/// a delivery receipt.
#[test]
fn invalid_reg_ids_come_from_type_16_receipts() {
    let data = r#"{
        "msgId1": {"param": "param", "type": 1, "targets": "alias1,alias2", "jobkey": "123", "barStatus": "Enable", "timestamp": 1585142800000},
        "msgId2": {"param": "param", "type": 16, "targets": "regId1,regId2", "jobkey": "123", "errorCode": 1},
        "msgId3": {"param": "param", "type": 16, "targets": "alias3", "jobkey": "123", "errorCode": 2}
    }"#;
    assert_eq!(invalid_reg_ids(data).unwrap(), ["regId1", "regId2"]);
    assert!(invalid_reg_ids("not json").is_err());
}

#[test]
fn debug_hides_the_app_secret() {
    let p = MiPusher::new("dummy-app-secret", "com.example.app").unwrap();
    let shown = format!("{p:?}");
    assert!(shown.contains("com.example.app"));
    assert!(!shown.contains("dummy-app-secret"));
}
