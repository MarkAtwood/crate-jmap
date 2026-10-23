// Streaming blob upload/download integration tests.
//
// Digest oracles are hand-typed from external sources, never computed by
// this crate:
//   - SHA-256("abc") from NIST FIPS 180-4 Appendix A example 1.
//   - SHA-256("") from RFC 6234 / NIST CAVS.

use futures::StreamExt;
use jmap_base_client::ClientError;
use jmap_base_client::{
    BlobUploadBody, DownloadBlobParams, JmapUrlTemplate, UploadBlobStreamParams,
    upload_body_from_reader,
};
use jmap_base_client::{ClientConfig, JmapClient};
use jmap_base_client::{DefaultTransport, NoneAuth};
use wiremock::matchers::{body_bytes, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn client_for(server: &MockServer, config: ClientConfig) -> JmapClient {
    JmapClient::new(DefaultTransport, NoneAuth, &server.uri(), config)
        .expect("client construction must succeed")
}

fn upload_template(server: &MockServer) -> JmapUrlTemplate {
    JmapUrlTemplate::new(format!("{}/upload/{{accountId}}/", server.uri()))
}

fn download_template(server: &MockServer) -> JmapUrlTemplate {
    JmapUrlTemplate::new(format!(
        "{}/download/{{accountId}}/{{blobId}}/{{name}}",
        server.uri()
    ))
}

/// The bytes "abc" as two separate chunks, so the client must join them.
fn abc_in_two_chunks() -> BlobUploadBody {
    Box::pin(futures::stream::iter([
        Ok(bytes::Bytes::from_static(b"ab")),
        Ok(bytes::Bytes::from_static(b"c")),
    ]))
}

async fn mount_upload(server: &MockServer, response_json: &str) {
    Mock::given(method("POST"))
        .and(path("/upload/account1/"))
        .and(header("Content-Type", "text/plain"))
        .and(body_bytes(b"abc".to_vec()))
        .respond_with(ResponseTemplate::new(200).set_body_string(response_json.to_owned()))
        .mount(server)
        .await;
}

static ACCOUNT_ID: std::sync::LazyLock<jmap_types::Id> =
    std::sync::LazyLock::new(|| jmap_types::Id::from("account1"));
static BLOB_ID: std::sync::LazyLock<jmap_types::Id> =
    std::sync::LazyLock::new(|| jmap_types::Id::from("blob-abc"));

fn params<'a>(
    template: &'a JmapUrlTemplate,
    body: BlobUploadBody,
    content_length: Option<u64>,
) -> UploadBlobStreamParams<'a> {
    UploadBlobStreamParams {
        upload_url_template: template,
        account_id: &ACCOUNT_ID,
        content_type: "text/plain",
        body,
        content_length,
    }
}

/// Oracle: RFC 8620 §6.1 — "the data to be uploaded is sent in the body of
/// the POST request"; RFC 9112 §6.1 — a message whose length is not known
/// in advance is sent with `Transfer-Encoding: chunked`. The server only
/// answers when it receives exactly "abc"; its `sha256` is the NIST
/// SHA-256("abc"), so the client's streamed digest must agree with it.
#[tokio::test]
async fn upload_stream_unknown_length_sends_chunked_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/account1/"))
        .and(header("Transfer-Encoding", "chunked"))
        .and(body_bytes(b"abc".to_vec()))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{"accountId":"account1","blobId":"B1","type":"text/plain","size":3,"sha256":"{ABC_SHA256}"}}"#
        )))
        .mount(&server)
        .await;
    let client = client_for(&server, ClientConfig::default());
    let template = upload_template(&server);

    let resp = client
        .upload_blob_stream(params(&template, abc_in_two_chunks(), None))
        .await
        .expect("streamed upload must succeed");
    assert_eq!(resp.blob_id, "B1");
    assert_eq!(resp.size, 3);
}

