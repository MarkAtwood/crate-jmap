//! `Blob/set` (blobext-03 §3, §4, §7; RFC 8620 §5.3).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jmap_blob_types::{
    BlobAccountCapability, BlobCreateObject, BlobObject, BlobRef, BlobSetResponse, BlobUpdated,
    DataSourceObject, DigestAlgorithm,
};
use jmap_server::{
    BackendSetError, HandlerOutput, SERVER_FAIL_INTERNAL_DESC, backend_set_error_value,
    enforce_max_objects_in_set, server_fail_from_backend,
};
use jmap_types::{Id, JmapError, SetError, SetErrorType, UTCDate};
use serde_json::{Map, Value, json};

use crate::backend::{BlobBackend, BlobCreate, BlobSource, CreatedBlob, TouchOutcome};
use crate::order::topo_order;
use crate::start::{Start, compute_digest, start};

/// Handle `Blob/set` (blobext-03 §4).
///
/// `created_ids` are the request's creation ids so far, for
/// `#creationId` references to blobs created by earlier calls; `using` is
/// the request's capability list, which selects blob2 or RFC 9404
/// behaviour ([`BlobMode`](crate::BlobMode)).
///
/// Creates run first, in dependency order, so a source may name a blob
/// created by another entry of the same `create` map; then updates
/// (touches) and destroys, whose ids may also be `#creationId`s. Every
/// check that can be made before storing runs here, and the backend
/// re-verifies source access in [`BlobBackend::create_blob`]. Blobs have
/// no state: `oldState` and `newState` are `null`.
///
/// Per-entry failures go to `notCreated`, `notUpdated` and
/// `notDestroyed`:
///
/// - create: `invalidProperties` naming an unknown or ill-typed property,
///   a top-level `digest:*` whose algorithm is not advertised or does not
///   match (`["digest:<alg>"]`), a wrong whole `size` (`["size"]`), or a
///   bad data source (`["data"]`: not exactly one data key, invalid
///   base64, a range past its source's end, a wrong `size` or `position`,
///   an unadvertised or mismatched source digest, a creation cycle);
///   `notFound` for a source blob the caller cannot see or a
///   `#creationId` with no creation; `tooLarge` above `maxDataSources` or
///   `maxSizeBlobSet`; and whatever `create_blob` returns.
/// - update: `invalidProperties` for a property other than `expires` that
///   is not at its current value, or an `expires` that is not a UTCDate
///   or null; `invalidPatch` for a patch that is not an object;
///   `notFound`; and whatever `touch_blob` returns.
/// - destroy: whatever `destroy_blob` returns (`notFound`,
///   `blobHasReference`, ...).
///
/// # Errors
///
/// A method-level [`JmapError`]: `invalidArguments` for both blob
/// capabilities in `using`, a non-null `ifInState`, or malformed
/// `create`/`update`/`destroy`; `accountNotFound`;
/// `accountNotSupportedByMethod`; `requestTooLarge` above
/// `maxObjectsInSet`; `serverFail` for a backend error outside a single
/// entry, or an advertised digest algorithm without a digester.
pub async fn handle_blob_set<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    created_ids: &BTreeMap<Id, Id>,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let Start {
        account_id,
        mut args,
        capability,
        ..
    } = start(backend, caller, args, using).await?;
    if !args.get("ifInState").is_none_or(Value::is_null) {
        return Err(JmapError::invalid_arguments(
            "Blob/set takes no ifInState: blobs have no state (blobext-03 §4)",
        ));
    }
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;
    let create = take_object(&mut args, "create")?;
    let update = take_object(&mut args, "update")?;
    let destroy = take_ids(&mut args, "destroy")?;

    let mut ctx = Ctx {
        backend,
        caller,
        account_id: &account_id,
        capability: &capability,
        created_ids,
        in_call: create.keys().map(|k| Id::from(k.as_str())).collect(),
        local: BTreeMap::new(),
    };
    let mut response = BlobSetResponse::new(account_id.clone());

    let (created, not_created) = create_all(&mut ctx, create, |_| Ok(())).await?;
    response.not_created = not_created;
    for (cid, created) in created {
        if !created.omit_from_response {
            let mut object = BlobObject::new(created.id, created.media_type, Some(created.size));
            object.expires = Some(created.expires);
            response.created.insert(cid, object);
        }
    }

    for (key, patch) in update {
        let blob_id = match ctx.resolve_key(&key) {
            Ok(id) => id,
            Err(e) => {
                response.not_updated.insert(Id::from(key.as_str()), *e);
                continue;
            }
        };
        match ctx.update_one(&blob_id, patch).await? {
            Ok(updated) => {
                response.updated.insert(blob_id, updated);
            }
            Err(e) => {
                response.not_updated.insert(blob_id, *e);
            }
        }
    }

    for key in destroy {
        let blob_id = match ctx.resolve_key(&key) {
            Ok(id) => id,
            Err(e) => {
                response.not_destroyed.insert(Id::from(key.as_str()), *e);
                continue;
            }
        };
        match backend.destroy_blob(caller, &account_id, &blob_id).await {
            Ok(()) => response.destroyed.push(blob_id),
            Err(e) => {
                response.not_destroyed.insert(blob_id, backend_set_error(e));
            }
        }
    }

    let primary = serde_json::to_value(&response)
        .map_err(|e| JmapError::server_fail(format!("serializing Blob/set response: {e}")))?;
    Ok(HandlerOutput::new(primary))
}

