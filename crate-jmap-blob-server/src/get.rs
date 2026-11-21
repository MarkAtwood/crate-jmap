//! `Blob/get` (blobext-03 §5; RFC 9404 §4.2).

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jmap_blob_types::{
    BlobAccountCapability, BlobGetArgs, BlobGetItem, BlobGetResponse, BlobProperty, BlobRef,
    DataSourceObject, DataSourceProperty, DigestAlgorithm,
};
use jmap_server::{HandlerOutput, enforce_max_objects_in_get, server_fail_from_backend};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::backend::{BlobBackend, ChunkRef, DigestQuery};
use crate::mode::BlobMode;
use crate::range::{select_range, utf8_text};
use crate::start::{Start, compute_digest, start};

/// Handle `Blob/get` (blobext-03 §5; RFC 9404 §4.2).
///
/// `created_ids` are the request's creation ids so far, for
/// `#creationId` references to blobs created by earlier calls; `using` is
/// the request's capability list, which selects blob2 or RFC 9404
/// behaviour ([`BlobMode`]).
///
/// Every `ids` entry the caller cannot see, and every `#creationId` with
/// no creation, is listed in `notFound` as sent. `data`, `data:asText`,
/// `data:asBase64` and `digest:*` cover the selected range; `size` is the
/// whole blob's; `chunks` always describes the whole blob, with `position`
/// and chunk digests computed here rather than trusted from the backend.
///
/// Not yet: RFC 9404 mode follows the design (docs/design/blobext.md §5
/// step 7) but its conformance tests land with bd JMAP-tna1.2.11.
///
/// # Errors
///
/// A method-level [`JmapError`]:
///
/// - `invalidArguments`: both blob capabilities in `using`; malformed
///   arguments; `ids` null or absent (blobs cannot be enumerated);
///   `properties` absent while `offset` or `length` is set (blobext-03:
///   "the client MUST specify the properties to return"); an unknown
///   property or `dataSourceProperties` name, or a `digest:*` name whose
///   algorithm the account does not advertise.
/// - `accountNotFound` / `accountNotSupportedByMethod`: unknown account,
///   or one without the blob capability.
/// - `requestTooLarge`: more ids than `maxObjectsInGet`.
/// - `serverFail`: a backend error, or an advertised digest algorithm with
///   neither a stored digest nor a digester (never a wrong value).
pub async fn handle_blob_get<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    created_ids: &BTreeMap<Id, Id>,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let Start {
        mode,
        account_id,
        args: mut map,
        capability,
    } = start(backend, caller, args, using).await?;
    enforce_max_objects_in_get(&map, backend.max_objects_in_get(caller, &account_id))?;

    if mode == BlobMode::Rfc9404 {
        // RFC 9404 has no `dataSourceProperties`: an unknown argument,
        // ignored like any other.
        map.remove("dataSourceProperties");
    }
    map.insert(
        "accountId".to_owned(),
        Value::String(account_id.to_string()),
    );
    let args: BlobGetArgs = serde_json::from_value(Value::Object(map))
        .map_err(|e| JmapError::invalid_arguments(format!("invalid Blob/get arguments: {e}")))?;
    let Some(ids) = args.ids.clone() else {
        return Err(JmapError::invalid_arguments(
            "ids must be a list: blobs cannot be enumerated",
        ));
    };
    let plan = Plan::new(&args, &capability, mode)?;
    let mut response = BlobGetResponse::new(account_id.clone());
    for requested in ids {
        let blob_id = match &requested {
            BlobRef::Creation(cid) => created_ids.get(cid).cloned(),
            other => other.as_id().cloned(),
        };
        let item = match blob_id {
            Some(blob_id) => get_one(backend, caller, &account_id, blob_id, &plan, &args).await?,
            None => None,
        };
        match item {
            Some(item) => response.list.push(item),
            // `Id::from` keeps a `#creationId` as sent, so the client can
            // match it to its request.
            None => response.not_found.push(Id::from(requested.to_string())),
        }
    }
    let primary = serde_json::to_value(&response)
        .map_err(|e| JmapError::server_fail(format!("serializing Blob/get response: {e}")))?;
    Ok(HandlerOutput::new(primary))
}

