//! `Blob/convert` (blobext-03 §8) against the memory backend: every case in
//! docs/design/blobext.md §10 "server, Blob/convert".
//!
//! Tar archive and extract are the memory backend's own; every other recipe
//! runs through a stub converter (decision 13) whose output names the
//! inputs it was given (`GZ:B4`), so a test can see how each reference
//! resolved. Oracles: the §9.4-§9.7 examples and the §8 rules quoted in
//! each test; expiry times follow the fixed clock (`NOW` is
//! 2025-11-15T00:00:00Z, +1 h, or +5 min for `noPersist`).

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::{
    BlobBackend, BlobCreate, BlobSource, handle_blob_convert, handle_blob_get, handle_blob_set,
    register_blob_handlers,
};
use jmap_blob_types::{BlobAccountCapability, Recipe};
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapError, JmapRequest, SetError, SetErrorType, State};
use serde_json::{Value, json};

const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const BLOB: &str = "urn:ietf:params:jmap:blob";
const NOW: i64 = 1_763_164_800;
const EXPIRES: &str = "2025-11-15T01:00:00Z";
const EXPIRES_NO_PERSIST: &str = "2025-11-15T00:05:00Z";

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

fn acc() -> Id {
    id("abc")
}

fn list(types: &[&str]) -> Option<Vec<String>> {
    Some(types.iter().map(|t| (*t).to_owned()).collect())
}

/// Every conversion advertised, with small limits to test against.
fn capability() -> BlobAccountCapability {
    let mut c = MemoryBackend::default_capability();
    c.supported_image_read_types = list(&["image/png", "image/jpeg"]);
    c.supported_image_write_types = list(&["image/png", "image/jpeg"]);
    c.supported_archive_types =
        list(&["application/x-tar", "application/zip", "application/x-cpio"]);
    c.supported_extract_types = list(&["application/x-tar"]);
    c.supported_compress_types = list(&["application/gzip"]);
    c.supported_decompress_types = list(&["application/gzip"]);
    c.supported_delta_types = list(&["text/x-diff"]);
    c.supported_patch_types = list(&["text/x-diff"]);
    c.max_convert_size = Some(10_000);
    c.max_image_dimension = Some(1000);
    c.max_archive_entries = Some(8);
    c
}

fn set_error(t: &str, why: &str) -> Box<SetError> {
    Box::new(SetError::new(SetErrorType::from(t)).with_description(why))
}

/// The stub converter: output names the (resolved) inputs. `decompressed`
/// is what a DecompressRecipe yields. ImageConvert with quality 13 fails
/// (`conversionFailed`); a DecompressRecipe without a type cannot detect
/// a format (`unknownFormat`).
fn stub(decompressed: Vec<u8>) -> impl Fn(&Recipe) -> Result<Vec<u8>, Box<SetError>> {
    move |recipe| match recipe {
        Recipe::ImageConvert(r) if r.quality == Some(13) => {
            Err(set_error("conversionFailed", "stub: no image"))
        }
        Recipe::ImageConvert(r) => Ok(format!("IMG:{}", r.blob_id).into_bytes()),
        Recipe::Archive(r) => {
            let ids: Vec<String> = r
                .entries
                .iter()
                .filter_map(|e| e.blob_id.as_ref().map(ToString::to_string))
                .collect();
            Ok(format!("ZIP:{}", ids.join(",")).into_bytes())
        }
        Recipe::Compress(r) => Ok(format!("GZ:{}", r.blob_id).into_bytes()),
        Recipe::Decompress(r) if r.media_type.is_none() => {
            Err(set_error("unknownFormat", "stub: cannot detect"))
        }
        Recipe::Decompress(_) => Ok(decompressed.clone()),
        Recipe::Delta(r) => Ok(format!("DIFF:{}:{}", r.blob_id, r.new_blob_id).into_bytes()),
        Recipe::Patch(r) => Ok(format!("NEW:{}:{}", r.blob_id, r.delta_blob_id).into_bytes()),
        _ => Err(set_error("conversionFailed", "stub: unknown recipe")),
    }
}

fn backend() -> MemoryBackend {
    let b = MemoryBackend::new().with_account(&acc());
    b.set_clock_for_test(NOW);
    b.advertise_for_test(&acc(), capability());
    b.stub_converter_for_test(stub(Vec::new()));
    b
}

fn using(uris: &[&str]) -> Vec<String> {
    let mut v = vec!["urn:ietf:params:jmap:core".to_owned()];
    v.extend(uris.iter().map(|u| (*u).to_owned()));
    v
}

