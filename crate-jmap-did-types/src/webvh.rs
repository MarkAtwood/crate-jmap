//! `did:webvh` DID log verification over supplied bytes
//! (draft-atwood-jmap-did-00 §Resolving `did:webvh`, L352 at
//! jmap-chat-spec 6bcd9d8: "A client or server resolves a `did:webvh` DID
//! as defined in {{DID-WEBVH}} [...]. This document adds no resolution
//! requirements.").
//!
//! [`verify_webvh_log`] processes a `did.jsonl` DID log, and optionally
//! its `did-witness.json` witness proofs file, by the Read (Resolve)
//! rules of the DIF did:webvh DID Method v1.0
//! (<https://identity.foundation/didwebvh/v1.0/>):
//!
//! - the DID and every entry's `state.id` match the method-specific
//!   identifier ABNF and its decoded-value rules, and every `state.id`
//!   carries the DID's SCID;
//! - the genesis entry hashes to the SCID, and each entry's `entryHash`
//!   chains it to its predecessor (JCS, SHA-256 multihash, base58btc);
//! - each entry is signed (`eddsa-jcs-2022`) by an active update key:
//!   the previous entry's `updateKeys`, or under pre-rotation the entry's
//!   own, each of whose hashes the previous entry's `nextKeyHashes`
//!   committed to;
//! - version numbers count up from 1, `versionTime` strictly increases
//!   and is at most five minutes ahead of the supplied time, the
//!   `parameters` are those v1.0 defines with valid values, and nothing
//!   follows a deactivation;
//! - a `state.id` changes only when the DID is portable, and the new
//!   document lists the previous DID in `alsoKnownAs`;
//! - every entry made while witnesses are active has a threshold of
//!   distinct witness approvals over its own or a later `versionId`.
//!
//! As the method allows, the first invalid entry and every later one are
//! marked invalid while the versions before it stay available, so that
//! objects accepted under an earlier version can still be checked
//! (draft-atwood-jmap-did-00 §Resolving `did:webvh`, L354 at 6bcd9d8).
//! The log does not prove it is the newest one: compare
//! [`WebvhLog::latest`] with a `versionId` seen before to detect a
//! rollback (L356).
//!
//! Fetching the log and witness file is the caller's job; this module
//! does no I/O.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, VerifyingKey};
use jmap_types::UTCDate;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::jcs::canonicalize;
use crate::key::{decode_base58btc, encode_base58btc, BASE58_ALPHABET};
use crate::{Did, DidKey, KeyType};

/// The only `method` value of did:webvh v1.0.
const METHOD_V1_0: &str = "did:webvh:1.0";

/// The only cryptosuite v1.0 permits, for entry and witness proofs.
const CRYPTOSUITE: &str = "eddsa-jcs-2022";

/// The placeholder for the SCID in the preliminary genesis entry.
const SCID_PLACEHOLDER: &str = "{SCID}";

/// Length of a base58btc SHA-256 multihash: the `scid` production is
/// `46(base58btc-char)`.
const SCID_LEN: usize = 46;

/// How far `versionTime` may run ahead of the resolver's clock: "Resolvers
/// SHOULD use a tolerance of no more than 5 minutes."
const MAX_FUTURE_SKEW: Duration = Duration::from_secs(5 * 60);

/// `ttl` default: "Defaults to `3600` (1 hour)".
const DEFAULT_TTL: u64 = 3600;

/// `ttl` upper bound: "with a range of 0 to 2^31".
const MAX_TTL: u64 = 1 << 31;

/// The `witness` parameter in effect: a `threshold` of the listed
/// `did:key` witnesses must approve each entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WebvhWitness {
    /// Approvals needed, between 1 and the number of witnesses.
    pub threshold: u64,
    /// The witnesses' `did:key` DIDs, each Ed25519 and listed once.
    pub witnesses: Vec<String>,
}

