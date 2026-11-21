//! draft-atwood-jmap-graph-provenance-00 server rules, for an account that
//! advertises `urn:ietf:params:jmap:graph:provenance`
//! ([`GraphBackend::has_capability`](crate::GraphBackend::has_capability)
//! with [`JMAP_GRAPH_PROVENANCE_URI`]).
//!
//! Pure checks over a `GraphNode`, `GraphEdge` or Graph as JSON, shared by the
//! handlers (on the object as they fetched it) and a backend (re-checked
//! atomically with the write, as the reference
//! `memory::MemoryBackend` does):
//!
//! - [`check_record`]: §Provenance Properties and §Curation Properties
//!   values (`invalidProperties` naming the property).
//! - [`check_record_create`]: no create as `retracted` (§Retraction) and
//!   the create limits for a user without `mayCurate` (§Sharing Rights).
//! - [`check_record_update`]: §Retraction (irreversible, cleared
//!   properties) and the §Sharing Rights split between `mayWrite` and
//!   `mayCurate`; [`retraction_patch`] adds the clearing to a patch.
//! - [`require_curate`]: "Destroying a record requires `mayCurate`".
//! - [`check_retention`]: Graph `retention` (§Graph Extensions).
//!
//! The evidence limit (§Limits), the `GraphEdge` rule for a retracted
//! `fromId`, the retention sweep and the query filters need the store, so
//! they live in the backend and [`crate::query`].

pub use jmap_graph_types::{
    EVIDENCE_EDGE_TYPE, JMAP_GRAPH_PROVENANCE_URI, MAX_EVIDENCE_EDGES, MAX_PURPOSES,
};
use jmap_graph_types::{GraphRights, PermittedUse, ProvenanceBasis, RecordStatus, Retention};
use jmap_types::{SetError, SetErrorType, UTCDate};
use serde_json::{Map, Value, json};

/// The §Curation Properties: written by curators only.
pub const CURATION_PROPERTIES: &[&str] = &["status", "pinned", "curationReason"];

/// The properties retraction clears, with the value it gives each:
/// "the server sets its `blobId`, `mediaType`, `uri`, `basis`,
/// `confidence`, `validUntil`, `validFor`, and `credentialDigest` to
/// `null`, and its `annotations` to `{}`, in the same update."
pub fn retraction_cleared() -> [(&'static str, Value); 9] {
    [
        ("blobId", Value::Null),
        ("mediaType", Value::Null),
        ("uri", Value::Null),
        ("basis", Value::Null),
        ("confidence", Value::Null),
        ("validUntil", Value::Null),
        ("validFor", Value::Null),
        ("credentialDigest", Value::Null),
        ("annotations", json!({})),
    ]
}

/// Server-set properties an update handler adds to a patch; they never
/// count as a change the caller made.
const SERVER_SET_ON_UPDATE: &[&str] = &["updated", "updatedBy"];

fn invalid(properties: &[&str], description: &str) -> Box<SetError> {
    Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties(properties.iter().copied())
            .with_description(description),
    )
}

fn forbidden(description: &str) -> Box<SetError> {
    Box::new(SetError::new(SetErrorType::Forbidden).with_description(description))
}

/// The record's `status`, absent as `active`; `None` when it is not a
/// status string.
fn status_of(m: &Map<String, Value>) -> Option<RecordStatus> {
    match m.get("status") {
        None => Some(RecordStatus::Active),
        Some(v) => RecordStatus::deserialize_opt(v),
    }
}

trait DeserializeOpt: Sized {
    fn deserialize_opt(v: &Value) -> Option<Self>;
}

impl<T: serde::de::DeserializeOwned> DeserializeOpt for T {
    fn deserialize_opt(v: &Value) -> Option<Self> {
        serde_json::from_value(v.clone()).ok()
    }
}

/// Whether `m` is a retracted record.
pub fn is_retracted(m: &Map<String, Value>) -> bool {
    status_of(m) == Some(RecordStatus::Retracted)
}