async fn put(b: &MemoryBackend, bytes: &[u8]) -> Id {
    b.create_blob(
        &(),
        &acc(),
        BlobCreate::new(vec![BlobSource::Bytes(bytes.to_vec())], bytes.len() as u64),
    )
    .await
    .unwrap()
    .id
}

async fn read(b: &MemoryBackend, blob: &Id) -> Vec<u8> {
    let size = b.blob_info(&(), &acc(), blob).await.unwrap().unwrap().size;
    b.read_blob_range(&(), &acc(), blob, 0..size)
        .await
        .unwrap()
        .unwrap()
}

async fn call_with(
    b: &MemoryBackend,
    args: Value,
    created: &BTreeMap<Id, Id>,
    uris: &[&str],
) -> Result<Value, JmapError> {
    handle_blob_convert(b, &(), args, created, &using(uris))
        .await
        .map(|out| out.primary)
}

async fn call(b: &MemoryBackend, args: Value) -> Result<Value, JmapError> {
    call_with(b, args, &BTreeMap::new(), &[BLOB2]).await
}

async fn convert(b: &MemoryBackend, create: Value) -> Value {
    call(b, json!({"accountId": "abc", "create": create}))
        .await
        .unwrap()
}

/// The `notCreated` entry of a single conversion `entry`.
async fn rejected(b: &MemoryBackend, entry: Value) -> Value {
    let resp = convert(b, json!({ "c": entry })).await;
    assert_eq!(resp["created"], Value::Null, "{entry} was created: {resp}");
    resp["notCreated"]["c"].clone()
}

/// The `created` entry of a single conversion `entry`.
async fn accepted(b: &MemoryBackend, entry: Value) -> Value {
    let resp = convert(b, json!({ "c": entry })).await;
    assert_eq!(resp["notCreated"], Value::Null, "{entry} failed: {resp}");
    resp["created"]["c"].clone()
}

fn error_type(r: Result<Value, JmapError>) -> String {
    match r {
        Err(e) => serde_json::to_value(&e).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned(),
        Ok(v) => panic!("expected an error, got {v}"),
    }
}

async fn dispatch(b: Arc<MemoryBackend>, request: Value) -> Value {
    let mut d = Dispatcher::new();
    register_blob_handlers(&mut d, b);
    let req: JmapRequest = serde_json::from_value(request).unwrap();
    serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap()
}

/// The three files of the §9.4-§9.6 examples: B1, B2, B3.
async fn site(b: &MemoryBackend) -> [Id; 3] {
    [
        put(b, b"<h1>hi</h1>\n").await,
        put(b, b"h1 { color: red }\n").await,
        put(b, b"\xff\xd8 jpeg").await,
    ]
}

// ---- the -03 examples ----------------------------------------------------