/// The did:webvh parameters in effect after a log entry, with v1.0
/// defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WebvhParameters {
    /// Multikeys authorized to sign updates.
    pub update_keys: Vec<String>,
    /// Pre-rotation commitments; non-empty means pre-rotation is active.
    pub next_key_hashes: Vec<String>,
    /// The witness configuration, `None` for `{}` (no witnesses).
    pub witness: Option<WebvhWitness>,
    /// Watcher URLs.
    pub watchers: Vec<String>,
    /// Whether the DID can move to another domain or path.
    pub portable: bool,
    /// Whether the DID is deactivated.
    pub deactivated: bool,
    /// Suggested cache lifetime, in seconds.
    pub ttl: u64,
}

/// One verified version of a `did:webvh` DID.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WebvhVersion {
    /// The entry's `versionId`, `<number>-<entryHash>`.
    pub version_id: String,
    /// The version number before the dash, counting from 1.
    pub version_number: u64,
    /// The entry's `versionTime`, as written.
    pub version_time: String,
    /// The DID document: the entry's `state`, as written.
    pub document: Value,
    /// The parameters in effect after this entry.
    pub parameters: WebvhParameters,
}

/// A verified `did:webvh` DID log: the valid versions, oldest first, and
/// the first invalid entry if there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WebvhLog {
    scid: String,
    earlier: Vec<WebvhVersion>,
    latest: WebvhVersion,
    invalid: Option<(usize, WebvhError)>,
}

impl WebvhLog {
    /// The DID's self-certifying identifier.
    pub fn scid(&self) -> &str {
        &self.scid
    }

    /// Every valid version, oldest first. The last is
    /// [`latest`](Self::latest).
    pub fn versions(&self) -> impl Iterator<Item = &WebvhVersion> {
        self.earlier.iter().chain(std::iter::once(&self.latest))
    }

    /// The valid version whose `versionId` is `version_id`, as the
    /// `versionId` DID URL query resolves it.
    pub fn version(&self, version_id: &str) -> Option<&WebvhVersion> {
        self.versions().find(|v| v.version_id == version_id)
    }

    /// The last valid version. Its `version_id` is the one to remember
    /// and compare against later resolutions to detect a rollback.
    pub fn latest(&self) -> &WebvhVersion {
        &self.latest
    }

    /// The position in the log of the first invalid entry, and why it is
    /// invalid. That entry and every later one are excluded from
    /// [`versions`](Self::versions).
    pub fn invalid_entry(&self) -> Option<(usize, &WebvhError)> {
        self.invalid.as_ref().map(|(i, e)| (*i, e))
    }

    /// Whether the last valid version deactivated the DID.
    pub fn is_deactivated(&self) -> bool {
        self.latest.parameters.deactivated
    }

    /// The version that resolving the DID without a version query
    /// returns: [`latest`](Self::latest), or `None` when a later entry is
    /// invalid ("If the DID or DID version being resolved is invalid,
    /// return an appropriate error code") or the DID is deactivated ("a
    /// resolver MUST NOT return the DIDDoc").
    pub fn current(&self) -> Option<&WebvhVersion> {
        (self.invalid.is_none() && !self.is_deactivated()).then_some(&self.latest)
    }
}

/// Why a `did:webvh` log, or one of its entries, does not verify.
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WebvhError {
    /// The DID, or an entry's `state.id`, is not a `did:webvh` DID per
    /// the method-specific identifier rules.
    InvalidDid,
    /// The log has no entries.
    EmptyLog,
    /// An entry is not a JSON object with string `versionId` and
    /// `versionTime`, object `parameters` and `state`, and a `proof`, or
    /// holds a number JCS cannot serialize.
    MalformedEntry,
    /// The `versionId` is not `<number>-<entryHash>` with the next
    /// version number.
    InvalidVersionId,
    /// The `versionTime` is not a UTC ISO 8601 time, is not later than
    /// the previous entry's, or is more than five minutes in the future.
    InvalidVersionTime,
    /// The `method` parameter is missing from the first entry or is not
    /// `did:webvh:1.0`.
    UnsupportedMethod,
    /// A parameter is not one v1.0 defines, has an invalid value, or
    /// appears where it is not allowed.
    InvalidParameter {
        /// The parameter's name.
        name: String,
    },
    /// The genesis entry does not hash to its `scid`, or the DID or a
    /// `state.id` carries a different SCID.
    ScidMismatch,
    /// The `entryHash` does not match the entry.
    EntryHashMismatch,
    /// The entry has no proof, or a proof is not an `eddsa-jcs-2022`
    /// `assertionMethod` proof by an active update key that verifies.
    InvalidProof,
    /// Pre-rotation is active and the entry omits `updateKeys` or
    /// `nextKeyHashes`, or has an update key the previous entry did not
    /// commit to.
    PreRotation,
    /// The `state.id` changed although the DID is not portable, or the
    /// new document does not list the previous DID in `alsoKnownAs`.
    IllegalMove,
    /// The entry follows one that deactivated the DID.
    AfterDeactivation,
    /// The entry needs witnessing and fewer than the threshold of
    /// witnesses approved it or a later entry.
    WitnessThreshold,
    /// The entry needs witnessing and the witness proofs file is not a
    /// JSON array of `{versionId, proof}` objects.
    MalformedWitnessFile,
    /// No valid entry's `state.id` is the DID being resolved.
    DidNotInLog,
}

