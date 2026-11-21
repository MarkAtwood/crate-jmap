//! RFC 8620 (JMAP Core) facts checked with `jmap-base-client` against the
//! configured server. Run with `-- --ignored`; see docs/interop/README.md.

use jmap_base_client::{ClientError, JmapClient, JmapRequestBuilder, Session, extract_response};
use jmap_interop::{InteropEnv, known_divergence};
use jmap_types::{JmapResponse, MethodErrorType};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const SUBMISSION: &str = "urn:ietf:params:jmap:submission";

async fn alice() -> (JmapClient, Session) {
    let env = InteropEnv::require();
    let client = env.client(&env.user).expect("client");
    let session = client.fetch_session().await.expect("Session");
    (client, session)
}

/// Send `calls` (method, arguments, call id) with `using` and return the
/// Response.
async fn send(
    client: &JmapClient,
    session: &Session,
    using: &[&str],
    calls: &[(&str, Value, &str)],
) -> JmapResponse {
    let mut req = JmapRequestBuilder::new(using);
    for (method, args, id) in calls {
        req.add_call(*method, args.clone(), *id).expect("add_call");
    }
    client
        .call_session(session, &req.build().expect("build"))
        .await
        .expect("API request")
}

fn method_error(resp: &JmapResponse, call_id: &str) -> MethodErrorType {
    match extract_response::<Value>(resp, call_id) {
        Err(ClientError::MethodError(e)) => e.error_type,
        other => panic!("expected a method-level error for {call_id}, got {other:?}"),
    }
}

/// RFC 8620 §2: the Session object's required members, and the primary
/// mail account listed in `accounts` with the mail capability.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn session_discovery() {
    let _server = jmap_interop::exclusive().await;
    let env = InteropEnv::require();
    let (_, session) = alice().await;

    let core = session
        .capabilities
        .get(CORE)
        .expect("RFC 8620 §2: capabilities MUST include urn:ietf:params:jmap:core");
    // RFC 8620 §2: the core capability's limits are UnsignedInt, the
    // collations a String[].
    for key in [
        "maxSizeUpload",
        "maxConcurrentUpload",
        "maxSizeRequest",
        "maxConcurrentRequests",
        "maxCallsInRequest",
        "maxObjectsInGet",
        "maxObjectsInSet",
    ] {
        assert!(core[key].is_u64(), "core capability {key} = {}", core[key]);
    }
    assert!(core["collationAlgorithms"].is_array());

    assert_eq!(session.username.expose_unredacted(), env.user.username);
    let mail_account = session
        .primary_account_id(MAIL)
        .expect("a mail server names a primary mail account");
    let account = session
        .accounts
        .get(mail_account)
        .expect("RFC 8620 §2: primaryAccounts values are ids in accounts");
    assert!(
        account.account_capabilities.contains_key(MAIL),
        "RFC 8621 §1.3.1: the mail account's accountCapabilities has urn:ietf:params:jmap:mail"
    );
    assert!(
        session.capabilities.contains_key(MAIL),
        "RFC 8621 §1.3.1: the Session capabilities list urn:ietf:params:jmap:mail"
    );
}

/// RFC 8620 §2 and §3.1: the Session resource and the API need valid
/// credentials; a wrong password is answered 401.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn wrong_password_is_refused() {
    let _server = jmap_interop::exclusive().await;
    let env = InteropEnv::require();
    let client = env.client(&env.wrong_password()).expect("client");
    match client.fetch_session().await {
        Err(ClientError::AuthFailed(401)) => {}
        other => panic!("expected AuthFailed(401), got {other:?}"),
    }
}

/// RFC 8620 §4: `Core/echo` returns its arguments unchanged.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn core_echo() {
    let _server = jmap_interop::exclusive().await;
    let (client, session) = alice().await;
    let args = json!({"hello": true, "high": 5, "list": ["a", {"b": null}]});
    let resp = send(
        &client,
        &session,
        &[CORE],
        &[("Core/echo", args.clone(), "e")],
    )
    .await;
    let inv = &resp.method_responses[0];
    assert_eq!(inv.name, "Core/echo");
    assert_eq!(inv.call_id, "e");
    assert_eq!(inv.arguments, args);
}

/// RFC 8620 §3.4: one response per call, in order, with the call ids
/// echoed; §3.6.2: an unknown method name is an `unknownMethod` error and
/// does not stop later calls.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn unknown_method_is_a_method_error() {
    let _server = jmap_interop::exclusive().await;
    let (client, session) = alice().await;
    let resp = send(
        &client,
        &session,
        &[CORE],
        &[
            ("Core/notAMethod", json!({}), "bad"),
            ("Core/echo", json!({"after": 1}), "after"),
        ],
    )
    .await;
    assert_eq!(resp.method_responses.len(), 2);
    assert_eq!(resp.method_responses[0].name, "error");
    assert_eq!(method_error(&resp, "bad"), MethodErrorType::UnknownMethod);
    assert_eq!(resp.method_responses[1].call_id, "after");
    assert_eq!(resp.method_responses[1].arguments, json!({"after": 1}));
}

