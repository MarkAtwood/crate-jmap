//! `LegacySessionClient` against RFC 9404 §4 (bd JMAP-tna1.11): each
//! request body must equal the example's request, and the example's
//! response must parse.
//!
//! Oracles: RFC 9404 §4.1.1 (Blob/upload), §4.2.1 (Blob/get; its two calls
//! checked as one request each) and §4.3.1 (Blob/lookup) verbatim; §3.1
//! for the capability and primary account; §4.3 for the required
//! `accountId` argument, which the §4.3.1 example leaves out, and for the
//! response's `list`-only shape. blobext-03 §2.1: a request never names
//! both `blob` and `blob2`.

use std::collections::BTreeMap;

use jmap_base_client::{ClientConfig, ClientError, DefaultTransport, JmapClient, NoneAuth};
use jmap_blob_client::{
    BlobProperty, DataSourceObject, DigestAlgorithm, JmapBlobExt, LegacySessionClient, UploadObject,
};
use jmap_types::Id;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CORE: &str = "urn:ietf:params:jmap:core";
const BLOB: &str = "urn:ietf:params:jmap:blob";
const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const MAIL: &str = "urn:ietf:params:jmap:mail";

/// A session advertising RFC 9404 only, with "account1" primary for it
/// and the §3.1.1 account capability.
fn client(server: &MockServer) -> LegacySessionClient {
    let session: jmap_base_client::Session = serde_json::from_value(json!({
        "capabilities": {CORE: {}, BLOB: {}, MAIL: {}},
        "accounts": {"account1": {
            "name": "a", "isPersonal": true, "isReadOnly": false,
            "accountCapabilities": {BLOB: {
                "maxSizeBlobSet": 50000000,
                "maxDataSources": 100,
                "supportedTypeNames" : ["Mailbox", "Thread", "Email"],
                "supportedDigestAlgorithms" : ["sha", "sha-256"]
            }}
        }},
        "primaryAccounts": {BLOB: "account1"},
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
    .with_blob_legacy_session(session)
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

/// §3.1: the account capability and the primary `blob` account.
#[test]
fn capability_and_account_come_from_the_blob_uri() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let server = rt.block_on(MockServer::start());
    let c = client(&server);
    assert_eq!(c.blob_account_id().unwrap(), Id::from("account1"));
    let cap = c.blob_capability().unwrap().expect("capability");
    assert_eq!(cap.max_data_sources, 100);
    assert_eq!(cap.max_size_blob_set, Some(50_000_000));
}

/// §4.1.1, with `data:asBase64` unwrapped as the text says it is sent.
#[tokio::test]
async fn example_4_1_1_blob_upload() {
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABAQMAAAAl21bKAAAAA1BMVEX/AAAZ4gk3AAAAAXRSTlN/gFy0ywAAAApJREFUeJxjYgAAAAYAAzY3fKgAAAAASUVORK5CYII=";
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB],
        json!(["Blob/upload", {
            "accountId": "account1",
            "create": {"1": {"data": [{"data:asBase64": png}], "type": "image/png"}}
        }, "0"]),
        json!(["Blob/upload", {
            "accountId": "account1",
            "created": {"1": {
                "id": "G4c6751edf9dd6903ff54b792e432fba781271beb",
                "type": "image/png",
                "size": 95
            }}
        }, "0"]),
    )
    .await;
    let mut upload = UploadObject::new(vec![DataSourceObject::base64(png)]);
    upload.media_type = Some("image/png".to_owned());
    let resp = client(&server)
        .blob_upload(BTreeMap::from([(Id::from("1"), upload)]))
        .await
        .expect("upload");
    let created = &resp.created["1"];
    assert_eq!(
        created.id.as_ref(),
        "G4c6751edf9dd6903ff54b792e432fba781271beb"
    );
    assert_eq!(created.size, Some(95));
}

fn br(id: &str) -> jmap_blob_types::BlobRef {
    Id::from(id).into()
}

/// §4.2.1, call R1: the size and text of a blob and an unknown id.
#[tokio::test]
async fn example_4_2_1_blob_get_r1() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB],
        json!(["Blob/get", {
            "accountId" : "account1",
            "ids" : ["Gc0854fb9fb03c41cce3802cb0d220529e6eef94e", "not-a-blob"],
            "properties" : ["data:asText", "digest:sha", "size"]
        }, "0"]),
        json!(["Blob/get", {
            "accountId": "account1",
            "list": [{
                "id": "Gc0854fb9fb03c41cce3802cb0d220529e6eef94e",
                "data:asText": "The quick brown fox jumped over the lazy dog.",
                "digest:sha": "wIVPufsDxBzOOALLDSIFKebu+U4=",
                "size": 45
            }],
            "notFound": ["not-a-blob"]
        }, "0"]),
    )
    .await;
    let resp = client(&server)
        .blob_get(
            vec![
                br("Gc0854fb9fb03c41cce3802cb0d220529e6eef94e"),
                br("not-a-blob"),
            ],
            Some(vec![
                BlobProperty::DataAsText,
                BlobProperty::Digest(DigestAlgorithm::from("sha")),
                BlobProperty::Size,
            ]),
            None,
            None,
        )
        .await
        .expect("get");
    let item = &resp.list[0];
    assert_eq!(
        item.data_as_text,
        Some(Some(
            "The quick brown fox jumped over the lazy dog.".to_owned()
        ))
    );
    assert_eq!(
        item.digests[&DigestAlgorithm::from("sha")],
        "wIVPufsDxBzOOALLDSIFKebu+U4="
    );
    assert_eq!(item.size, Some(45));
    assert_eq!(resp.not_found, [Id::from("not-a-blob")]);
}