/// The value of curation property `key` in `m`, with its default.
fn curation_value(m: &Map<String, Value>, key: &str) -> Value {
    match (key, m.get(key)) {
        (_, Some(v)) => v.clone(),
        ("status", None) => json!("active"),
        ("pinned", None) => json!(false),
        _ => Value::Null,
    }
}

/// Whether `credentialDigest` is "43 base64url characters" (RFC 4648 §5,
/// no padding) that encode a 32-octet SHA-256 digest: 43 characters carry
/// 258 bits, so the last one holds the digest's final 4 bits and 2 zero
/// bits, and is one of the 16 characters whose value is a multiple of 4.
/// Stricter than the draft's wording, which no correct encoder violates
/// (decision 41).
fn is_digest(s: &str) -> bool {
    s.len() == 43
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && s.bytes()
            .last()
            .is_some_and(|b| b"AEIMQUYcgkosw048".contains(&b))
}

/// Whether [`PermittedUse`] `p` passes [`PermittedUse::is_well_formed`] and each
/// `purposes` entry is an RFC 3987 IRI ("Between 1 and 8 IRIs"). The IRI
/// check is stricter than the draft's listed rejections (decision 41).
fn is_permitted_use(p: &PermittedUse) -> bool {
    p.is_well_formed()
        && p.purposes
            .iter()
            .flatten()
            .all(|i| iri_string::types::IriStr::new(i.as_str()).is_ok())
}

/// The §Provenance Properties and §Curation Properties of `m`, the
/// `GraphNode` as it would be stored, against the draft's types and
/// values. Each failure is `invalidProperties` naming the property; all
/// failing properties are named together.
///
/// - `basis`: `null` or `declared`, `observed`, `verified`, `inferred`
///   ("The server rejects any other value").
/// - `confidence`: `null` or a Number "from 0 to 1 inclusive".
/// - `validUntil`: `null` or a [`UTCDate`].
/// - `validFor`: `null` or a [`PermittedUse`] with at least one member and
///   1 to 8 `purposes`, each an RFC 3987 IRI, when present.
/// - `credentialDigest`: `null` or "43 base64url characters" that can
///   encode a SHA-256 digest (see `is_digest`).
/// - `status`: `active`, `deprecated` or `retracted` (a String, not
///   `null`); `pinned`: a Boolean; `curationReason`: `null` or a String.
///
/// # Errors
///
/// `invalidProperties` naming each failing property.
pub fn check_record(m: &Map<String, Value>) -> Result<(), Box<SetError>> {
    let mut bad: Vec<&str> = Vec::new();
    let present = |k: &str| m.get(k).filter(|v| !v.is_null());
    if let Some(v) = present("basis")
        && !matches!(
            ProvenanceBasis::deserialize_opt(v),
            Some(
                ProvenanceBasis::Declared
                    | ProvenanceBasis::Observed
                    | ProvenanceBasis::Verified
                    | ProvenanceBasis::Inferred
            )
        )
    {
        bad.push("basis");
    }
    if let Some(v) = present("confidence")
        && !v.as_f64().is_some_and(|c| (0.0..=1.0).contains(&c))
    {
        bad.push("confidence");
    }
    if let Some(v) = present("validUntil")
        && !v
            .as_str()
            .is_some_and(|s| UTCDate::new_validated(s).is_ok())
    {
        bad.push("validUntil");
    }
    if let Some(v) = present("validFor")
        && !PermittedUse::deserialize_opt(v).is_some_and(|p| is_permitted_use(&p))
    {
        bad.push("validFor");
    }
    if let Some(v) = present("credentialDigest")
        && !v.as_str().is_some_and(is_digest)
    {
        bad.push("credentialDigest");
    }
    if !matches!(
        status_of(m),
        Some(RecordStatus::Active | RecordStatus::Deprecated | RecordStatus::Retracted)
    ) {
        bad.push("status");
    }
    if m.get("pinned").is_some_and(|v| !v.is_boolean()) {
        bad.push("pinned");
    }
    if m.get("curationReason")
        .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        bad.push("curationReason");
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(invalid(
            &bad,
            "not a valid value (draft-atwood-jmap-graph-provenance-00 §Provenance Properties, §Curation Properties)",
        ))
    }
}

