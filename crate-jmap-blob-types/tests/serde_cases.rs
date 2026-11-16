//! Edge cases of the blob wire types: custom serde, tri-state fields,
//! `BlobRef`, `DigestAlgorithm` ordering, digest vectors, capability
//! checks, recipe parsing and the `Other` catch-alls.
//!
//! Oracles are hand-written JSON and published digest vectors (RFC 9404
//! §4.2.1, RFC 9530 Appendix B), never output of the code under test.

use std::collections::BTreeMap;

use jmap_blob_types::{
    ArchiveEntry, BlobAccountCapability, BlobCreateObject, BlobGetItem, BlobObject, BlobRef,
    BlobSetResponse, CapabilityError, ColorSpace, CompressionMethod, ConvertRequest,
    DataSourceObject, DigestAlgorithm, EntryType, Recipe, compress_level_range,
};
use jmap_types::Id;
use serde_json::json;

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

fn digest(alg: DigestAlgorithm, bytes: &[u8]) -> String {
    let mut d = alg.builtin_digester().expect("built-in digester");
    d.update(bytes);
    d.finish()
}

// ---- digests ---------------------------------------------------------

/// RFC 9404 §4.2.1: the blob and its range digests.
#[test]
fn rfc9404_digest_vectors() {
    let blob = b"The quick brown fox jumped over the lazy dog.";
    assert_eq!(blob.len(), 45);
    assert_eq!(
        digest(DigestAlgorithm::Sha, blob),
        "wIVPufsDxBzOOALLDSIFKebu+U4="
    );
    let range = &blob[4..4 + 9];
    assert_eq!(range, b"quick bro");
    assert_eq!(
        digest(DigestAlgorithm::Sha, range),
        "QiRAPtfyX8K6tm1iOAtZ87Xj3Ww="
    );
    assert_eq!(
        digest(DigestAlgorithm::Sha256, range),
        "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA="
    );
}

/// RFC 9530 Appendix B.1 (`{"hello": "world"}` + LF) and B.2 (empty).
#[test]
fn rfc9530_digest_vectors() {
    assert_eq!(
        digest(DigestAlgorithm::Sha256, b"{\"hello\": \"world\"}\n"),
        "RK/0qy18MlBSVnWgjwz6lZEWjP/lF5HF9bvEF8FabDg="
    );
    assert_eq!(
        digest(DigestAlgorithm::Sha256, b""),
        "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
    );
}

/// FIPS 180-4 SHA-512 of the empty string, in padded base64.
#[test]
fn sha512_empty_vector() {
    assert_eq!(
        digest(DigestAlgorithm::Sha512, b""),
        "z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="
    );
}

/// Feeding the input in pieces gives the same digest.
#[test]
fn digester_streams() {
    let mut d = DigestAlgorithm::Sha.builtin_digester().unwrap();
    d.update(b"The quick brown fox ");
    d.update(b"jumped over the lazy dog.");
    assert_eq!(d.finish(), "wIVPufsDxBzOOALLDSIFKebu+U4=");
}

#[test]
fn digest_map_orders_by_wire_string() {
    let mut m = BTreeMap::new();
    m.insert(DigestAlgorithm::Sha512, "a".to_owned());
    m.insert(DigestAlgorithm::Sha, "b".to_owned());
    m.insert(DigestAlgorithm::Sha256, "c".to_owned());
    let keys: Vec<&str> = m.keys().map(DigestAlgorithm::as_str).collect();
    assert_eq!(keys, ["sha", "sha-256", "sha-512"]);
    assert_eq!(
        serde_json::to_string(&m).unwrap(),
        r#"{"sha":"b","sha-256":"c","sha-512":"a"}"#
    );
}

#[test]
fn forged_other_equals_known_variant() {
    assert_eq!(DigestAlgorithm::Other("sha".into()), DigestAlgorithm::Sha);
    let mut m = BTreeMap::new();
    m.insert(DigestAlgorithm::Sha, 1);
    assert_eq!(m.get(&DigestAlgorithm::Other("sha".into())), Some(&1));
}

// ---- capability ------------------------------------------------------

#[test]
fn check_rejects_63_data_sources() {
    let mut cap = BlobAccountCapability::default();
    cap.check().unwrap();
    cap.max_data_sources = 63;
    assert_eq!(cap.check(), Err(CapabilityError::TooFewDataSources(63)));
}

