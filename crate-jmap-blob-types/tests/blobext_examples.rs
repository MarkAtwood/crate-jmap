//! Round-trips of the draft-ietf-jmap-blobext-03 examples (§2.1.1, §9.1
//! to §9.7), the RFC 9404 §3.1.1 capability and the RFC 9404 §4.1.1 and
//! §4.1.2 Blob/upload examples.
//!
//! Oracle: the JSON below is copied from the drafts (test-oracle rule).
//! Where the types normalize (an empty `/set` result map is written as
//! `null`), the expected output is written out by hand next to the input.

use jmap_blob_types::{
    BlobAccountCapability, BlobConvertArgs, BlobConvertResponse, BlobGetArgs, BlobGetResponse,
    BlobLookupArgs, BlobLookupResponse, BlobProperty, BlobRef, BlobSetArgs, BlobSetResponse,
    BlobUploadArgs, BlobUploadCallResponse, DataSourceProperty, DigestAlgorithm, EntryType, Recipe,
    Rfc9404AccountCapability,
};
use jmap_types::Id;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// Parse `input`, serialize the result, and check it equals `expected`.
fn round_trip<T: DeserializeOwned + Serialize>(input: Value, expected: Value) -> T {
    let parsed: T = serde_json::from_value(input).expect("parses");
    assert_eq!(serde_json::to_value(&parsed).unwrap(), expected);
    parsed
}

/// Round trip that must reproduce `input` exactly.
fn exact<T: DeserializeOwned + Serialize>(input: Value) -> T {
    round_trip(input.clone(), input)
}

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

/// §2.1.1.
#[test]
fn capability_example() {
    let doc = json!({
      "urn:ietf:params:jmap:blob2": {
        "maxSizeBlobSet": 52428800,
        "maxDataSources": 64,
        "supportedTypeNames": ["Email", "Thread", "Mailbox"],
        "supportedDigestAlgorithms": ["sha-256", "sha"],
        "uploadUrl": "https://up.example.com/jmap/upload/{accountId}/",
        "chunkSize": 5242880,
        "supportedImageReadTypes": [
          "image/png",
          "image/jpeg",
          "image/gif",
          "image/tiff"
        ],
        "supportedImageWriteTypes": [
          "image/png",
          "image/jpeg"
        ],
        "supportedArchiveTypes": [
          "application/zip",
          "application/x-tar"
        ],
        "supportedExtractTypes": [
          "application/zip",
          "application/x-tar",
          "application/vnd.ms-tnef"
        ],
        "supportedCompressTypes": [
          "application/gzip",
          "application/zstd"
        ],
        "supportedDecompressTypes": [
          "application/gzip",
          "application/x-bzip2",
          "application/zstd"
        ],
        "supportedDeltaTypes": [
          "application/x-rdiff-delta",
          "text/x-diff"
        ],
        "supportedPatchTypes": [
          "application/x-rdiff-delta",
          "text/x-diff"
        ],
        "maxConvertSize": 104857600,
        "maxArchiveEntries": 10000,
        "maxImageDimension": 8192
      }
    });
    let cap: BlobAccountCapability = exact(doc[jmap_blob_types::JMAP_BLOB2_URI].clone());
    assert_eq!(
        cap.supported_digest_algorithms,
        vec![DigestAlgorithm::Sha256, DigestAlgorithm::Sha]
    );
    assert_eq!(cap.max_image_dimension, Some(8192));
    cap.check().unwrap();
}

/// §2.1: all 17 keys MUST be present, so a `null` list is written.
#[test]
fn capability_writes_null_members() {
    let v = serde_json::to_value(BlobAccountCapability::default()).unwrap();
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 17);
    assert_eq!(obj["supportedImageReadTypes"], Value::Null);
    assert_eq!(obj["maxSizeBlobSet"], Value::Null);
    assert_eq!(obj["maxDataSources"], json!(64));
}

/// RFC 9404 §3.1.1, the account capability.
#[test]
fn rfc9404_capability_example() {
    let cap: Rfc9404AccountCapability = exact(json!({
        "maxSizeBlobSet": 50000000,
        "maxDataSources": 100,
        "supportedTypeNames" : [
          "Mailbox",
          "Thread",
          "Email"
        ],
        "supportedDigestAlgorithms" : [
          "sha",
          "sha-256"
        ]
    }));
    assert_eq!(cap.max_data_sources, 100);
}

