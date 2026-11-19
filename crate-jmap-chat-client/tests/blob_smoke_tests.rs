//! Wiremock smoke tests for `Blob/*` method paths in jmap-chat-client.
//!
//! The Blob extension is cross-protocol JMAP under
//! `urn:ietf:params:jmap:blob2`, with its own primary account. Both calls
//! name blob2 and target that account (here [`BLOB_ACCOUNT`], not the chat
//! account); Blob/lookup also names the chat capability so `"Message"` is a
//! known type name (blobext-03 §6 `unknownDataType`).
//!
//! `blob_convert` follows the standard RFC 8620 §5.3 `/set`-shaped
//! `create` pattern keyed by the CALL_ID constant
//! ([`jmap_chat_client::CALL_ID`] = "r1"); the resulting
//! `BlobObject` lands at `response.created[CALL_ID]`.
//!
//! Spec oracles:
//!   - draft-ietf-jmap-blobext-03 §2.1 (the blob2 primary account), §4
//!     (BlobObject), §6 (Blob/lookup: required `typeNames: String[]`, no
//!     `notFound`), §7 (`expires`), §8 (Blob/convert, `isIncomplete`,
//!     `description`), §8.1 (ImageConvertRecipe)

#[path = "helpers.rs"]
mod helpers;

use helpers::{TEST_ACCOUNT_ID, mock_jmap_post, recorded_args, recorded_body};
use jmap_chat_client::CALL_ID;
use jmap_types::Id;
use serde_json::json;
use wiremock::MockServer;

/// The blob2 primary account, distinct from the chat one.
const BLOB_ACCOUNT: &str = "Ablob";

/// A one-call response with call id "0", the id jmap-blob-client sends.
fn blob_response(method: &str, args: serde_json::Value) -> serde_json::Value {
    json!({"sessionState": "s1", "methodResponses": [[method, args, "0"]]})
}

/// [`helpers::make_session`]'s shape plus a blob2 capability whose
/// primary account is [`BLOB_ACCOUNT`] (RFC 8620 §2; blobext-03 §2.1).
fn make_blob_client(server: &MockServer) -> jmap_chat_client::SessionClient {
    use jmap_chat_client::JmapChatExt;
    let uri = server.uri();
    let session = json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:chat": {},
            "urn:ietf:params:jmap:blob2": {}
        },
        "accounts": {
            TEST_ACCOUNT_ID: {
                "name": "john@example.com", "isPersonal": true, "isReadOnly": false,
                "accountCapabilities": {"urn:ietf:params:jmap:chat": {}}
            },
            BLOB_ACCOUNT: {
                "name": "blobs", "isPersonal": true, "isReadOnly": false,
                "accountCapabilities": {"urn:ietf:params:jmap:blob2": {
                    "maxSizeBlobSet": null, "maxDataSources": 64,
                    "supportedTypeNames": ["Message"],
                    "supportedDigestAlgorithms": ["sha-256"]
                }}
            }
        },
        "primaryAccounts": {
            "urn:ietf:params:jmap:chat": TEST_ACCOUNT_ID,
            "urn:ietf:params:jmap:blob2": BLOB_ACCOUNT
        },
        "username": "john@example.com",
        "apiUrl": format!("{uri}/api/"),
        "downloadUrl": format!("{uri}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
        "uploadUrl": format!("{uri}/ul/{{accountId}}/"),
        "eventSourceUrl": format!("{uri}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
        "state": "s1"
    });
    jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &uri,
        jmap_base_client::ClientConfig::default(),
    )
    .expect("client")
    .with_chat_session(serde_json::from_value(session).expect("session"))
}

/// `Blob/lookup` threads `ids` and `typeNames`, targets the blob2 primary
/// account, and names core, blob2 and chat in `using`. A `notFound` in the
/// response is not part of -03 §6 and lands in `extra`.
#[tokio::test]
async fn blob_lookup_threads_ids_and_declares_blob2_and_chat() {
    let server = MockServer::start().await;
    let resp_body = blob_response(
        "Blob/lookup",
        json!({
            "accountId": BLOB_ACCOUNT,
            "list": [
                {"id": "B1", "matchedIds": {"Message": ["msg-100", "msg-101"]}},
                {"id": "B2", "matchedIds": {}}
            ],
            "notFound": ["B3"]
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = make_blob_client(&server);
    let blob_ids = [Id::from("B1"), Id::from("B2")];
    let resp = sc
        .blob_lookup(&blob_ids, &["Message"])
        .await
        .expect("blob_lookup: must succeed");

    assert_eq!(resp.account_id.as_ref(), BLOB_ACCOUNT);
    assert_eq!(resp.list.len(), 2, "list must have 2 entries");
    let matched = resp.list[0]
        .matched_ids
        .get("Message")
        .expect("Message key must be present");
    assert_eq!(matched[0].as_ref(), "msg-100", "matched[0]");
    assert_eq!(resp.extra.get("notFound"), Some(&json!(["B3"])));

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(BLOB_ACCOUNT),
        "blob2 primary account"
    );
    assert_ne!(BLOB_ACCOUNT, TEST_ACCOUNT_ID);
    assert_eq!(args["ids"], json!(["B1", "B2"]));
    assert_eq!(args["typeNames"], json!(["Message"]));
    assert_eq!(
        body["using"],
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:blob2",
            "urn:ietf:params:jmap:chat"
        ])
    );
}

