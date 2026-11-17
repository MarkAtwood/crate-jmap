//! The object-history client against a mock server (bd JMAP-tna1.3.12):
//! each request body must equal the hand-written, spec-shaped request, and
//! the spec's responses must decode.
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3.2.1 (ContactCard/get with
//! `includeReplaced` and `historyLimit`), §4.1 (Email/query with
//! `onlyDestroyed`, `destroyedAfter`, `limit`), §5.3 (Email/set `restore`
//! and its response); the -04 editor's copy §4 (queryChanges with the
//! query's window) and §5.1 (`atomicFailure` MAY carry `notRestored`); §2.1
//! (the account capability).

use jmap_base_client::{
    ClientConfig, ClientError, ConditionalError, DefaultTransport, JmapClient, NoneAuth, SetCall,
};
use jmap_object_history_client::{
    DestroyedQueryCall, DestroyedQueryChangesCall, HistoryGetCall, JmapObjectHistoryExt,
    SessionClient, has_older, older_page_before,
};
use jmap_object_history_types::HistoryGetArguments;
use jmap_types::{Id, PatchObject, UTCDate};
use serde_json::{Map, Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CORE: &str = "urn:ietf:params:jmap:core";
const HISTORY: &str = "urn:ietf:params:jmap:object-history";
const CONTACTS: &str = "urn:ietf:params:jmap:contacts";
const MAIL: &str = "urn:ietf:params:jmap:mail";

fn client(server: &MockServer) -> SessionClient {
    let session: jmap_base_client::Session = serde_json::from_value(json!({
        "capabilities": {CORE: {}, HISTORY: {}, MAIL: {}, CONTACTS: {}},
        "accounts": {"abc": {
            "name": "a", "isPersonal": true, "isReadOnly": false,
            "accountCapabilities": {HISTORY: {
                "maxHistoryDuration": 2592000,
                "supportedTypes": ["ContactCard", "Email"]
            }}
        }},
        "primaryAccounts": {MAIL: "abc"},
        "username": "a",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/d/{{accountId}}/{{blobId}}/{{name}}?type={{type}}", server.uri()),
        "uploadUrl": format!("{}/u/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/e", server.uri()),
        "state": "s"
    }))
    .expect("session");
    JmapClient::new(
        DefaultTransport,
        NoneAuth,
        &server.uri(),
        ClientConfig::default(),
    )
    .expect("client")
    .with_object_history_session(session)
}

async fn expect(server: &MockServer, using: &[&str], request: Value, response: Value) {
    Mock::given(method("POST"))
        .and(path("/api/"))
        .and(body_json(json!({"using": using, "methodCalls": [request]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [response], "sessionState": "s"
        })))
        .expect(1)
        .mount(server)
        .await;
}

#[test]
fn capability_from_the_session() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    let c = client(&server);
    let cap = c.capability(&Id::from("abc")).unwrap().unwrap();
    assert_eq!(cap.max_history_duration, Some(2_592_000));
    assert!(c.supports(&Id::from("abc"), "Email").unwrap());
    assert!(!c.supports(&Id::from("abc"), "Thread").unwrap());
}

/// §3.2.1: the request verbatim; the three entries decode, and paging
/// reads the oldest one.
#[tokio::test]
async fn example_3_2_1_history_get() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, CONTACTS, HISTORY],
        json!(["ContactCard/get", {
            "accountId": "abc", "ids": ["contact1"], "properties": ["name", "emails"],
            "includeReplaced": true, "historyLimit": 10
        }, "0"]),
        json!(["ContactCard/get", {"accountId": "abc", "state": "abc123", "list": [
            {"id": "contact1", "name": {"full": "Robert Smith"},
             "objectHistory": {"version": 1, "replaced": "2026-02-20T14:15:00Z",
                               "hasOlder": false, "expires": "2026-03-22T14:15:00Z"}},
            {"id": "contact1", "name": {"full": "Bob Smith"},
             "objectHistory": {"version": 2, "replaced": "2026-03-01T09:00:00Z",
                               "hasOlder": true, "expires": "2026-03-31T09:00:00Z"}},
            {"id": "contact1", "name": {"full": "Bob Smith"},
             "objectHistory": {"version": 3, "replaced": null, "hasOlder": true, "expires": null}}
        ], "notFound": []}, "0"]),
    )
    .await;
    let mut args = HistoryGetArguments::new();
    args.include_replaced = true;
    args.history_limit = Some(10);
    let call = HistoryGetCall::new(Id::from("abc"), CONTACTS, args)
        .ids(Some(vec![Id::from("contact1")]))
        .properties(Some(vec!["name".into(), "emails".into()]));
    let resp = client(&server)
        .get_history::<Map<String, Value>>("ContactCard", &call)
        .await
        .unwrap();
    assert_eq!(resp.list.len(), 3);
    assert_eq!(resp.list[2].object["name"]["full"], "Bob Smith");
    assert_eq!(
        older_page_before(&resp.list),
        Some(&UTCDate::from("2026-02-20T14:15:00Z"))
    );
    assert!(!has_older(&resp.list));
}