impl fmt::Display for WebvhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDid => f.write_str("not a valid did:webvh DID"),
            Self::EmptyLog => f.write_str("did:webvh log is empty"),
            Self::MalformedEntry => f.write_str("did:webvh log entry is malformed"),
            Self::InvalidVersionId => f.write_str("did:webvh versionId is out of sequence"),
            Self::InvalidVersionTime => f.write_str("did:webvh versionTime is invalid"),
            Self::UnsupportedMethod => f.write_str("did:webvh method is not did:webvh:1.0"),
            Self::InvalidParameter { name } => {
                write!(f, "did:webvh parameter {name:?} is invalid")
            }
            Self::ScidMismatch => f.write_str("did:webvh SCID does not match"),
            Self::EntryHashMismatch => f.write_str("did:webvh entryHash does not match"),
            Self::InvalidProof => {
                f.write_str("did:webvh entry is not signed by an active update key")
            }
            Self::PreRotation => f.write_str("did:webvh entry breaks its pre-rotation commitment"),
            Self::IllegalMove => f.write_str("did:webvh DID moved without portability"),
            Self::AfterDeactivation => f.write_str("did:webvh entry follows a deactivation"),
            Self::WitnessThreshold => {
                f.write_str("did:webvh entry lacks a threshold of witness approvals")
            }
            Self::MalformedWitnessFile => f.write_str("did:webvh witness proofs file is malformed"),
            Self::DidNotInLog => {
                f.write_str("no valid did:webvh log entry is for the DID being resolved")
            }
        }
    }
}

impl std::error::Error for WebvhError {}

/// Verifies a `did:webvh` DID log for `did`.
///
/// `log` is the `did.jsonl` content, one entry per line. `witness_file`
/// is the `did-witness.json` content, or `None` when none was retrieved;
/// it is read only when an entry needs witnessing. `now` is the
/// resolver's current time, against which `versionTime` is checked.
///
/// The parallel `did:web` document a controller may publish is outside
/// the log and is not consulted.
///
/// # Errors
///
/// Returns a [`WebvhError`] when `did` is not a `did:webvh` DID, the log
/// is empty, the first entry is invalid, or no valid entry's `state.id`
/// is `did`. An invalid later entry does not fail the call; see
/// [`WebvhLog::invalid_entry`].
pub fn verify_webvh_log(
    did: &Did,
    log: &[u8],
    witness_file: Option<&[u8]>,
    now: SystemTime,
) -> Result<WebvhLog, WebvhError> {
    let did_scid = webvh_scid(did.as_str()).ok_or(WebvhError::InvalidDid)?;
    let lines: Vec<&[u8]> = log
        .split(|&b| b == b'\n')
        .filter(|line| !line.trim_ascii().is_empty())
        .collect();
    if lines.is_empty() {
        return Err(WebvhError::EmptyLog);
    }

    let mut walk = Walk::default();
    let mut verified: Vec<(WebvhVersion, Option<WebvhWitness>)> = Vec::new();
    let mut invalid = None;
    for (index, line) in lines.iter().enumerate() {
        match walk.step(index, line, did_scid, now) {
            Ok(step) => verified.push(step),
            Err(error) => {
                invalid = Some((index, error));
                break;
            }
        }
    }

    if let Some(index) = first_unwitnessed(&verified, witness_file) {
        let error = if witness_file.is_some_and(|f| parse_witness_file(f).is_none()) {
            WebvhError::MalformedWitnessFile
        } else {
            WebvhError::WitnessThreshold
        };
        verified.truncate(index);
        invalid = Some((index, error));
    }

    let mut versions: Vec<WebvhVersion> = verified.into_iter().map(|(v, _)| v).collect();
    let Some(latest) = versions.pop() else {
        return Err(invalid.map_or(WebvhError::EmptyLog, |(_, e)| e));
    };
    let is_did =
        |v: &WebvhVersion| v.document.get("id").and_then(Value::as_str) == Some(did.as_str());
    if !versions.iter().chain(std::iter::once(&latest)).any(is_did) {
        return Err(WebvhError::DidNotInLog);
    }
    Ok(WebvhLog {
        scid: walk.scid,
        earlier: versions,
        latest,
        invalid,
    })
}