/// §4.2.1, call R2: a range, with two digests of it.
#[tokio::test]
async fn example_4_2_1_blob_get_r2_range() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB],
        json!(["Blob/get", {
            "accountId" : "account1",
            "ids" : ["Gc0854fb9fb03c41cce3802cb0d220529e6eef94e"],
            "properties" : ["data:asText", "digest:sha", "digest:sha-256", "size"],
            "offset" : 4,
            "length" : 9
        }, "0"]),
        json!(["Blob/get", {
            "accountId": "account1",
            "list": [{
                "id": "Gc0854fb9fb03c41cce3802cb0d220529e6eef94e",
                "data:asText": "quick bro",
                "digest:sha": "QiRAPtfyX8K6tm1iOAtZ87Xj3Ww=",
                "digest:sha-256": "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA=",
                "size": 45
            }]
        }, "0"]),
    )
    .await;
    let resp = client(&server)
        .blob_get(
            vec![br("Gc0854fb9fb03c41cce3802cb0d220529e6eef94e")],
            Some(vec![
                BlobProperty::DataAsText,
                BlobProperty::Digest(DigestAlgorithm::from("sha")),
                BlobProperty::Digest(DigestAlgorithm::from("sha-256")),
                BlobProperty::Size,
            ]),
            Some(4),
            Some(9),
        )
        .await
        .expect("get");
    let item = &resp.list[0];
    assert_eq!(item.data_as_text, Some(Some("quick bro".to_owned())));
    assert_eq!(
        item.digests[&DigestAlgorithm::from("sha-256")],
        "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA="
    );
}

/// §4.3.1, plus the `accountId` §4.3 requires. `using` gains mail, the
/// capability of Mailbox, Thread and Email. The response has no
/// `accountId`; its `notFound` is kept in `extra`.
#[tokio::test]
async fn example_4_3_1_blob_lookup() {
    let server = MockServer::start().await;
    expect(
        &server,
        &[CORE, BLOB, MAIL],
        json!(["Blob/lookup", {
            "accountId": "account1",
            "typeNames": ["Mailbox", "Thread", "Email"],
            "ids": ["Gd2f81008cf07d2425418f7f02a3ca63a8bc82003", "not-a-blob"]
        }, "0"]),
        json!(["Blob/lookup", {
            "list": [{
                "id": "Gd2f81008cf07d2425418f7f02a3ca63a8bc82003",
                "matchedIds": {
                    "Mailbox": ["M54e97373", "Mcbe6b662"],
                    "Thread": ["T1530616e"],
                    "Email": ["E16e70a73eb4", "E84b0930cf16"]
                }
            }],
            "notFound": ["not-a-blob"]
        }, "0"]),
    )
    .await;
    let resp = client(&server)
        .blob_lookup(
            vec![
                Id::from("Gd2f81008cf07d2425418f7f02a3ca63a8bc82003"),
                Id::from("not-a-blob"),
            ],
            vec!["Mailbox".into(), "Thread".into(), "Email".into()],
            &[],
        )
        .await
        .expect("lookup");
    assert_eq!(resp.account_id, None);
    let info = &resp.list[0];
    assert_eq!(
        info.matched_ids["Email"],
        [Id::from("E16e70a73eb4"), Id::from("E84b0930cf16")]
    );
    assert_eq!(resp.extra["notFound"], json!(["not-a-blob"]));
}

/// blobext-03 §2.1: `blob2` never joins `blob` in one request, and
/// blobext-only Blob/get properties are refused before sending.
#[tokio::test]
async fn never_names_blob2_and_refuses_blobext_properties() {
    let server = MockServer::start().await;
    let c = client(&server);
    let err = c
        .blob_lookup(vec![Id::from("B1")], vec!["Email".into()], &[BLOB2])
        .await
        .expect_err("blob2 refused");
    assert!(matches!(err, ClientError::InvalidArgument(_)), "{err:?}");
    let err = c
        .blob_get(vec![br("B1")], Some(vec![BlobProperty::Chunks]), None, None)
        .await
        .expect_err("chunks refused");
    assert!(matches!(err, ClientError::InvalidArgument(_)), "{err:?}");
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
}
