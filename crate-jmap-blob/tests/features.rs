//! The umbrella's re-exports under each feature set.
//!
//! Oracles: RFC 9404 §3.1.1 and draft-ietf-jmap-blobext-03 §2.1.1 (the
//! capability examples, parsed through the re-exports), and the crate's
//! documented feature table.

use jmap_blob::types::{
    BlobAccountCapability, DigestAlgorithm, JMAP_BLOB_URI, JMAP_BLOB2_URI, Rfc9404AccountCapability,
};
use serde_json::json;

/// RFC 9404 §3.1.1, the account capability object.
#[test]
fn rfc9404_capability_example_parses() {
    assert_eq!(jmap_blob::CAPABILITY, JMAP_BLOB_URI);
    assert_eq!(JMAP_BLOB_URI, "urn:ietf:params:jmap:blob");
    let cap: Rfc9404AccountCapability = serde_json::from_value(json!({
        "maxSizeBlobSet": 50000000,
        "maxDataSources": 100,
        "supportedTypeNames" : ["Mailbox", "Thread", "Email"],
        "supportedDigestAlgorithms" : ["sha", "sha-256"]
    }))
    .unwrap();
    assert_eq!(cap.max_size_blob_set, Some(50_000_000));
    assert_eq!(cap.max_data_sources, 100);
    assert_eq!(cap.supported_type_names, ["Mailbox", "Thread", "Email"]);
    assert_eq!(
        cap.supported_digest_algorithms,
        [
            DigestAlgorithm::from("sha"),
            DigestAlgorithm::from("sha-256")
        ]
    );
}

/// draft-ietf-jmap-blobext-03 §2.1.1, verbatim.
#[test]
fn blobext03_capability_example_parses() {
    assert_eq!(JMAP_BLOB2_URI, "urn:ietf:params:jmap:blob2");
    let example = json!({
        "maxSizeBlobSet": 52428800,
        "maxDataSources": 64,
        "supportedTypeNames": ["Email", "Thread", "Mailbox"],
        "supportedDigestAlgorithms": ["sha-256", "sha"],
        "uploadUrl": "https://up.example.com/jmap/upload/{accountId}/",
        "chunkSize": 5242880,
        "supportedImageReadTypes": ["image/png", "image/jpeg", "image/gif", "image/tiff"],
        "supportedImageWriteTypes": ["image/png", "image/jpeg"],
        "supportedArchiveTypes": ["application/zip", "application/x-tar"],
        "supportedExtractTypes": ["application/zip", "application/x-tar",
            "application/vnd.ms-tnef"],
        "supportedCompressTypes": ["application/gzip", "application/zstd"],
        "supportedDecompressTypes": ["application/gzip", "application/x-bzip2",
            "application/zstd"],
        "supportedDeltaTypes": ["application/x-rdiff-delta", "text/x-diff"],
        "supportedPatchTypes": ["application/x-rdiff-delta", "text/x-diff"],
        "maxConvertSize": 104857600,
        "maxArchiveEntries": 10000,
        "maxImageDimension": 8192
    });
    let cap: BlobAccountCapability = serde_json::from_value(example.clone()).unwrap();
    assert_eq!(cap.max_data_sources, 64);
    assert_eq!(cap.chunk_size, Some(5_242_880));
    assert_eq!(cap.max_image_dimension, Some(8192));
    assert_eq!(serde_json::to_value(&cap).unwrap(), example);
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-blob-client.
    fn _takes<T: jmap_blob::client::JmapBlobExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    use jmap_blob::server::BlobMode;
    // blobext-03 §2.1: a request naming only the RFC 9404 URI gets RFC 9404
    // semantics.
    let using = [JMAP_BLOB_URI.to_owned()];
    assert!(matches!(BlobMode::from_using(&using), BlobMode::Rfc9404));
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    let _ = jmap_blob::server::memory::MemoryBackend::new();
}