/// The create rules for record `m`, after [`check_record`]:
///
/// - "the server rejects a create with `status` `retracted` with an
///   `invalidProperties` [`SetError`] naming `status`" (§Retraction);
/// - "On create, a user without `mayCurate` may set `status` only to
///   `active`, `pinned` only to `false`, and `curationReason` only to
///   `null`; the server rejects other values with a `forbidden` `SetError`"
///   (§Sharing Rights).
///
/// # Errors
///
/// `invalidProperties` naming `status`, or `forbidden`.
pub fn check_record_create(m: &Map<String, Value>, may_curate: bool) -> Result<(), Box<SetError>> {
    if is_retracted(m) {
        return Err(invalid(&["status"], "a record cannot be created retracted"));
    }
    if !may_curate
        && CURATION_PROPERTIES
            .iter()
            .any(|k| curation_value(m, k) != curation_value(&Map::new(), k))
    {
        return Err(forbidden(
            "setting status, pinned or curationReason on create requires mayCurate",
        ));
    }
    Ok(())
}

/// The update rules for record `current` becoming `next` through a patch
/// whose top-level properties are `keys`, by a caller with `rights` on the
/// record's Graph:
///
/// 1. "The server rejects any update to a retracted record other than one
///    to `pinned` or `curationReason` ... with an `invalidProperties`
///    [`SetError`] naming the property" (so `status` never leaves
///    `retracted`).
/// 2. "An update that makes `status` `retracted` and also sets any of
///    those properties to a value other than the one listed is rejected
///    with an `invalidProperties` `SetError` naming that property."
/// 3. "The server rejects, with a `forbidden` `SetError`, a `GraphNode/set`
///    update by a user without `mayCurate` that changes `status`,
///    `pinned`, or `curationReason`. An update that changes only those
///    properties requires `mayCurate` and does not require `mayWrite`";
///    any other property needs `mayWrite`, as in graph-00.
///
/// A curation property counts as changed when its value differs from the
/// stored one (with the defaults `active`, `false` and `null`). A patch of
/// curation properties only that changes none needs either right.
///
/// # Errors
///
/// `invalidProperties` naming the properties, or `forbidden`.
pub fn check_record_update(
    current: &Map<String, Value>,
    next: &Map<String, Value>,
    keys: &[String],
    rights: &GraphRights,
) -> Result<(), Box<SetError>> {
    let written: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|k| !SERVER_SET_ON_UPDATE.contains(k))
        .collect();
    if is_retracted(current) {
        let locked: Vec<&str> = written
            .iter()
            .copied()
            .filter(|k| !["pinned", "curationReason"].contains(k))
            .collect();
        if !locked.is_empty() {
            return Err(invalid(
                &locked,
                "a retracted record accepts changes to pinned and curationReason only",
            ));
        }
    } else if is_retracted(next) {
        let conflicting: Vec<&str> = retraction_cleared()
            .into_iter()
            .filter(|(k, cleared)| {
                written.contains(k) && next.get(*k).unwrap_or(&Value::Null) != cleared
            })
            .map(|(k, _)| k)
            .collect();
        if !conflicting.is_empty() {
            return Err(invalid(
                &conflicting,
                "retraction clears this property; it cannot be set in the same update",
            ));
        }
    }
    let may_curate = rights.may_curate().unwrap_or(false);
    let curation_changed = CURATION_PROPERTIES
        .iter()
        .any(|k| curation_value(current, k) != curation_value(next, k));
    // The values a retraction clears are the server's, not the caller's
    // writes, so a curator without mayWrite can retract.
    let retracting = !is_retracted(current) && is_retracted(next);
    let cleared = retraction_cleared();
    let is_clearing = |k: &str| {
        retracting
            && cleared
                .iter()
                .any(|(c, v)| *c == k && next.get(k).unwrap_or(&Value::Null) == v)
    };
    let other = written
        .iter()
        .any(|k| !CURATION_PROPERTIES.contains(k) && !is_clearing(k));
    if curation_changed && !may_curate {
        return Err(forbidden(
            "changing status, pinned or curationReason requires mayCurate",
        ));
    }
    if other && !rights.may_write {
        return Err(forbidden("this requires mayWrite on the Graph"));
    }
    if !curation_changed && !other && !may_curate && !rights.may_write {
        return Err(forbidden(
            "this requires mayCurate or mayWrite on the Graph",
        ));
    }
    Ok(())
}

