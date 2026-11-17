//! RFC 8620 §6.3 `Blob/copy` (docs/design/blobext.md B23): core, not
//! blobext, so it needs neither blob capability.

use std::collections::BTreeMap;

use jmap_blob_types::BlobCopyResponse;
use jmap_server::{HandlerOutput, extract_account_id, resolve_blob_ref, server_fail_from_backend};
use jmap_types::{Id, JmapError, SetError, SetErrorType};
use serde_json::Value;

use crate::backend::BlobBackend;
use crate::set::backend_set_error;

/// Handle `Blob/copy` (RFC 8620 §6.3).
///
/// Copies each of `blobIds` from `fromAccountId` to `accountId` through
/// [`BlobBackend::copy_blob`]. `copied` maps each blobId, as sent, to its
/// id in `accountId`; `notCopied` maps it to the SetError, `notFound` when
/// "the blobId to be copied cannot be found". A blobId may be a
/// `#creationId` of a blob created earlier in the request (RFC 8620
/// §5.3), resolved against `created_ids`; an unknown reference, like any
/// value that is not an Id, is `notFound`. A repeated blobId is copied
/// once.
///
/// The request's `using` does not matter: the method is core, and the
/// accounts need not advertise a blob capability.
///
/// # Errors
///
/// A method-level [`JmapError`]: `invalidArguments` for a missing or
/// malformed `accountId`, `fromAccountId` or `blobIds`, or for
/// `fromAccountId` equal to `accountId` (§6.3 copies "*between* two
/// different accounts"; §5.4: it "MUST be different"); `accountNotFound`;
/// `fromAccountNotFound`; `requestTooLarge` for more `blobIds` than
/// `maxObjectsInSet`; `serverFail` for a backend error outside a single
/// blob.
pub async fn handle_blob_copy<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    created_ids: &BTreeMap<Id, Id>,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;
    if !account_exists(backend, caller, &account_id).await? {
        return Err(JmapError::account_not_found());
    }
    let from_account_id = match args.get("fromAccountId") {
        Some(Value::String(s)) => Id::new_validated(s)
            .map_err(|e| JmapError::invalid_arguments(format!("fromAccountId: {e}")))?,
        _ => {
            return Err(JmapError::invalid_arguments(
                "fromAccountId is required and must be a string (RFC 8620 §6.3)",
            ));
        }
    };
    if from_account_id == account_id {
        return Err(JmapError::invalid_arguments(
            "fromAccountId must be different from accountId: Blob/copy copies \
             between two different accounts (RFC 8620 §6.3)",
        ));
    }
    if !account_exists(backend, caller, &from_account_id).await? {
        return Err(JmapError::from_account_not_found());
    }
    let blob_ids: Vec<&str> = match args.get("blobIds") {
        Some(Value::Array(ids)) => ids
            .iter()
            .map(|v| v.as_str())
            .collect::<Option<_>>()
            .ok_or_else(|| JmapError::invalid_arguments("blobIds must be an array of strings"))?,
        _ => {
            return Err(JmapError::invalid_arguments(
                "blobIds is required and must be an array (RFC 8620 §6.3)",
            ));
        }
    };
    let max = backend.max_objects_in_set(caller, &account_id);
    if u64::try_from(blob_ids.len()).map_or(true, |n| n > max) {
        return Err(JmapError::request_too_large());
    }

    let mut response = BlobCopyResponse::new(from_account_id.clone(), account_id.clone());
    for raw in blob_ids {
        if response.copied.contains_key(raw) || response.not_copied.contains_key(raw) {
            continue;
        }
        let resolved = resolve_blob_ref(raw, created_ids);
        let Ok(blob_id) = Id::new_validated(resolved.into_owned()) else {
            response.not_copied.insert(
                raw.to_owned(),
                SetError::new(SetErrorType::NotFound).with_description(format!("no blob {raw}")),
            );
            continue;
        };
        match backend
            .copy_blob(caller, &from_account_id, &account_id, &blob_id)
            .await
        {
            Ok(id) => {
                response.copied.insert(raw.to_owned(), id);
            }
            Err(e) => {
                response
                    .not_copied
                    .insert(raw.to_owned(), backend_set_error(e));
            }
        }
    }
    let primary = serde_json::to_value(&response)
        .map_err(|e| JmapError::server_fail(format!("serializing Blob/copy response: {e}")))?;
    Ok(HandlerOutput::new(primary))
}

async fn account_exists<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
) -> Result<bool, JmapError> {
    backend
        .account_exists(caller, account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))
}