/// Oracle: RFC 9112 §6.2 — a sender with a known body length sends
/// `Content-Length` set to the number of octets; "abc" is 3 octets.
#[tokio::test]
async fn upload_stream_known_length_sends_content_length() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/account1/"))
        .and(header("Content-Length", "3"))
        .and(body_bytes(b"abc".to_vec()))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"accountId":"account1","blobId":"B2","type":"text/plain","size":3}"#,
        ))
        .mount(&server)
        .await;
    let client = client_for(&server, ClientConfig::default());
    let template = upload_template(&server);

    let resp = client
        .upload_blob_stream(params(&template, abc_in_two_chunks(), Some(3)))
        .await
        .expect("streamed upload with known length must succeed");
    assert_eq!(resp.blob_id, "B2");
}

/// Oracle: RFC 9112 §6.2 — `Content-Length` states the body's octet count,
/// so a declared length the 3-octet body "abc" does not match (too long or
/// too short) cannot be sent as a valid message and must fail as
/// `ClientError::Http` rather than reaching the server's response handling.
#[tokio::test]
async fn upload_stream_wrong_declared_length_fails() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/account1/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"accountId":"account1","blobId":"B6","type":"text/plain","size":3}"#,
        ))
        .mount(&server)
        .await;
    let client = client_for(&server, ClientConfig::default());
    let template = upload_template(&server);

    for declared in [2, 5] {
        let err = client
            .upload_blob_stream(params(&template, abc_in_two_chunks(), Some(declared)))
            .await
            .expect_err("a wrong declared length must fail");
        assert!(
            matches!(err, ClientError::Http(_)),
            "declared {declared}: expected Http, got {err:?}"
        );
    }
}

/// Oracle: a reader holding the literal bytes "abc" must put exactly those
/// bytes on the wire (RFC 8620 §6.1 upload body).
#[tokio::test]
async fn upload_stream_from_async_read() {
    let server = MockServer::start().await;
    mount_upload(
        &server,
        r#"{"accountId":"account1","blobId":"B3","type":"text/plain","size":3}"#,
    )
    .await;
    let client = client_for(&server, ClientConfig::default());
    let template = upload_template(&server);

    let resp = client
        .upload_blob_stream(params(
            &template,
            upload_body_from_reader(&b"abc"[..]),
            None,
        ))
        .await
        .expect("reader upload must succeed");
    assert_eq!(resp.blob_id, "B3");
}

/// Oracle: draft-atwood-jmap-cid-00 §3 — the upload response `sha256` is
/// the digest of the stored blob. A server claiming SHA-256("") (RFC 6234)
/// for a body of "abc" must yield `BlobIntegrityMismatch` with `expected`
/// = the client's digest of what it sent (NIST SHA-256("abc")) and
/// `actual` = the server's claim.
#[tokio::test]
async fn upload_stream_sha256_mismatch_is_integrity_error() {
    let server = MockServer::start().await;
    mount_upload(
        &server,
        &format!(
            r#"{{"accountId":"account1","blobId":"B4","type":"text/plain","size":3,"sha256":"{EMPTY_SHA256}"}}"#
        ),
    )
    .await;
    let client = client_for(&server, ClientConfig::default());
    let template = upload_template(&server);

    let err = client
        .upload_blob_stream(params(&template, abc_in_two_chunks(), None))
        .await
        .expect_err("digest mismatch must fail");
    match err {
        ClientError::BlobIntegrityMismatch { expected, actual } => {
            assert_eq!(expected, ABC_SHA256);
            assert_eq!(actual, EMPTY_SHA256);
        }
        other => panic!("expected BlobIntegrityMismatch, got {other:?}"),
    }
}

/// Oracle: RFC 8620 §6.1 — the response `size` is "the size of the file
/// in octets". A server reporting 0 for the 3 octets "abc" must yield
/// `UnexpectedResponse`, as `upload_blob` does (bd:JMAP-6lsm.8).
#[tokio::test]
async fn upload_stream_size_mismatch_is_rejected() {
    let server = MockServer::start().await;
    mount_upload(
        &server,
        r#"{"accountId":"account1","blobId":"B5","type":"text/plain","size":0}"#,
    )
    .await;
    let client = client_for(&server, ClientConfig::default());
    let template = upload_template(&server);

    let err = client
        .upload_blob_stream(params(&template, abc_in_two_chunks(), None))
        .await
        .expect_err("size mismatch must fail");
    assert!(
        matches!(&err, ClientError::UnexpectedResponse(msg) if msg.contains("size mismatch")),
        "expected UnexpectedResponse size mismatch, got {err:?}"
    );
}

