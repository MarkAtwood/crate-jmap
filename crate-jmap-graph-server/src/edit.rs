//! The draft-atwood-jmap-graph-edit-00 rules that are not method handlers:
//! the GraphNode `editFormat` and `snapshotSeq` checks (§GraphNode
//! Extensions, §Snapshots) and base64url `data` decoding.
//!
//! [`check_node_edit`] is a pure function so that `GraphNode/set` and a
//! backend re-verifying atomically apply the same rules.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jmap_graph_types::edit::{GRAPH_NODE_EDIT_FORMAT_PROPERTY, GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY};
use serde_json::{Map, Value};

use crate::backend::{SetError, SetErrorType};

const EDIT_FORMAT: &str = GRAPH_NODE_EDIT_FORMAT_PROPERTY;
const SNAPSHOT_SEQ: &str = GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY;

/// The decoded length of `data`, or `None` if it is not "encoded with
/// base64url without padding (RFC 4648, Section 5)". Padding, characters
/// outside the URL-safe alphabet and non-zero trailing bits are refused.
pub(crate) fn base64url_len(data: &str) -> Option<usize> {
    URL_SAFE_NO_PAD.decode(data).ok().map(|v| v.len())
}

fn invalid(property: &str, description: &str) -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_description(description)
        .with_properties([property])
}

fn prop<'a>(m: Option<&'a Map<String, Value>>, key: &str) -> &'a Value {
    m.and_then(|m| m.get(key)).unwrap_or(&Value::Null)
}

/// Check a GraphNode as it would be stored, `next`, against
/// draft-atwood-jmap-graph-edit-00 §GraphNode Extensions and §Snapshots.
///
/// `current` is the stored node of an update and `None` for a create;
/// `sets_snapshot_seq` is whether the create or patch names `snapshotSeq`;
/// `head` is the head of the node's edit log; `projected` is whether the
/// node's Graph is a projected Graph. Absent properties are `null`.
///
/// # Errors
///
/// The SetError of the first rule `next` breaks:
///
/// - `editFormat` not String|null, or changed once non-null ("the server
///   rejects any other change with an `invalidProperties` SetError naming
///   `editFormat`"); `snapshotSeq` not UnsignedInt|null.
/// - A non-null `snapshotSeq` while `editFormat` is `null` (naming
///   `snapshotSeq`).
/// - Setting `editFormat` on a node of a projected Graph (`forbidden`).
/// - Giving a node "a non-null `editFormat` while it has a non-null
///   `blobId` and a `null` `snapshotSeq`" (naming `editFormat`).
/// - On a node whose `editFormat` was already set, "A `GraphNode/set`
///   update changes `blobId` if and only if it sets `snapshotSeq`.
///   Otherwise the server rejects it with an `invalidProperties` SetError
///   naming the property that is missing."
/// - "a value of 0, a value greater than the head of the edit log, a value
///   less than the current `snapshotSeq`, or `null` when the current
///   `snapshotSeq` is not `null`" (naming `snapshotSeq`).
/// - "`blobId` is `null` exactly when `snapshotSeq` is `null`" (naming
///   `blobId`).
#[allow(clippy::result_large_err)]
pub(crate) fn check_node_edit(
    current: Option<&Map<String, Value>>,
    next: &Map<String, Value>,
    sets_snapshot_seq: bool,
    head: u64,
    projected: bool,
) -> Result<(), SetError> {
    let next = Some(next);
    let format = prop(next, EDIT_FORMAT);
    let seq = prop(next, SNAPSHOT_SEQ);
    if !(format.is_string() || format.is_null()) {
        return Err(invalid(EDIT_FORMAT, "editFormat is a String or null"));
    }
    if !(seq.is_u64() || seq.is_null()) {
        return Err(invalid(
            SNAPSHOT_SEQ,
            "snapshotSeq is an UnsignedInt or null",
        ));
    }
    let current_format = prop(current, EDIT_FORMAT);
    if !current_format.is_null() && format != current_format {
        return Err(invalid(EDIT_FORMAT, "editFormat never changes once set"));
    }
    if format.is_null() {
        return if seq.is_null() {
            Ok(())
        } else {
            Err(invalid(
                SNAPSHOT_SEQ,
                "snapshotSeq is null while editFormat is null",
            ))
        };
    }
    let blob = prop(next, "blobId");
    if current_format.is_null() {
        if projected {
            return Err(SetError::new(SetErrorType::Forbidden)
                .with_description("a node of a projected Graph has no edit log"));
        }
        if !blob.is_null() && seq.is_null() {
            return Err(invalid(
                EDIT_FORMAT,
                "a node with content and no snapshot cannot be given an editFormat",
            ));
        }
    } else if (blob != prop(current, "blobId")) != sets_snapshot_seq {
        let missing = if sets_snapshot_seq {
            "blobId"
        } else {
            SNAPSHOT_SEQ
        };
        return Err(invalid(
            missing,
            "an update changes blobId if and only if it sets snapshotSeq",
        ));
    }
    if sets_snapshot_seq {
        let current_seq = prop(current, SNAPSHOT_SEQ).as_u64();
        let bad = match seq.as_u64() {
            None => current_seq.is_some(),
            Some(n) => n == 0 || n > head || current_seq.is_some_and(|c| n < c),
        };
        if bad {
            return Err(invalid(
                SNAPSHOT_SEQ,
                "snapshotSeq must be between the current snapshotSeq and the head of the edit log",
            ));
        }
    }
    if blob.is_null() != seq.is_null() {
        return Err(invalid(
            "blobId",
            "blobId is null exactly when snapshotSeq is null",
        ));
    }
    Ok(())
}