/// The state one `Blob/set` (or `Blob/upload`) call shares across entries.
pub(crate) struct Ctx<'a, B: BlobBackend> {
    pub(crate) backend: &'a B,
    pub(crate) caller: &'a B::CallerCtx,
    pub(crate) account_id: &'a Id,
    pub(crate) capability: &'a BlobAccountCapability,
    /// Creation ids from earlier calls of the request.
    pub(crate) created_ids: &'a BTreeMap<Id, Id>,
    /// Every creation id of this call's `create` map. A `#ref` to one of
    /// them never falls back to `created_ids`: if its entry failed, the
    /// reference is `notFound`.
    pub(crate) in_call: BTreeSet<Id>,
    /// Blobs created so far by this call, including omitted `noPersist`
    /// ones (usable within the call, blobext-03 §4).
    pub(crate) local: BTreeMap<Id, Id>,
}

impl<B: BlobBackend> Ctx<'_, B> {
    /// The blob a reference names, or `None` for a `#creationId` with no
    /// creation (gap e).
    pub(crate) fn resolve(&self, r: &BlobRef) -> Option<Id> {
        match r {
            BlobRef::Creation(cid) if self.in_call.contains(cid) => self.local.get(cid).cloned(),
            BlobRef::Creation(cid) => self
                .local
                .get(cid)
                .or_else(|| self.created_ids.get(cid))
                .cloned(),
            other => other.as_id().cloned(),
        }
    }

    /// An `update` key or `destroy` element, resolved.
    fn resolve_key(&self, key: &str) -> Result<Id, Box<SetError>> {
        let r = BlobRef::parse(key).map_err(|e| {
            Box::new(invalid(["id"]).with_description(format!("not a blob id: {e}")))
        })?;
        self.resolve(&r)
            .ok_or_else(|| Box::new(not_found("no blob was created under this creation id")))
    }

    /// Touch one blob (blobext-03 §4 update).
    async fn update_one(
        &self,
        blob_id: &Id,
        patch: Value,
    ) -> Result<Result<Option<BlobUpdated>, Box<SetError>>, JmapError> {
        let Value::Object(patch) = patch else {
            return Ok(Err(Box::new(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("a patch must be an object"),
            )));
        };
        let Some(meta) = self
            .backend
            .blob_info(self.caller, self.account_id, blob_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        else {
            return Ok(Err(Box::new(not_found("no such blob"))));
        };
        let mut requested = None;
        for (key, value) in patch {
            let current = match key.as_str() {
                "expires" => {
                    requested = Some(match value {
                        Value::Null => None,
                        Value::String(s) => match UTCDate::new_validated(s) {
                            Ok(d) => Some(d),
                            Err(_) => return Ok(Err(Box::new(invalid(["expires"])))),
                        },
                        _ => return Ok(Err(Box::new(invalid(["expires"])))),
                    });
                    continue;
                }
                "id" => json!(blob_id),
                "type" => json!(meta.media_type),
                "size" => json!(meta.size),
                _ => Value::Null,
            };
            if !matches!(key.as_str(), "id" | "type" | "size") || value != current {
                return Ok(Err(Box::new(invalid([key]).with_description(
                    "only expires can change; other properties only at their current value",
                ))));
            }
        }
        // A patch that does not mention expires changes nothing.
        let Some(requested) = requested else {
            return Ok(Ok(None));
        };
        match self
            .backend
            .touch_blob(self.caller, self.account_id, blob_id, requested)
            .await
        {
            Ok(TouchOutcome::Applied(expires)) => Ok(Ok(Some(BlobUpdated::with_expires(expires)))),
            // `Honoured` and any future outcome: the request stands.
            Ok(_) => Ok(Ok(None)),
            Err(e) => Ok(Err(Box::new(backend_set_error(e)))),
        }
    }
}

