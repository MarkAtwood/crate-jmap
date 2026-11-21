//! draft-atwood-jmap-graph-provenance-00 — JMAP Graph Provenance, the
//! `urn:ietf:params:jmap:graph:provenance` capability.
//!
//! - [`JMAP_GRAPH_PROVENANCE_URI`] (§Capability; both capability objects are
//!   `{}` and are not modelled).
//! - The §Provenance Properties and §Curation Properties of a record, read
//!   from a [`GraphNode`]'s `extra` with [`GraphNode::basis`],
//!   [`GraphNode::confidence`], [`GraphNode::valid_until`],
//!   [`GraphNode::valid_for`], [`GraphNode::credential_digest`],
//!   [`GraphNode::status`], [`GraphNode::pinned`] and
//!   [`GraphNode::curation_reason`]; [`ProvenanceBasis`], [`RecordStatus`]
//!   and [`PermittedUse`].
//! - [`EVIDENCE_EDGE_TYPE`] (§Evidence).
//! - [`GraphRights::may_curate`] (§Sharing Rights).
//! - [`Graph::retention`] and [`Retention`] (§Graph Extensions).
//! - The §Query Filters are fields of
//!   [`GraphNodeFilterCondition`](crate::GraphNodeFilterCondition):
//!   `basis`, `status`, `pinned` and `valid_at`.
//! - The §Limits: [`MAX_EVIDENCE_EDGES`], [`MAX_PURPOSES`], and the
//!   [`RETENTION_DAYS`] range.
//!
//! Every provenance property is the author's claim: "The server stores and
//! filters it and never evaluates it" (§Design Principles). Validating the
//! values the draft constrains is a server check.

use std::ops::RangeInclusive;

use jmap_types::{UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::{Graph, GraphNode, GraphRights};

/// Capability URI for JMAP Graph Provenance
/// (draft-atwood-jmap-graph-provenance-00 §Capability). "This capability is
/// meaningful only when `urn:ietf:params:jmap:graph` is also advertised for
/// the account." Its session-level and account-level values are both `{}`.
pub const JMAP_GRAPH_PROVENANCE_URI: &str = "urn:ietf:params:jmap:graph:provenance";

/// The `GraphEdge` `type` of an evidence edge (§Evidence): "A record's
/// evidence is the set of `GraphEdge` objects whose `fromId` is the record
/// and whose `type` is `urn:ietf:params:jmap:graph:provenance#evidence`."
pub const EVIDENCE_EDGE_TYPE: &str = "urn:ietf:params:jmap:graph:provenance#evidence";

/// §Limits: "A record has at most 32 evidence edges."
pub const MAX_EVIDENCE_EDGES: usize = 32;

/// §Limits: "A [`PermittedUse`] has at most 8 `purposes`" (and at least 1).
pub const MAX_PURPOSES: usize = 8;

/// §Graph Extensions: each Retention value, "when not `null`, is between 1
/// and 36500".
pub const RETENTION_DAYS: RangeInclusive<u64> = 1..=36500;

/// Wire names of the [`GraphNode`] properties this document adds.
pub mod property {
    /// `basis` (§Provenance Properties).
    pub const BASIS: &str = "basis";
    /// `confidence` (§Provenance Properties).
    pub const CONFIDENCE: &str = "confidence";
    /// `validUntil` (§Provenance Properties).
    pub const VALID_UNTIL: &str = "validUntil";
    /// `validFor` (§Provenance Properties).
    pub const VALID_FOR: &str = "validFor";
    /// `credentialDigest` (§Provenance Properties).
    pub const CREDENTIAL_DIGEST: &str = "credentialDigest";
    /// `status` (§Curation Properties).
    pub const STATUS: &str = "status";
    /// `pinned` (§Curation Properties).
    pub const PINNED: &str = "pinned";
    /// `curationReason` (§Curation Properties).
    pub const CURATION_REASON: &str = "curationReason";
    /// The provenance properties, which "follow `mayWrite`" (§Sharing
    /// Rights).
    pub const PROVENANCE: &[&str] = &[BASIS, CONFIDENCE, VALID_UNTIL, VALID_FOR, CREDENTIAL_DIGEST];
    /// The curation properties, which need `mayCurate` (§Sharing Rights).
    pub const CURATION: &[&str] = &[STATUS, PINNED, CURATION_REASON];
    /// `mayCurate` on `GraphRights` (§Sharing Rights).
    pub const MAY_CURATE: &str = "mayCurate";
    /// `retention` on Graph (§Graph Extensions).
    pub const RETENTION: &str = "retention";
}

/// A record's `basis` (§Provenance Properties): "How the author came to the
/// record's content." The server rejects any value not listed here; `Other`
/// keeps it so a server can name it in the `invalidProperties` `SetError`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProvenanceBasis {
    /// `"declared"`: "the author's own statement, about themselves or as
    /// their position."
    Declared,
    /// `"observed"`: "something the author saw, measured, or was told
    /// directly."
    Observed,
    /// `"verified"`: "checked against evidence."
    Verified,
    /// `"inferred"`: "a conclusion the author reached, including the output
    /// of a model."
    Inferred,
    /// A value not defined by this version of the draft.
    Other(String),
}