/// -03 §6 makes `typeNames` a required `String[]`, sent as an array. Empty
/// `blob_ids` or `type_names` is rejected before sending (a lookup of no
/// blobs or no types matches nothing), and a session with no blob2
/// primary account is `InvalidSession`.
#[tokio::test]
async fn blob_lookup_type_names_is_always_an_array_and_guards_hold() {
    let server = MockServer::start().await;
    let resp_body = blob_response(
        "Blob/lookup",
        json!({"accountId": BLOB_ACCOUNT, "list": [{"id": "B1", "matchedIds": {}}]}),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = make_blob_client(&server);
    let _ = sc
        .blob_lookup(&[Id::from("B1")], &["Message"])
        .await
        .expect("blob_lookup: must succeed");
    let args = recorded_args(&server).await;
    assert_eq!(args["typeNames"], json!(["Message"]));

    let empty: [Id; 0] = [];
    for result in [
        sc.blob_lookup(&empty, &["Message"]).await,
        sc.blob_lookup(&[Id::from("B1")], &[]).await,
    ] {
        match result {
            Err(jmap_base_client::ClientError::InvalidArgument(msg)) => {
                assert!(msg.contains("must not be empty"), "got: {msg:?}");
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }

    // helpers::make_client's session has no blob2 primary account.
    let chat_only = helpers::make_client(&server);
    assert!(matches!(
        chat_only.blob_lookup(&[Id::from("B1")], &["Message"]).await,
        Err(jmap_base_client::ClientError::InvalidSession(_))
    ));
}

/// `Blob/convert` must serialise an `ImageConvertRecipe` keyed by the
/// CALL_ID constant in the `create` map (blob.rs:194-213, draft §8.1).
/// The recipe includes `blobId`, `type`, and optional `width` /
/// `height`. On success the converted BlobObject lands at
/// `response.created[CALL_ID]`. Also pins USING_BLOB on the wire
/// (blob_convert must NOT use USING_CHAT).
#[tokio::test]
async fn blob_convert_serialises_recipe_and_decodes_blob_object() {
    let server = MockServer::start().await;
    let resp_body = blob_response(
        "Blob/convert",
        json!({
            "accountId": BLOB_ACCOUNT,
            "created": {
                "r1": {
                    "id": "B-thumb",
                    "type": "image/webp",
                    "size": 4096,
                    "expires": "2026-11-17T12:00:00Z"
                }
            },
            "notCreated": {}
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = make_blob_client(&server);
    let from_blob = Id::from("B-original");
    let resp = sc
        .blob_convert(&from_blob, "image/webp", Some(200), Some(150))
        .await
        .expect("blob_convert: must succeed");

    // Result is keyed by CALL_ID per the method's contract.
    let new_blob = resp
        .created
        .get(CALL_ID)
        .expect("CALL_ID-keyed entry must exist");
    assert_eq!(
        new_blob.id.as_ref(),
        "B-thumb",
        "converted blob id mismatch"
    );
    assert_eq!(
        new_blob.media_type.as_deref(),
        Some("image/webp"),
        "content_type mismatch"
    );
    assert_eq!(new_blob.size, Some(4096), "size mismatch");
    assert_eq!(
        new_blob
            .expires
            .clone()
            .flatten()
            .as_ref()
            .map(AsRef::as_ref),
        Some("2026-11-17T12:00:00Z")
    );
    assert!(!new_blob.is_incomplete);

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(BLOB_ACCOUNT),
        "blob2 primary account"
    );
    // The create map is keyed by CALL_ID and carries an imageConvert
    // recipe.
    let recipe = &args["create"][CALL_ID]["imageConvert"];
    assert_eq!(
        recipe["blobId"],
        json!("B-original"),
        "imageConvert.blobId mismatch"
    );
    assert_eq!(
        recipe["type"],
        json!("image/webp"),
        "imageConvert.type mismatch"
    );
    assert_eq!(recipe["width"], json!(200), "imageConvert.width mismatch");
    assert_eq!(recipe["height"], json!(150), "imageConvert.height mismatch");

    // RFC 8620 §3.3 + draft-ietf-jmap-blobext-03 §8 — declare blob2.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:blob2"]),
        "Blob/convert using must equal USING_BLOB exactly"
    );
}

/// `Blob/convert` with `width: None` and `height: None` must omit both
/// keys from the recipe (the underlying ImageConvertRecipe carries
/// `skip_serializing_if = "Option::is_none"` per blob.rs:120-123).
/// Empty `content_type` must short-circuit client-side before send
/// (blob.rs:188-192).
#[tokio::test]
async fn blob_convert_omits_dimensions_when_none_and_rejects_empty_type() {
    let server = MockServer::start().await;
    let resp_body = blob_response(
        "Blob/convert",
        json!({
            "accountId": BLOB_ACCOUNT,
            "created": {
                "r1": {
                    "id": "B-converted",
                    "type": "image/png",
                    "size": null,
                    "expires": null,
                    "isIncomplete": true,
                    "description": "truncated source"
                }
            },
            "notCreated": {}
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = make_blob_client(&server);
    let from_blob = Id::from("B-original");
    let resp = sc
        .blob_convert(&from_blob, "image/png", None, None)
        .await
        .expect("blob_convert: must succeed");
    // -03 §4: `size: null` (deferred) parses; §7 `expires: null`; §8 the
    // partial-conversion fields.
    let obj = &resp.created[CALL_ID];
    assert_eq!(obj.size, None);
    assert_eq!(obj.expires, Some(None));
    assert!(obj.is_incomplete);
    assert_eq!(obj.description.as_deref(), Some("truncated source"));

    let args = recorded_args(&server).await;
    let recipe = &args["create"][CALL_ID]["imageConvert"];
    assert!(
        recipe.get("width").is_none(),
        "width must be omitted when None"
    );
    assert!(
        recipe.get("height").is_none(),
        "height must be omitted when None"
    );

    // Empty content_type guard.
    let err = sc
        .blob_convert(&from_blob, "", None, None)
        .await
        .expect_err("must reject empty content_type");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("content_type may not be empty"),
                "got: {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