/// The validated `properties` and `dataSourceProperties` of one call.
#[derive(Debug, Default)]
struct Plan {
    data: bool,
    as_text: bool,
    as_base64: bool,
    size: bool,
    image_data: bool,
    chunks: bool,
    digests: Vec<DigestAlgorithm>,
    chunk_props: ChunkPlan,
}

/// Which [`DataSourceObject`] properties each `chunks` entry carries.
#[derive(Debug, Default)]
struct ChunkPlan {
    blob_id: bool,
    size: bool,
    offset: bool,
    length: bool,
    position: bool,
    digests: Vec<DigestAlgorithm>,
}

impl Plan {
    fn new(
        args: &BlobGetArgs,
        capability: &BlobAccountCapability,
        mode: BlobMode,
    ) -> Result<Self, JmapError> {
        // RFC 9404 §3.1 does not carry `adler`, so its view does not
        // advertise it.
        let advertised = match mode {
            BlobMode::Rfc9404 => capability.rfc9404().supported_digest_algorithms,
            _ => capability.supported_digest_algorithms.clone(),
        };
        let check_digest = |alg: &DigestAlgorithm, what: &str| {
            if advertised.contains(alg) {
                Ok(())
            } else {
                Err(JmapError::invalid_arguments(format!(
                    "{what} {} names a digest algorithm the account does not support",
                    alg.property_name()
                )))
            }
        };

        let properties = match &args.properties {
            Some(p) => p.clone(),
            None if mode == BlobMode::Rfc9404
                || (args.offset.is_none() && args.length.is_none()) =>
            {
                vec![BlobProperty::Data, BlobProperty::Size]
            }
            None => {
                return Err(JmapError::invalid_arguments(
                    "properties is required when offset or length is set (blobext-03 §5)",
                ));
            }
        };
        let mut plan = Self::default();
        for p in &properties {
            match p {
                BlobProperty::Id => {}
                BlobProperty::Data => plan.data = true,
                BlobProperty::DataAsText => plan.as_text = true,
                BlobProperty::DataAsBase64 => plan.as_base64 = true,
                BlobProperty::Size => plan.size = true,
                BlobProperty::ImageData if mode != BlobMode::Rfc9404 => plan.image_data = true,
                BlobProperty::Chunks if mode != BlobMode::Rfc9404 => plan.chunks = true,
                BlobProperty::Digest(alg) => {
                    check_digest(alg, "property")?;
                    if !plan.digests.contains(alg) {
                        plan.digests.push(alg.clone());
                    }
                }
                other => {
                    return Err(JmapError::invalid_arguments(format!(
                        "unknown Blob/get property {other}"
                    )));
                }
            }
        }

        // Checked even when `chunks` is not requested (blobext-03 §5: the
        // rejection is not conditional on chunks).
        let chunk_props = args
            .data_source_properties
            .clone()
            .unwrap_or_else(DataSourceProperty::defaults);
        for p in &chunk_props {
            let c = &mut plan.chunk_props;
            match p {
                DataSourceProperty::BlobId => c.blob_id = true,
                DataSourceProperty::Size => c.size = true,
                DataSourceProperty::Offset => c.offset = true,
                DataSourceProperty::Length => c.length = true,
                DataSourceProperty::Position => c.position = true,
                DataSourceProperty::Digest(alg) => {
                    check_digest(alg, "dataSourceProperties entry")?;
                    if !c.digests.contains(alg) {
                        c.digests.push(alg.clone());
                    }
                }
                other => {
                    return Err(JmapError::invalid_arguments(format!(
                        "unknown dataSourceProperties name {other}"
                    )));
                }
            }
        }
        Ok(plan)
    }

    fn wants_octets(&self) -> bool {
        self.data || self.as_text || self.as_base64
    }
}