/// §4.1, then the -04 queryChanges with the same window.
#[tokio::test]
async fn example_4_1_destroyed_query_and_changes() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, MAIL, HISTORY],
        json!(["Email/query", {
            "accountId": "abc", "onlyDestroyed": true,
            "destroyedAfter": "2026-03-01T00:00:00Z", "limit": 50
        }, "0"]),
        json!(["Email/query", {"accountId": "abc", "queryState": "dq7",
            "canCalculateChanges": true, "position": 0, "ids": ["email57", "email42"]}, "0"]),
    )
    .await;
    expect(
        &server,
        &[CORE, MAIL, HISTORY],
        json!(["Email/queryChanges", {
            "accountId": "abc", "onlyDestroyed": true,
            "destroyedAfter": "2026-03-01T00:00:00Z", "sinceQueryState": "dq7"
        }, "0"]),
        json!(["Email/queryChanges", {"accountId": "abc", "oldQueryState": "dq7",
            "newQueryState": "dq8", "removed": ["email42"], "added": []}, "0"]),
    )
    .await;
    let c = client(&server);
    let query = DestroyedQueryCall::new(Id::from("abc"), MAIL)
        .window(Some(UTCDate::from("2026-03-01T00:00:00Z")), None)
        .limit(Some(50));
    let q = c.query_destroyed("Email", &query).await.unwrap();
    assert_eq!(q.ids, [Id::from("email57"), Id::from("email42")]);
    let changes = c
        .query_destroyed_changes("Email", &DestroyedQueryChangesCall::new(query, "dq7"))
        .await
        .unwrap();
    assert_eq!(changes.removed, [Id::from("email42")]);
}

/// §5.3: the restore request verbatim and its response decoded.
#[tokio::test]
async fn example_5_3_restore() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, MAIL, HISTORY],
        json!(["Email/set", {"accountId": "abc",
            "restore": {"email42": {"mailboxIds": {"mb1": true}}}}, "0"]),
        json!(["Email/set", {"accountId": "abc", "oldState": "s110", "newState": "s111",
            "created": null, "updated": null, "destroyed": null, "notCreated": null,
            "notUpdated": null, "notDestroyed": null,
            "restored": {"email42": null}, "notRestored": null}, "0"]),
    )
    .await;
    let mut call = SetCall::default();
    call.account_id = Some(Id::from("abc"));
    let mut patch = Map::new();
    patch.insert("mailboxIds".into(), json!({"mb1": true}));
    call.restore
        .insert(Id::from("email42"), Some(PatchObject::from_map(patch)));
    let (_, outcome) = client(&server)
        .restore("Email", &call, &[MAIL])
        .await
        .unwrap();
    assert_eq!(outcome.restored[&Id::from("email42")], None);
    assert!(outcome.not_restored.is_empty());
}

/// -04 §5.1: an atomic set whose restore failed answers `atomicFailure`
/// with `notRestored`.
#[tokio::test]
async fn atomic_failure_carries_not_restored() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [["error", {"type": "atomicFailure",
                "notRestored": {"email42": {"type": "notFound"}}}, "0"]],
            "sessionState": "s"
        })))
        .mount(&server)
        .await;
    let mut call = SetCall::default();
    call.account_id = Some(Id::from("abc"));
    call.conditional.atomic = true;
    call.restore.insert(Id::from("email42"), None);
    let err = client(&server)
        .restore("Email", &call, &[MAIL])
        .await
        .unwrap_err();
    let ClientError::MethodError(e) = &err else {
        panic!("{err:?}")
    };
    let Some(ConditionalError::AtomicFailure(f)) = ConditionalError::from_method_error(e) else {
        panic!("{err:?}")
    };
    assert!(f.not_restored().unwrap().contains_key(&Id::from("email42")));
}