/// Add the clearing of a retraction to `patch` (top-level property →
/// value), when `next` is retracted and `current` is not: each property of
/// [`retraction_cleared`] is set to its cleared value, replacing any
/// pointer into it, which [`check_record_update`] has found equal.
pub fn retraction_patch(
    current: &Map<String, Value>,
    next: &Map<String, Value>,
    patch: &mut Map<String, Value>,
) {
    if is_retracted(current) || !is_retracted(next) {
        return;
    }
    for (k, cleared) in retraction_cleared() {
        let prefix = format!("{k}/");
        patch.retain(|p, _| !p.starts_with(&prefix));
        patch.insert(k.to_owned(), cleared);
    }
}

/// "Destroying a record requires `mayCurate`, in place of the `mayWrite`
/// that {{JMAP-GRAPH}} requires; the server rejects a destroy by any other
/// user with a `forbidden` [`SetError`]."
///
/// # Errors
///
/// `forbidden` without `mayCurate`.
pub fn require_curate(rights: &GraphRights) -> Result<(), Box<SetError>> {
    if rights.may_curate().unwrap_or(false) {
        Ok(())
    } else {
        Err(forbidden("destroying a record requires mayCurate"))
    }
}

/// Graph `retention` in `m`, the Graph as it would be stored: `null` or a
/// Retention whose values, "when not `null`, \[are\] between 1 and 36500.
/// The server rejects others with an `invalidProperties` [`SetError`] naming
/// `retention`."
///
/// # Errors
///
/// `invalidProperties` naming `retention`.
pub fn check_retention(m: &Map<String, Value>) -> Result<(), Box<SetError>> {
    match m.get("retention") {
        None | Some(Value::Null) => Ok(()),
        Some(v) if Retention::deserialize_opt(v).is_some_and(|r| r.is_well_formed()) => Ok(()),
        Some(_) => Err(invalid(
            &["retention"],
            "retention values are null or 1 to 36500 days",
        )),
    }
}

/// Whether the `mayCurate` of every [`GraphRights`] in `shareWith` is absent
/// or a Boolean.
pub fn share_with_rights_ok(m: &Map<String, Value>) -> bool {
    m.get("shareWith")
        .and_then(Value::as_object)
        .is_none_or(|s| {
            s.values()
                .all(|r| r.get("mayCurate").is_none_or(Value::is_boolean))
        })
}