/// RFC 8620 §3.6.2: a method on an account id the user does not have is
/// `accountNotFound`.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn unknown_account_is_account_not_found() {
    let _server = jmap_interop::exclusive().await;
    let (client, session) = alice().await;
    let resp = send(
        &client,
        &session,
        &[CORE, MAIL],
        &[(
            "Mailbox/get",
            json!({"accountId": "no-such-account-zz9", "ids": []}),
            "m",
        )],
    )
    .await;
    assert_eq!(method_error(&resp, "m"), MethodErrorType::AccountNotFound);
}

/// RFC 8620 §3.7: a result reference to a call id that is not in the
/// request is `invalidResultReference`.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn dangling_result_reference_is_invalid() {
    let _server = jmap_interop::exclusive().await;
    let (client, session) = alice().await;
    let account = session.primary_account_id(MAIL).expect("mail account");
    let resp = send(
        &client,
        &session,
        &[CORE, MAIL],
        &[(
            "Mailbox/get",
            json!({
                "accountId": account,
                "#ids": {"resultOf": "nope", "name": "Mailbox/query", "path": "/ids"},
            }),
            "m",
        )],
    )
    .await;
    assert_eq!(
        method_error(&resp, "m"),
        MethodErrorType::InvalidResultReference
    );
}

/// RFC 8620 §3.7: a result reference resolves against an earlier call's
/// response.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn result_reference_resolves() {
    let _server = jmap_interop::exclusive().await;
    let (client, session) = alice().await;
    let account = session.primary_account_id(MAIL).expect("mail account");
    let resp = send(
        &client,
        &session,
        &[CORE, MAIL],
        &[
            ("Mailbox/query", json!({"accountId": account}), "q"),
            (
                "Mailbox/get",
                json!({
                    "accountId": account,
                    "#ids": {"resultOf": "q", "name": "Mailbox/query", "path": "/ids"},
                    "properties": ["id"],
                }),
                "g",
            ),
        ],
    )
    .await;
    let query: Value = extract_response(&resp, "q").expect("Mailbox/query");
    let get: Value = extract_response(&resp, "g").expect("Mailbox/get");
    let mut queried: Vec<&str> = query["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().expect("id"))
        .collect();
    let mut got: Vec<&str> = get["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|m| m["id"].as_str().expect("id"))
        .collect();
    queried.sort_unstable();
    got.sort_unstable();
    assert!(!queried.is_empty(), "the account has mailboxes");
    assert_eq!(queried, got);
}

/// RFC 8620 §3.6.1: a `using` capability the server does not support is
/// the request-level `unknownCapability` error, an RFC 7807 problem
/// details response with HTTP status 400.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn unknown_capability_is_a_request_error() {
    let _server = jmap_interop::exclusive().await;
    let (client, session) = alice().await;
    let mut req = JmapRequestBuilder::new(&[CORE, "urn:example:not-a-capability"]);
    req.add_call("Core/echo", json!({}), "e").expect("add_call");
    match client
        .call_session(&session, &req.build().expect("build"))
        .await
    {
        Err(ClientError::Problem { status, details }) => {
            assert_eq!(status, 400);
            let error_type = details
                .jmap_error_type()
                .unwrap_or_else(|| panic!("not a JMAP problem type: {}", details.problem_type));
            if error_type != MethodErrorType::UnknownCapability {
                known_divergence(&format!(
                    "RFC 8620 §3.6.1: an unsupported `using` capability is \
                     unknownCapability, the server answered {}",
                    details.problem_type
                ));
            }
        }
        other => panic!("expected an unknownCapability problem, got {other:?}"),
    }
}

/// RFC 8620 §5.3: a `/set` response carries `oldState` (`String|null`)
/// and `newState` (`String`), also when nothing changed, and a destroy of
/// an unknown id is `notFound` in `notDestroyed`.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn set_responses_carry_states() {
    let _server = jmap_interop::exclusive().await;
    let (client, session) = alice().await;
    let account = session.primary_account_id(MAIL).expect("mail account");
    let destroy_none = json!({"accountId": account, "destroy": ["zz9-none"]});
    let resp = send(
        &client,
        &session,
        &[CORE, MAIL, SUBMISSION],
        &[
            ("Mailbox/set", destroy_none.clone(), "m"),
            ("Identity/set", destroy_none.clone(), "i"),
            ("EmailSubmission/set", destroy_none, "s"),
        ],
    )
    .await;
    for call_id in ["m", "i", "s"] {
        let inv = resp
            .method_responses
            .iter()
            .find(|inv| inv.call_id == call_id)
            .expect("a response per call");
        let args = &inv.arguments;
        assert_eq!(
            args["notDestroyed"]["zz9-none"]["type"], "notFound",
            "{}: {args}",
            inv.name
        );
        if !args["newState"].is_string() {
            known_divergence(&format!(
                "RFC 8620 §5.3: {} response has no newState: {args}",
                inv.name
            ));
        }
        let old_state_ok = args
            .get("oldState")
            .is_some_and(|s| s.is_string() || s.is_null());
        if !old_state_ok {
            known_divergence(&format!(
                "RFC 8620 §5.3: {} response has no oldState (String|null): {args}",
                inv.name
            ));
        }
    }
}
