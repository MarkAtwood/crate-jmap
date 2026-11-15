//! Wiremock tests of the Quota/* calls.
//!
//! Oracle: the RFC 9425 §5.1 and §5.2 request and response examples
//! (`types` given registered names; see the jmap-quotas-server tests), the
//! RFC 8620 §3.3 request envelope, and RFC 9425 §4.1's `using` rule.

use jmap_quotas_client::{ClientError, JmapQuotasExt, SessionClient};
use jmap_quotas_types::{QuotaComparator, QuotaComparatorProperty, QuotaFilter, Scope};
use jmap_types::{Id, State};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ACC: &str = "u33084183";
const Q1: &str = "2a06df0d-9865-4e74-a92f-74dcc814270e";

fn session(server: &MockServer, primary: bool) -> jmap_base_client::Session {
    let primary_accounts = if primary {
        json!({ "urn:ietf:params:jmap:quota": ACC })
    } else {
        json!({})
    };
    serde_json::from_value(json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:quota": {},
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:calendars": {},
            "urn:ietf:params:jmap:i18n": {}
        },
        "accounts": { ACC: { "name": "bob@example.com", "isPersonal": true,
            "isReadOnly": false, "accountCapabilities": { "urn:ietf:params:jmap:quota": {},
            "urn:ietf:params:jmap:i18n": {} } } },
        "primaryAccounts": primary_accounts,
        "username": "bob@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": "s1"
    }))
    .unwrap()
}

fn client(server: &MockServer, primary: bool) -> SessionClient {
    jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .unwrap()
    .with_quotas_session(session(server, primary))
}

async fn respond(server: &MockServer, name: &str, args: Value) {
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "methodResponses": [[name, args, "r1"]], "sessionState": "s1"
        })))
        .mount(server)
        .await;
}

/// The one request the mock received, as JSON.
async fn sent(server: &MockServer) -> Value {
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    serde_json::from_slice(&reqs[0].body).unwrap()
}

/// §5.1: ids null fetches every Quota; `using` holds every Session
/// capability so no Quota is hidden (§4.1).
#[tokio::test]
async fn section_5_1_fetch_all() {
    let server = MockServer::start().await;
    respond(
        &server,
        "Quota/get",
        json!({
            "accountId": ACC, "state": "78540",
            "list": [{
                "id": Q1, "resourceType": "count", "used": 1056, "warnLimit": 1600,
                "softLimit": 1800, "hardLimit": 2000, "scope": "account",
                "name": "bob@example.com",
                "description": "Personal account usage.",
                "types": ["Email", "Calendar", "ContactCard"]
            }],
            "notFound": []
        }),
    )
    .await;
    let resp = client(&server, true).quota_get(None).await.unwrap();
    assert_eq!(resp.state, "78540");
    let q = &resp.list[0];
    assert_eq!(
        (q.used, q.soft_limit, q.hard_limit),
        (1056, Some(1800), 2000)
    );
    assert_eq!(q.scope, Scope::Account);

    let req = sent(&server).await;
    assert_eq!(
        req["using"],
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:quota",
            "urn:ietf:params:jmap:calendars",
            "urn:ietf:params:jmap:i18n",
            "urn:ietf:params:jmap:mail"
        ])
    );
    assert_eq!(
        req["methodCalls"],
        json!([["Quota/get", { "accountId": ACC, "ids": null }, "r1"]])
    );
}

/// `with_type_capabilities` narrows `using` to core, quota and those.
#[tokio::test]
async fn type_capabilities_narrow_using() {
    let server = MockServer::start().await;
    respond(
        &server,
        "Quota/get",
        json!({ "accountId": ACC, "state": "1", "list": [], "notFound": [] }),
    )
    .await;
    client(&server, true)
        .with_type_capabilities(["urn:ietf:params:jmap:mail"])
        .quota_get(Some(&[Id::from(Q1)]))
        .await
        .unwrap();
    let req = sent(&server).await;
    assert_eq!(
        req["using"],
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:quota",
            "urn:ietf:params:jmap:mail"
        ])
    );
    assert_eq!(req["methodCalls"][0][1]["ids"], json!([Q1]));
}

