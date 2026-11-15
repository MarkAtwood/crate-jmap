//! RFC 9404 `Blob/upload` (§4.1), served under `urn:ietf:params:jmap:blob`
//! (docs/design/DECISIONS.md decision 11).

use std::collections::BTreeMap;

use jmap_blob_types::{BlobObject, BlobUploadCallResponse};
use jmap_server::{HandlerOutput, enforce_max_objects_in_set};
use jmap_types::{Id, JmapError, SetError};
use serde_json::{Map, Value};

use crate::backend::BlobBackend;
use crate::set::{Ctx, create_all, invalid};
use crate::start::{Start, start};

/// Handle `Blob/upload` (RFC 9404 §4.1).
///
/// `created_ids` are the request's creation ids so far, for
/// `#creationId` references to blobs created by earlier calls; `using` is
/// the request's capability list (both blob capabilities at once are
/// refused). Like Blob/set and Blob/convert, it runs whichever blob
/// capability the request names.
///
/// Each UploadObject (`data` and `type` only) is created exactly like a
/// `Blob/set` create: entries run in `#creationId` dependency order, with
/// the same source checks and SetErrors (`invalidProperties` `["data"]`,
/// `notFound`, `tooLarge`). Uploads persist, and every one appears in
/// `created` with its `id`, `type`, `size` and `expires`, so the
/// dispatcher adds each to `createdIds` (§4.1: servers "MUST add an entry
/// to the createdIds map"). The response is `accountId`, `created` and
/// `notCreated` only: blobs have no state (decision 21(iii)).
///
/// Per-entry failures go to `notCreated`: `invalidProperties` naming a
/// property other than `data` and `type` (an RFC 9404 UploadObject has no
/// `size`, `digest:*` or `noPersist`), an invalid creation id (`["id"]`)
/// or a creation cycle (`["data"]`), plus the Blob/set create errors.
///
/// # Errors
///
/// A method-level [`JmapError`]: `invalidArguments` for both blob
/// capabilities in `using`, a missing or malformed `create`, or a
/// non-empty `update` or `destroy` (§4.1: "only create is allowed";
/// decision 21(iv)); `accountNotFound`; `accountNotSupportedByMethod`;
/// `requestTooLarge` above `maxObjectsInSet`; `serverFail` for a backend
/// error outside a single entry.
pub async fn handle_blob_upload<B: BlobBackend>(
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
    // Decision 21(iv): dropping an update or destroy would silently lose a
    // write the client asked for.
    for key in ["update", "destroy"] {
        let empty = match args.get(key) {
            None | Some(Value::Null) => true,
            Some(Value::Object(m)) => m.is_empty(),
            Some(Value::Array(a)) => a.is_empty(),
            Some(_) => false,
        };
        if !empty {
            return Err(JmapError::invalid_arguments(format!(
                "Blob/upload takes no {key}: \"only create is allowed\" (RFC 9404 §4.1)"
            )));
        }
    }
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;
    let create = match args.remove("create") {
        Some(Value::Object(m)) => m,
        _ => {
            return Err(JmapError::invalid_arguments(
                "create is required and must be an object (RFC 9404 §4.1)",
            ));
        }
    };

    let mut ctx = Ctx {
        backend,
        caller,
        account_id: &account_id,
        capability: &capability,
        created_ids,
        in_call: create.keys().map(|k| Id::from(k.as_str())).collect(),
        local: BTreeMap::new(),
    };
    let (created, not_created) = create_all(&mut ctx, create, upload_object_only).await?;

    let mut response = BlobUploadCallResponse::new(account_id);
    response.not_created = not_created;
    for (cid, blob) in created {
        // Never omitted: every upload must reach createdIds (§4.1).
        let mut object = BlobObject::new(blob.id, blob.media_type, Some(blob.size));
        object.expires = Some(blob.expires);
        response.created.insert(cid, object);
    }
    let primary = serde_json::to_value(&response)
        .map_err(|e| JmapError::server_fail(format!("serializing Blob/upload response: {e}")))?;
    Ok(HandlerOutput::new(primary))
}

/// RFC 9404 §4.1: an UploadObject has `data` and `type` only; the blobext
/// create properties (`size`, `digest:*`, `noPersist`) and anything else
/// are unknown.
fn upload_object_only(entry: &Map<String, Value>) -> Result<(), Box<SetError>> {
    let unknown: Vec<&String> = entry
        .keys()
        .filter(|k| !matches!(k.as_str(), "data" | "type"))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(Box::new(
            invalid(unknown.into_iter().cloned())
                .with_description("an UploadObject has only data and type (RFC 9404 §4.1)"),
        ))
    }
}