/// Run the §Retention sweep for `account_id` at `now`: destroy each record
/// that the Graph's `retention` makes eligible, "as if by a `GraphNode/set`
/// destroy, which also destroys its edges", through
/// [`GraphBackend::destroy_object`](crate::GraphBackend::destroy_object),
/// so clients "learn of destroyed records through `GraphNode/changes`".
/// Returns how many records it destroyed.
///
/// Eligible: a record not retracted whose `updated` is `recordDays` or more
/// days before `now`, or a retracted one `tombstoneDays` or more days
/// before. "Pinned records are never eligible. Nor is a node that has an
/// edit log", which here is a node whose `editFormat` is not `null`
/// (draft-atwood-jmap-graph-edit-00). An account that does not advertise
/// the capability is skipped.
///
/// The draft says the server "SHOULD do so within one day": a deployment
/// runs this at least daily for each account, as it runs the chat expiry
/// sweep. `caller` reads every Graph of the account and destroys with the
/// backend's rights check, so pass its owner ("Destruction by retention
/// ... is the server's own and needs no right"; the owner has every
/// right). A record destroyed meanwhile (`notFound`) is skipped. A stored
/// `updated` that does not parse never counts as passed.
///
/// ponytail: loads every Graph and `GraphNode` of the account on each run.
///
/// # Errors
///
/// The first backend error other than `notFound`. Records destroyed
/// before it stay destroyed; the next run continues.
pub async fn sweep_retention<B: crate::GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &jmap_types::Id,
    now: std::time::SystemTime,
) -> Result<usize, jmap_server::BackendSetError<B::Error>> {
    use jmap_graph_types::{Graph, GraphNode};
    use jmap_server::BackendSetError;

    if !backend.has_capability(caller, account_id, JMAP_GRAPH_PROVENANCE_URI) {
        return Ok(0);
    }
    let now = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let (graphs, _) = backend
        .get_objects::<Graph>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    let retention: std::collections::HashMap<jmap_types::Id, Retention> = graphs
        .iter()
        .filter_map(|g| Some((g.id.clone(), g.retention().ok().flatten()?)))
        .collect();
    if retention.is_empty() {
        return Ok(0);
    }
    let (nodes, _) = backend
        .get_objects::<GraphNode>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    let mut destroyed = 0;
    for node in nodes {
        let Some(r) = retention.get(&node.graph_id) else {
            continue;
        };
        let retracted = node.status().is_ok_and(|s| s == RecordStatus::Retracted);
        let days = if retracted {
            r.tombstone_days
        } else {
            r.record_days
        };
        let has_edit_log = node
            .extra
            .get(jmap_graph_types::GRAPH_NODE_EDIT_FORMAT_PROPERTY)
            .is_some_and(|v| !v.is_null());
        let passed = days.is_some_and(|d| {
            node.updated.to_epoch_seconds().is_ok_and(|t| {
                i64::try_from(d.saturating_mul(86_400))
                    .ok()
                    .and_then(|o| t.checked_add(o))
                    .is_some_and(|due| now >= due)
            })
        });
        if !passed || has_edit_log || node.pinned().unwrap_or(false) {
            continue;
        }
        match backend
            .destroy_object::<GraphNode>(caller, account_id, &node.id)
            .await
        {
            Ok(()) => destroyed += 1,
            Err(BackendSetError::SetError(e)) if e.error_type == SetErrorType::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(destroyed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().cloned().expect("object")
    }

    fn rights(write: bool, curate: bool) -> GraphRights {
        let mut r = GraphRights::default();
        r.may_read = true;
        r.may_write = write;
        r.extra.insert("mayCurate".into(), json!(curate));
        r
    }

    /// §Provenance Properties example: the SHA-256 digest of a credential,
    /// 43 base64url characters.
    #[test]
    fn digest_from_the_example_is_well_formed() {
        assert!(is_digest("htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc_ds"));
        assert!(!is_digest("htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc_d")); // 42
        assert!(!is_digest("htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc_d=")); // padding
        assert!(!is_digest("htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc+ds")); // base64, not base64url
        // 43 base64url characters, but the last one has nonzero padding
        // bits, so no SHA-256 digest encodes to it.
        assert!(!is_digest("htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc_dt"));
    }

    #[test]
    fn curation_only_update_needs_may_curate_not_may_write() {
        let current = map(json!({ "id": "n" }));
        let next = map(json!({ "id": "n", "status": "deprecated" }));
        let keys = vec!["status".to_owned(), "updated".to_owned()];
        assert!(check_record_update(&current, &next, &keys, &rights(false, true)).is_ok());
        let err = check_record_update(&current, &next, &keys, &rights(true, false)).unwrap_err();
        assert_eq!(err.error_type, SetErrorType::Forbidden);
    }

    #[test]
    fn retraction_patch_replaces_pointers_into_cleared_properties() {
        let current = map(json!({ "id": "n", "annotations": { "a": 1 } }));
        let next = map(json!({ "id": "n", "status": "retracted", "annotations": {} }));
        let mut patch = map(json!({ "status": "retracted", "annotations/a": null }));
        retraction_patch(&current, &next, &mut patch);
        assert_eq!(patch["annotations"], json!({}));
        assert!(!patch.contains_key("annotations/a"));
        assert_eq!(patch["blobId"], Value::Null);
    }
}
