//! Wiremock smoke tests for `PushSubscription/*` method paths in
//! jmap-chat-client.
//!
//! PushSubscriptions are NOT account-scoped (RFC 8620 §7.2): no
//! `accountId` field appears in the wire arguments. The `using` array
//! depends on whether the call touches the JMAP Chat Push extension:
//! when it does, [`USING_CHAT_PUSH`] (`core` + `chat:push`); otherwise
//! [`USING_CORE`] (just `core`).
//!
//! Spec oracles:
//!   - RFC 8620 §7.2 (PushSubscription object, set semantics, push
//!     verification)
//!   - draft-atwood-jmap-chat-push-00 §3 (chatPush property on
//!     PushSubscription) and §4.1 (ChatPushConfig fields)

#[path = "helpers.rs"]
mod helpers;

use helpers::{jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response};
use jmap_types::{Id, UTCDate};
use serde_json::json;
use wiremock::MockServer;

/// `PushSubscription/set` create without `chat_push` MUST emit only
/// `core` in the `using` array (RFC 8620 §3.3 — capabilities only when
/// used), and MUST omit `accountId` from the args (RFC 8620 §7.2 —
/// push subscriptions are not account-scoped). The wire create object
/// carries the caller-supplied creation key and the required
/// `deviceClientId` + `url` fields.
#[tokio::test]
async fn push_subscription_create_without_chat_push_uses_core_only() {
    let server = MockServer::start().await;
    // PushSubscription/set response shape: created map keyed by creation
    // id. accountId may be null per RFC 8620 §7.2 (response type is
    // PushSubscriptionCreateResponse with Option<Id> account_id).
    let resp_body = jmap_response(
        "PushSubscription/set",
        json!({
            "accountId": null,
            "created": { "my-sub-1": { "id": "ps-server-1" } },
            "notCreated": null
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let input = jmap_chat_client::methods::PushSubscriptionCreateInput::new(
        "device-abc",
        "https://push.example.com/endpoint",
    )
    .with_client_id("my-sub-1");
    let resp = sc
        .push_subscription_create(&input)
        .await
        .expect("push_subscription_create: must succeed");
    let created = resp.created.expect("created must be present");
    assert!(
        created.contains_key("my-sub-1"),
        "created must contain my-sub-1"
    );

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    // Push subscriptions are not account-scoped: NO accountId on the
    // wire (RFC 8620 §7.2).
    assert!(
        args.get("accountId").is_none(),
        "accountId must be absent for PushSubscription methods"
    );
    let create = &args["create"]["my-sub-1"];
    assert_eq!(
        create["deviceClientId"],
        json!("device-abc"),
        "deviceClientId mismatch"
    );
    assert_eq!(
        create["url"],
        json!("https://push.example.com/endpoint"),
        "url mismatch"
    );
    assert!(
        create.get("chatPush").is_none(),
        "chatPush must be absent when input.chat_push is None"
    );
    // RFC 8620 §3.3 — declare only the capabilities actually used.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core"]),
        "using must contain only core (no chatPush extension declared)"
    );
}

/// `PushSubscription/set` create with `chat_push: Some(_)` MUST switch
/// the `using` array to include `urn:ietf:params:jmap:chat:push` and
/// serialise the per-account ChatPushConfig map under a `chatPush` key
/// on the create object (draft-atwood-jmap-chat-push-00 §3.1).
#[tokio::test]
async fn push_subscription_create_with_chat_push_declares_extension() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "PushSubscription/set",
        json!({
            "accountId": null,
            "created": { "my-sub-1": { "id": "ps-server-1" } },
            "notCreated": null
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let account_a = Id::from("acct-a");
    let mut cp_a = jmap_chat_types::ChatPushConfig::default();
    cp_a.kinds = Some(vec!["direct".into(), "group".into()]);
    let chat_push_entries = [(&account_a, cp_a)];
    let input = jmap_chat_client::methods::PushSubscriptionCreateInput::new(
        "device-abc",
        "https://push.example.com/endpoint",
    )
    .with_client_id("my-sub-1")
    .with_chat_push(&chat_push_entries);
    let _ = sc
        .push_subscription_create(&input)
        .await
        .expect("push_subscription_create: must succeed");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    let chat_push = &args["create"]["my-sub-1"]["chatPush"];
    assert!(chat_push.is_object(), "chatPush must be a map");
    assert_eq!(
        chat_push["acct-a"],
        json!({ "kinds": ["direct", "group"] }),
        "chatPush map must be keyed by accountId with ChatPushConfig values"
    );
    // RFC 8620 §3.3 — chat:push capability declared because the request
    // uses it.
    assert_eq!(
        body["using"],
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat:push"
        ]),
        "using must equal USING_CHAT_PUSH exactly when chat_push is set"
    );
}

/// `PushSubscription/set` create must reject an empty `device_client_id`
/// or empty `url` client-side, before any HTTP request
/// (misc.rs:205-214).
#[tokio::test]
async fn push_subscription_create_empty_device_id_or_url_rejected() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    // Empty device_client_id.
    let bad_device =
        jmap_chat_client::methods::PushSubscriptionCreateInput::new("", "https://x.example/p");
    let err = sc
        .push_subscription_create(&bad_device)
        .await
        .expect_err("must reject empty device_client_id");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("device_client_id may not be empty"),
                "got: {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    // Empty url.
    let bad_url = jmap_chat_client::methods::PushSubscriptionCreateInput::new("device-abc", "");
    let err = sc
        .push_subscription_create(&bad_url)
        .await
        .expect_err("must reject empty url");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("url may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    // No HTTP request must have been sent.
    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(reqs.is_empty(), "no HTTP request must be sent");
}

/// `PushSubscription/set` create with a duplicate `accountId` in the
/// `chat_push` slice must short-circuit with `InvalidArgument`
/// (misc.rs:380-398, the build_chat_push_map helper).
#[tokio::test]
async fn push_subscription_create_duplicate_chat_push_account_id_rejected() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let account_a = Id::from("acct-a");
    let cp_1 = jmap_chat_types::ChatPushConfig::default();
    let cp_2 = jmap_chat_types::ChatPushConfig::default();
    let chat_push = [(&account_a, cp_1), (&account_a, cp_2)];
    let input = jmap_chat_client::methods::PushSubscriptionCreateInput::new(
        "device-abc",
        "https://x.example/p",
    )
    .with_chat_push(&chat_push);
    let err = sc
        .push_subscription_create(&input)
        .await
        .expect_err("must reject duplicate accountId");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("duplicate accountId 'acct-a'"),
                "error must name the duplicate accountId: got {msg:?}"
            );
            assert!(
                msg.contains("push_subscription_create"),
                "error must name the context for diagnostics: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(reqs.is_empty(), "no HTTP request must be sent");
}