/// RFC 9404 §4.1.1, the simple Blob/upload (the example wraps the base64
/// for publication; "the entire data:asBase64 field would be sent as a
/// continuous string").
#[test]
fn rfc9404_upload_simple() {
    let args: BlobUploadArgs = exact(json!({
     "accountId": "account1",
     "create": {
      "1": {
       "data" : [
       {
        "data:asBase64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABAQMAAAAl21bKAAAAA1BMVEX/AAAZ4gk3AAAAAXRSTlN/gFy0ywAAAApJREFUeJxjYgAAAAYAAzY3fKgAAAAASUVORK5CYII="
       }
      ],
      "type": "image/png"
      }
     }
    }));
    assert_eq!(
        args.create[&id("1")].media_type.as_deref(),
        Some("image/png")
    );
    let created = json!({
        "1": {
            "id": "G4c6751edf9dd6903ff54b792e432fba781271beb",
            "type": "image/png",
            "size": 95
        }
    });
    let resp: BlobUploadCallResponse = round_trip(
        json!({"accountId": "account1", "created": created}),
        json!({"accountId": "account1", "created": created, "notCreated": null}),
    );
    assert_eq!(resp.created["1"].size, Some(95));
}

/// RFC 9404 §4.1.2, the complex Blob/upload: `#b4` sources with ranges.
/// The example's requests leave out `accountId`, which RFC 8620 §5.3
/// requires, so it is added. The responses' `oldState: null` has no field
/// (decision 21(iii)) and round-trips through `extra`.
#[test]
fn rfc9404_upload_complex() {
    let args: BlobUploadArgs = exact(json!({
      "accountId": "account1",
      "create": {
        "cat": {
          "data": [
            {"data:asText": "How"},
            {"blobId": "#b4", "length": 7, "offset": 3},
            {"data:asText": "was t"},
            {"blobId": "#b4", "length": 1, "offset": 1},
            {"data:asBase64": "YXQ/"}
          ]
        }
      }
    }));
    let cat = &args.create[&id("cat")];
    assert_eq!(cat.data.len(), 5);
    assert_eq!(cat.data[1].blob_id, Some(BlobRef::Creation(id("b4"))));
    let resp: BlobUploadCallResponse = round_trip(
        json!({
         "oldState": null,
         "created": {
           "cat": {
             "id": "Gcc60576f036321ae6e8037ffc56bdee589bd3e23",
             "size": 19,
             "type": "application/octet-stream"
           }
         },
         "notCreated": null,
         "accountId": "account1"
        }),
        json!({
         "oldState": null,
         "created": {
           "cat": {
             "id": "Gcc60576f036321ae6e8037ffc56bdee589bd3e23",
             "size": 19,
             "type": "application/octet-stream"
           }
         },
         "notCreated": null,
         "accountId": "account1"
        }),
    );
    assert_eq!(resp.extra["oldState"], Value::Null);
}

/// §9.1 request and response.
#[test]
fn uploading_a_blob_inline() {
    let args: BlobSetArgs = exact(json!({
      "accountId": "abc",
      "create": {
        "b1": {
          "data": [{"data:asText": "Hello, world!"}],
          "type": "text/plain"
        }
      }
    }));
    let b1 = &args.create.as_ref().unwrap()[&id("b1")];
    assert_eq!(b1.media_type.as_deref(), Some("text/plain"));
    assert!(!b1.no_persist);

    let resp: BlobSetResponse = round_trip(
        json!({
          "accountId": "abc",
          "created": {
            "b1": {
              "id": "Babc123",
              "type": "text/plain",
              "size": 13
            }
          },
          "notCreated": null
        }),
        json!({
          "accountId": "abc",
          "oldState": null,
          "newState": null,
          "created": {
            "b1": {
              "id": "Babc123",
              "type": "text/plain",
              "size": 13
            }
          },
          "updated": null,
          "destroyed": null,
          "notCreated": null,
          "notUpdated": null,
          "notDestroyed": null
        }),
    );
    assert_eq!(resp.created["b1"].size, Some(13));
    assert_eq!(resp.created["b1"].expires, None);
}