#[test]
fn check_rejects_uppercase_digest_names() {
    let mut cap = BlobAccountCapability::default();
    cap.supported_digest_algorithms = vec![DigestAlgorithm::from("SHA-256")];
    assert_eq!(
        cap.check(),
        Err(CapabilityError::DigestNotLowercase("SHA-256".into()))
    );
}

#[test]
fn rfc9404_view_keeps_four_fields_and_drops_adler() {
    let mut cap = BlobAccountCapability::default();
    cap.max_size_blob_set = Some(1000);
    cap.supported_type_names = vec!["Email".into()];
    cap.supported_digest_algorithms = vec![
        DigestAlgorithm::Sha256,
        DigestAlgorithm::Adler,
        DigestAlgorithm::Sha,
    ];
    cap.supported_archive_types = Some(vec!["application/x-tar".into()]);
    assert_eq!(
        serde_json::to_value(cap.rfc9404()).unwrap(),
        json!({
            "maxSizeBlobSet": 1000,
            "maxDataSources": 64,
            "supportedTypeNames": ["Email"],
            "supportedDigestAlgorithms": ["sha-256", "sha"]
        })
    );
}

// ---- BlobRef ---------------------------------------------------------

#[test]
fn blob_ref_cases() {
    assert_eq!(
        serde_json::from_value::<BlobRef>(json!("B1")).unwrap(),
        BlobRef::Id(id("B1"))
    );
    assert_eq!(
        serde_json::from_value::<BlobRef>(json!("#c1")).unwrap(),
        BlobRef::Creation(id("c1"))
    );
    for bad in ["#", "#bad id", "", "bad id", "##c1"] {
        assert!(
            serde_json::from_value::<BlobRef>(json!(bad)).is_err(),
            "{bad:?} must be rejected"
        );
    }
    for wire in ["B1", "#c1"] {
        let r: BlobRef = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(serde_json::to_value(&r).unwrap(), json!(wire));
    }
}

#[test]
fn blob_ref_map_orders_by_wire_string() {
    let mut m = BTreeMap::new();
    m.insert(BlobRef::Id(id("A1")), 1);
    m.insert(BlobRef::Creation(id("z9")), 2);
    m.insert(BlobRef::Id(id("0x")), 3);
    assert_eq!(
        serde_json::to_string(&m).unwrap(),
        r##"{"#z9":2,"0x":3,"A1":1}"##
    );
}

// ---- DataSourceObject and BlobCreateObject ---------------------------

#[test]
fn data_source_digest_keys_and_extra() {
    let dso: DataSourceObject = serde_json::from_value(json!({
        "blobId": "#b1",
        "offset": 4,
        "length": 9,
        "digest:sha-256": "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA=",
        "digest:foo": "xyz",
        "digest:sha": null,
        "vendor:x": 1
    }))
    .unwrap();
    assert_eq!(dso.blob_id, Some(BlobRef::Creation(id("b1"))));
    assert_eq!(dso.digests.len(), 2);
    assert_eq!(dso.digests[&DigestAlgorithm::Other("foo".into())], "xyz");
    assert_eq!(
        dso.extra,
        json!({"vendor:x": 1}).as_object().unwrap().clone()
    );
    // A null digest reads as absent and is not written back.
    assert_eq!(
        serde_json::to_value(&dso).unwrap(),
        json!({
            "blobId": "#b1",
            "offset": 4,
            "length": 9,
            "digest:sha-256": "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA=",
            "digest:foo": "xyz",
            "vendor:x": 1
        })
    );
}

#[test]
fn data_source_rejects_non_string_digest() {
    assert!(serde_json::from_value::<DataSourceObject>(json!({"digest:sha": 5})).is_err());
}

/// Both inline keys parse: the "exactly one" rule is the server's
/// `invalidProperties` check, not a deserialize error.
#[test]
fn data_source_keeps_two_data_keys_for_the_server() {
    let dso: DataSourceObject =
        serde_json::from_value(json!({"data:asText": "a", "data:asBase64": "YQ=="})).unwrap();
    assert!(dso.data_as_text.is_some() && dso.data_as_base64.is_some());
}