/// State carried from one log entry to the next.
#[derive(Default)]
struct Walk {
    scid: String,
    version_id: String,
    version_time: Option<(i64, u32)>,
    state_id: String,
    parameters: Option<WebvhParameters>,
}

impl Walk {
    /// Verifies entry `index` and advances past it, returning the version
    /// and the witness configuration that must approve it.
    fn step(
        &mut self,
        index: usize,
        line: &[u8],
        did_scid: &str,
        now: SystemTime,
    ) -> Result<(WebvhVersion, Option<WebvhWitness>), WebvhError> {
        let Ok(Value::Object(mut entry)) = serde_json::from_slice::<Value>(line) else {
            return Err(WebvhError::MalformedEntry);
        };
        let proof = entry.remove("proof").ok_or(WebvhError::MalformedEntry)?;
        let version_id = string_field(&entry, "versionId")?;
        let version_time = string_field(&entry, "versionTime")?;
        let (Some(Value::Object(params)), Some(Value::Object(state))) =
            (entry.get("parameters"), entry.get("state"))
        else {
            return Err(WebvhError::MalformedEntry);
        };

        let previous = self.parameters.as_ref();
        if previous.is_some_and(|p| p.deactivated) {
            return Err(WebvhError::AfterDeactivation);
        }

        let (number, entry_hash) = version_id
            .split_once('-')
            .filter(|(n, h)| {
                !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !h.contains('-')
            })
            .ok_or(WebvhError::InvalidVersionId)?;
        let version_number = number
            .parse::<u64>()
            .ok()
            .filter(|&n| usize::try_from(n).is_ok_and(|n| n == index + 1))
            .ok_or(WebvhError::InvalidVersionId)?;

        let time = parse_version_time(version_time).ok_or(WebvhError::InvalidVersionTime)?;
        if self.version_time.is_some_and(|prev| time <= prev) || time > latest_allowed(now) {
            return Err(WebvhError::InvalidVersionTime);
        }

        let update = Update::parse(params, previous)?;
        let next = update.parameters;

        let committed = previous
            .map(|p| &p.next_key_hashes)
            .filter(|hashes| !hashes.is_empty());
        let pre_rotation = committed.is_some();
        if let Some(committed) = committed {
            if !update.sets_update_keys
                || !update.sets_next_key_hashes
                || !next
                    .update_keys
                    .iter()
                    .all(|k| committed.contains(&sha256_multihash(k.as_bytes())))
            {
                return Err(WebvhError::PreRotation);
            }
        }

        let state_id = state
            .get("id")
            .and_then(Value::as_str)
            .ok_or(WebvhError::InvalidDid)?;
        let state_scid = webvh_scid(state_id).ok_or(WebvhError::InvalidDid)?;

        if previous.is_none() {
            let scid = params
                .get("scid")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_parameter("scid"))?;
            if scid != did_scid
                || state_scid != scid
                || genesis_scid(&entry, scid) != Some(scid.to_owned())
            {
                return Err(WebvhError::ScidMismatch);
            }
            self.scid = scid.to_owned();
        } else {
            if state_scid != self.scid {
                return Err(WebvhError::ScidMismatch);
            }
            if state_id != self.state_id
                && (!next.portable || !also_known_as(state, &self.state_id))
            {
                return Err(WebvhError::IllegalMove);
            }
        }

