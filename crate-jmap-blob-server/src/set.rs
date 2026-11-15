//! `Blob/set` (blobext-03 §4).

use std::collections::BTreeMap;

use jmap_server::HandlerOutput;
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::backend::BlobBackend;

/// Handle `Blob/set` (blobext-03 §4).
///
/// `created_ids` are the request's creation ids so far, for
/// `#creationId` references to blobs created by earlier calls; `using` is
/// the request's capability list, which selects blob2 or RFC 9404
/// behaviour ([`BlobMode`](crate::BlobMode)).
///
/// Not implemented yet: answers `serverFail` until bd JMAP-tna1.2.8.
///
/// # Errors
///
/// A method-level [`JmapError`].
pub async fn handle_blob_set<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    created_ids: &BTreeMap<Id, Id>,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let _ = (backend, caller, args, created_ids, using);
    Err(JmapError::server_fail(
        "Blob/set is not implemented yet (JMAP-tna1.2.8)",
    ))
}