#[test]
fn create_object_whole_blob_digests_and_no_persist() {
    let input = json!({
        "data": [{"data:asBase64": "SGVsbG8="}, {"blobId": "B2", "length": 3}],
        "size": 8,
        "digest:sha": "abc=",
        "noPersist": true
    });
    let c: BlobCreateObject = serde_json::from_value(input.clone()).unwrap();
    assert!(c.no_persist);
    assert_eq!(c.size, Some(8));
    assert_eq!(c.digests[&DigestAlgorithm::Sha], "abc=");
    assert!(c.extra.is_empty());
    assert_eq!(serde_json::to_value(&c).unwrap(), input);
}

// ---- Blob/set response -----------------------------------------------

#[test]
fn absent_and_null_states_parse() {
    for v in [
        json!({"accountId": "a"}),
        json!({"accountId": "a", "oldState": null, "newState": null}),
    ] {
        let r: BlobSetResponse = serde_json::from_value(v).unwrap();
        assert_eq!((r.old_state, r.new_state), (None, None));
    }
}

#[test]
fn expires_is_tri_state() {
    let absent: BlobObject = serde_json::from_value(json!({"id": "B1"})).unwrap();
    assert_eq!(absent.expires, None);
    assert_eq!(absent.media_type, None);
    let null: BlobObject =
        serde_json::from_value(json!({"id": "B1", "type": null, "size": 1, "expires": null}))
            .unwrap();
    assert_eq!(null.expires, Some(None));
    assert_eq!(
        serde_json::to_value(&null).unwrap(),
        json!({"id": "B1", "type": null, "size": 1, "expires": null})
    );
    let date: BlobObject = serde_json::from_value(
        json!({"id": "B1", "type": null, "size": null, "expires": "2026-12-01T00:00:00Z"}),
    )
    .unwrap();
    assert!(matches!(date.expires, Some(Some(_))));
    assert_eq!(date.size, None);
}

#[test]
fn updated_entries_parse() {
    let r: BlobSetResponse = serde_json::from_value(json!({
        "accountId": "a",
        "updated": {"B1": {"expires": "2026-12-01T00:00:00Z"}, "B2": null}
    }))
    .unwrap();
    let b1 = r.updated[&id("B1")].as_ref().unwrap();
    assert_eq!(
        b1.expires.clone().flatten().map(|d| d.as_ref().to_owned()),
        Some("2026-12-01T00:00:00Z".to_owned())
    );
    assert_eq!(r.updated[&id("B2")], None);
}

// ---- Blob/get item ---------------------------------------------------

#[test]
fn get_item_tri_states_and_flags() {
    let input = json!({
        "id": "B1",
        "data:asText": null,
        "isEncodingProblem": true,
        "imageData": null,
        "digest:sha": "wIVPufsDxBzOOALLDSIFKebu+U4="
    });
    let item: BlobGetItem = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(item.data_as_text, Some(None));
    assert_eq!(item.image_data, Some(None));
    assert!(item.is_encoding_problem && !item.is_truncated);
    assert_eq!(item.digests.len(), 1);
    assert_eq!(serde_json::to_value(&item).unwrap(), input);

    let bare: BlobGetItem = serde_json::from_value(json!({"id": "B1"})).unwrap();
    assert_eq!((bare.data_as_text, bare.image_data), (None, None));
    assert_eq!(
        serde_json::to_value(BlobGetItem::new(id("B1"))).unwrap(),
        json!({"id": "B1"})
    );
}

#[test]
fn get_item_image_data() {
    let input = json!({
        "id": "B1",
        "imageData": {
            "width": 640, "height": 480, "orientation": 6,
            "date": "2026-01-02T03:04:05Z",
            "gps": {"latitude": -33.86, "longitude": 151.21},
            "duration": 1.5, "comment": "hi"
        }
    });
    let item: BlobGetItem = serde_json::from_value(input.clone()).unwrap();
    let img = item.image_data.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(img.gps.as_ref().unwrap().longitude, 151.21);
    assert_eq!(serde_json::to_value(&item).unwrap(), input);
}

// ---- Blob/convert ----------------------------------------------------