        let predecessor = if previous.is_none() {
            &self.scid
        } else {
            &self.version_id
        };
        let mut hashed = entry.clone();
        hashed.insert("versionId".to_owned(), Value::String(predecessor.clone()));
        let digest = canonicalize(&Value::Object(hashed)).ok_or(WebvhError::MalformedEntry)?;
        if sha256_multihash(digest.as_bytes()) != entry_hash {
            return Err(WebvhError::EntryHashMismatch);
        }

        let active_keys = match previous {
            Some(p) if !pre_rotation => &p.update_keys,
            _ => &next.update_keys,
        };
        let document =
            canonicalize(&Value::Object(entry.clone())).ok_or(WebvhError::MalformedEntry)?;
        if !entry_proofs_verify(&proof, document.as_bytes(), active_keys) {
            return Err(WebvhError::InvalidProof);
        }

        let witness = match previous.and_then(|p| p.witness.clone()) {
            Some(active) => Some(active),
            None if update.sets_witness => next.witness.clone(),
            None => None,
        };

        let version = WebvhVersion {
            version_id: version_id.to_owned(),
            version_number,
            version_time: version_time.to_owned(),
            document: Value::Object(state.clone()),
            parameters: next.clone(),
        };
        self.version_id = version.version_id.clone();
        self.version_time = Some(time);
        self.state_id = state_id.to_owned();
        self.parameters = Some(next);
        Ok((version, witness))
    }
}

fn string_field<'a>(entry: &'a Map<String, Value>, name: &str) -> Result<&'a str, WebvhError> {
    entry
        .get(name)
        .and_then(Value::as_str)
        .ok_or(WebvhError::MalformedEntry)
}

fn invalid_parameter(name: &str) -> WebvhError {
    WebvhError::InvalidParameter {
        name: name.to_owned(),
    }
}

/// One entry's `parameters` applied to those in effect before it.
struct Update {
    parameters: WebvhParameters,
    sets_update_keys: bool,
    sets_next_key_hashes: bool,
    sets_witness: bool,
}

impl Update {
    /// Applies `params` over `previous` (`None` for the first entry),
    /// checking each value. A JSON `null` stands for the parameter's
    /// default: "resolver implementations SHOULD gracefully accept `null`
    /// and immediately convert the value to their equivalent default
    /// value".
    fn parse(
        params: &Map<String, Value>,
        previous: Option<&WebvhParameters>,
    ) -> Result<Self, WebvhError> {
        let first = previous.is_none();
        let mut next = previous.cloned().unwrap_or(WebvhParameters {
            update_keys: Vec::new(),
            next_key_hashes: Vec::new(),
            witness: None,
            watchers: Vec::new(),
            portable: false,
            deactivated: false,
            ttl: DEFAULT_TTL,
        });
        match params.get("method") {
            Some(Value::String(m)) if m == METHOD_V1_0 => {}
            None if !first => {}
            _ => return Err(WebvhError::UnsupportedMethod),
        }
        for (name, value) in params {
            let bad = || invalid_parameter(name);
            match name.as_str() {
                "method" => {}
                "scid" if first && value.is_string() => {}
                "updateKeys" => next.update_keys = strings(value).ok_or_else(bad)?,
                "nextKeyHashes" => next.next_key_hashes = strings(value).ok_or_else(bad)?,
                "watchers" => next.watchers = strings(value).ok_or_else(bad)?,
                "witness" => next.witness = witness(value).ok_or_else(bad)?,
                "portable" => match value {
                    Value::Bool(true) if first => next.portable = true,
                    Value::Bool(false) => next.portable = false,
                    _ => return Err(bad()),
                },
                "deactivated" => next.deactivated = value.as_bool().ok_or_else(bad)?,
                "ttl" => {
                    next.ttl = match value {
                        Value::Null => DEFAULT_TTL,
                        v => v.as_u64().filter(|&t| t <= MAX_TTL).ok_or_else(bad)?,
                    }
                }
                _ => return Err(bad()),
            }
        }
        if first && !params.contains_key("scid") {
            return Err(invalid_parameter("scid"));
        }
        if first && !params.contains_key("updateKeys") {
            return Err(invalid_parameter("updateKeys"));
        }
        Ok(Self {
            parameters: next,
            sets_update_keys: params.contains_key("updateKeys"),
            sets_next_key_hashes: params.contains_key("nextKeyHashes"),
            sets_witness: params.contains_key("witness"),
        })
    }
}