/// Created blobs and failures of one `create` map, by creation id.
pub(crate) type CreateResults = (BTreeMap<String, CreatedBlob>, BTreeMap<String, SetError>);

/// Run a `Blob/set` or `Blob/upload` `create` map: check each creation id,
/// screen each entry with `screen` and then [`parse_create`], order the
/// valid entries by their in-call references (a cycle rejects its members
/// with `["data"]`), and create them with [`create_one`]. Every created
/// blob is recorded in `ctx.local`, including one the backend asks to
/// omit, so later entries can read it.
///
/// # Errors
///
/// A method-level `serverFail` from [`create_one`].
pub(crate) async fn create_all<B: BlobBackend>(
    ctx: &mut Ctx<'_, B>,
    create: Map<String, Value>,
    screen: impl Fn(&Map<String, Value>) -> Result<(), Box<SetError>>,
) -> Result<CreateResults, JmapError> {
    let mut created = BTreeMap::new();
    let mut not_created = BTreeMap::new();
    let mut parsed = BTreeMap::new();
    for (cid, value) in create {
        if let Err(e) = Id::new_validated(cid.as_str()) {
            not_created.insert(
                cid,
                invalid(["id"]).with_description(format!("creation id is not an Id: {e}")),
            );
            continue;
        }
        let checked = match &value {
            Value::Object(map) => screen(map),
            _ => Ok(()),
        }
        .and_then(|()| parse_create(value, ctx.capability));
        match checked {
            Ok(obj) => {
                parsed.insert(Id::from(cid.as_str()), obj);
            }
            Err(e) => {
                not_created.insert(cid, *e);
            }
        }
    }
    let nodes: Vec<Id> = parsed.keys().cloned().collect();
    let order = topo_order(&nodes, |cid| {
        parsed
            .get(cid)
            .map(|obj| creation_refs(&obj.data))
            .unwrap_or_default()
    });
    for cid in &order.cyclic {
        not_created.insert(
            cid.to_string(),
            invalid(["data"]).with_description("creation references form a cycle"),
        );
    }
    for cid in order.order {
        let Some(obj) = parsed.remove(&cid) else {
            continue;
        };
        match create_one(ctx, obj).await? {
            Ok(blob) => {
                ctx.local.insert(cid.clone(), blob.id.clone());
                created.insert(cid.to_string(), blob);
            }
            Err(e) => {
                not_created.insert(cid.to_string(), *e);
            }
        }
    }
    Ok((created, not_created))
}

/// Steps 1-2 of a create: the entry's properties, typed (blobext-03 §4).
///
/// # Errors
///
/// `invalidProperties` naming an unknown or ill-typed property, a
/// non-null `digest:<alg>` whose algorithm is not advertised, or `data`
/// when a source does not parse; `tooLarge` above `maxDataSources`.
pub(crate) fn parse_create(
    value: Value,
    capability: &BlobAccountCapability,
) -> Result<BlobCreateObject, Box<SetError>> {
    let Value::Object(mut map) = value else {
        return Err(Box::new(
            invalid(Vec::<String>::new()).with_description("a create entry must be an object"),
        ));
    };
    if !map.contains_key("data") {
        return Err(Box::new(
            invalid(["data"]).with_description("data is required"),
        ));
    }
    for (key, v) in &map {
        let ok = match key.as_str() {
            "data" => v.is_array(),
            "type" => v.is_null() || v.is_string(),
            "size" => v.is_null() || v.is_u64(),
            "noPersist" => v.is_null() || v.is_boolean(),
            _ => match DigestAlgorithm::from_property_name(key) {
                Some(alg) => {
                    v.is_null()
                        || (v.is_string() && capability.supported_digest_algorithms.contains(&alg))
                }
                None => false,
            },
        };
        if !ok {
            return Err(Box::new(invalid([key.clone()])));
        }
    }
    if map.get("noPersist").is_some_and(Value::is_null) {
        map.remove("noPersist");
    }
    let obj: BlobCreateObject = serde_json::from_value(Value::Object(map)).map_err(|e| {
        Box::new(invalid(["data"]).with_description(format!("invalid data source: {e}")))
    })?;
    if u64::try_from(obj.data.len()).is_ok_and(|n| n > capability.max_data_sources) {
        return Err(Box::new(
            SetError::new(SetErrorType::TooLarge)
                .with_description("more data sources than maxDataSources"),
        ));
    }
    Ok(obj)
}

