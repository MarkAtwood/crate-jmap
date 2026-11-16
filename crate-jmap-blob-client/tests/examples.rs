//! `jmap-blob-client` against draft-ietf-jmap-blobext-03 §9 (bd
//! JMAP-tna1.5): each request body must equal the example's request, and
//! the example's response must parse.
//!
//! Oracles: the §9 examples verbatim (`accountId` "abc"), plus §2.1 for
//! the Session, §4 / §7 for absent and `null` `oldState`, `newState` and
//! `expires`, and the JMAP Data Types registry for Blob/lookup's `using`.

use std::collections::BTreeMap;

use jmap_base_client::{ClientConfig, ClientError, DefaultTransport, JmapClient, NoneAuth};
use jmap_blob_client::{BlobGetInput, JmapBlobExt, SessionClient};
use jmap_blob_types::DigestAlgorithm;
use jmap_types::{Id, UTCDate};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn br(id: &str) -> jmap_blob_types::BlobRef {
    Id::from(id).into()
}

const CORE: &str = "urn:ietf:params:jmap:core";
const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const MAIL: &str = "urn:ietf:params:jmap:mail";

fn session(
    server: &MockServer,
    capabilities: Value,
    account_cap: Value,
) -> jmap_base_client::Session {
    serde_json::from_value(json!({
        "capabilities": capabilities,
        "accounts": {"abc": {
            "name": "a", "isPersonal": true, "isReadOnly": false,
            "accountCapabilities": {BLOB2: account_cap}
        }},
        "primaryAccounts": {BLOB2: "abc"},
        "username": "a",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/d/{{accountId}}/{{blobId}}/{{name}}?type={{type}}", server.uri()),
        "uploadUrl": format!("{}/u/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/e", server.uri()),
        "state": "s"
    }))
    .expect("session")
}

fn capability() -> Value {
    json!({
        "maxSizeBlobSet": null, "maxDataSources": 64,
        "supportedTypeNames": ["Mailbox", "Email"],
        "supportedDigestAlgorithms": ["sha-256", "sha"],
        "uploadUrl": null, "chunkSize": null
    })
}

fn client(server: &MockServer, capabilities: Value) -> SessionClient {
    let c = JmapClient::new(
        DefaultTransport,
        NoneAuth,
        &server.uri(),
        ClientConfig::default(),
    )
    .expect("client");
    c.with_blob_session(session(server, capabilities, capability()))
}

/// Expect exactly `request` (one call, id "0") with `using`, and answer
/// `response`.
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

fn caps() -> Value {
    json!({CORE: {}, BLOB2: {}, MAIL: {}})
}

/// §9.1: uploading a blob inline; the response has no `oldState` or
/// `newState`.
#[tokio::test]
async fn example_9_1_blob_set() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB2],
        json!(["Blob/set", {
            "accountId": "abc",
            "create": {"b1": {"data": [{"data:asText": "Hello, world!"}], "type": "text/plain"}}
        }, "0"]),
        json!(["Blob/set", {
            "accountId": "abc",
            "created": {"b1": {"id": "Babc123", "type": "text/plain", "size": 13}},
            "notCreated": null
        }, "0"]),
    )
    .await;
    let create = BTreeMap::from([(
        Id::from("b1"),
        serde_json::from_value(
            json!({"data": [{"data:asText": "Hello, world!"}], "type": "text/plain"}),
        )
        .expect("BlobCreateObject"),
    )]);
    let r = client(&server, caps())
        .blob_create(create)
        .await
        .expect("Blob/set");
    let created = &r.created["b1"];
    assert_eq!(created.id, Id::from("Babc123"));
    assert_eq!(created.size, Some(13));
    assert!(r.old_state.is_none() && r.new_state.is_none());
}

/// §4 / §7: `null` states and `expires` parse like absent ones.
#[tokio::test]
async fn null_states_and_expires_parse() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB2],
        json!(["Blob/set", {"accountId": "abc", "update": {"B1": {"expires": null}}}, "0"]),
        json!(["Blob/set", {
            "accountId": "abc", "oldState": null, "newState": null,
            "updated": {"B1": {"expires": null}}
        }, "0"]),
    )
    .await;
    let r = client(&server, caps())
        .blob_touch(vec![br("B1")], None)
        .await
        .expect("touch");
    assert!(r.old_state.is_none() && r.new_state.is_none());

    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB2],
        json!(["Blob/set", {"accountId": "abc", "update": {"B1": {"expires": "2026-12-01T00:00:00Z"}}}, "0"]),
        json!(["Blob/set", {"accountId": "abc", "updated": {"B1": null}}, "0"]),
    )
    .await;
    client(&server, caps())
        .blob_touch(vec![br("B1")], Some(UTCDate::from("2026-12-01T00:00:00Z")))
        .await
        .expect("touch with a date");
}

