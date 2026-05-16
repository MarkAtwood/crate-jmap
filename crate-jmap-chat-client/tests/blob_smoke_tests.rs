//! Wiremock smoke tests for `Blob/*` method paths in jmap-chat-client.
//!
//! The Blob extension is cross-protocol JMAP under
//! `urn:ietf:params:jmap:blob2` — distinct from
//! `urn:ietf:params:jmap:chat` — so the wire `using` array MUST switch
//! to USING_BLOB (core + blob2) when these methods are called.
//!
//! `blob_convert` follows the standard RFC 8620 §5.3 `/set`-shaped
//! `create` pattern keyed by the CALL_ID constant
//! ([`jmap_chat_client::methods::CALL_ID`] = "r1"); the resulting
//! `BlobObject` lands at `response.created[CALL_ID]`.
//!
//! Spec oracles:
//!   - draft-ietf-jmap-blobext-01 §4 (BlobObject), §6 (Blob/lookup),
//!     §8 (Blob/convert), §8.1 (ImageConvertRecipe)

#[path = "helpers.rs"]
mod helpers;

use helpers::{jmap_response, mock_jmap_post, recorded_args, recorded_body, TEST_ACCOUNT_ID};
use jmap_chat_client::methods::CALL_ID;
use jmap_types::Id;
use serde_json::json;
use wiremock::MockServer;

/// `Blob/lookup` with non-empty `blob_ids` and explicit `type_names`
/// must thread both through the wire (`ids` and `typeNames` keys), and
/// MUST declare the blob2 capability — NOT the chat capability — in
/// `using`. Pins USING_BLOB for the Blob/* family (one assertion per
/// method-family per workspace convention).
#[tokio::test]
async fn blob_lookup_threads_ids_and_declares_blob2_capability() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Blob/lookup",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "list": [
                {
                    "id": "B1",
                    "matchedIds": {
                        "Message": ["msg-100", "msg-101"]
                    }
                },
                {
                    "id": "B2",
                    "matchedIds": {}
                }
            ],
            "notFound": ["B3"]
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let blob_ids = [Id::from("B1"), Id::from("B2"), Id::from("B3")];
    let type_names = ["Message"];
    let resp = sc
        .blob_lookup(&blob_ids, Some(&type_names))
        .await
        .expect("blob_lookup: must succeed");

    assert_eq!(
        resp.account_id.as_ref(),
        TEST_ACCOUNT_ID,
        "accountId mismatch"
    );
    assert_eq!(resp.list.len(), 2, "list must have 2 entries");
    assert_eq!(resp.not_found.len(), 1, "not_found must have 1 entry");
    assert_eq!(
        resp.not_found[0].as_ref(),
        "B3",
        "not_found content mismatch"
    );
    let entry_b1 = &resp.list[0];
    assert_eq!(entry_b1.id.as_ref(), "B1", "B1 id mismatch");
    let matched = entry_b1
        .matched_ids
        .get("Message")
        .expect("Message key must be present");
    assert_eq!(matched.len(), 2, "B1 matched Messages count");
    assert_eq!(matched[0].as_ref(), "msg-100", "matched[0]");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
    );
    assert_eq!(
        args["ids"],
        json!(["B1", "B2", "B3"]),
        "ids must thread verbatim"
    );
    assert_eq!(
        args["typeNames"],
        json!(["Message"]),
        "typeNames must thread verbatim"
    );
    // RFC 8620 §3.3 + draft-ietf-jmap-blobext-01 — declare blob2, NOT
    // chat. A regression that mistakenly used USING_CHAT here would
    // produce 'unknown method' against a spec-compliant server.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:blob2"]),
        "Blob/* using must equal USING_BLOB exactly (core + blob2)"
    );
}

/// `Blob/lookup` with `type_names: None` must emit `typeNames: null`
/// on the wire so the server queries all registered types
/// (draft-ietf-jmap-blobext-01 §6 — null is the canonical "all types"
/// form). The empty-ids guard is the only client-side validation
/// (blob.rs:152-155).
#[tokio::test]
async fn blob_lookup_type_names_none_sends_null_and_empty_ids_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Blob/lookup",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "list": [],
            "notFound": ["B-missing"]
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let blob_ids = [Id::from("B-missing")];
    let _ = sc
        .blob_lookup(&blob_ids, None)
        .await
        .expect("blob_lookup: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["typeNames"],
        json!(null),
        "typeNames must be JSON null when caller passes None"
    );

    // Empty-ids guard.
    let empty: [Id; 0] = [];
    let err = sc
        .blob_lookup(&empty, None)
        .await
        .expect_err("must reject empty blob_ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("blob_ids may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
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
    let resp_body = jmap_response(
        "Blob/convert",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "created": {
                "r1": {
                    "id": "B-thumb",
                    "type": "image/webp",
                    "size": 4096
                }
            },
            "notCreated": {}
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let from_blob = Id::from("B-original");
    let resp = sc
        .blob_convert(&from_blob, "image/webp", Some(200), Some(150))
        .await
        .expect("blob_convert: must succeed");

    // Result is keyed by CALL_ID per the method's contract.
    let created = resp.created.expect("created must be present");
    let new_blob = created
        .get(CALL_ID)
        .expect("CALL_ID-keyed entry must exist");
    assert_eq!(
        new_blob.id.as_ref(),
        "B-thumb",
        "converted blob id mismatch"
    );
    assert_eq!(
        new_blob.content_type.as_deref(),
        Some("image/webp"),
        "content_type mismatch"
    );
    assert_eq!(new_blob.size, Some(4096), "size mismatch");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
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

    // RFC 8620 §3.3 + draft-ietf-jmap-blobext-01 §8 — declare blob2.
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
    let resp_body = jmap_response(
        "Blob/convert",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "created": {
                "r1": {
                    "id": "B-converted",
                    "type": "image/png",
                    "size": null
                }
            },
            "notCreated": {}
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let from_blob = Id::from("B-original");
    let _ = sc
        .blob_convert(&from_blob, "image/png", None, None)
        .await
        .expect("blob_convert: must succeed");

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
