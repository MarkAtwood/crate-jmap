//! The memory reference backend against each `BlobBackend` hook contract
//! (docs/design/blobext.md §6, §7), and handler registration.
//!
//! Oracles: hand-written bytes and the RFC 9530 Appendix B sha-256 vector;
//! expected ids follow the documented `B` + counter scheme.

use std::sync::Arc;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::{
    BlobBackend, BlobCreate, BlobSource, ChunkRef, CreatedBlob, DigestQuery, TouchOutcome,
    register_blob_handlers,
};
use jmap_blob_types::{BlobAccountCapability, DigestAlgorithm};
use jmap_server::{BackendSetError, Dispatcher, JmapAccounts};
use jmap_types::{Id, JmapRequest, SetErrorType, State, UTCDate};
use serde_json::{Value, json};

/// 2025-11-15T00:00:00Z.
const NOW: i64 = 1_763_164_800;

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

fn acc() -> Id {
    id("a1")
}

fn backend() -> MemoryBackend {
    let b = MemoryBackend::new().with_account(&acc());
    b.set_clock_for_test(NOW);
    b
}

fn date(secs: i64) -> UTCDate {
    jmap_server::utc_date_from_epoch_seconds(secs).unwrap()
}

async fn create(b: &MemoryBackend, sources: Vec<BlobSource>) -> CreatedBlob {
    let size = sources
        .iter()
        .map(|s| match s {
            BlobSource::Bytes(v) => v.len() as u64,
            BlobSource::Blob { range, .. } => range.end - range.start,
            _ => unreachable!(),
        })
        .sum();
    b.create_blob(&(), &acc(), BlobCreate::new(sources, size))
        .await
        .unwrap()
}

async fn text(b: &MemoryBackend, s: &str) -> Id {
    create(b, vec![BlobSource::Bytes(s.as_bytes().to_vec())])
        .await
        .id
}

fn part(blob_id: &Id, range: std::ops::Range<u64>) -> BlobSource {
    BlobSource::Blob {
        blob_id: blob_id.clone(),
        range,
    }
}

async fn read_all(b: &MemoryBackend, blob: &Id) -> String {
    let size = b.blob_info(&(), &acc(), blob).await.unwrap().unwrap().size;
    let bytes = b
        .read_blob_range(&(), &acc(), blob, 0..size)
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

fn set_error_type<E>(r: Result<impl std::fmt::Debug, BackendSetError<E>>) -> SetErrorType {
    match r {
        Err(BackendSetError::SetError(e)) => e.error_type,
        other => panic!("expected a SetError, got {:?}", other.map(|_| ()).is_ok()),
    }
}

// ---- accounts and capability -----------------------------------------

#[tokio::test]
async fn accounts_must_be_registered() {
    let b = backend();
    assert!(b.account_exists(&(), &acc()).await.unwrap());
    assert!(!b.account_exists(&(), &id("nobody")).await.unwrap());
    assert!(
        b.blob_capability(&(), &id("nobody"))
            .await
            .unwrap()
            .is_none()
    );
    let r = b
        .create_blob(&(), &id("nobody"), BlobCreate::new(Vec::new(), 0))
        .await;
    assert!(matches!(r, Err(BackendSetError::Other(_))));
    assert!(
        !b.account_exists(&(), &id("nobody")).await.unwrap(),
        "a failed create registers nothing"
    );
}

#[tokio::test]
async fn default_capability_per_decision_13() {
    let cap = backend()
        .blob_capability(&(), &acc())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&cap).unwrap(),
        json!({
            "maxSizeBlobSet": null,
            "maxDataSources": 64,
            "supportedTypeNames": [],
            "supportedDigestAlgorithms": ["sha-256", "sha"],
            "uploadUrl": null,
            "chunkSize": null,
            "supportedImageReadTypes": null,
            "supportedImageWriteTypes": null,
            "supportedArchiveTypes": ["application/x-tar"],
            "supportedExtractTypes": ["application/x-tar"],
            "supportedCompressTypes": null,
            "supportedDecompressTypes": null,
            "supportedDeltaTypes": null,
            "supportedPatchTypes": null,
            "maxConvertSize": null,
            "maxArchiveEntries": null,
            "maxImageDimension": null
        })
    );
    cap.check().unwrap();
}

