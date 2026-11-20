//! The memory backend's conversions (docs/design/blobext.md §7, decision
//! 13; blobext-03 §8, §8.2 to §8.4): ustar archive and extract, deferred
//! results, the stub converter.
//!
//! Oracles: hand-written entry lists and contents; the tar layout follows
//! POSIX ustar (512-octet blocks, two zero blocks at the end).

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::{BlobBackend, BlobCreate, BlobSource, ConvertOutcome, ConvertParams};
use jmap_blob_types::{
    ArchiveEntry, ArchiveRecipe, BlobRef, CompressRecipe, EntryType, ExtractRecipe, Recipe,
};
use jmap_server::BackendSetError;
use jmap_types::{Id, SetError, SetErrorType};

const NOW: i64 = 1_763_164_800;

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

fn acc() -> Id {
    id("a1")
}

fn backend() -> MemoryBackend<()> {
    let b = MemoryBackend::<()>::default().with_account(&acc());
    b.set_clock_for_test(NOW);
    b
}

async fn text(b: &MemoryBackend<()>, s: &str) -> Id {
    let bytes = s.as_bytes().to_vec();
    let size = bytes.len() as u64;
    b.create_blob(
        &(),
        &acc(),
        BlobCreate::new(vec![BlobSource::Bytes(bytes)], size),
    )
    .await
    .unwrap()
    .id
}

async fn read(b: &MemoryBackend<()>, blob: &Id) -> Option<Vec<u8>> {
    let size = b.blob_info(&(), &acc(), blob).await.unwrap()?.size;
    b.read_blob_range(&(), &acc(), blob, 0..size).await.unwrap()
}

async fn convert(
    b: &MemoryBackend<()>,
    recipe: Recipe,
) -> Result<ConvertOutcome, BackendSetError<jmap_blob_server::memory::MemoryError>> {
    b.convert(&(), &acc(), ConvertParams::new(recipe)).await
}

fn error_of<T>(r: Result<T, BackendSetError<jmap_blob_server::memory::MemoryError>>) -> SetError {
    match r {
        Err(BackendSetError::SetError(e)) => *e,
        Err(BackendSetError::Other(e)) => panic!("expected a SetError, got {e}"),
        Ok(_) => panic!("expected a SetError, got Ok"),
        Err(_) => unreachable!(),
    }
}

fn tar_of(entries: Vec<ArchiveEntry>) -> Recipe {
    Recipe::Archive(ArchiveRecipe::new("application/x-tar", entries))
}

fn extract(blob: &Id, media_type: Option<&str>) -> Recipe {
    Recipe::Extract(ExtractRecipe::new(
        BlobRef::Id(blob.clone()),
        media_type.map(str::to_owned),
    ))
}

/// Two files and a directory, as in blobext-03 §9.5.
async fn site(b: &MemoryBackend<()>) -> (Id, Id, Recipe) {
    let html = text(b, "<h1>hi</h1>\n").await;
    let css = text(b, "h1 { color: red }\n").await;
    let mut dir = ArchiveEntry::new("site/");
    dir.entry_type = Some(EntryType::Directory);
    let mut index = ArchiveEntry::file("site/index.html", BlobRef::Id(html.clone()));
    index.mode = Some("0644".into());
    let style = ArchiveEntry::file("site/style.css", BlobRef::Id(css.clone()));
    (html, css, tar_of(vec![dir, index, style]))
}

#[tokio::test]
async fn archive_is_deferred_until_read() {
    let b = backend();
    let (_, _, recipe) = site(&b).await;
    let out = convert(&b, recipe).await.unwrap();
    assert_eq!(out.media_type.as_deref(), Some("application/x-tar"));
    assert_eq!(out.size, None, "deferred: size unknown");
    assert!(out.expires.is_some());
    assert!(!out.omit_from_response);

    // blob_info generates it (blobext-03 §8: size MUST be returned).
    // 3 headers + 2 content blocks + 2 end blocks.
    let meta = b.blob_info(&(), &acc(), &out.id).await.unwrap().unwrap();
    assert_eq!(meta.size, 7 * 512);
    let tar = read(&b, &out.id).await.unwrap();
    assert_eq!(&tar[257..262], b"ustar");
    assert_eq!(&tar[0..6], b"site/\0");
    // Blocks: site/ header, index.html header, its content, style.css
    // header, its content, two end blocks.
    assert_eq!(&tar[512..512 + 16], b"site/index.html\0");
    assert_eq!(&tar[1024..1024 + 12], b"<h1>hi</h1>\n");
    assert_eq!(&tar[1536..1536 + 15], b"site/style.css\0");
    assert!(tar[tar.len() - 1024..].iter().all(|&x| x == 0));
}