/// -03 §9.4 through the stub (zip), verbatim apart from the minted ids, the
/// stub's size, and `expires` (§7 requires it; the example leaves it out).
/// An empty `notCreated` is `null` (gap a).
#[tokio::test]
async fn example_9_4_zip() {
    let b = backend();
    let [html, css, jpg] = site(&b).await;
    let resp = call(
        &b,
        json!({
            "accountId": "abc",
            "create": {
                "z1": {
                    "archive": {
                        "type": "application/zip",
                        "entries": [
                            {"name": "site/index.html", "blobId": html},
                            {"name": "site/style.css", "blobId": css},
                            {"name": "site/photo.jpg", "blobId": jpg}
                        ]
                    }
                }
            }
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        resp,
        json!({
            "accountId": "abc",
            "created": {
                "z1": {
                    "id": "B4",
                    "type": "application/zip",
                    "size": 12,
                    "expires": EXPIRES
                }
            },
            "notCreated": null
        })
    );
    assert_eq!(read(&b, &id("B4")).await, b"ZIP:B1,B2,B3");
}

fn example_9_5_request(files: &[Id; 3]) -> Value {
    let [html, css, jpg] = files;
    json!({
        "t1": {
            "noPersist": true,
            "archive": {
                "type": "application/x-tar",
                "entries": [
                    {"name": "site/index.html", "blobId": html,
                     "modified": "2026-03-01T12:00:00Z", "mode": "0644"},
                    {"name": "site/style.css", "blobId": css,
                     "modified": "2026-03-01T12:00:00Z", "mode": "0644"},
                    {"name": "site/photo.jpg", "blobId": jpg,
                     "modified": "2026-02-15T09:30:00Z", "mode": "0644"}
                ]
            }
        },
        "t2": {
            "compress": {
                "blobId": "#t1",
                "type": "application/gzip"
            }
        }
    })
}

/// -03 §9.5: a noPersist tar (memory backend) compressed through the stub.
/// With the omit knob `t1` is left out of `created` and `createdIds`, yet
/// `t2` still reads it.
#[tokio::test]
async fn example_9_5_tar_gz_omitted() {
    let b = Arc::new(backend());
    b.omit_no_persist_for_test(true);
    let files = site(&b).await;
    let resp = dispatch(
        b.clone(),
        json!({
            "using": using(&[BLOB2]),
            "methodCalls": [["Blob/convert", {
                "accountId": "abc",
                "create": example_9_5_request(&files)
            }, "0"]],
            "createdIds": {}
        }),
    )
    .await;
    assert_eq!(
        resp["methodResponses"][0][1],
        json!({
            "accountId": "abc",
            "created": {
                "t2": {
                    "id": "B5",
                    "type": "application/gzip",
                    "size": 5,
                    "expires": EXPIRES
                }
            },
            "notCreated": null
        })
    );
    assert_eq!(resp["createdIds"], json!({"t2": "B5"}));
    // t2 compressed t1 (B4), the tar, which exists for the request.
    assert_eq!(read(&b, &id("B5")).await, b"GZ:B4");
    assert_eq!(&read(&b, &id("B4")).await[257..262], b"ustar");
}

/// Without the knob the noPersist tar is reported: deferred, so `size` is
/// null (§8), and expiring after 5 minutes.
#[tokio::test]
async fn example_9_5_tar_gz_reported() {
    let b = backend();
    let files = site(&b).await;
    let resp = convert(&b, example_9_5_request(&files)).await;
    assert_eq!(
        resp["created"]["t1"],
        json!({
            "id": "B4",
            "type": "application/x-tar",
            "size": null,
            "expires": EXPIRES_NO_PERSIST
        })
    );
    assert_eq!(resp["created"]["t2"]["id"], "B5");
}

/// -03 §9.6: decompress (stub) then extract (memory backend). The extract's
/// id and size are the decompressed tar's; the stub gives that blob no
/// type, so `type` is null where the example has `application/x-tar`.
/// Every file entry gets a blobId of its content.
#[tokio::test]
async fn example_9_6_extract() {
    let b = backend();
    let files = site(&b).await;
    let tar_id = convert(&b, example_9_5_request(&files)).await["created"]["t1"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let tar = read(&b, &id(&tar_id)).await;
    b.stub_converter_for_test(stub(tar.clone()));
    let gz = put(&b, b"gz").await;
    b.omit_no_persist_for_test(true);
    let resp = convert(
        &b,
        json!({
            "u1": {
                "noPersist": true,
                "decompress": {"blobId": gz, "type": "application/gzip"}
            },
            "u2": {
                "extract": {"blobId": "#u1", "type": "application/x-tar"}
            }
        }),
    )
    .await;
    assert_eq!(resp["notCreated"], Value::Null);
    let created = resp["created"].as_object().unwrap();
    assert_eq!(created.keys().collect::<Vec<_>>(), ["u2"]);
    let u2 = &created["u2"];
    // B1-B5 are the files and the setup's t1/t2, B6 the gzip blob, B7
    // u1's decompressed tar; the entries are B8, B9, B10.
    assert_eq!(u2["id"], "B7");
    assert_eq!(u2["type"], Value::Null);
    assert_eq!(u2["size"], json!(tar.len()));
    assert_eq!(u2["expires"], EXPIRES_NO_PERSIST);
    assert_eq!(
        u2["entries"],
        json!([
            {"name": "site/index.html", "blobId": "B8", "entryType": "file",
             "modified": "2026-03-01T12:00:00Z", "mode": "0644",
             "uid": 0, "gid": 0},
            {"name": "site/style.css", "blobId": "B9", "entryType": "file",
             "modified": "2026-03-01T12:00:00Z", "mode": "0644",
             "uid": 0, "gid": 0},
            {"name": "site/photo.jpg", "blobId": "B10", "entryType": "file",
             "modified": "2026-02-15T09:30:00Z", "mode": "0644",
             "uid": 0, "gid": 0}
        ])
    );
    assert_eq!(read(&b, &id("B9")).await, b"h1 { color: red }\n");
}

/// -03 §9.7 through the stub: delta, then patch with that delta.
#[tokio::test]
async fn example_9_7_delta_and_patch() {
    let b = backend();
    let old = put(&b, b"old\n").await;
    let new = put(&b, b"new\n").await;
    let d1 = convert(
        &b,
        json!({"d1": {"delta": {"blobId": old, "newBlobId": new, "type": "text/x-diff"}}}),
    )
    .await;
    assert_eq!(
        d1["created"]["d1"],
        json!({"id": "B3", "type": "text/x-diff", "size": 10, "expires": EXPIRES})
    );
    let p1 = convert(
        &b,
        json!({"p1": {"patch": {"blobId": old, "deltaBlobId": "B3", "deltaType": "text/x-diff"}}}),
    )
    .await;
    // The stub leaves the patched blob untyped (the example's
    // application/octet-stream is the server's choice).
    assert_eq!(
        p1["created"]["p1"],
        json!({"id": "B4", "type": null, "size": 9, "expires": EXPIRES})
    );
    assert_eq!(read(&b, &id("B4")).await, b"NEW:B1:B3");
}

// ---- method-level errors -------------------------------------------------

/// Decision 19: Blob/convert is not a Foo/set; `atomic: true` and a
/// non-empty `ifUnchangedBy` are refused, their harmless forms accepted.
#[tokio::test]
async fn conditional_arguments() {
    let b = backend();
    let blob = put(&b, b"x").await;
    let create = json!({"c": {"compress": {"blobId": blob, "type": "application/gzip"}}});
    for (extra, ok) in [
        (json!({"atomic": true}), false),
        (json!({"atomic": "yes"}), false),
        (json!({"ifUnchangedBy": {"x": {}}}), false),
        (json!({"ifUnchangedBy": []}), false),
        (json!({"atomic": false}), true),
        (json!({"atomic": null}), true),
        (json!({"ifUnchangedBy": {}}), true),
        (json!({"ifUnchangedBy": null}), true),
    ] {
        let mut args = json!({"accountId": "abc", "create": create});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let r = call(&b, args).await;
        if ok {
            assert!(r.unwrap()["created"]["c"].is_object(), "{extra}");
        } else {
            assert_eq!(error_type(r), "invalidArguments", "{extra}");
        }
    }
}

#[tokio::test]
async fn method_errors() {
    let b = backend();
    for args in [
        json!({"accountId": "abc"}),
        json!({"accountId": "abc", "create": null}),
        json!({"accountId": "abc", "create": []}),
        json!({"create": {}}),
    ] {
        assert_eq!(
            error_type(call(&b, args.clone()).await),
            "invalidArguments",
            "{args}"
        );
    }
    let r = call(&b, json!({"accountId": "zz", "create": {}})).await;
    assert_eq!(error_type(r), "accountNotFound");
    let r = call_with(
        &b,
        json!({"accountId": "abc", "create": {}}),
        &BTreeMap::new(),
        &[BLOB, BLOB2],
    )
    .await;
    assert_eq!(error_type(r), "invalidArguments");
    let many: serde_json::Map<String, Value> = (0..501)
        .map(|n| (format!("c{n}"), json!({"compress": {}})))
        .collect();
    let r = call(&b, json!({"accountId": "abc", "create": many})).await;
    assert_eq!(error_type(r), "requestTooLarge");
}

/// Blob/convert runs in RFC 9404 mode too (§5 of the design: only `using`
/// conflicts are refused).
#[tokio::test]
async fn runs_in_rfc9404_mode() {
    let b = backend();
    let blob = put(&b, b"x").await;
    let r = call_with(
        &b,
        json!({"accountId": "abc", "create": {"c": {"compress": {"blobId": blob, "type": "application/gzip"}}}}),
        &BTreeMap::new(),
        &[BLOB],
    )
    .await
    .unwrap();
    assert_eq!(r["created"]["c"]["id"], "B2");
}

// ---- shape (gap h paths) -------------------------------------------------

fn assert_invalid(err: &Value, properties: Value) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], properties, "{err}");
}