impl_string_enum!(ProvenanceBasis, "a provenance basis string",
    "declared" => Declared,
    "observed" => Observed,
    "verified" => Verified,
    "inferred" => Inferred,
);

/// A record's `status` (§Curation Properties). Default `"active"`. The
/// server rejects any value not listed here; `Other` keeps it so a server
/// can name it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum RecordStatus {
    /// `"active"`: "the record stands."
    #[default]
    Active,
    /// `"deprecated"`: "a curator has demoted the record."
    Deprecated,
    /// `"retracted"`: "a curator has withdrawn the record."
    Retracted,
    /// A value not defined by this version of the draft.
    Other(String),
}

impl_string_enum!(RecordStatus, "a record status string",
    "active" => Active,
    "deprecated" => Deprecated,
    "retracted" => Retracted,
);

/// The `validFor` [`PermittedUse`] of a record (§Provenance Properties): "The
/// uses the author intends the record for", "at least one of which is
/// present". It "is not an access control and is not enforced by the
/// server."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermittedUse {
    /// "Between 1 and 8 IRIs (RFC 3987) naming purposes." Terms of the
    /// Data Privacy Vocabulary are recommended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purposes: Option<Vec<String>>,
    /// "Plain text describing the intended use, for people to read."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Vendor / site / private extension fields, kept across round-trip.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PermittedUse {
    /// A [`PermittedUse`] naming `purposes`.
    pub fn with_purposes(purposes: Vec<String>) -> Self {
        Self {
            purposes: Some(purposes),
            ..Self::default()
        }
    }

    /// A [`PermittedUse`] with only a `note`.
    pub fn with_note(note: impl Into<String>) -> Self {
        Self {
            note: Some(note.into()),
            ..Self::default()
        }
    }

    /// Whether the §Provenance Properties rules hold: at least one member,
    /// and `purposes`, when present, has 1 to [`MAX_PURPOSES`] entries.
    /// The server rejects a [`PermittedUse`] that fails this with
    /// `invalidProperties` naming `validFor`.
    pub fn is_well_formed(&self) -> bool {
        match &self.purposes {
            Some(p) => (1..=MAX_PURPOSES).contains(&p.len()),
            None => self.note.is_some(),
        }
    }
}

/// A Graph's `retention` (§Graph Extensions): "How long the server keeps
/// the Graph's records."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Retention {
    /// "Records that are not retracted are destroyed this many days after
    /// their `updated` time. `null` means never."
    #[serde(default)]
    pub record_days: Option<u64>,
    /// "Retracted records are destroyed this many days after their
    /// `updated` time. `null` means never."
    #[serde(default)]
    pub tombstone_days: Option<u64>,
    /// Vendor / site / private extension fields, kept across round-trip.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Retention {
    /// Whether every non-null value is in [`RETENTION_DAYS`]. "The server
    /// rejects others with an `invalidProperties` `SetError` naming
    /// `retention`."
    pub fn is_well_formed(&self) -> bool {
        [self.record_days, self.tombstone_days]
            .into_iter()
            .flatten()
            .all(|d| RETENTION_DAYS.contains(&d))
    }
}

/// Read `key` from `extra` as `T`, absent or `null` as `None`.
fn extension<T: serde::de::DeserializeOwned>(
    extra: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<T>, serde_json::Error> {
    match extra.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => T::deserialize(v).map(Some),
    }
}

impl GraphNode {
    /// The record's `basis` (§Provenance Properties), `None` when "the
    /// author stated no basis". Read from [`extra`](Self::extra).
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a string.
    pub fn basis(&self) -> Result<Option<ProvenanceBasis>, serde_json::Error> {
        extension(&self.extra, property::BASIS)
    }

    /// The record's `confidence` (§Provenance Properties): "The author's
    /// own estimate, from 0 to 1 inclusive". `None` "means no estimate; it
    /// does not mean 1."
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a number.
    pub fn confidence(&self) -> Result<Option<f64>, serde_json::Error> {
        extension(&self.extra, property::CONFIDENCE)
    }

    /// The record's `validUntil` (§Provenance Properties): "A time after
    /// which the author no longer stands by the record."
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a [`UTCDate`] string.
    pub fn valid_until(&self) -> Result<Option<UTCDate>, serde_json::Error> {
        extension(&self.extra, property::VALID_UNTIL)
    }