#[tokio::test]
async fn advertise_for_test_replaces_the_capability() {
    let b = backend();
    let mut cap = BlobAccountCapability::default();
    cap.supported_type_names = vec!["Email".into()];
    b.advertise_for_test(&acc(), cap.clone());
    assert_eq!(b.blob_capability(&(), &acc()).await.unwrap(), Some(cap));
}

// ---- create, info, read ----------------------------------------------

#[tokio::test]
async fn inline_create_info_and_ranges() {
    let b = backend();
    let mut c = BlobCreate::new(vec![BlobSource::Bytes(b"Hello, world!".to_vec())], 13);
    c.media_type = Some("text/plain".into());
    let created = b.create_blob(&(), &acc(), c).await.unwrap();
    assert_eq!(created.id, id("B1"));
    assert_eq!(created.size, 13);
    assert_eq!(created.media_type.as_deref(), Some("text/plain"));
    assert_eq!(created.expires, Some(date(NOW + 3600)));
    assert!(!created.omit_from_response);

    let meta = b
        .blob_info(&(), &acc(), &created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (meta.size, meta.media_type.as_deref()),
        (13, Some("text/plain"))
    );
    assert_eq!(meta.expires, Some(date(NOW + 3600)));

    let r = b
        .read_blob_range(&(), &acc(), &created.id, 7..12)
        .await
        .unwrap();
    assert_eq!(r.as_deref(), Some(&b"world"[..]));
    assert!(
        b.blob_info(&(), &acc(), &id("B99"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        b.read_blob_range(&(), &acc(), &id("B99"), 0..0)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn no_persist_expires_sooner_and_can_be_omitted() {
    let b = backend();
    let mut c = BlobCreate::new(vec![BlobSource::Bytes(b"x".to_vec())], 1);
    c.no_persist = true;
    let created = b.create_blob(&(), &acc(), c.clone()).await.unwrap();
    assert_eq!(created.expires, Some(date(NOW + 300)));
    assert!(!created.omit_from_response, "reported by default");

    b.omit_no_persist_for_test(true);
    assert!(
        b.create_blob(&(), &acc(), c)
            .await
            .unwrap()
            .omit_from_response
    );
    let plain = BlobCreate::new(vec![BlobSource::Bytes(b"y".to_vec())], 1);
    assert!(
        !b.create_blob(&(), &acc(), plain)
            .await
            .unwrap()
            .omit_from_response
    );
}

#[tokio::test]
async fn blob_sources_become_chunks() {
    let b = backend();
    let hello = text(&b, "Hello, ").await; // B1
    let world = text(&b, "big world!").await; // B2
    // "Hello, " + inline "the " + "world!" (offset 4 of B2).
    let joined = create(
        &b,
        vec![
            part(&hello, 0..7),
            BlobSource::Bytes(b"the ".to_vec()),
            part(&world, 4..10),
        ],
    )
    .await;
    assert_eq!(joined.size, 17);
    assert_eq!(read_all(&b, &joined.id).await, "Hello, the world!");

    // The inline piece became a leaf blob of its own (B3); the result is B4.
    assert_eq!(joined.id, id("B4"));
    let chunks = b
        .blob_chunks(&(), &acc(), &joined.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        chunks,
        vec![
            ChunkRef::new(hello.clone(), 0, 7, 7),
            ChunkRef::new(id("B3"), 0, 4, 4),
            ChunkRef::new(world.clone(), 4, 6, 10),
        ]
    );
    assert_eq!(read_all(&b, &id("B3")).await, "the ");

    // A range of a chunked blob is cut down to its leaves.
    let tail = create(&b, vec![part(&joined.id, 5..17)]).await;
    assert_eq!(read_all(&b, &tail.id).await, ", the world!");
    assert_eq!(
        b.blob_chunks(&(), &acc(), &tail.id).await.unwrap().unwrap(),
        vec![
            ChunkRef::new(hello, 5, 2, 7),
            ChunkRef::new(id("B3"), 0, 4, 4),
            ChunkRef::new(world, 4, 6, 10),
        ]
    );
    // A chunked range read across chunk boundaries.
    let r = b
        .read_blob_range(&(), &acc(), &joined.id, 5..9)
        .await
        .unwrap();
    assert_eq!(r.as_deref(), Some(&b", th"[..]));
}

#[tokio::test]
async fn bytes_blob_reports_one_whole_chunk() {
    let b = backend();
    let blob = text(&b, "abc").await;
    assert_eq!(
        b.blob_chunks(&(), &acc(), &blob).await.unwrap().unwrap(),
        vec![ChunkRef::new(blob, 0, 3, 3)]
    );
    assert!(
        b.blob_chunks(&(), &acc(), &id("B99"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn zero_sources_make_an_empty_blob() {
    let b = backend();
    let c = create(&b, Vec::new()).await;
    assert_eq!(c.size, 0);
    assert_eq!(read_all(&b, &c.id).await, "");
}

#[tokio::test]
async fn missing_or_short_source_is_not_found() {
    let b = backend();
    let r = b
        .create_blob(
            &(),
            &acc(),
            BlobCreate::new(vec![part(&id("B99"), 0..1)], 1),
        )
        .await;
    assert_eq!(set_error_type(r), SetErrorType::NotFound);
    let small = text(&b, "ab").await;
    let r = b
        .create_blob(&(), &acc(), BlobCreate::new(vec![part(&small, 0..3)], 3))
        .await;
    assert_eq!(set_error_type(r), SetErrorType::NotFound);
}

// ---- digests ----------------------------------------------------------

#[tokio::test]
async fn stored_sha256_for_whole_blobs_only() {
    let b = backend();
    let blob = text(&b, "{\"hello\": \"world\"}\n").await;
    let whole = DigestQuery::new(blob.clone(), DigestAlgorithm::Sha256, 0..19);
    // RFC 9530 Appendix B.1.
    assert_eq!(
        b.stored_digest(&(), &acc(), &whole)
            .await
            .unwrap()
            .as_deref(),
        Some("RK/0qy18MlBSVnWgjwz6lZEWjP/lF5HF9bvEF8FabDg=")
    );
    let part = DigestQuery::new(blob.clone(), DigestAlgorithm::Sha256, 0..5);
    assert_eq!(b.stored_digest(&(), &acc(), &part).await.unwrap(), None);
    let sha1 = DigestQuery::new(blob, DigestAlgorithm::Sha, 0..19);
    assert_eq!(b.stored_digest(&(), &acc(), &sha1).await.unwrap(), None);
}

#[tokio::test]
async fn default_digester_is_builtin() {
    let b = backend();
    assert!(b.digester(&DigestAlgorithm::Sha).is_some());
    assert!(b.digester(&DigestAlgorithm::Md5).is_none());
}

// ---- touch --------------------------------------------------------------

#[tokio::test]
async fn touch_honours_up_to_seven_days() {
    let b = backend();
    let blob = text(&b, "t").await;
    let in_two_days = date(NOW + 2 * 86_400);
    assert_eq!(
        b.touch_blob(&(), &acc(), &blob, Some(in_two_days.clone()))
            .await
            .unwrap(),
        TouchOutcome::Honoured
    );
    let meta = b.blob_info(&(), &acc(), &blob).await.unwrap().unwrap();
    assert_eq!(meta.expires, Some(in_two_days));

    let week = date(NOW + 7 * 86_400);
    assert_eq!(
        b.touch_blob(&(), &acc(), &blob, Some(date(NOW + 30 * 86_400)))
            .await
            .unwrap(),
        TouchOutcome::Applied(Some(week.clone()))
    );
    assert_eq!(
        b.touch_blob(&(), &acc(), &blob, None).await.unwrap(),
        TouchOutcome::Applied(Some(week))
    );
    let r = b.touch_blob(&(), &acc(), &id("B99"), None).await;
    assert_eq!(set_error_type(r), SetErrorType::NotFound);
}

// ---- destroy and references -------------------------------------------

#[tokio::test]
async fn destroy_contracts() {
    let b = backend();
    let free = text(&b, "free").await;
    b.destroy_blob(&(), &acc(), &free).await.unwrap();
    assert!(b.blob_info(&(), &acc(), &free).await.unwrap().is_none());
    let r = b.destroy_blob(&(), &acc(), &free).await;
    assert_eq!(set_error_type(r), SetErrorType::NotFound);

    // Referenced by an object.
    let attached = text(&b, "attached").await;
    b.add_reference_for_test(&acc(), "Email", &id("E1"), &attached);
    let r = b.destroy_blob(&(), &acc(), &attached).await;
    assert_eq!(set_error_type(r), SetErrorType::from("blobHasReference"));
    // A referenced blob has no specific expiry.
    let meta = b.blob_info(&(), &acc(), &attached).await.unwrap().unwrap();
    assert_eq!(meta.expires, None);

    // Used as a chunk by another blob.
    let leaf = text(&b, "leaf").await;
    let parent = create(&b, vec![part(&leaf, 0..4)]).await;
    let r = b.destroy_blob(&(), &acc(), &leaf).await;
    assert_eq!(set_error_type(r), SetErrorType::from("blobHasReference"));
    b.destroy_blob(&(), &acc(), &parent.id).await.unwrap();
    b.destroy_blob(&(), &acc(), &leaf).await.unwrap();
}

#[tokio::test]
async fn blob_references_contracts() {
    let b = backend();
    let shared = text(&b, "shared").await;
    let lonely = text(&b, "lonely").await;
    b.add_reference_for_test(&acc(), "Email", &id("E1"), &shared);
    b.add_reference_for_test(&acc(), "Email", &id("E2"), &shared);
    b.add_reference_for_test(&acc(), "Mailbox", &id("M1"), &shared);
    b.add_reference_for_test(&acc(), "Email", &id("E3"), &shared);
    b.hide_object_for_test(&acc(), "Email", &id("E3"));
    // A reference to a blob that does not exist.
    b.add_reference_for_test(&acc(), "Email", &id("E4"), &id("B99"));

    let r = b
        .blob_references(
            &(),
            &acc(),
            "Email",
            &[shared.clone(), lonely.clone(), id("B99")],
        )
        .await
        .unwrap();
    assert_eq!(r[&shared], [id("E1"), id("E2")]);
    assert!(r[&lonely].is_empty());
    assert!(!r.contains_key(&id("B99")), "no entry for a missing blob");

    let m = b
        .blob_references(&(), &acc(), "Mailbox", std::slice::from_ref(&shared))
        .await
        .unwrap();
    assert_eq!(m[&shared], [id("M1")]);
}

#[tokio::test]
async fn lookup_type_capability_reads_the_registry() {
    let b = backend();
    assert_eq!(
        b.lookup_type_capability("Email"),
        Some("urn:ietf:params:jmap:mail")
    );
    // Identity cannot reference blobs (RFC 9404 Table 1).
    assert_eq!(b.lookup_type_capability("Identity"), None);
    assert_eq!(b.lookup_type_capability("NoSuchType"), None);
}

#[tokio::test]
async fn image_data_defaults_to_none() {
    let b = backend();
    let blob = text(&b, "not an image").await;
    assert_eq!(b.image_data(&(), &acc(), &blob).await.unwrap(), None);
}

// ---- registration ------------------------------------------------------

#[tokio::test]
async fn handlers_are_registered() {
    let mut d = Dispatcher::new();
    register_blob_handlers(&mut d, Arc::new(backend()));
    let req: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:blob2"],
        "methodCalls": [
            ["Blob/get", {"accountId": "a1", "ids": []}, "0"],
            ["Blob/set", {"accountId": "a1"}, "1"],
            ["Blob/lookup", {"accountId": "a1", "typeNames": [], "ids": []}, "2"],
            ["Blob/convert", {"accountId": "a1", "create": {}}, "3"],
            ["Blob/upload", {"accountId": "a1", "create": {}}, "4"],
            ["Blob/nope", {"accountId": "a1"}, "5"]
        ]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let errors: Vec<&Value> = resp["methodResponses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|inv| &inv[1]["type"])
        .collect();
    // The five Blob methods are mounted and answer (no error type); an
    // unregistered method is unknownMethod.
    assert_eq!(resp["methodResponses"][0][0], "Blob/get");
    assert_eq!(resp["methodResponses"][1][0], "Blob/set");
    assert_eq!(resp["methodResponses"][2][0], "Blob/lookup");
    assert_eq!(resp["methodResponses"][3][0], "Blob/convert");
    assert_eq!(resp["methodResponses"][4][0], "Blob/upload");
    assert_eq!(
        errors,
        [
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &json!("unknownMethod")
        ]
    );
}