#[tokio::test]
async fn malformed_requests_name_their_path() {
    let b = backend();
    let ok = json!({"blobId": "B1", "type": "application/gzip"});
    let entry = |e: Value| json!({"archive": {"type": "application/x-tar", "entries": [e]}});
    for (request, properties) in [
        (json!({}), json!([])),
        (json!("compress"), json!([])),
        (
            json!({"compress": ok, "delta": {}}),
            json!(["compress", "delta"]),
        ),
        (
            json!({"compress": ok, "noPersist": "yes"}),
            json!(["noPersist"]),
        ),
        (json!({"compress": ok, "speed": 1}), json!(["speed"])),
        (json!({"compress": "x"}), json!(["compress"])),
        (
            json!({"compress": {"blobId": "B1"}}),
            json!(["compress/type"]),
        ),
        (
            json!({"compress": {"blobId": "not an id", "type": "application/gzip"}}),
            json!(["compress/blobId"]),
        ),
        (
            json!({"compress": {"blobId": "B1", "type": "application/gzip", "level": -1}}),
            json!(["compress/level"]),
        ),
        (
            json!({"compress": {"blobId": "B1", "type": "application/gzip", "speed": 1}}),
            json!(["compress/speed"]),
        ),
        (
            json!({"patch": {"blobId": "B1", "deltaBlobId": "B1"}}),
            json!(["patch/deltaType"]),
        ),
        (
            json!({"archive": {"type": "application/x-tar", "entries": "x"}}),
            json!(["archive/entries"]),
        ),
        (entry(json!("x")), json!(["archive/entries/0"])),
        (entry(json!({"name": 1})), json!(["archive/entries/0/name"])),
        (
            entry(json!({"name": "a", "blobId": "B1", "modified": "yesterday"})),
            json!(["archive/entries/0/modified"]),
        ),
        (
            entry(json!({"name": "a", "blobId": "B1", "bogus": true})),
            json!(["archive/entries/0/bogus"]),
        ),
    ] {
        assert_invalid(&rejected(&b, request).await, properties);
    }
    // An invalid creation id.
    let resp = convert(&b, json!({"not an id": {"compress": ok}})).await;
    assert_invalid(&resp["notCreated"]["not an id"], json!(["id"]));
}