/// A JSON array of strings, or `null` for the empty default.
fn strings(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Null => Some(Vec::new()),
        Value::Array(items) => items
            .iter()
            .map(|i| i.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    }
}

/// The `witness` parameter: `{}` or `null` for none, otherwise a
/// `threshold` from 1 to the number of distinct `witnesses`, each an
/// Ed25519 `did:key` listed once. The `did:key` spelling is ASCII, so the
/// byte comparison is the NFC comparison the method asks for.
fn witness(value: &Value) -> Option<Option<WebvhWitness>> {
    let object = match value {
        Value::Null => return Some(None),
        Value::Object(o) if o.is_empty() => return Some(None),
        Value::Object(o) => o,
        _ => return None,
    };
    let ids: Vec<String> = object
        .get("witnesses")?
        .as_array()?
        .iter()
        .map(|w| w.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect::<Option<_>>()?;
    let distinct: BTreeSet<&String> = ids.iter().collect();
    let threshold = object.get("threshold")?.as_u64()?;
    let valid = !ids.is_empty()
        && distinct.len() == ids.len()
        && (1..=ids.len() as u64).contains(&threshold)
        && ids
            .iter()
            .all(|id| id.strip_prefix("did:key:").and_then(ed25519_key).is_some());
    valid.then_some(Some(WebvhWitness {
        threshold,
        witnesses: ids,
    }))
}

fn also_known_as(state: &Map<String, Value>, did: &str) -> bool {
    state
        .get("alsoKnownAs")
        .and_then(Value::as_array)
        .is_some_and(|aka| aka.iter().any(|a| a.as_str() == Some(did)))
}

/// The SCID the genesis `entry` (without its proof) yields: its
/// `versionId` set to the placeholder and every occurrence of `scid`
/// replaced by it, then JCS, SHA-256 multihash and base58btc.
fn genesis_scid(entry: &Map<String, Value>, scid: &str) -> Option<String> {
    let mut preliminary = entry.clone();
    preliminary.insert(
        "versionId".to_owned(),
        Value::String(SCID_PLACEHOLDER.to_owned()),
    );
    let text = serde_json::to_string(&preliminary).ok()?;
    let replaced: Value = serde_json::from_str(&text.replace(scid, SCID_PLACEHOLDER)).ok()?;
    Some(sha256_multihash(canonicalize(&replaced)?.as_bytes()))
}

/// `base58btc(multihash(data, SHA-256))`, the only hash v1.0 permits.
fn sha256_multihash(data: &[u8]) -> String {
    let mut multihash = vec![0x12, 0x20];
    multihash.extend_from_slice(&Sha256::digest(data));
    encode_base58btc(&multihash)
}

/// The Ed25519 key a multikey (`z6Mk...`) encodes.
fn ed25519_key(multikey: &str) -> Option<VerifyingKey> {
    let did = Did::parse(&format!("did:key:{multikey}")).ok()?;
    let key = DidKey::from_did(&did).ok()?;
    if key.key_type() != KeyType::Ed25519 {
        return None;
    }
    VerifyingKey::from_bytes(key.public_key().try_into().ok()?).ok()
}

/// Verifies one `eddsa-jcs-2022` Data Integrity proof over the JCS form
/// of the unsecured document, returning the `did:key` multikey of its
/// `verificationMethod`. A fragment, when present (or when
/// `fragment_required`), must repeat that multikey.
fn verify_proof<'a>(
    proof: &'a Value,
    document_jcs: &[u8],
    fragment_required: bool,
) -> Option<&'a str> {
    let Value::Object(fields) = proof else {
        return None;
    };
    let field = |name| fields.get(name).and_then(Value::as_str);
    if field("type") != Some("DataIntegrityProof")
        || field("cryptosuite") != Some(CRYPTOSUITE)
        || field("proofPurpose") != Some("assertionMethod")
    {
        return None;
    }
    let method = field("verificationMethod")?.strip_prefix("did:key:")?;
    let multikey = match method.split_once('#') {
        Some((body, fragment)) if body == fragment => body,
        None if !fragment_required => method,
        _ => return None,
    };
    let key = ed25519_key(multikey)?;
    let signature = Signature::from_slice(&decode_base58btc(field("proofValue")?)?).ok()?;
    let mut config = fields.clone();
    config.remove("proofValue");
    let mut hash_data = Sha256::digest(canonicalize(&Value::Object(config))?.as_bytes()).to_vec();
    hash_data.extend_from_slice(&Sha256::digest(document_jcs));
    key.verify_strict(&hash_data, &signature).ok()?;
    Some(multikey)
}