#[tokio::test]
async fn extract_lists_the_archive() {
    let b = backend();
    let (_, _, recipe) = site(&b).await;
    let archive = convert(&b, recipe).await.unwrap().id;

    for media_type in [Some("application/x-tar"), None] {
        let out = convert(&b, extract(&archive, media_type)).await.unwrap();
        // §8.4: the id, type and size are the archive's.
        assert_eq!(out.id, archive);
        assert_eq!(out.media_type.as_deref(), Some("application/x-tar"));
        assert_eq!(out.size, Some(7 * 512));
        assert!(!out.is_incomplete);
        let entries = out.entries.unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["site/", "site/index.html", "site/style.css"]);
        assert_eq!(entries[0].entry_type, Some(EntryType::Directory));
        assert_eq!(entries[0].blob_id, None);
        assert_eq!(entries[1].mode.as_deref(), Some("0644"));
        // Each file entry gets a blobId of its content.
        let Some(BlobRef::Id(content)) = &entries[2].blob_id else {
            panic!("file entry blobId")
        };
        assert_eq!(read(&b, content).await.unwrap(), b"h1 { color: red }\n");
    }
}

#[tokio::test]
async fn fail_deferred_means_not_found_everywhere() {
    let b = backend();
    let (_, _, recipe) = site(&b).await;
    let archive = convert(&b, recipe).await.unwrap().id;
    b.fail_deferred_for_test(&acc(), &archive);
    assert!(b.blob_info(&(), &acc(), &archive).await.unwrap().is_none());
    assert!(
        b.read_blob_range(&(), &acc(), &archive, 0..1)
            .await
            .unwrap()
            .is_none()
    );
    let r = b.destroy_blob(&(), &acc(), &archive).await;
    assert_eq!(error_of(r).error_type, SetErrorType::NotFound);
    let r = convert(&b, extract(&archive, None)).await;
    assert_eq!(error_of(r).error_type, SetErrorType::NotFound);
}

#[tokio::test]
async fn destroyed_entry_blob_fails_the_deferred_archive() {
    let b = backend();
    let (html, _, recipe) = site(&b).await;
    let archive = convert(&b, recipe).await.unwrap().id;
    b.destroy_blob(&(), &acc(), &html).await.unwrap();
    assert!(b.blob_info(&(), &acc(), &archive).await.unwrap().is_none());
}

#[tokio::test]
async fn deferred_archive_feeds_other_operations() {
    let b = backend();
    let (_, _, recipe) = site(&b).await;
    let inner = convert(&b, recipe).await.unwrap().id;
    // An archive containing the (still deferred) archive.
    let outer = convert(
        &b,
        tar_of(vec![ArchiveEntry::file(
            "inner.tar",
            BlobRef::Id(inner.clone()),
        )]),
    )
    .await
    .unwrap()
    .id;
    // Blob/set from a deferred blob materializes it.
    let copy = b
        .create_blob(
            &(),
            &acc(),
            BlobCreate::new(
                vec![BlobSource::Blob {
                    blob_id: outer.clone(),
                    range: 0..512,
                }],
                512,
            ),
        )
        .await
        .unwrap();
    assert_eq!(&read(&b, &copy.id).await.unwrap()[0..10], b"inner.tar\0");
    let meta = b.blob_info(&(), &acc(), &outer).await.unwrap().unwrap();
    // One header, 7 blocks of content, two end blocks.
    assert_eq!(meta.size, 10 * 512);
}