// ---- capability ----------------------------------------------------------

/// §8.1-§8.8: a type outside its advertised list is `invalidProperties`;
/// a `null` list means the conversion is unsupported (gap h: the recipe
/// key). Media types compare case-insensitively.
#[tokio::test]
async fn advertised_types() {
    let b = backend();
    let blob = put(&b, b"x").await;
    for (request, properties) in [
        (
            json!({"compress": {"blobId": blob, "type": "application/x-xz"}}),
            json!(["compress/type"]),
        ),
        (
            json!({"decompress": {"blobId": blob, "type": "application/zstd"}}),
            json!(["decompress/type"]),
        ),
        (
            json!({"archive": {"type": "application/x-7z-compressed", "entries": []}}),
            json!(["archive/type"]),
        ),
        (
            json!({"extract": {"blobId": blob, "type": "application/zip"}}),
            json!(["extract/type"]),
        ),
        (
            json!({"imageConvert": {"blobId": blob, "type": "image/gif"}}),
            json!(["imageConvert/type"]),
        ),
        (
            json!({"delta": {"blobId": blob, "newBlobId": blob, "type": "application/x-bsdiff"}}),
            json!(["delta/type"]),
        ),
        (
            json!({"patch": {"blobId": blob, "deltaBlobId": blob, "deltaType": "application/x-bsdiff"}}),
            json!(["patch/deltaType"]),
        ),
    ] {
        assert_invalid(&rejected(&b, request).await, properties);
    }
    accepted(
        &b,
        json!({"compress": {"blobId": blob, "type": "Application/GZIP"}}),
    )
    .await;

    let mut cap = capability();
    cap.supported_compress_types = None;
    cap.supported_image_write_types = None;
    b.advertise_for_test(&acc(), cap);
    for (request, key) in [
        (
            json!({"compress": {"blobId": blob, "type": "application/gzip"}}),
            "compress",
        ),
        (
            json!({"imageConvert": {"blobId": blob, "type": "image/png"}}),
            "imageConvert",
        ),
    ] {
        assert_invalid(&rejected(&b, request).await, json!([key]));
    }
}

/// §8.4, §8.6: a null extract or decompress type skips the list check and
/// asks the backend to detect the format, which can fail with
/// `unknownFormat`.
#[tokio::test]
async fn null_type_auto_detects() {
    let b = backend();
    let blob = put(&b, b"not an archive").await;
    let e = rejected(&b, json!({"decompress": {"blobId": blob}})).await;
    assert_eq!(e["type"], "unknownFormat");
    let e = rejected(&b, json!({"extract": {"blobId": blob, "type": null}})).await;
    assert_eq!(e["type"], "unknownFormat");
}

/// §8.1: "the requested dimensions exceed maxImageDimension" → `tooLarge`;
/// quality is 1 to 100; background a CSS hex colour.
#[tokio::test]
async fn image_limits() {
    let b = backend();
    let img = put(&b, b"png").await;
    let image = |extra: Value| {
        let mut r = json!({"blobId": img, "type": "image/png"});
        r.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({ "imageConvert": r })
    };
    let e = rejected(&b, image(json!({"width": 1001}))).await;
    assert_eq!(e["type"], "tooLarge");
    assert_eq!(e["properties"], json!(["imageConvert/width"]));
    let e = rejected(&b, image(json!({"height": 1001}))).await;
    assert_eq!(e["properties"], json!(["imageConvert/height"]));
    for q in [0, 101] {
        assert_invalid(
            &rejected(&b, image(json!({ "quality": q }))).await,
            json!(["imageConvert/quality"]),
        );
    }
    for bg in ["white", "#ff", "ffffff"] {
        assert_invalid(
            &rejected(&b, image(json!({ "background": bg }))).await,
            json!(["imageConvert/background"]),
        );
    }
    let ok = image(json!({
        "width": 1000, "height": 1, "quality": 100, "background": "#ffffff",
        "colorSpace": "sRGB", "ignoreAspect": true, "stripMetadata": null, "autoOrient": false
    }));
    accepted(&b, ok).await;
}