/// Whether `proof` (one proof or a non-empty array) verifies throughout,
/// each by a key in `active_keys`.
fn entry_proofs_verify(proof: &Value, document_jcs: &[u8], active_keys: &[String]) -> bool {
    let proofs = match proof {
        Value::Array(proofs) => proofs.as_slice(),
        single => std::slice::from_ref(single),
    };
    !proofs.is_empty()
        && proofs.iter().all(|p| {
            verify_proof(p, document_jcs, false)
                .is_some_and(|key| active_keys.iter().any(|k| k == key))
        })
}

/// The `did-witness.json` entries: `versionId` to its proofs.
fn parse_witness_file(file: &[u8]) -> Option<Vec<(String, Vec<Value>)>> {
    let Value::Array(entries) = serde_json::from_slice(file).ok()? else {
        return None;
    };
    entries
        .into_iter()
        .map(|e| {
            let version_id = e.get("versionId")?.as_str()?.to_owned();
            let proofs = e.get("proof")?.as_array()?.clone();
            Some((version_id, proofs))
        })
        .collect()
}

/// The position of the first entry that needs witnessing and lacks a
/// threshold of approvals.
///
/// A witness approves an entry with a valid proof over that entry's
/// `versionId` or a later one in `verified`; proofs for other
/// `versionId`s are ignored, and each witness counts once.
fn first_unwitnessed(
    verified: &[(WebvhVersion, Option<WebvhWitness>)],
    witness_file: Option<&[u8]>,
) -> Option<usize> {
    let first_required = verified.iter().position(|(_, w)| w.is_some())?;
    let file = witness_file
        .and_then(parse_witness_file)
        .unwrap_or_default();
    let positions: BTreeMap<&str, usize> = verified
        .iter()
        .enumerate()
        .map(|(i, (v, _))| (v.version_id.as_str(), i))
        .collect();
    let approvals: Vec<(usize, String)> = file
        .iter()
        .filter_map(|(version_id, proofs)| {
            Some((*positions.get(version_id.as_str())?, version_id, proofs))
        })
        .flat_map(|(position, version_id, proofs)| {
            let data = canonicalize(&serde_json::json!({ "versionId": version_id }));
            proofs.iter().filter_map(move |p| {
                let witness = verify_proof(p, data.as_ref()?.as_bytes(), true)?;
                Some((position, format!("did:key:{witness}")))
            })
        })
        .collect();
    verified
        .iter()
        .enumerate()
        .skip(first_required)
        .find_map(|(index, (_, required))| {
            let required = required.as_ref()?;
            let approving: BTreeSet<&str> = approvals
                .iter()
                .filter(|(position, id)| *position >= index && required.witnesses.contains(id))
                .map(|(_, id)| id.as_str())
                .collect();
            (approving.len() < usize::try_from(required.threshold).unwrap_or(usize::MAX))
                .then_some(index)
        })
}

/// `now` plus the future tolerance, as `(seconds, nanoseconds)` since the
/// Unix epoch.
fn latest_allowed(now: SystemTime) -> (i64, u32) {
    let since_epoch = (now + MAX_FUTURE_SKEW)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (
        i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX),
        since_epoch.subsec_nanos(),
    )
}