#[test]
fn convert_request_needs_exactly_one_recipe() {
    let none = serde_json::from_value::<ConvertRequest>(json!({"noPersist": true}));
    assert!(none.unwrap_err().to_string().contains("exactly one"));

    let one: ConvertRequest = serde_json::from_value(json!({
        "decompress": {"blobId": "B1"},
        "vendor:x": true
    }))
    .unwrap();
    assert!(matches!(one.recipe, Recipe::Decompress(_)));
    assert_eq!(one.extra.len(), 1);

    let two = serde_json::from_value::<ConvertRequest>(json!({
        "extract": {"blobId": "B1"},
        "compress": {"blobId": "B1", "type": "application/gzip"}
    }));
    let msg = two.unwrap_err().to_string();
    assert!(msg.contains("extract") && msg.contains("compress"), "{msg}");
}

#[test]
fn image_convert_recipe_round_trips() {
    let input = json!({
        "imageConvert": {
            "blobId": "#up",
            "type": "image/jpeg",
            "width": 100,
            "height": 50,
            "ignoreAspect": false,
            "quality": 80,
            "colorSpace": "sRGB",
            "background": "#ffffff",
            "stripMetadata": true,
            "autoOrient": true
        }
    });
    let req: ConvertRequest = serde_json::from_value(input.clone()).unwrap();
    let Recipe::ImageConvert(r) = &req.recipe else {
        panic!("image recipe")
    };
    assert_eq!(r.color_space, Some(ColorSpace::Srgb));
    assert_eq!(serde_json::to_value(&req).unwrap(), input);
}

#[test]
fn archive_entry_wire_names() {
    let input = json!({
        "name": "dev/sda",
        "entryType": "blockDevice",
        "linkTarget": "x",
        "ownerName": "root",
        "groupName": "disk",
        "devMajor": 8,
        "devMinor": 0,
        "uid": 0,
        "gid": 6,
        "created": "2026-01-01T00:00:00Z",
        "compressionMethod": "deflate"
    });
    let e: ArchiveEntry = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(e.entry_type, Some(EntryType::BlockDevice));
    assert_eq!(e.compression_method, Some(CompressionMethod::Deflate));
    assert_eq!(serde_json::to_value(&e).unwrap(), input);
    assert_eq!(
        serde_json::to_value(EntryType::CharDevice).unwrap(),
        json!("charDevice")
    );
}

#[test]
fn other_variants_round_trip() {
    fn rt<T: serde::de::DeserializeOwned + serde::Serialize>(s: &str) {
        let v: T = serde_json::from_value(json!(s)).unwrap();
        assert_eq!(serde_json::to_value(&v).unwrap(), json!(s));
    }
    rt::<EntryType>("socket");
    rt::<CompressionMethod>("bzip2");
    rt::<ColorSpace>("cmyk");
    rt::<DigestAlgorithm>("sha3-256");
    rt::<jmap_blob_types::BlobProperty>("vendor:prop");
    rt::<jmap_blob_types::DataSourceProperty>("vendor:prop");
    assert_eq!(
        serde_json::from_value::<EntryType>(json!("socket")).unwrap(),
        EntryType::Other("socket".into())
    );
}

#[test]
fn compress_level_ranges() {
    assert_eq!(compress_level_range("application/gzip"), Some(1..=9));
    assert_eq!(compress_level_range("application/x-bzip2"), Some(1..=9));
    assert_eq!(compress_level_range("application/x-xz"), Some(0..=9));
    assert_eq!(compress_level_range("Application/ZSTD"), Some(1..=22));
    assert_eq!(compress_level_range("application/zip"), None);
}

#[test]
fn recipe_keys_match_key() {
    assert_eq!(
        Recipe::KEYS,
        [
            "imageConvert",
            "archive",
            "extract",
            "compress",
            "decompress",
            "delta",
            "patch"
        ]
    );
}

/// Every property `BlobObject` serializes is classified once, and the
/// lists are the ones blobext-03 §4 gives (`id`, `size` server-set;
/// `type` create-only), which the conditional evaluator relies on.
#[test]
fn blob_object_set_object_lists() {
    use jmap_types::{JmapObject, SetObject, check_property_classification};
    let mut blob = BlobObject::new(Id::from("B1"), Some("text/plain".to_owned()), Some(13));
    blob.expires = Some(None);
    assert_eq!(check_property_classification(&blob), Ok(()));
    assert_eq!(BlobObject::TYPE_NAME, "Blob");
    assert_eq!(BlobObject::SERVER_SET, ["id", "size"]);
    assert_eq!(BlobObject::CLIENT_SETTABLE, ["type", "expires"]);
    assert_eq!(BlobObject::IMMUTABLE, ["type"]);
}