async fn mount_download(server: &MockServer, body: &'static [u8]) {
    Mock::given(method("GET"))
        .and(path("/download/account1/blob-abc/file.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .mount(server)
        .await;
}

fn download_params<'a>(
    template: &'a JmapUrlTemplate,
    expected_sha256: Option<&'a jmap_cid_types::Sha256>,
) -> DownloadBlobParams<'a> {
    DownloadBlobParams {
        download_url_template: template,
        account_id: &ACCOUNT_ID,
        blob_id: &BLOB_ID,
        name: "file.bin",
        accept_type: None,
        expected_sha256,
    }
}

/// Oracle: NIST FIPS 180-4 Appendix A — the body "abc" matches the
/// expected SHA-256("abc"), so the stream yields exactly "abc" and ends
/// without an error. `content_length()` reports the 3 octets the server
/// advertised (RFC 9110 §8.6).
#[tokio::test]
async fn download_stream_matching_digest_yields_body_then_ends() {
    let server = MockServer::start().await;
    mount_download(&server, b"abc").await;
    let client = client_for(&server, ClientConfig::default());
    let template = download_template(&server);
    let expected = jmap_cid_types::Sha256::from_hex(ABC_SHA256).expect("NIST digest parses");

    let stream = client
        .download_blob_stream(download_params(&template, Some(&expected)))
        .await
        .expect("download must start");
    assert_eq!(stream.content_length(), Some(3));

    let items: Vec<_> = stream.collect().await;
    let mut body = Vec::new();
    for item in items {
        body.extend_from_slice(&item.expect("no chunk may be an error"));
    }
    assert_eq!(body, b"abc");
}

/// Oracle: the body "abc" checked against SHA-256("") (RFC 6234) must end
/// the stream with `BlobIntegrityMismatch` carrying the caller's digest as
/// `expected` and NIST SHA-256("abc") as `actual`, after the bytes.
#[tokio::test]
async fn download_stream_digest_mismatch_is_last_item() {
    let server = MockServer::start().await;
    mount_download(&server, b"abc").await;
    let client = client_for(&server, ClientConfig::default());
    let template = download_template(&server);
    let expected = jmap_cid_types::Sha256::from_hex(EMPTY_SHA256).expect("RFC 6234 digest parses");

    let mut items: Vec<_> = client
        .download_blob_stream(download_params(&template, Some(&expected)))
        .await
        .expect("download must start")
        .collect()
        .await;

    let last = items.pop().expect("stream must yield items");
    match last {
        Err(ClientError::BlobIntegrityMismatch { expected, actual }) => {
            assert_eq!(expected, EMPTY_SHA256);
            assert_eq!(actual, ABC_SHA256);
        }
        other => panic!("expected final BlobIntegrityMismatch, got {other:?}"),
    }
    let mut body = Vec::new();
    for item in items {
        body.extend_from_slice(&item.expect("chunks before the digest check are Ok"));
    }
    assert_eq!(body, b"abc");
}

/// Oracle: security requirement shared with `download_blob` — an
/// advertised `Content-Length` of 3 octets over a 2-byte
/// `max_download_body` cap is rejected before any chunk is read.
#[tokio::test]
async fn download_stream_rejects_advertised_length_over_cap() {
    let server = MockServer::start().await;
    mount_download(&server, b"abc").await;
    let mut config = ClientConfig::default();
    config.max_download_body = 2;
    let client = client_for(&server, config);
    let template = download_template(&server);

    let err = client
        .download_blob_stream(download_params(&template, None))
        .await
        .expect_err("oversized download must fail at start");
    assert!(
        matches!(
            err,
            ClientError::ResponseTooLarge {
                actual: 3,
                limit: 2
            }
        ),
        "expected ResponseTooLarge {{ actual: 3, limit: 2 }}, got {err:?}"
    );
}