/// The in-call creation ids a create entry's sources name.
pub(crate) fn creation_refs(data: &[DataSourceObject]) -> Vec<Id> {
    data.iter()
        .filter_map(|d| match &d.blob_id {
            Some(BlobRef::Creation(cid)) => Some(cid.clone()),
            _ => None,
        })
        .collect()
}

/// One data source, checked: what it contributes and where from.
struct Checked {
    /// Inline octets (already cut to the range), or the source blob.
    source: BlobSource,
    /// Octets contributed.
    len: u64,
    /// Digests the client gave for this source.
    digests: BTreeMap<DigestAlgorithm, String>,
}

/// Steps 4-8 of a create (docs/design/blobext.md §5): resolve and check
/// every source, check the whole size and digests, and create the blob.
///
/// The outer `Err` is method-level; the inner one is the entry's
/// `notCreated` value.
pub(crate) async fn create_one<B: BlobBackend>(
    ctx: &mut Ctx<'_, B>,
    obj: BlobCreateObject,
) -> Result<Result<CreatedBlob, Box<SetError>>, JmapError> {
    let fail = |e: B::Error| server_fail_from_backend(&e);
    let advertised = &ctx.capability.supported_digest_algorithms;

    // Sources: shape, range, size and position (no octets read yet).
    let mut checked = Vec::with_capacity(obj.data.len());
    let mut total: u64 = 0;
    for (i, dso) in obj.data.into_iter().enumerate() {
        let bad = |why: &str| Ok(Err(Box::new(bad_source(i, why))));
        let keys = usize::from(dso.data_as_text.is_some())
            + usize::from(dso.data_as_base64.is_some())
            + usize::from(dso.blob_id.is_some());
        if keys != 1 {
            return bad("needs exactly one of data:asText, data:asBase64 and blobId");
        }
        if let Some(name) = dso.extra.keys().next() {
            return bad(&format!("unknown property {name}"));
        }
        if let Some(alg) = dso.digests.keys().find(|a| !advertised.contains(a)) {
            return bad(&format!("{} is not supported", alg.property_name()));
        }
        let origin = if let Some(text) = dso.data_as_text {
            Origin::Inline(text.into_bytes())
        } else if let Some(b64) = dso.data_as_base64 {
            let Ok(bytes) = STANDARD.decode(b64.as_bytes()) else {
                return bad("data:asBase64 is not valid base64");
            };
            Origin::Inline(bytes)
        } else if let Some(r) = &dso.blob_id {
            let Some(blob_id) = ctx.resolve(r) else {
                return Ok(Err(Box::new(not_found(&format!(
                    "data/{i}: no blob was created under {r}"
                )))));
            };
            let Some(meta) = ctx
                .backend
                .blob_info(ctx.caller, ctx.account_id, &blob_id)
                .await
                .map_err(fail)?
            else {
                return Ok(Err(Box::new(
                    not_found(&format!("data/{i}: blob {blob_id} not found"))
                        .with_not_found(vec![blob_id]),
                )));
            };
            Origin::Blob(blob_id, meta.size)
        } else {
            return bad("needs exactly one of data:asText, data:asBase64 and blobId");
        };
        let full_size = match &origin {
            Origin::Inline(bytes) => len_u64(bytes),
            Origin::Blob(_, size) => *size,
        };
        let offset = dso.offset.unwrap_or(0);
        if offset > full_size {
            return bad("offset is past the end of the source");
        }
        let end = match dso.length {
            None => full_size,
            Some(length) => match offset.checked_add(length) {
                Some(end) if end <= full_size => end,
                _ => return bad("offset + length is past the end of the source"),
            },
        };
        if dso.size.is_some_and(|s| s != full_size) {
            return bad("size is not the size of the source");
        }
        if dso.position.is_some_and(|p| p != total) {
            return bad("position is not where the source starts in the blob");
        }
        let len = end - offset;
        total = match total.checked_add(len) {
            Some(t) => t,
            None => return Ok(Err(Box::new(too_large("the blob is too large")))),
        };
        let source = match origin {
            Origin::Inline(bytes) => BlobSource::Bytes(slice(&bytes, offset..end).to_vec()),
            Origin::Blob(blob_id, _) => BlobSource::Blob {
                blob_id,
                range: offset..end,
            },
        };
        checked.push(Checked {
            source,
            len,
            digests: dso.digests,
        });
    }

    if let Some(max) = ctx.capability.max_size_blob_set
        && total > max
    {
        return Ok(Err(Box::new(
            too_large("the blob is larger than maxSizeBlobSet").with_max_size(max),
        )));
    }
    if obj.size.is_some_and(|s| s != total) {
        return Ok(Err(Box::new(invalid(["size"]).with_description(format!(
            "the sources contribute {total} octets"
        )))));
    }

    // Digests: read source octets only when some digest needs them.
    let need_octets = !obj.digests.is_empty() || checked.iter().any(|c| !c.digests.is_empty());
    if need_octets {
        let mut parts: Vec<Vec<u8>> = Vec::with_capacity(checked.len());
        for (i, c) in checked.iter().enumerate() {
            let octets = match &c.source {
                BlobSource::Bytes(b) => b.clone(),
                BlobSource::Blob { blob_id, range } => match ctx
                    .backend
                    .read_blob_range(ctx.caller, ctx.account_id, blob_id, range.clone())
                    .await
                    .map_err(fail)?
                {
                    Some(o) if len_u64(&o) == c.len => o,
                    _ => {
                        return Ok(Err(Box::new(
                            not_found(&format!("data/{i}: blob {blob_id} not found"))
                                .with_not_found(vec![blob_id.clone()]),
                        )));
                    }
                },
            };
            for (alg, want) in &c.digests {
                if compute_digest(ctx.backend, alg, &[&octets])? != *want {
                    return Ok(Err(Box::new(bad_source(
                        i,
                        &format!("{} does not match the source", alg.property_name()),
                    ))));
                }
            }
            parts.push(octets);
        }
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        for (alg, want) in &obj.digests {
            if compute_digest(ctx.backend, alg, &refs)? != *want {
                return Ok(Err(Box::new(
                    invalid([alg.property_name()])
                        .with_description("the digest does not match the blob"),
                )));
            }
        }
    }

    let mut create = BlobCreate::new(checked.into_iter().map(|c| c.source).collect(), total);
    create.media_type = obj.media_type;
    create.no_persist = obj.no_persist;
    Ok(ctx
        .backend
        .create_blob(ctx.caller, ctx.account_id, create)
        .await
        .map_err(|e| Box::new(backend_set_error(e))))
}

