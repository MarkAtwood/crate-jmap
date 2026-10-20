//! Wiremock integration tests for `SessionClient::filenode_download`.
//!
//! Governing text: draft-atwood-jmap-cid-00 §FileNode Extension, "Client
//! Usage" (jmap-chat-spec/draft-atwood-jmap-cid-00.md L228-239):
//!
//! > When downloading a file blob identified by a FileNode, clients SHOULD
//! > compute the SHA-256 digest of the received bytes and compare it against
//! > the FileNode's `sha256`. A `sha256` value of null has two possible
//! > causes: the FileNode is a directory (which has no associated blob and
//! > should not be downloaded), or the FileNode is a legacy file entry
//! > created before this capability was active. Clients MUST check the
//! > FileNode type to distinguish these cases. For a legacy file FileNode
//! > with null `sha256`, integrity verification via this mechanism is not
//! > available; clients SHOULD treat such downloads as unverified. A mismatch
//! > on a non-null `sha256` indicates storage corruption or modification;
//! > clients SHOULD treat the downloaded content as untrusted [...]
//!
//! Digest oracle: FIPS 180-4 / NIST CSRC "SHA256.pdf" example, one-block
//! message "abc" ->
//! `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

#[path = "helpers.rs"]
mod helpers;

use jmap_base_client::ClientError;
use jmap_filenode_client::FileNodeDownload;
use jmap_filenode_types::FileNode;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// FIPS 180-4 SHA-256("abc").
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// Download path for blob `Babc` of account `A13824` named `abc.txt`, per the
/// `downloadUrl` template in `helpers::make_session`.
const DL_PATH: &str = "/dl/A13824/Babc/abc.txt";

fn node(value: serde_json::Value) -> FileNode {
    serde_json::from_value(value).expect("fixture must deserialize as a FileNode")
}

/// A `File` node (draft-ietf-jmap-filenode-16 §3.1 file properties) carrying
/// the given `sha256`.
fn file_node(sha256: serde_json::Value) -> FileNode {
    node(json!({
        "@type": "File",
        "id": "n3",
        "parentId": null,
        "name": "abc.txt",
        "blobId": "Babc",
        "size": 3,
        "mediaType": "text/plain",
        "executable": false,
        "sha256": sha256
    }))
}

async fn serve(server: &MockServer, body: &'static [u8], hits: u64) {
    Mock::given(method("GET"))
        .and(path(DL_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .expect(hits)
        .mount(server)
        .await;
}

/// Non-null `sha256` equal to the digest of the received bytes: verified.
#[tokio::test]
async fn file_with_matching_sha256_is_verified() {
    let server = MockServer::start().await;
    serve(&server, b"abc", 1).await;
    let sc = helpers::make_client(&server);

    let got = sc
        .filenode_download(&file_node(json!(SHA256_ABC)))
        .await
        .expect("matching digest must download");

    assert_eq!(
        got,
        FileNodeDownload::Verified(bytes::Bytes::from_static(b"abc"))
    );
}

/// "A mismatch on a non-null `sha256` indicates storage corruption or
/// modification; clients SHOULD treat the downloaded content as untrusted":
/// the bytes are withheld and the mismatch is reported.
#[tokio::test]
async fn file_with_mismatched_sha256_is_rejected() {
    let server = MockServer::start().await;
    serve(&server, b"abC", 1).await;
    let sc = helpers::make_client(&server);

    let err = sc
        .filenode_download(&file_node(json!(SHA256_ABC)))
        .await
        .expect_err("mismatched digest must fail");

    match err {
        ClientError::BlobIntegrityMismatch { expected, actual } => {
            assert_eq!(expected, SHA256_ABC);
            assert_ne!(actual, SHA256_ABC);
        }
        other => panic!("expected BlobIntegrityMismatch, got {other:?}"),
    }
}

/// "For a legacy file FileNode with null `sha256` [...] clients SHOULD treat
/// such downloads as unverified."
#[tokio::test]
async fn legacy_file_with_null_sha256_is_unverified() {
    let server = MockServer::start().await;
    serve(&server, b"abc", 1).await;
    let sc = helpers::make_client(&server);

    let got = sc
        .filenode_download(&file_node(serde_json::Value::Null))
        .await
        .expect("legacy file must download");

    assert_eq!(
        got,
        FileNodeDownload::Unverified(bytes::Bytes::from_static(b"abc"))
    );
}

/// A directory "has no associated blob and should not be downloaded": its
/// null `sha256` is refused without any HTTP request.
#[tokio::test]
async fn directory_with_null_sha256_is_not_downloaded() {
    let server = MockServer::start().await;
    serve(&server, b"abc", 0).await;
    let sc = helpers::make_client(&server);
    let dir = node(json!({
        "@type": "Directory",
        "id": "n1",
        "parentId": null,
        "name": "abc.txt",
        "role": null,
        "sha256": null
    }));

    let err = sc
        .filenode_download(&dir)
        .await
        .expect_err("directory must be refused");

    assert!(
        matches!(err, ClientError::InvalidArgument(_)),
        "got {err:?}"
    );
}

/// "Clients MUST check the FileNode type to distinguish these cases": a node
/// without `@type` cannot be classified, so it is refused even though it
/// carries a `blobId`.
#[tokio::test]
async fn untyped_node_is_not_downloaded() {
    let server = MockServer::start().await;
    serve(&server, b"abc", 0).await;
    let sc = helpers::make_client(&server);
    let untyped = node(json!({
        "id": "n3",
        "parentId": null,
        "name": "abc.txt",
        "blobId": "Babc",
        "sha256": null
    }));

    let err = sc
        .filenode_download(&untyped)
        .await
        .expect_err("untyped node must be refused");

    assert!(
        matches!(err, ClientError::InvalidArgument(_)),
        "got {err:?}"
    );
}