    /// Whether the record is expired at `now` (§Expiry): "A record whose
    /// `validUntil` is earlier than the current time is expired." Dates
    /// compare as instants ([`UTCDate::cmp_instant`]). A record with no, or
    /// an unreadable, `validUntil` is not expired.
    pub fn is_expired_at(&self, now: &UTCDate) -> bool {
        self.valid_until()
            .ok()
            .flatten()
            .and_then(|until| until.cmp_instant(now).ok())
            .is_some_and(std::cmp::Ordering::is_lt)
    }

    /// The record's `validFor` (§Provenance Properties).
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a [`PermittedUse`].
    pub fn valid_for(&self) -> Result<Option<PermittedUse>, serde_json::Error> {
        extension(&self.extra, property::VALID_FOR)
    }

    /// The record's `credentialDigest` (§Provenance Properties): the
    /// unpadded base64url SHA-256 of a credential's exact bytes, on a node
    /// that stands for a credential.
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a string.
    pub fn credential_digest(&self) -> Result<Option<String>, serde_json::Error> {
        extension(&self.extra, property::CREDENTIAL_DIGEST)
    }

    /// The record's `status` (§Curation Properties), default `"active"`.
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a string.
    pub fn status(&self) -> Result<RecordStatus, serde_json::Error> {
        extension(&self.extra, property::STATUS).map(Option::unwrap_or_default)
    }

    /// The record's `pinned` (§Curation Properties), default `false`.
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a boolean.
    pub fn pinned(&self) -> Result<bool, serde_json::Error> {
        extension(&self.extra, property::PINNED).map(Option::unwrap_or_default)
    }

    /// The record's `curationReason` (§Curation Properties): "Plain text,
    /// written with the most recent change of `status` or `pinned`, saying
    /// why."
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a string.
    pub fn curation_reason(&self) -> Result<Option<String>, serde_json::Error> {
        extension(&self.extra, property::CURATION_REASON)
    }
}

impl GraphRights {
    /// `mayCurate` (§Sharing Rights): "The user may change the `status`,
    /// `pinned`, and `curationReason` properties of any record in this
    /// Graph." "`mayCurate` is not implied by `mayWrite`"; absent is
    /// `false`. Read from [`extra`](Self::extra).
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a boolean.
    pub fn may_curate(&self) -> Result<bool, serde_json::Error> {
        extension(&self.extra, property::MAY_CURATE).map(Option::unwrap_or_default)
    }
}