/// Where a data source's octets come from.
enum Origin {
    Inline(Vec<u8>),
    /// A blob and its full size.
    Blob(Id, u64),
}

fn len_u64(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// `range` of `bytes`, which the caller has checked lies within them.
fn slice(bytes: &[u8], range: Range<u64>) -> &[u8] {
    let start = usize::try_from(range.start).unwrap_or(bytes.len());
    let end = usize::try_from(range.end).unwrap_or(bytes.len());
    bytes.get(start..end).unwrap_or_default()
}

pub(crate) fn invalid<I, S>(props: I) -> SetError
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    SetError::new(SetErrorType::InvalidProperties).with_properties(props)
}

/// The `["data"]` error for source `i` (blobext-03 §3: the server "MUST
/// NOT guess").
fn bad_source(i: usize, why: &str) -> SetError {
    invalid(["data"]).with_description(format!("data/{i}: {why}"))
}

pub(crate) fn not_found(why: &str) -> SetError {
    SetError::new(SetErrorType::NotFound).with_description(why)
}

pub(crate) fn too_large(why: &str) -> SetError {
    SetError::new(SetErrorType::TooLarge).with_description(why)
}

/// A backend per-entry failure as its SetError; an `Other` error becomes a
/// redacted `serverFail` for that entry only, exactly as
/// [`backend_set_error_value`] writes it.
pub(crate) fn backend_set_error<E: std::fmt::Display>(e: BackendSetError<E>) -> SetError {
    serde_json::from_value(backend_set_error_value(e)).unwrap_or_else(|_| {
        SetError::new(SetErrorType::from("serverFail")).with_description(SERVER_FAIL_INTERNAL_DESC)
    })
}

/// Remove `key` from `args` as a JSON object (absent or null = empty).
fn take_object(args: &mut Map<String, Value>, key: &str) -> Result<Map<String, Value>, JmapError> {
    match args.remove(key) {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(m)) => Ok(m),
        Some(_) => Err(JmapError::invalid_arguments(format!(
            "{key} must be an object or null"
        ))),
    }
}

/// Remove `key` from `args` as a list of strings (absent or null = empty).
fn take_ids(args: &mut Map<String, Value>, key: &str) -> Result<Vec<String>, JmapError> {
    match args.remove(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                _ => Err(JmapError::invalid_arguments(format!(
                    "{key} must be a list of ids"
                ))),
            })
            .collect(),
        Some(_) => Err(JmapError::invalid_arguments(format!(
            "{key} must be a list of ids or null"
        ))),
    }
}