#[tokio::test]
async fn blob_destroy_request() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB2],
        json!(["Blob/set", {"accountId": "abc", "destroy": ["B1", "B2"]}, "0"]),
        json!(["Blob/set", {"accountId": "abc", "destroyed": ["B1", "B2"]}, "0"]),
    )
    .await;
    client(&server, caps())
        .blob_destroy(vec![br("B1"), br("B2")])
        .await
        .expect("destroy");
}

/// §9.2: querying blob chunks.
#[tokio::test]
async fn example_9_2_blob_get_chunks() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB2],
        json!(["Blob/get", {
            "accountId": "abc",
            "ids": ["B1a2b3c"],
            "properties": ["size", "chunks"],
            "dataSourceProperties": ["blobId", "size", "offset", "length", "position", "digest:sha-256"]
        }, "0"]),
        json!(["Blob/get", {
            "accountId": "abc",
            "list": [{
                "id": "B1a2b3c", "size": 10485760,
                "chunks": [
                    {"blobId": "Bchunk1", "size": 5242880, "offset": 0, "length": 5242880,
                     "position": 0, "digest:sha-256": "a1b2c3..."},
                    {"blobId": "Bchunk2", "size": 5242880, "offset": 0, "length": 5242880,
                     "position": 5242880, "digest:sha-256": "d4e5f6..."}
                ]
            }],
            "notFound": []
        }, "0"]),
    )
    .await;
    let mut input = BlobGetInput::default();
    input.properties = Some(serde_json::from_value(json!(["size", "chunks"])).unwrap());
    input.data_source_properties = Some(
        serde_json::from_value(json!([
            "blobId",
            "size",
            "offset",
            "length",
            "position",
            "digest:sha-256"
        ]))
        .unwrap(),
    );
    let r = client(&server, caps())
        .blob_get(vec![br("B1a2b3c")], input)
        .await
        .expect("Blob/get");
    assert_eq!(r.list.len(), 1);
    assert_eq!(r.list[0].size, Some(10_485_760));
}

/// §9.3: looking up references; Mailbox and Email add the mail
/// capability (registry) to `using`.
#[tokio::test]
async fn example_9_3_blob_lookup_adds_mail() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB2, MAIL],
        json!(["Blob/lookup", {
            "accountId": "abc", "typeNames": ["Mailbox", "Email"], "ids": ["Bfirst", "Bsecond"]
        }, "0"]),
        json!(["Blob/lookup", {
            "accountId": "abc",
            "list": [
                {"id": "Bfirst", "matchedIds": {"Mailbox": ["M1"], "Email": ["E1", "E2"]}},
                {"id": "Bsecond", "matchedIds": {"Mailbox": [], "Email": []}}
            ]
        }, "0"]),
    )
    .await;
    let r = client(&server, caps())
        .blob_lookup(
            vec![Id::from("Bfirst"), Id::from("Bsecond")],
            vec!["Mailbox".into(), "Email".into()],
            &[],
        )
        .await
        .expect("Blob/lookup");
    assert_eq!(r.list.len(), 2);
}

#[tokio::test]
async fn blob_lookup_refuses_bad_input() {
    let server = MockServer::start().await;
    let c = client(&server, json!({CORE: {}, BLOB2: {}}));
    // The session lacks mail, so Email cannot be looked up.
    let err = c
        .blob_lookup(vec![Id::from("B1")], vec!["Email".into()], &[])
        .await
        .unwrap_err();
    assert!(matches!(err, ClientError::InvalidArgument(_)), "{err:?}");
    for (ids, names) in [
        (vec![], vec!["Email".to_owned()]),
        (vec![Id::from("B1")], vec![]),
    ] {
        let err = c.blob_lookup(ids, names, &[]).await.unwrap_err();
        assert!(matches!(err, ClientError::InvalidArgument(_)), "{err:?}");
    }
}

