//! The steps every `Blob/*` handler starts with (docs/design/blobext.md §5
//! "Common start").

use jmap_blob_types::BlobAccountCapability;
use jmap_server::{extract_account_id, server_fail_from_backend};
use jmap_types::{Id, JmapError};
use serde_json::{Map, Value};

use crate::backend::BlobBackend;
use crate::mode::BlobMode;

/// A call that passed the common start.
pub(crate) struct Start {
    /// The request's blob mode; never [`BlobMode::Conflict`].
    pub(crate) mode: BlobMode,
    pub(crate) account_id: Id,
    /// The arguments without `accountId`.
    pub(crate) args: Map<String, Value>,
    /// The account's blob capability, as advertised.
    pub(crate) capability: BlobAccountCapability,
}

/// Reject a request naming both blob capabilities, then resolve the
/// account and its blob capability.
///
/// # Errors
///
/// `invalidArguments` for both capabilities in `using` (blobext-03 §2.1:
/// "a client MUST NOT include both") or a missing or malformed
/// `accountId`; `accountNotFound`; `accountNotSupportedByMethod` when the
/// account lacks the capability; `serverFail` for a backend error.
pub(crate) async fn start<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<Start, JmapError> {
    let mode = BlobMode::from_using(using);
    if mode == BlobMode::Conflict {
        return Err(JmapError::invalid_arguments(
            "using must not contain both urn:ietf:params:jmap:blob and \
             urn:ietf:params:jmap:blob2 (blobext-03 §2.1)",
        ));
    }
    let (account_id, args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    let capability = backend
        .blob_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
        .ok_or_else(JmapError::account_not_supported_by_method)?;
    Ok(Start {
        mode,
        account_id,
        args,
        capability,
    })
}

/// The base64 `alg` digest of `octets` through the backend's digester.
///
/// # Errors
///
/// `serverFail` when the backend has no digester for `alg`: an advertised
/// algorithm is never answered with a wrong or empty value.
pub(crate) fn compute_digest<B: BlobBackend>(
    backend: &B,
    alg: &jmap_blob_types::DigestAlgorithm,
    parts: &[&[u8]],
) -> Result<String, JmapError> {
    let mut digester = backend.digester(alg).ok_or_else(|| {
        JmapError::server_fail(format!("advertised digest algorithm {alg} has no digester"))
    })?;
    for part in parts {
        digester.update(part);
    }
    Ok(digester.finish())
}