/// §9.2 request and response.
#[test]
fn querying_blob_chunks() {
    let args: BlobGetArgs = exact(json!({
      "accountId": "abc",
      "ids": ["B1a2b3c"],
      "properties": ["size", "chunks"],
      "dataSourceProperties": [
        "blobId", "size", "offset", "length", "position",
        "digest:sha-256"
      ]
    }));
    assert_eq!(
        args.properties.unwrap(),
        vec![BlobProperty::Size, BlobProperty::Chunks]
    );
    assert_eq!(
        args.data_source_properties.unwrap()[5],
        DataSourceProperty::Digest(DigestAlgorithm::Sha256)
    );

    let resp: BlobGetResponse = exact(json!({
      "accountId": "abc",
      "list": [{
        "id": "B1a2b3c",
        "size": 10485760,
        "chunks": [
          {
            "blobId": "Bchunk1",
            "size": 5242880,
            "offset": 0,
            "length": 5242880,
            "position": 0,
            "digest:sha-256": "a1b2c3..."
          },
          {
            "blobId": "Bchunk2",
            "size": 5242880,
            "offset": 0,
            "length": 5242880,
            "position": 5242880,
            "digest:sha-256": "d4e5f6..."
          }
        ]
      }],
      "notFound": []
    }));
    let chunks = resp.list[0].chunks.as_ref().unwrap();
    assert_eq!(chunks[1].position, Some(5242880));
    assert_eq!(chunks[1].digests[&DigestAlgorithm::Sha256], "d4e5f6...");
    assert!(chunks[1].extra.is_empty());
}

/// §9.3 request and response.
#[test]
fn looking_up_blob_references() {
    let args: BlobLookupArgs = exact(json!({
      "accountId": "abc",
      "typeNames": ["Mailbox", "Email"],
      "ids": ["Bfirst", "Bsecond"]
    }));
    assert_eq!(args.type_names, ["Mailbox", "Email"]);

    let resp: BlobLookupResponse = exact(json!({
      "accountId": "abc",
      "list": [
        {
          "id": "Bfirst",
          "matchedIds": {
            "Mailbox": ["M1"],
            "Email": ["E1", "E2"]
          }
        },
        {
          "id": "Bsecond",
          "matchedIds": {
            "Mailbox": [],
            "Email": []
          }
        }
      ]
    }));
    assert_eq!(resp.list[0].matched_ids["Email"], [id("E1"), id("E2")]);
    assert!(resp.list[1].matched_ids["Mailbox"].is_empty());
}

/// The §9.4 to §9.7 responses write `"notCreated": {}`; the types write
/// an empty result map as `null`.
fn convert_response(created: Value) -> BlobConvertResponse {
    round_trip(
        json!({"accountId": "abc", "created": created.clone(), "notCreated": {}}),
        json!({"accountId": "abc", "created": created, "notCreated": null}),
    )
}

/// §9.4 request and response.
#[test]
fn creating_a_zip_archive() {
    let args: BlobConvertArgs = exact(json!({
      "accountId": "abc",
      "create": {
        "z1": {
          "archive": {
            "type": "application/zip",
            "entries": [
              {
                "name": "site/index.html",
                "blobId": "Baaaa"
              },
              {
                "name": "site/style.css",
                "blobId": "Bbbbb"
              },
              {
                "name": "site/photo.jpg",
                "blobId": "Bcccc"
              }
            ]
          }
        }
      }
    }));
    let Recipe::Archive(a) = &args.create[&id("z1")].recipe else {
        panic!("archive recipe")
    };
    assert_eq!(a.entries[2].blob_id, Some(BlobRef::Id(id("Bcccc"))));

    let resp = convert_response(json!({
      "z1": {
        "id": "B9f2a4e",
        "type": "application/zip",
        "size": 104857
      }
    }));
    assert_eq!(resp.created["z1"].size, Some(104857));
}