/// §8: "If the conversion failed entirely … the server MUST return a
/// conversionFailed SetError".
#[tokio::test]
async fn conversion_failed_from_backend() {
    let b = backend();
    let img = put(&b, b"png").await;
    let e = rejected(
        &b,
        json!({"imageConvert": {"blobId": img, "type": "image/png", "quality": 13}}),
    )
    .await;
    assert_eq!(e["type"], "conversionFailed");
}

// ---- archive entries (§8.3) ----------------------------------------------

#[tokio::test]
async fn archive_entry_rules() {
    let b = backend();
    let f = put(&b, b"x").await;
    // cpio goes to the stub, which checks nothing: these are the
    // handler's own checks.
    let cpio = |e: Value| json!({"archive": {"type": "application/x-cpio", "entries": [{"name": "ok", "blobId": f}, e]}});
    let tar = |e: Value| json!({"archive": {"type": "application/x-tar", "entries": [{"name": "ok", "blobId": f}, e]}});
    let zip = |e: Value| json!({"archive": {"type": "application/zip", "entries": [e]}});
    for (request, property) in [
        (cpio(json!({"name": "a"})), "archive/entries/1/blobId"),
        (
            cpio(json!({"name": "d/", "entryType": "directory", "blobId": f})),
            "archive/entries/1/blobId",
        ),
        (
            cpio(json!({"name": "d", "entryType": "directory"})),
            "archive/entries/1/name",
        ),
        (
            cpio(json!({"name": "l", "entryType": "symlink"})),
            "archive/entries/1/linkTarget",
        ),
        (
            cpio(json!({"name": "a", "blobId": f, "linkTarget": "b"})),
            "archive/entries/1/linkTarget",
        ),
        (
            cpio(json!({"name": "a/../b", "blobId": f})),
            "archive/entries/1/name",
        ),
        (
            cpio(json!({"name": "../b", "blobId": f})),
            "archive/entries/1/name",
        ),
        (
            cpio(json!({"name": "a", "blobId": f, "mode": "0999"})),
            "archive/entries/1/mode",
        ),
        (
            cpio(json!({"name": "a", "blobId": f, "mode": "17777"})),
            "archive/entries/1/mode",
        ),
        (
            cpio(json!({"name": "a", "blobId": f, "mode": ""})),
            "archive/entries/1/mode",
        ),
        (
            zip(json!({"name": "l", "entryType": "symlink", "linkTarget": "x"})),
            "archive/entries/0/entryType",
        ),
        (
            zip(json!({"name": "p", "entryType": "fifo"})),
            "archive/entries/0/entryType",
        ),
        // Not a §8.3 type: the handler lets it through, the tar writer
        // refuses it.
        (
            tar(json!({"name": "s", "entryType": "socket"})),
            "archive/entries/1/entryType",
        ),
    ] {
        assert_invalid(&rejected(&b, request).await, json!([property]));
    }
    // Every §8.3 type is fine in tar and cpio; zip takes files and
    // directories.
    let all = |t: &str| {
        json!({"archive": {"type": t, "entries": [
            {"name": "d/", "entryType": "directory"},
            {"name": "d/f", "blobId": f, "mode": "0755"},
            {"name": "d/s", "entryType": "symlink", "linkTarget": "f"},
            {"name": "d/h", "entryType": "hardlink", "linkTarget": "d/f"},
            {"name": "d/p", "entryType": "fifo"},
            {"name": "d/b", "entryType": "blockDevice", "devMajor": 8, "devMinor": 0},
            {"name": "d/c", "entryType": "charDevice", "devMajor": 1, "devMinor": 3}
        ]}})
    };
    accepted(&b, all("application/x-tar")).await;
    accepted(&b, all("application/x-cpio")).await;
    accepted(&b, zip(json!({"name": "d/", "entryType": "directory"}))).await;
}