/// The `list` entry for `blob_id`, or `None` when the caller cannot see
/// it (including a deferred conversion whose generation failed).
async fn get_one<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    blob_id: Id,
    plan: &Plan,
    args: &BlobGetArgs,
) -> Result<Option<BlobGetItem>, JmapError> {
    let fail = |e: B::Error| server_fail_from_backend(&e);
    // Materializes a deferred conversion result (blobext-03 §8).
    let Some(meta) = backend
        .blob_info(caller, account_id, &blob_id)
        .await
        .map_err(fail)?
    else {
        return Ok(None);
    };
    let selection = select_range(meta.size, args.offset, args.length);
    let mut item = BlobGetItem::new(blob_id.clone());
    item.is_truncated = selection.is_truncated;
    if plan.size {
        item.size = Some(meta.size);
    }

    // Stored digests first; the octets are read only when some property
    // needs them.
    let mut digests = BTreeMap::new();
    let mut missing = Vec::new();
    for alg in &plan.digests {
        let query = DigestQuery::new(blob_id.clone(), alg.clone(), selection.range.clone());
        match backend
            .stored_digest(caller, account_id, &query)
            .await
            .map_err(fail)?
        {
            Some(d) => {
                digests.insert(alg.clone(), d);
            }
            None => missing.push(alg),
        }
    }
    if plan.wants_octets() || !missing.is_empty() {
        let Some(octets) = backend
            .read_blob_range(caller, account_id, &blob_id, selection.range.clone())
            .await
            .map_err(fail)?
        else {
            return Ok(None);
        };
        let text = utf8_text(&octets);
        if plan.as_text || (plan.data && text.is_some()) {
            item.data_as_text = Some(text.map(str::to_owned));
        }
        if plan.as_base64 || (plan.data && text.is_none()) {
            item.data_as_base64 = Some(STANDARD.encode(&octets));
        }
        item.is_encoding_problem = (plan.data || plan.as_text) && text.is_none();
        for alg in missing {
            digests.insert(alg.clone(), compute_digest(backend, alg, &[&octets])?);
        }
    }
    item.digests = digests;

    if plan.image_data {
        item.image_data = Some(
            backend
                .image_data(caller, account_id, &blob_id)
                .await
                .map_err(fail)?,
        );
    }
    if plan.chunks {
        let Some(chunks) = backend
            .blob_chunks(caller, account_id, &blob_id)
            .await
            .map_err(fail)?
        else {
            return Ok(None);
        };
        item.chunks =
            Some(project_chunks(backend, caller, account_id, chunks, &plan.chunk_props).await?);
    }
    Ok(Some(item))
}

/// The `chunks` array: each stored chunk projected to the requested
/// [`DataSourceObject`] properties, with `position` the running sum of the
/// preceding lengths and each digest over the octets the chunk contributes.
async fn project_chunks<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    chunks: Vec<ChunkRef>,
    props: &ChunkPlan,
) -> Result<Vec<DataSourceObject>, JmapError> {
    let fail = |e: B::Error| server_fail_from_backend(&e);
    let mut out = Vec::with_capacity(chunks.len());
    let mut position: u64 = 0;
    for chunk in chunks {
        let end = chunk
            .offset
            .checked_add(chunk.length)
            .ok_or_else(|| JmapError::server_fail("backend chunk range overflows"))?;
        let range = chunk.offset..end;
        let mut dso = DataSourceObject::default();
        if props.blob_id {
            dso.blob_id = Some(BlobRef::Id(chunk.blob_id.clone()));
        }
        if props.size {
            dso.size = Some(chunk.size);
        }
        if props.offset {
            dso.offset = Some(chunk.offset);
        }
        if props.length {
            dso.length = Some(chunk.length);
        }
        if props.position {
            dso.position = Some(position);
        }
        let mut octets: Option<Vec<u8>> = None;
        for alg in &props.digests {
            let query = DigestQuery::new(chunk.blob_id.clone(), alg.clone(), range.clone());
            let digest = match backend
                .stored_digest(caller, account_id, &query)
                .await
                .map_err(fail)?
            {
                Some(d) => d,
                None => {
                    if octets.is_none() {
                        // A chunk the blob is built from must be readable;
                        // its absence is a backend inconsistency.
                        octets = Some(
                            backend
                                .read_blob_range(caller, account_id, &chunk.blob_id, range.clone())
                                .await
                                .map_err(fail)?
                                .ok_or_else(|| {
                                    JmapError::server_fail("backend lost a chunk of a blob")
                                })?,
                        );
                    }
                    compute_digest(backend, alg, &[octets.as_deref().unwrap_or_default()])?
                }
            };
            dso.digests.insert(alg.clone(), digest);
        }
        position = position
            .checked_add(chunk.length)
            .ok_or_else(|| JmapError::server_fail("backend chunk lengths overflow"))?;
        out.push(dso);
    }
    Ok(out)
}
