//! Room/* and GroupInfo/fetch bindings against draft-atwood-jmap-chat-mimi-00
//! (jmap-chat-spec 6bcd9d8) §Examples, L501-589.
//!
//! Each test answers with the draft's example response and checks that the
//! request arguments equal the draft's example request. The examples
//! shorten payloads with "..."; these tests drop the "..." and keep the
//! prefixes, which are themselves valid unpadded base64url.

#[path = "helpers.rs"]
mod helpers;

use helpers::{assert_using, sent, serve};
use jmap_chat_mimi_types::{GroupInfoCode, SubmitResponseCode, UpdateResponseCode};
use serde_json::json;
use wiremock::MockServer;

const ROOM: &str = "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";

#[tokio::test]
async fn room_create_matches_draft_example() {
    // Oracle: draft L501-519, "Alice's client creates a room".
    let server = serve(
        "Room/create",
        json!({
            "accountId": "u1",
            "roomId": ROOM,
            "chatId": "01JA8Z3K6V2Q4R5S7T9W0XYZAB"
        }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .room_create("AAEABEAAEJr1".parse().unwrap(), "QAABAQ".parse().unwrap())
        .await
        .expect("Room/create");
    assert_eq!(resp.room_id, ROOM);
    assert_eq!(resp.chat_id.as_ref(), "01JA8Z3K6V2Q4R5S7T9W0XYZAB");

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["Room/create", {
            "accountId": "u1",
            "groupInfo": "AAEABEAAEJr1",
            "ratchetTree": "QAABAQ"
        }, "r1"])
    );
}

#[tokio::test]
async fn room_update_matches_draft_example() {
    // Oracle: draft L521-540, "Alice's client commits Bob's addition and
    // the hub accepts it".
    let server = serve(
        "Room/update",
        json!({
            "accountId": "u1",
            "responseCode": "success",
            "errorDescription": "",
            "acceptedAt": "2026-10-18T17:02:11.482Z"
        }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .room_update(ROOM, "AAEAAQAQmvVK".parse().unwrap())
        .await
        .expect("Room/update");
    assert_eq!(resp.response_code, UpdateResponseCode::Success);
    assert_eq!(resp.error_description, "");
    assert_eq!(
        resp.accepted_at.as_ref().map(|t| t.as_ref()),
        Some("2026-10-18T17:02:11.482Z")
    );

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["Room/update", {
            "accountId": "u1",
            "roomId": ROOM,
            "request": "AAEAAQAQmvVK"
        }, "r1"])
    );
}

#[tokio::test]
async fn room_send_matches_draft_example() {
    // Oracle: draft L562-580, "A client sends a message in a stale epoch".
    let server = serve(
        "Room/send",
        json!({
            "accountId": "u1",
            "responseCode": "epochTooOld",
            "currentEpoch": 4
        }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .room_send(ROOM, "AAEAAgAQmvVK".parse().unwrap())
        .await
        .expect("Room/send");
    assert_eq!(resp.response_code, SubmitResponseCode::EpochTooOld);
    assert_eq!(resp.current_epoch, Some(4));

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["Room/send", {
            "accountId": "u1",
            "roomId": ROOM,
            "message": "AAEAAgAQmvVK"
        }, "r1"])
    );
}

#[tokio::test]
async fn room_unbind_matches_draft_example() {
    // Oracle: draft L582-589, "Bob's client processes a commit that
    // removes Bob's last client from the room, and tells Bob's server",
    // and L388-389: the response is `accountId` only. The draft's example
    // uses account u2; this session's account is u1.
    let server = serve("Room/unbind", json!({ "accountId": "u1" })).await;
    let resp = helpers::make_client(&server, true)
        .room_unbind(ROOM)
        .await
        .expect("Room/unbind");
    assert_eq!(resp.account_id.as_ref(), "u1");

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["Room/unbind", { "accountId": "u1", "roomId": ROOM }, "r1"])
    );
}

#[tokio::test]
async fn group_info_fetch_matches_draft_example() {
    // Oracle: draft L542-560, "A new client of Alice fetches GroupInfo
    // before an external join".
    let server = serve(
        "GroupInfo/fetch",
        json!({
            "accountId": "u1",
            "status": "success",
            "response": "AAEbbWltaTov"
        }),
    )
    .await;
    let resp = helpers::make_client(&server, true)
        .group_info_fetch(ROOM, "AAEAAQAgsJ4f".parse().unwrap())
        .await
        .expect("GroupInfo/fetch");
    assert_eq!(resp.status, GroupInfoCode::Success);
    assert_eq!(resp.response.as_str(), "AAEbbWltaTov");

    let (using, call) = sent(&server).await;
    assert_using(&using);
    assert_eq!(
        call,
        json!(["GroupInfo/fetch", {
            "accountId": "u1",
            "roomId": ROOM,
            "request": "AAEAAQAgsJ4f"
        }, "r1"])
    );
}

#[tokio::test]
async fn invalid_arguments_is_a_method_error() {
    // Oracle: draft L386: "The server MUST reject the call with
    // `invalidArguments` if no Chat in the account has that `roomId`", an
    // RFC 8620 §3.6.2 method-level error.
    let server = serve("error", json!({ "type": "invalidArguments" })).await;
    let err = helpers::make_client(&server, true)
        .room_unbind(ROOM)
        .await
        .expect_err("invalidArguments");
    assert!(
        matches!(
            &err,
            jmap_base_client::ClientError::MethodError(e)
                if e.error_type.to_string() == "invalidArguments"
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn no_mimi_primary_account_sends_nothing() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server, false);
    let err = sc.room_unbind(ROOM).await.expect_err("no MIMI account");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidSession(_)),
        "{err:?}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn mimi_capability_reads_account_object() {
    // Oracle: draft L157-172, the account-level capability fields.
    let server = MockServer::start().await;
    let capability = helpers::make_client(&server, true)
        .mimi_capability()
        .expect("well-formed")
        .expect("present");
    assert_eq!(capability.user_uri, "mimi://a.example/u/alice");
    assert_eq!(capability.room_id_prefix, "mimi://a.example/r/");
    assert_eq!(capability.cipher_suites, [1]);
    assert_eq!(capability.max_key_packages, 100);
}