#[tokio::test]
async fn max_archive_entries() {
    let b = backend();
    let f = put(&b, b"x").await;
    let entries: Vec<Value> = (0..9)
        .map(|i| json!({"name": format!("f{i}"), "blobId": f}))
        .collect();
    let e = rejected(
        &b,
        json!({"archive": {"type": "application/x-tar", "entries": entries}}),
    )
    .await;
    assert_eq!(e["type"], "tooLarge");
    assert_eq!(e["properties"], json!(["archive/entries"]));
    accepted(
        &b,
        json!({"archive": {"type": "application/x-tar", "entries": entries[..8]}}),
    )
    .await;
}

// ---- inputs --------------------------------------------------------------

/// §8.1-§8.8: "tooLarge" when a referenced blob exceeds maxConvertSize,
/// including archive entries and the second input of delta and patch.
#[tokio::test]
async fn max_convert_size() {
    let b = backend();
    let small = put(&b, &[b'x'; 10_000]).await;
    let big = put(&b, &[b'x'; 10_001]).await;
    for (request, property) in [
        (
            json!({"compress": {"blobId": big, "type": "application/gzip"}}),
            "compress/blobId",
        ),
        (
            json!({"archive": {"type": "application/x-tar", "entries": [
            {"name": "a", "blobId": small}, {"name": "b", "blobId": big}]}}),
            "archive/entries/1/blobId",
        ),
        (
            json!({"delta": {"blobId": small, "newBlobId": big, "type": "text/x-diff"}}),
            "delta/newBlobId",
        ),
        (
            json!({"patch": {"blobId": small, "deltaBlobId": big, "deltaType": "text/x-diff"}}),
            "patch/deltaBlobId",
        ),
    ] {
        let e = rejected(&b, request).await;
        assert_eq!(e["type"], "tooLarge");
        assert_eq!(e["properties"], json!([property]));
        assert_eq!(e["maxSize"], 10_000);
    }
    accepted(
        &b,
        json!({"compress": {"blobId": small, "type": "application/gzip"}}),
    )
    .await;
}

#[tokio::test]
async fn missing_inputs_are_not_found() {
    let b = backend();
    let e = rejected(
        &b,
        json!({"compress": {"blobId": "B99", "type": "application/gzip"}}),
    )
    .await;
    assert_eq!(e["type"], "notFound");
    assert_eq!(e["notFound"], json!(["B99"]));
    let e = rejected(
        &b,
        json!({"compress": {"blobId": "#nope", "type": "application/gzip"}}),
    )
    .await;
    assert_eq!(e["type"], "notFound");
}

/// §8: references resolve to earlier entries whatever the map order, to a
/// failed entry as `notFound`, and to earlier calls' creations.
#[tokio::test]
async fn creation_references() {
    let b = backend();
    let blob = put(&b, b"x").await;
    let resp = convert(
        &b,
        json!({
            // "a" sorts first but needs "b".
            "a": {"compress": {"blobId": "#b", "type": "application/gzip"}},
            "b": {"compress": {"blobId": blob, "type": "application/gzip"}},
            "bad": {"compress": {"blobId": blob, "type": "application/x-xz"}},
            "child": {"compress": {"blobId": "#bad", "type": "application/gzip"}}
        }),
    )
    .await;
    assert_eq!(resp["created"]["b"]["id"], "B2");
    assert_eq!(resp["created"]["a"]["id"], "B3");
    assert_eq!(read(&b, &id("B3")).await, b"GZ:B2");
    assert_eq!(resp["notCreated"]["bad"]["type"], "invalidProperties");
    assert_eq!(resp["notCreated"]["child"]["type"], "notFound");

    let earlier = BTreeMap::from([(id("up"), blob.clone())]);
    let resp = call_with(
        &b,
        json!({"accountId": "abc", "create": {"c": {"compress": {"blobId": "#up", "type": "application/gzip"}}}}),
        &earlier,
        &[BLOB2],
    )
    .await
    .unwrap();
    let out = resp["created"]["c"]["id"].as_str().unwrap();
    assert_eq!(read(&b, &id(out)).await, b"GZ:B1");
}

/// §8: "If a dependency cycle is detected, all members of the cycle MUST
/// be rejected with an "invalidProperties" error." An entry that only
/// depends on the cycle has nothing to read: `notFound`.
#[tokio::test]
async fn cycles() {
    let b = backend();
    let resp = convert(
        &b,
        json!({
            "x": {"compress": {"blobId": "#y", "type": "application/gzip"}},
            "y": {"delta": {"blobId": "B1", "newBlobId": "#x", "type": "text/x-diff"}},
            "s": {"compress": {"blobId": "#s", "type": "application/gzip"}},
            "after": {"compress": {"blobId": "#x", "type": "application/gzip"}}
        }),
    )
    .await;
    assert_invalid(&resp["notCreated"]["x"], json!(["compress"]));
    assert_invalid(&resp["notCreated"]["y"], json!(["delta"]));
    assert_invalid(&resp["notCreated"]["s"], json!(["compress"]));
    assert_eq!(resp["notCreated"]["after"]["type"], "notFound");
    assert_eq!(resp["created"], Value::Null);
}