/// §9.5 request and response.
#[test]
fn creating_a_compressed_tar_archive() {
    let args: BlobConvertArgs = exact(json!({
      "accountId": "abc",
      "create": {
        "t1": {
          "noPersist": true,
          "archive": {
            "type": "application/x-tar",
            "entries": [
              {
                "name": "site/index.html",
                "blobId": "Baaaa",
                "modified": "2026-03-01T12:00:00Z",
                "mode": "0644"
              },
              {
                "name": "site/style.css",
                "blobId": "Bbbbb",
                "modified": "2026-03-01T12:00:00Z",
                "mode": "0644"
              },
              {
                "name": "site/photo.jpg",
                "blobId": "Bcccc",
                "modified": "2026-02-15T09:30:00Z",
                "mode": "0644"
              }
            ]
          }
        },
        "t2": {
          "compress": {
            "blobId": "#t1",
            "type": "application/gzip"
          }
        }
      }
    }));
    assert!(args.create[&id("t1")].no_persist);
    let Recipe::Compress(c) = &args.create[&id("t2")].recipe else {
        panic!("compress recipe")
    };
    assert_eq!(c.blob_id, BlobRef::Creation(id("t1")));

    convert_response(json!({
      "t2": {
        "id": "Bd81c7f",
        "type": "application/gzip",
        "size": 98304
      }
    }));
}

/// §9.6 request and response.
#[test]
fn extracting_a_compressed_tar_archive() {
    let args: BlobConvertArgs = exact(json!({
      "accountId": "abc",
      "create": {
        "u1": {
          "noPersist": true,
          "decompress": {
            "blobId": "Bd81c7f",
            "type": "application/gzip"
          }
        },
        "u2": {
          "extract": {
            "blobId": "#u1",
            "type": "application/x-tar"
          }
        }
      }
    }));
    assert_eq!(args.create[&id("u2")].recipe.key(), "extract");

    let resp = convert_response(json!({
      "u2": {
        "id": "Be3a901",
        "type": "application/x-tar",
        "size": 102400,
        "entries": [
          {
            "name": "site/index.html",
            "blobId": "Bdd001",
            "entryType": "file",
            "modified": "2026-03-01T12:00:00Z",
            "mode": "0644"
          },
          {
            "name": "site/style.css",
            "blobId": "Bdd002",
            "entryType": "file",
            "modified": "2026-03-01T12:00:00Z",
            "mode": "0644"
          },
          {
            "name": "site/photo.jpg",
            "blobId": "Bdd003",
            "entryType": "file",
            "modified": "2026-02-15T09:30:00Z",
            "mode": "0644"
          }
        ]
      }
    }));
    let entries = resp.created["u2"].entries.as_ref().unwrap();
    assert_eq!(entries[0].entry_type, Some(EntryType::File));
    assert_eq!(entries[2].mode.as_deref(), Some("0644"));
}

/// §9.7 requests and responses.
#[test]
fn computing_and_applying_a_delta() {
    let delta: BlobConvertArgs = exact(json!({
      "accountId": "abc",
      "create": {
        "d1": {
          "delta": {
            "blobId": "BoldVersion",
            "newBlobId": "BnewVersion",
            "type": "text/x-diff"
          }
        }
      }
    }));
    let Recipe::Delta(d) = &delta.create[&id("d1")].recipe else {
        panic!("delta recipe")
    };
    assert_eq!(d.new_blob_id, BlobRef::Id(id("BnewVersion")));
    convert_response(json!({
      "d1": {
        "id": "Bdelta789",
        "type": "text/x-diff",
        "size": 1234
      }
    }));

    let patch: BlobConvertArgs = exact(json!({
      "accountId": "abc",
      "create": {
        "p1": {
          "patch": {
            "blobId": "BoldVersion",
            "deltaBlobId": "Bdelta789",
            "deltaType": "text/x-diff"
          }
        }
      }
    }));
    let Recipe::Patch(p) = &patch.create[&id("p1")].recipe else {
        panic!("patch recipe")
    };
    assert_eq!(p.delta_type, "text/x-diff");
    convert_response(json!({
      "p1": {
        "id": "Breconstructed",
        "type": "application/octet-stream",
        "size": 48576
      }
    }));
}