/// §9.4 to §9.6: Blob/convert requests and responses.
#[tokio::test]
async fn examples_9_4_to_9_6_blob_convert() {
    let cases = [
        (
            json!({"z1": {"archive": {"type": "application/zip", "entries": [
                {"name": "site/index.html", "blobId": "Baaaa"},
                {"name": "site/style.css", "blobId": "Bbbbb"},
                {"name": "site/photo.jpg", "blobId": "Bcccc"}
            ]}}}),
            json!({"accountId": "abc", "created": {"z1": {"id": "B9f2a4e", "type": "application/zip", "size": 104857}}, "notCreated": {}}),
        ),
        (
            json!({
                "t1": {"noPersist": true, "archive": {"type": "application/x-tar", "entries": [
                    {"name": "site/index.html", "blobId": "Baaaa", "modified": "2026-03-01T12:00:00Z", "mode": "0644"},
                    {"name": "site/style.css", "blobId": "Bbbbb", "modified": "2026-03-01T12:00:00Z", "mode": "0644"},
                    {"name": "site/photo.jpg", "blobId": "Bcccc", "modified": "2026-02-15T09:30:00Z", "mode": "0644"}
                ]}},
                "t2": {"compress": {"blobId": "#t1", "type": "application/gzip"}}
            }),
            json!({"accountId": "abc", "created": {"t2": {"id": "Bd81c7f", "type": "application/gzip", "size": 98304}}, "notCreated": {}}),
        ),
        (
            json!({
                "u1": {"noPersist": true, "decompress": {"blobId": "Bd81c7f", "type": "application/gzip"}},
                "u2": {"extract": {"blobId": "#u1", "type": "application/x-tar"}}
            }),
            json!({"accountId": "abc", "created": {"u2": {
                "id": "Be3a901", "type": "application/x-tar", "size": 102400,
                "entries": [
                    {"name": "site/index.html", "blobId": "Bdd001", "entryType": "file", "modified": "2026-03-01T12:00:00Z", "mode": "0644"},
                    {"name": "site/style.css", "blobId": "Bdd002", "entryType": "file", "modified": "2026-03-01T12:00:00Z", "mode": "0644"},
                    {"name": "site/photo.jpg", "blobId": "Bdd003", "entryType": "file", "modified": "2026-02-15T09:30:00Z", "mode": "0644"}
                ]
            }}, "notCreated": {}}),
        ),
    ];
    for (create, response) in cases {
        let server = MockServer::start().await;
        expect(
            &server,
            &[CORE, BLOB2],
            json!(["Blob/convert", {"accountId": "abc", "create": create.clone()}, "0"]),
            json!(["Blob/convert", response, "0"]),
        )
        .await;
        let create: BTreeMap<Id, jmap_blob_types::ConvertRequest> =
            serde_json::from_value(create).expect("ConvertRequest");
        let r = client(&server, caps())
            .blob_convert(create)
            .await
            .expect("Blob/convert");
        assert_eq!(r.created.len(), 1);
    }
}

#[test]
fn capability_helpers() {
    let server_uri = "http://127.0.0.1:9";
    let session: jmap_base_client::Session = serde_json::from_value(json!({
        "capabilities": {CORE: {}, BLOB2: {}},
        "accounts": {"abc": {"name": "a", "isPersonal": true, "isReadOnly": false,
            "accountCapabilities": {BLOB2: capability()}}},
        "primaryAccounts": {BLOB2: "abc"},
        "username": "a", "apiUrl": format!("{server_uri}/api/"),
        "downloadUrl": format!("{server_uri}/d"), "uploadUrl": format!("{server_uri}/u/{{accountId}}/"),
        "eventSourceUrl": format!("{server_uri}/e"), "state": "s"
    }))
    .unwrap();
    let c = JmapClient::new(
        DefaultTransport,
        NoneAuth,
        server_uri,
        ClientConfig::default(),
    )
    .unwrap()
    .with_blob_session(session);
    assert_eq!(c.blob_account_id().unwrap(), Id::from("abc"));
    assert_eq!(c.lookup_type_names().unwrap(), ["Mailbox", "Email"]);
    // The server's order of preference among what the client supports.
    assert_eq!(
        c.preferred_digest_algorithm(&[DigestAlgorithm::Sha, DigestAlgorithm::Sha256])
            .unwrap(),
        Some(DigestAlgorithm::Sha256)
    );
    assert_eq!(
        c.preferred_digest_algorithm(&[DigestAlgorithm::Sha512])
            .unwrap(),
        None
    );
    // No account uploadUrl: the session's.
    assert_eq!(
        c.upload_url().unwrap().as_str(),
        format!("{server_uri}/u/{{accountId}}/")
    );
    assert_eq!(
        c.clone()
            .for_account(Id::from("other"))
            .blob_account_id()
            .unwrap(),
        Id::from("other")
    );

    // SHA-256 of "Hello, world!", computed outside the crate.
    let sha256 = "MV9b23bQeMQ7isAGTkoBZGErH853yGk0W/yUx1iU7dM=";
    assert_eq!(
        SessionClient::verify_digest(&DigestAlgorithm::Sha256, b"Hello, world!", sha256),
        Some(true)
    );
    assert_eq!(
        SessionClient::verify_digest(&DigestAlgorithm::Sha256, b"Hello", sha256),
        Some(false)
    );
    assert_eq!(
        SessionClient::verify_digest(&DigestAlgorithm::Other("md5".into()), b"x", "y"),
        None
    );
}