/// §5.2: Quota/changes with maxChanges, its updatedProperties, and the
/// follow-up Quota/get of only those properties.
#[tokio::test]
async fn section_5_2_changes_then_get_used() {
    let server = MockServer::start().await;
    respond(
        &server,
        "Quota/changes",
        json!({
            "accountId": ACC, "oldState": "78540", "newState": "78542",
            "hasMoreChanges": false, "updatedProperties": ["used"],
            "created": [], "updated": [Q1], "destroyed": []
        }),
    )
    .await;
    let sc = client(&server, true);
    let changes = sc
        .quota_changes(&State::from("78540"), Some(20))
        .await
        .unwrap();
    assert_eq!(
        changes.updated_properties,
        Some(Some(vec!["used".to_owned()]))
    );
    assert_eq!(changes.updated, [Id::from(Q1)]);
    assert_eq!(
        sent(&server).await["methodCalls"][0],
        json!(["Quota/changes", { "accountId": ACC, "sinceState": "78540", "maxChanges": 20 }, "r1"])
    );

    let server = MockServer::start().await;
    respond(
        &server,
        "Quota/get",
        json!({
            "accountId": ACC, "state": "10826",
            "list": [{ "id": Q1, "used": 1246 }], "notFound": []
        }),
    )
    .await;
    let props: Vec<&str> = vec!["used"];
    let resp = client(&server, true)
        .quota_get_properties(Some(&changes.updated), &props)
        .await
        .unwrap();
    assert_eq!(resp.list[0]["used"], 1246);
    assert_eq!(
        sent(&server).await["methodCalls"][0][1],
        json!({ "accountId": ACC, "ids": [Q1], "properties": ["used"] })
    );
}

/// §4.4: filter and sort go on the wire as given.
#[tokio::test]
async fn query_and_query_changes_wire() {
    let server = MockServer::start().await;
    respond(
        &server,
        "Quota/query",
        json!({
            "accountId": ACC, "queryState": "7", "canCalculateChanges": true,
            "position": 0, "ids": [Q1]
        }),
    )
    .await;
    let filter: QuotaFilter =
        serde_json::from_value(json!({ "scope": "account", "type": "Email" })).unwrap();
    let mut by_used = QuotaComparator::new(QuotaComparatorProperty::Used);
    by_used.is_ascending = false;
    let resp = client(&server, true)
        .quota_query(
            Some(&filter),
            Some(&[by_used.clone()]),
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .unwrap();
    assert_eq!(resp.ids, [Id::from(Q1)]);
    let args = &sent(&server).await["methodCalls"][0][1];
    assert_eq!(
        args["filter"],
        json!({ "scope": "account", "type": "Email" })
    );
    assert_eq!(
        args["sort"],
        json!([{ "property": "used", "isAscending": false }])
    );

    let server = MockServer::start().await;
    respond(
        &server,
        "Quota/queryChanges",
        json!({
            "accountId": ACC, "oldQueryState": "7", "newQueryState": "8",
            "removed": [Q1], "added": [{ "id": Q1, "index": 0 }]
        }),
    )
    .await;
    let resp = client(&server, true)
        .quota_query_changes(
            &State::from("7"),
            Some(&filter),
            Some(&[by_used]),
            None,
            None,
            true,
        )
        .await
        .unwrap();
    assert_eq!(resp.new_query_state, "8");
    let args = &sent(&server).await["methodCalls"][0][1];
    assert_eq!(args["sinceQueryState"], "7");
    assert_eq!(args["calculateTotal"], true);
}

#[tokio::test]
async fn errors_before_sending() {
    let server = MockServer::start().await;
    assert!(matches!(
        client(&server, false).quota_get(None).await,
        Err(ClientError::InvalidSession(_))
    ));
    assert!(matches!(
        client(&server, true)
            .quota_changes(&State::from(""), None)
            .await,
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// draft-atwood-jmap-updates-00 §4.2, §5.2: `with_i18n` puts the
/// capability in `using` and `language` on each call, and the Quota keeps
/// the `locale` and `localizations` the server returns.
#[tokio::test]
async fn with_i18n_sends_language() {
    let server = MockServer::start().await;
    respond(
        &server,
        "Quota/get",
        json!({
            "accountId": ACC, "state": "1", "notFound": [],
            "list": [{ "id": Q1, "resourceType": "octets", "used": 1, "hardLimit": 2,
                       "scope": "account", "name": "Post", "types": ["Email"],
                       "locale": "de", "localizations": null }]
        }),
    )
    .await;
    let de: Vec<jmap_types::i18n::LanguageRange> = vec!["de".parse().unwrap()];
    let resp = client(&server, true)
        .with_type_capabilities(["urn:ietf:params:jmap:mail"])
        .with_i18n(Some(de))
        .unwrap()
        .quota_get(None)
        .await
        .unwrap();
    assert_eq!(resp.list[0].locale.as_ref().map(|t| t.as_str()), Some("de"));
    let req = sent(&server).await;
    assert!(
        req["using"]
            .as_array()
            .unwrap()
            .contains(&json!("urn:ietf:params:jmap:i18n")),
        "{req}"
    );
    assert_eq!(req["methodCalls"][0][1]["language"], json!(["de"]));
}