#[tokio::test]
async fn archive_entry_errors() {
    let b = backend();
    let r = convert(
        &b,
        tar_of(vec![ArchiveEntry::file("a", BlobRef::Id(id("B99")))]),
    )
    .await;
    assert_eq!(error_of(r).error_type, SetErrorType::NotFound);

    let real = text(&b, "x").await;
    let mut link = ArchiveEntry::new("l");
    link.entry_type = Some(EntryType::Symlink);
    let r = convert(
        &b,
        tar_of(vec![ArchiveEntry::file("a", BlobRef::Id(real)), link]),
    )
    .await;
    let e = error_of(r);
    assert_eq!(e.error_type, SetErrorType::InvalidProperties);
    assert_eq!(
        e.properties.unwrap(),
        ["archive/entries/1/linkTarget".to_owned()]
    );
}

#[tokio::test]
async fn extract_errors() {
    let b = backend();
    let r = convert(&b, extract(&id("B99"), None)).await;
    assert_eq!(error_of(r).error_type, SetErrorType::NotFound);

    let not_tar = text(&b, "PK\u{3}\u{4} not really a zip").await;
    let r = convert(&b, extract(&not_tar, None)).await;
    assert_eq!(error_of(r).error_type, SetErrorType::from("unknownFormat"));
    let r = convert(&b, extract(&not_tar, Some("application/zip"))).await;
    assert_eq!(error_of(r).error_type, SetErrorType::from("unknownFormat"));
    let r = convert(&b, extract(&not_tar, Some("application/x-tar"))).await;
    assert_eq!(
        error_of(r).error_type,
        SetErrorType::from("conversionFailed")
    );
}

#[tokio::test]
async fn truncated_archive_is_incomplete() {
    let b = backend();
    let (_, _, recipe) = site(&b).await;
    let archive = convert(&b, recipe).await.unwrap().id;
    b.blob_info(&(), &acc(), &archive).await.unwrap();
    // Keep the directory and index.html (blocks 0 to 2), the style.css
    // header (block 3) and 10 of its 18 content octets.
    let cut = b
        .create_blob(
            &(),
            &acc(),
            BlobCreate::new(
                vec![BlobSource::Blob {
                    blob_id: archive,
                    range: 0..(4 * 512 + 10),
                }],
                4 * 512 + 10,
            ),
        )
        .await
        .unwrap()
        .id;
    let out = convert(&b, extract(&cut, None)).await.unwrap();
    assert!(out.is_incomplete);
    assert!(out.description.is_some());
    assert_eq!(out.entries.unwrap().len(), 2);
}

#[tokio::test]
async fn no_persist_archive_can_be_omitted() {
    let b = backend();
    b.omit_no_persist_for_test(true);
    let (_, _, recipe) = site(&b).await;
    let mut params = ConvertParams::new(recipe);
    params.no_persist = true;
    let out = b.convert(&(), &acc(), params).await.unwrap();
    assert!(out.omit_from_response);
    // Still usable within the request.
    assert!(b.blob_info(&(), &acc(), &out.id).await.unwrap().is_some());
}

fn gzip(blob: &Id) -> Recipe {
    Recipe::Compress(CompressRecipe::new(
        BlobRef::Id(blob.clone()),
        "application/gzip",
    ))
}

#[tokio::test]
async fn other_recipes_need_the_stub() {
    let b = backend();
    let blob = text(&b, "data").await;
    let r = convert(&b, gzip(&blob)).await;
    assert!(matches!(r, Err(BackendSetError::Other(_))), "serverFail");

    b.stub_converter_for_test(|recipe| match recipe {
        Recipe::Compress(_) => Ok(b"compressed".to_vec()),
        _ => Err(Box::new(SetError::new(SetErrorType::from(
            "conversionFailed",
        )))),
    });
    let out = convert(&b, gzip(&blob)).await.unwrap();
    assert_eq!(out.media_type.as_deref(), Some("application/gzip"));
    assert_eq!(out.size, Some(10));
    assert_eq!(read(&b, &out.id).await.unwrap(), b"compressed");

    // Inputs are checked before the stub runs.
    let r = convert(&b, gzip(&id("B99"))).await;
    assert_eq!(error_of(r).error_type, SetErrorType::NotFound);
    // A stub SetError passes through.
    let zip = Recipe::Archive(ArchiveRecipe::new("application/zip", Vec::new()));
    let r = convert(&b, zip).await;
    assert_eq!(
        error_of(r).error_type,
        SetErrorType::from("conversionFailed")
    );
}