/// §8.4: an extract creates no blob; `noPersist` has no effect on it, and a
/// reference to its creation id names the archive.
#[tokio::test]
async fn extract_reference_names_the_archive() {
    let b = backend();
    b.omit_no_persist_for_test(true);
    let f = put(&b, b"x").await;
    let resp = convert(
        &b,
        json!({
            "t": {"archive": {"type": "application/x-tar", "entries": [{"name": "f", "blobId": f}]}},
            "e": {"noPersist": true, "extract": {"blobId": "#t", "type": "application/x-tar"}},
            "z": {"compress": {"blobId": "#e", "type": "application/gzip"}}
        }),
    )
    .await;
    assert_eq!(resp["created"]["t"]["id"], "B2");
    assert_eq!(resp["created"]["e"]["id"], "B2");
    assert_eq!(resp["created"]["e"]["expires"], EXPIRES);
    let z = resp["created"]["z"]["id"].as_str().unwrap();
    assert_eq!(read(&b, &id(z)).await, b"GZ:B2");
}

/// §8: "If the conversion completed but encountered problems … the
/// response for that creation also includes" `isIncomplete` and
/// `description`.
#[tokio::test]
async fn incomplete_extract() {
    let b = backend();
    let [html, css, _] = site(&b).await;
    let t = accepted(
        &b,
        json!({"archive": {"type": "application/x-tar", "entries": [
            {"name": "a", "blobId": html}, {"name": "b", "blobId": css}]}}),
    )
    .await;
    let tar = read(&b, &id(t["id"].as_str().unwrap())).await;
    // a: header + 1 block; b: header + 10 of its 18 content octets.
    let cut = put(&b, &tar[..3 * 512 + 10]).await;
    let e = accepted(&b, json!({"extract": {"blobId": cut}})).await;
    assert_eq!(e["isIncomplete"], true);
    assert!(e["description"].is_string());
    assert_eq!(e["entries"].as_array().unwrap().len(), 1);
}

// ---- deferred results (§8) -----------------------------------------------

async fn deferred_tar(b: &MemoryBackend) -> Id {
    let f = put(b, b"content").await;
    let t = accepted(
        b,
        json!({"archive": {"type": "application/x-tar", "entries": [{"name": "f", "blobId": f}]}}),
    )
    .await;
    assert_eq!(t["size"], Value::Null);
    id(t["id"].as_str().unwrap())
}

/// §8: "If a client later requests the size property via Blob/get for a
/// deferred blob, the server MUST generate the blob at that point and
/// return the actual size."
#[tokio::test]
async fn blob_get_size_materializes() {
    let b = backend();
    let tar = deferred_tar(&b).await;
    let resp = handle_blob_get(
        &b,
        &(),
        json!({"accountId": "abc", "ids": [tar], "properties": ["size"]}),
        &BTreeMap::new(),
        &using(&[BLOB2]),
    )
    .await
    .unwrap()
    .primary;
    // Header, one content block, two end blocks.
    assert_eq!(resp["list"][0]["size"], 4 * 512);
}

/// §8: "If the deferred generation later fails …, the server treats the
/// blob as not existing: … Blob/get reports it in notFound, and a creation
/// in Blob/set or Blob/convert that references it fails with a "notFound"
/// SetError."
#[tokio::test]
async fn failed_deferred_is_not_found() {
    let b = backend();
    let tar = deferred_tar(&b).await;
    b.fail_deferred_for_test(&acc(), &tar);

    let get = handle_blob_get(
        &b,
        &(),
        json!({"accountId": "abc", "ids": [tar], "properties": ["size"]}),
        &BTreeMap::new(),
        &using(&[BLOB2]),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(get["notFound"], json!([tar]));

    let set = handle_blob_set(
        &b,
        &(),
        json!({"accountId": "abc", "create": {"c": {"data": [{"blobId": tar}]}}}),
        &BTreeMap::new(),
        &using(&[BLOB2]),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(set["notCreated"]["c"]["type"], "notFound");

    let e = rejected(
        &b,
        json!({"compress": {"blobId": tar, "type": "application/gzip"}}),
    )
    .await;
    assert_eq!(e["type"], "notFound");
}
