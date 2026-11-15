//! `Blob/lookup` (blobext-03 §6).

use std::collections::{BTreeMap, BTreeSet};

use jmap_blob_types::errors::UNKNOWN_DATA_TYPE;
use jmap_blob_types::{BlobInfo, BlobLookupResponse, BlobRef};
use jmap_server::{HandlerOutput, enforce_max_objects_in_get, server_fail_from_backend};
use jmap_types::{Id, JmapError};
use serde_json::{Map, Value};

use crate::backend::BlobBackend;
use crate::start::{Start, start};

/// Handle `Blob/lookup` (blobext-03 §6; RFC 9404 §4.3).
///
/// `created_ids` are the request's creation ids so far, for
/// `#creationId` references to blobs created by earlier calls; `using` is
/// the request's capability list, which selects blob2 or RFC 9404
/// behaviour ([`BlobMode`](crate::BlobMode)) and must name the capability
/// of every requested type.
///
/// `list` has one entry per requested id, in request order (a repeated id
/// repeats its entry), and every entry's `matchedIds` has a key for every
/// requested type. There is no `notFound`: an unknown or invisible blob,
/// or a `#creationId` with no creation, gets empty arrays, so the response
/// does not reveal whether the blob exists. Both modes behave alike (RFC
/// 9404 §4.3 has the same rules; its §4.3.1 example's `notFound` is not
/// followed, docs/design/DECISIONS.md 21(ii)).
///
/// # Errors
///
/// The common start errors (`invalidArguments`, `accountNotFound`,
/// `accountNotSupportedByMethod`, `serverFail`); `invalidArguments` for a
/// missing or malformed `typeNames` or `ids`; `requestTooLarge` above
/// `maxObjectsInGet`; `unknownDataType` for a type name not in
/// `supportedTypeNames`, not able to reference blobs
/// ([`BlobBackend::lookup_type_capability`]), or whose capability `using`
/// lacks; `serverFail` for a backend error.
pub async fn handle_blob_lookup<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    created_ids: &BTreeMap<Id, Id>,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let Start {
        account_id,
        args: map,
        capability,
        ..
    } = start(backend, caller, args, using).await?;
    let type_names = required(&map, "typeNames", |s| {
        Ok::<_, std::convert::Infallible>(s.to_owned())
    })?;
    let ids = required(&map, "ids", BlobRef::parse)?;
    enforce_max_objects_in_get(&map, backend.max_objects_in_get(caller, &account_id))?;

    for name in &type_names {
        let capability_uri = capability
            .supported_type_names
            .iter()
            .any(|t| t == name)
            .then(|| backend.lookup_type_capability(name))
            .flatten();
        if !capability_uri.is_some_and(|uri| using.iter().any(|u| u == uri)) {
            let mut e = JmapError::custom(UNKNOWN_DATA_TYPE);
            e.description = Some(format!(
                "{name} is not a supported type name, cannot reference blobs, \
                 or its capability is not in using (blobext-03 §6)"
            ));
            return Err(e);
        }
    }

    let resolved: Vec<Option<Id>> = ids
        .iter()
        .map(|r| match r {
            BlobRef::Creation(cid) => created_ids.get(cid).cloned(),
            other => other.as_id().cloned(),
        })
        .collect();
    let unique: Vec<Id> = resolved
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    // type name → blob id → referencing object ids.
    let mut matches: BTreeMap<&str, BTreeMap<Id, Vec<Id>>> = BTreeMap::new();
    for name in &type_names {
        if matches.contains_key(name.as_str()) || unique.is_empty() {
            continue;
        }
        let found = backend
            .blob_references(caller, &account_id, name, &unique)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        matches.insert(name, found);
    }

    let list = ids
        .iter()
        .zip(&resolved)
        .map(|(requested, blob_id)| {
            let matched = type_names
                .iter()
                .map(|name| {
                    let objects = blob_id
                        .as_ref()
                        .and_then(|b| matches.get(name.as_str())?.get(b))
                        .cloned()
                        .unwrap_or_default();
                    (name.clone(), objects)
                })
                .collect();
            // A resolved `#creationId` reports the blobId it names; an
            // unresolved one is echoed as sent (`Id::from` keeps the `#`),
            // so the client can match it to its request.
            let id = blob_id
                .clone()
                .unwrap_or_else(|| Id::from(requested.to_string()));
            BlobInfo::new(id, matched)
        })
        .collect();
    let primary = serde_json::to_value(BlobLookupResponse::new(account_id, list))
        .map_err(|e| JmapError::server_fail(format!("serializing Blob/lookup response: {e}")))?;
    Ok(HandlerOutput::new(primary))
}

/// The required argument `key`: a list of strings, each through `parse`.
///
/// # Errors
///
/// `invalidArguments` when it is missing, null, not a list of strings, or
/// `parse` rejects an element.
fn required<T, E: std::fmt::Display>(
    map: &Map<String, Value>,
    key: &str,
    parse: impl Fn(&str) -> Result<T, E>,
) -> Result<Vec<T>, JmapError> {
    let Some(Value::Array(items)) = map.get(key) else {
        return Err(JmapError::invalid_arguments(format!(
            "{key} is required and must be a list"
        )));
    };
    items
        .iter()
        .map(|item| {
            let s = item.as_str().ok_or_else(|| {
                JmapError::invalid_arguments(format!("{key} must contain only strings"))
            })?;
            parse(s).map_err(|e| {
                JmapError::invalid_arguments(format!("invalid {key} entry {s:?}: {e}"))
            })
        })
        .collect()
}