/// `PushSubscription/set` update with `verification_code` + `types`
/// `Patch::Set` must thread both through the per-id patch object
/// (misc.rs:282-306). The update path uses USING_CORE because
/// `chat_push` is left at `Patch::Keep` (RFC 8620 §3.3 — declare
/// chat:push only when actually used).
#[tokio::test]
async fn push_subscription_update_verification_and_types_serialises() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "PushSubscription/set",
        "ps-1",
        "ps-2",
        json!({ "updated": { "ps-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("ps-1");
    let types_filter: &[&str] = &["Message", "Chat"];
    let mut patch = jmap_chat_client::methods::PushSubscriptionPatch::default();
    patch.verification_code = Some("vc-secret-12345");
    patch.types = jmap_chat_client::methods::Patch::Set(types_filter);
    let _ = sc
        .push_subscription_update(&id, &patch)
        .await
        .expect("push_subscription_update: must succeed");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    // Not account-scoped.
    assert!(
        args.get("accountId").is_none(),
        "accountId must be absent for PushSubscription methods"
    );
    let patch_obj = &args["update"]["ps-1"];
    assert_eq!(
        patch_obj["verificationCode"],
        json!("vc-secret-12345"),
        "verificationCode mismatch"
    );
    assert_eq!(
        patch_obj["types"],
        json!(["Message", "Chat"]),
        "types must serialise as a wire array"
    );
    assert!(
        patch_obj.get("chatPush").is_none(),
        "chatPush must be absent when Patch::Keep (default)"
    );
    // USING_CORE only — chat:push not used by this patch.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core"]),
        "using must be USING_CORE only when chat_push is Patch::Keep"
    );
}

/// `PushSubscription/set` update with `types: Patch::Clear` must emit
/// `"types": null` so the server delivers all types (RFC 8620 §7.2),
/// and with `chat_push: Patch::Clear` must emit `"chatPush": null` so
/// the server removes all inline push config
/// (draft-atwood-jmap-chat-push-00 §3.1). Confirms the
/// USING_CHAT_PUSH switch happens for any non-`Keep` chat_push value
/// (including `Clear`).
#[tokio::test]
async fn push_subscription_update_clear_emits_nulls_and_declares_chat_push() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "PushSubscription/set",
        "ps-1",
        "ps-2",
        json!({ "updated": { "ps-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("ps-1");
    let mut patch = jmap_chat_client::methods::PushSubscriptionPatch::default();
    patch.types = jmap_chat_client::methods::Patch::Clear;
    patch.chat_push = jmap_chat_client::methods::Patch::Clear;
    let _ = sc
        .push_subscription_update(&id, &patch)
        .await
        .expect("push_subscription_update: must succeed");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    let patch_obj = &args["update"]["ps-1"];
    assert_eq!(
        patch_obj["types"],
        json!(null),
        "Patch::Clear must serialise types as JSON null"
    );
    assert_eq!(
        patch_obj["chatPush"],
        json!(null),
        "Patch::Clear must serialise chatPush as JSON null"
    );
    // USING_CHAT_PUSH because chat_push is touched (even by Clear).
    assert_eq!(
        body["using"],
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat:push"
        ]),
        "using must equal USING_CHAT_PUSH when chat_push is touched"
    );
}

/// `PushSubscription/set` update with `expires: Patch::Set(_)` must
/// serialise the UTCDate verbatim under the camelCase `expires` key.
#[tokio::test]
async fn push_subscription_update_expires_set_emits_value() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "PushSubscription/set",
        "ps-1",
        "ps-2",
        json!({ "updated": { "ps-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("ps-1");
    let exp = UTCDate::from("2027-01-01T00:00:00Z");
    let mut patch = jmap_chat_client::methods::PushSubscriptionPatch::default();
    patch.expires = jmap_chat_client::methods::Patch::Set(&exp);
    let _ = sc
        .push_subscription_update(&id, &patch)
        .await
        .expect("push_subscription_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["ps-1"]["expires"],
        json!("2027-01-01T00:00:00Z"),
        "Patch::Set(&UTCDate) must serialise the wire string verbatim"
    );
}

/// `PushSubscription/set` update with an empty `id` must short-circuit
/// before send (misc.rs:275-279). Typed `&Id` does not statically
/// preclude an empty value.
#[tokio::test]
async fn push_subscription_update_empty_id_rejected() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);

    let empty_id = Id::from("");
    let patch = jmap_chat_client::methods::PushSubscriptionPatch::default();
    let err = sc
        .push_subscription_update(&empty_id, &patch)
        .await
        .expect_err("must reject empty id");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("id may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(reqs.is_empty(), "no HTTP request must be sent");
}

/// `PushSubscription/set` destroy must thread `ids` to the wire and
/// reject the empty slice client-side. Always USING_CORE — destroy is
/// property-blind so it never requires the chat:push capability
/// (misc.rs:336-338).
#[tokio::test]
async fn push_subscription_destroy_threads_ids_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "PushSubscription/set",
        "ps-1",
        "ps-2",
        json!({ "destroyed": ["ps-1"] }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("ps-1")];
    let _ = sc
        .push_subscription_destroy(&ids)
        .await
        .expect("push_subscription_destroy: must succeed");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("accountId").is_none(),
        "accountId must be absent for PushSubscription methods"
    );
    assert_eq!(args["destroy"], json!(["ps-1"]), "destroy must thread");
    // USING_CORE — destroy never requires chat:push.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core"]),
        "using must be USING_CORE only for destroy"
    );

    // Empty-slice guard.
    let empty: [Id; 0] = [];
    let err = sc
        .push_subscription_destroy(&empty)
        .await
        .expect_err("must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("ids may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