impl Graph {
    /// The Graph's `retention` (§Graph Extensions); `None` "means records
    /// are kept until destroyed". Read from [`extra`](Self::extra).
    ///
    /// # Errors
    ///
    /// The deserialization error when the value is not a Retention object.
    pub fn retention(&self) -> Result<Option<Retention>, serde_json::Error> {
        extension(&self.extra, property::RETENTION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A stored node: the §Examples create entry `r1` (copied verbatim)
    /// with the server-set GraphNode properties of draft-atwood-jmap-graph-00
    /// added.
    fn r1() -> serde_json::Value {
        let mut v = json!({
          "graphId": "G1",
          "blobId": "Bk7q",
          "mediaType": "text/markdown",
          "basis": "inferred",
          "confidence": 0.6,
          "validUntil": "2027-01-31T00:00:00Z",
          "validFor": {
            "purposes": ["https://w3id.org/dpv#ServiceProvision"]
          }
        });
        let o = v.as_object_mut().unwrap();
        o.insert("id".into(), json!("N77"));
        o.insert("uri".into(), json!(null));
        o.insert("annotations".into(), json!({}));
        o.insert("created".into(), json!("2026-11-19T10:00:00Z"));
        o.insert("updated".into(), json!("2026-11-19T10:00:00Z"));
        o.insert("createdBy".into(), json!("P1"));
        o.insert("updatedBy".into(), json!("P1"));
        v
    }

    #[test]
    fn example_record_reads_its_provenance() {
        let raw = r1();
        let n: GraphNode = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(n.basis().unwrap(), Some(ProvenanceBasis::Inferred));
        assert_eq!(n.confidence().unwrap(), Some(0.6));
        assert_eq!(
            n.valid_for().unwrap(),
            Some(PermittedUse::with_purposes(vec![
                "https://w3id.org/dpv#ServiceProvision".into()
            ]))
        );
        assert_eq!(
            n.valid_until().unwrap().unwrap().as_ref(),
            "2027-01-31T00:00:00Z"
        );
        // §Curation Properties defaults: status "active", pinned false.
        assert_eq!(n.status().unwrap(), RecordStatus::Active);
        assert!(!n.pinned().unwrap());
        assert_eq!(n.curation_reason().unwrap(), None);
        assert_eq!(serde_json::to_value(&n).unwrap(), raw);
    }

    #[test]
    fn example_credential_node_reads_its_digest() {
        // §Examples create entry c1, verbatim, as a stored node.
        let mut raw = r1();
        let o = raw.as_object_mut().unwrap();
        for k in [
            "blobId",
            "mediaType",
            "basis",
            "confidence",
            "validUntil",
            "validFor",
        ] {
            o.remove(k);
        }
        o.insert(
            "uri".into(),
            json!("urn:uuid:4f1c9a3e-8d2b-4e7f-9a61-0c5d2b7e8f13"),
        );
        o.insert(
            "credentialDigest".into(),
            json!("htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc_ds"),
        );
        let n: GraphNode = serde_json::from_value(raw).unwrap();
        let d = n.credential_digest().unwrap().unwrap();
        // "43 base64url characters" (§Provenance Properties).
        assert_eq!(d.len(), 43);
        assert_eq!(n.basis().unwrap(), None);
    }

    #[test]
    fn curator_example_status_and_reason() {
        // §Examples: "A curator demotes it".
        let mut raw = r1();
        raw["status"] = json!("deprecated");
        raw["curationReason"] = json!("Superseded by the March inventory.");
        let n: GraphNode = serde_json::from_value(raw).unwrap();
        assert_eq!(n.status().unwrap(), RecordStatus::Deprecated);
        assert_eq!(
            n.curation_reason().unwrap().as_deref(),
            Some("Superseded by the March inventory.")
        );
    }

    #[test]
    fn enums_keep_unknown_values_for_the_error() {
        // The server "rejects any other value with an invalidProperties
        // SetError naming basis/status": the value must decode so it can be
        // named, and round-trip.
        let b: ProvenanceBasis = serde_json::from_value(json!("rumoured")).unwrap();
        assert_eq!(b, ProvenanceBasis::Other("rumoured".into()));
        let s: RecordStatus = serde_json::from_value(json!("archived")).unwrap();
        assert_eq!(serde_json::to_value(s).unwrap(), json!("archived"));
    }

    #[test]
    fn permitted_use_well_formedness() {
        // §Provenance Properties: "a PermittedUse with neither member, or
        // with a purposes array that is empty or has more than 8 entries" is
        // rejected.
        assert!(!PermittedUse::default().is_well_formed());
        assert!(!PermittedUse::with_purposes(vec![]).is_well_formed());
        assert!(!PermittedUse::with_purposes(vec!["x".into(); 9]).is_well_formed());
        assert!(PermittedUse::with_purposes(vec!["x".into(); 8]).is_well_formed());
        assert!(PermittedUse::with_note("for the audit").is_well_formed());
    }

    #[test]
    fn retention_range() {
        // §Graph Extensions: "Each value, when not null, is between 1 and
        // 36500."
        let r: Retention =
            serde_json::from_value(json!({"recordDays": 30, "tombstoneDays": null})).unwrap();
        assert!(r.is_well_formed());
        for bad in [0, 36501] {
            let r: Retention = serde_json::from_value(json!({"recordDays": bad})).unwrap();
            assert!(!r.is_well_formed(), "{bad}");
        }
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"recordDays": 30, "tombstoneDays": null})
        );
    }

    #[test]
    fn may_curate_is_not_implied_by_may_write() {
        let r: GraphRights =
            serde_json::from_value(json!({"mayRead": true, "mayWrite": true, "mayAdmin": false}))
                .unwrap();
        assert!(!r.may_curate().unwrap());
        let r: GraphRights = serde_json::from_value(
            json!({"mayRead": true, "mayWrite": false, "mayAdmin": false, "mayCurate": true}),
        )
        .unwrap();
        assert!(r.may_curate().unwrap());
    }

    #[test]
    fn expiry_compares_valid_until() {
        // §Expiry: "A record whose validUntil is earlier than the current
        // time is expired."
        let n: GraphNode = serde_json::from_value(r1()).unwrap();
        assert!(!n.is_expired_at(&UTCDate::from("2027-01-30T00:00:00Z")));
        assert!(n.is_expired_at(&UTCDate::from("2027-02-01T00:00:00Z")));
        // Instants, not strings: fractional seconds compare by value.
        assert!(!n.is_expired_at(&UTCDate::from("2027-01-30T23:59:59.999Z")));
    }

    #[test]
    fn evidence_type_and_limits_match_the_draft() {
        assert_eq!(
            EVIDENCE_EDGE_TYPE,
            "urn:ietf:params:jmap:graph:provenance#evidence"
        );
        assert_eq!(
            JMAP_GRAPH_PROVENANCE_URI,
            "urn:ietf:params:jmap:graph:provenance"
        );
        assert_eq!((MAX_EVIDENCE_EDGES, MAX_PURPOSES), (32, 8));
    }
}