/// A `versionTime` as `(seconds, nanoseconds)` since the Unix epoch: an
/// ISO 8601 `YYYY-MM-DDTHH:MM:SS[.fraction]` time with an explicit `Z`
/// or `+00:00` ("values without a zone designator, or expressing a
/// non-UTC zone, MUST be rejected").
fn parse_version_time(s: &str) -> Option<(i64, u32)> {
    let local = s.strip_suffix('Z').or_else(|| s.strip_suffix("+00:00"))?;
    let (seconds, fraction) = local.split_once('.').unwrap_or((local, ""));
    let nanos = if local.contains('.') {
        if fraction.is_empty()
            || fraction.len() > 9
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        format!("{fraction:0<9}").parse().ok()?
    } else {
        0
    };
    let seconds = UTCDate::new_validated(format!("{seconds}Z"))
        .ok()?
        .to_epoch_seconds()
        .ok()?;
    Some((seconds, nanos))
}

/// The SCID of a `did:webvh` DID, or `None` when `did` does not match the
/// did:webvh method-specific identifier ABNF and its decoded-value rules
/// (DID Core §3.1 first, then
/// `scid ":" webvh-domain *( ":" webvh-path-segment )`).
fn webvh_scid(did: &str) -> Option<&str> {
    Did::parse(did).ok()?;
    let mut components = did.strip_prefix("did:webvh:")?.split(':');
    let scid = components.next()?;
    if scid.len() != SCID_LEN || !scid.bytes().all(|b| BASE58_ALPHABET.contains(&b)) {
        return None;
    }
    if !valid_domain(components.next()?) {
        return None;
    }
    components.all(valid_path_segment).then_some(scid)
}

/// The `webvh-domain` rules: `encoded-domain-name [ "%3A" port-number ]`
/// with at most one port separator and a port from 1 to 65535; then,
/// decoded once and IDNA-processed, a fully qualified domain name within
/// the DNS length limits that is not an IP address.
fn valid_domain(component: &str) -> bool {
    let lower = component.to_ascii_lowercase();
    let host = match lower
        .match_indices("%3a")
        .map(|(i, _)| i)
        .collect::<Vec<_>>()[..]
    {
        [] => component,
        [i] => {
            let port = &component[i + 3..];
            let valid_port = (1..=5).contains(&port.len())
                && port.bytes().all(|b| b.is_ascii_digit())
                && port.parse::<u32>().is_ok_and(|p| (1..=65535).contains(&p));
            if !valid_port {
                return false;
            }
            &component[..i]
        }
        _ => return false,
    };
    let labels_ok = host.split('.').count() >= 2
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'%')
        });
    if !labels_ok {
        return false;
    }
    let Some(decoded) = percent_decode(host).and_then(|d| String::from_utf8(d).ok()) else {
        return false;
    };
    if decoded.split('.').count() < 2 || decoded.split('.').any(str::is_empty) {
        return false;
    }
    let Ok(ascii) = idna::domain_to_ascii_strict(&decoded) else {
        return false;
    };
    !ends_in_number(&ascii)
}

/// The WHATWG URL "ends in a number" test: a host whose last label is
/// decimal digits or `0x` hex is parsed as an IPv4 address.
fn ends_in_number(host: &str) -> bool {
    let last = host.rsplit('.').next().unwrap_or(host);
    let hex = last.strip_prefix("0x").or_else(|| last.strip_prefix("0X"));
    (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()))
        || hex.is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The `webvh-path-segment` rules: decoded once, not `.` or `..`, no
/// `/`, `\` or NUL, and no leading or trailing whitespace. DID Core
/// parsing has already checked the `1*idchar` syntax.
fn valid_path_segment(segment: &str) -> bool {
    let Some(decoded) = percent_decode(segment) else {
        return false;
    };
    let text = String::from_utf8_lossy(&decoded);
    !text.is_empty()
        && text != "."
        && text != ".."
        && !text.contains(['/', '\\', '\0'])
        && !text.starts_with(char::is_whitespace)
        && !text.ends_with(char::is_whitespace)
}

/// Decodes every `%HH` escape once, or `None` on a malformed escape.
fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    Some(out)
}
