//! `did:plc` operation-log verification over supplied bytes
//! (draft-atwood-jmap-chat-did-00 §did:plc operation log integrity, L279
//! at jmap-chat-spec 6bcd9d8: "Resolution per {{methods}} includes the
//! operation-log integrity check defined by {{DID-PLC}}; a DID Document
//! obtained from a PLC directory that has presented an inconsistent or
//! non-monotonic operation log fails that check.").
//!
//! [`verify_plc_audit_log`] replays a directory's audit log
//! (`/:did/log/audit`) by the did:plc Method Specification v0.3.0 rules:
//!
//! - the genesis operation's DAG-CBOR SHA-256, base32-encoded and
//!   truncated to 24 characters, is the DID's identifier;
//! - every operation is signed (ECDSA-SHA256, low-S, 64-byte `r || s`,
//!   unpadded base64url) by a rotation key of the operation its `prev`
//!   CID names;
//! - an operation whose `prev` is not the latest operation nullifies the
//!   operations after its fork point, which is allowed only within 72
//!   hours of the first of them and only when it is signed by a rotation
//!   key at a lower index than that operation's signer;
//! - each entry's `cid` and `nullified` flag match the replay, and
//!   `createdAt` never decreases.
//!
//! Fetching the log is the caller's job; this module does no I/O.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_types::UTCDate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{Did, DidKey, KeyType};

/// "The PLC server provides a 72hr window during which a higher
/// authority rotation key can 'rewrite' history".
const RECOVERY_WINDOW_MS: i64 = 72 * 60 * 60 * 1000;

/// The CIDv1 prefix for `prev` references: version 1, `dag-cbor`
/// (`0x71`), `sha2-256` (`0x12`), digest length 32.
const CID_PREFIX: [u8; 4] = [0x01, 0x71, 0x12, 0x20];

/// RFC 4648 §6 base32 alphabet, lowercased.
const BASE32_LOWER: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// The verified state of a `did:plc`, in the shape of the directory's
/// `/:did/data` JSON object.
///
/// A legacy `create` genesis operation is converted to the regular
/// form: rotation keys `[recoveryKey, signingKey]`, the `atproto`
/// verification method `signingKey`, `alsoKnownAs` `["at://" + handle]`
/// and an `atproto_pds` service at `service` (with `https://` prepended
/// when it has no `http://` or `https://` scheme).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlcState {
    /// The DID.
    pub did: Did,
    /// Service id (without `#`) to signing key, in did:key form.
    pub verification_methods: BTreeMap<String, String>,
    /// Rotation keys in did:key form, most authoritative first.
    pub rotation_keys: Vec<String>,
    /// Other names for the DID, most preferred first.
    pub also_known_as: Vec<String>,
    /// Service id (without `#`) to service.
    pub services: BTreeMap<String, PlcService>,
}

impl PlcState {
    /// The DID document this state presents as, per did:plc Method
    /// Specification v0.3.0 §Presentation as DID Document, matching the
    /// directory's `/:did` output and its reference `formatDidDoc`:
    ///
    /// - `@context` is `https://www.w3.org/ns/did/v1` and
    ///   `https://w3id.org/security/multikey/v1`, then
    ///   `https://w3id.org/security/suites/ecdsa-2019/v1` for a P-256 key
    ///   and `https://w3id.org/security/suites/secp256k1-2019/v1` for a
    ///   secp256k1 key, each once, in verification-method order;
    /// - each verification method is a `Multikey` with `id` the DID,
    ///   `#` and the method's id (the spec example shows only `#atproto`;
    ///   the directory emits the full DID URL), `controller` the DID and
    ///   `publicKeyMultibase` the key without its `did:key:` prefix;
    /// - each service has `id` `#` and the service's id, and its endpoint
    ///   as `serviceEndpoint`.
    ///
    /// Verification methods and services come out in id order: the
    /// directory keeps operation order, which [`PlcState`] does not
    /// record. DID Core resolves both by `id`, so the order carries no
    /// meaning.
    pub fn document(&self) -> Value {
        let did = self.did.as_str();
        let mut context = vec![
            "https://www.w3.org/ns/did/v1",
            "https://w3id.org/security/multikey/v1",
        ];
        let mut methods = Vec::with_capacity(self.verification_methods.len());
        for (id, key) in &self.verification_methods {
            let suite = Did::parse(key)
                .ok()
                .and_then(|d| DidKey::from_did(&d).ok())
                .and_then(|k| match k.key_type() {
                    KeyType::P256 => Some("https://w3id.org/security/suites/ecdsa-2019/v1"),
                    KeyType::Secp256k1 => {
                        Some("https://w3id.org/security/suites/secp256k1-2019/v1")
                    }
                    _ => None,
                });
            if let Some(suite) = suite.filter(|s| !context.contains(s)) {
                context.push(suite);
            }
            methods.push(json!({
                "id": format!("{did}#{id}"),
                "type": "Multikey",
                "controller": did,
                "publicKeyMultibase": key.strip_prefix("did:key:").unwrap_or(key),
            }));
        }
        let services: Vec<Value> = self
            .services
            .iter()
            .map(|(id, service)| {
                json!({
                    "id": format!("#{id}"),
                    "type": service.service_type,
                    "serviceEndpoint": service.endpoint,
                })
            })
            .collect();
        json!({
            "@context": context,
            "id": did,
            "alsoKnownAs": self.also_known_as,
            "verificationMethod": methods,
            "service": services,
        })
    }
}

/// One entry of [`PlcState::services`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlcService {
    /// The service type, for example `AtprotoPersonalDataServer`.
    #[serde(rename = "type")]
    pub service_type: String,
    /// The service URL.
    pub endpoint: String,
}

/// The outcome of a verified audit log.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlcStatus {
    /// The latest valid operation is a regular or legacy operation.
    Active(PlcState),
    /// The latest valid operation is a `plc_tombstone`: the DID is
    /// permanently deactivated.
    Deactivated,
}

/// Error from [`verify_plc_audit_log`]. Each `index` is the position of
/// the offending entry in the audit log.
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[non_exhaustive]
#[derive(Debug)]
pub enum PlcError {
    /// The DID's method is not `plc`.
    NotPlc,
    /// The bytes are not a JSON array of audit-log entries.
    Json(serde_json::Error),
    /// The log has no entries.
    EmptyLog,
    /// An entry's `did` is not the DID being verified.
    WrongDid {
        /// The entry's position.
        index: usize,
    },
    /// An entry's `createdAt` is not `YYYY-MM-DDTHH:mm:ss.sssZ`.
    InvalidTimestamp {
        /// The entry's position.
        index: usize,
    },
    /// An entry's `createdAt` is earlier than the previous entry's.
    NonMonotonic {
        /// The entry's position.
        index: usize,
    },
    /// An operation is not a `plc_operation`, `plc_tombstone` or
    /// `create` object, lacks a string `sig`, or holds a floating-point
    /// number (which no PLC operation has).
    MalformedOperation {
        /// The entry's position.
        index: usize,
    },
    /// An entry's `cid` is not the CID of its operation.
    CidMismatch {
        /// The entry's position.
        index: usize,
    },
    /// A rotation key is not a P-256 or secp256k1 `did:key`.
    UnsupportedRotationKey {
        /// The entry's position.
        index: usize,
    },
    /// The first operation is a tombstone, has a non-null `prev`, or
    /// does not hash to the DID's identifier.
    InvalidGenesis,
    /// An operation's `prev` names no operation in the valid history,
    /// names a tombstone, is missing, or a legacy `create` operation
    /// appears after the genesis operation.
    Misordered {
        /// The entry's position.
        index: usize,
    },
    /// No permitted rotation key verifies the signature, or the
    /// signature is not canonical (padding, trailing bits, length or
    /// high-S).
    InvalidSignature {
        /// The entry's position.
        index: usize,
    },
    /// A recovery operation (one that nullifies operations) was created
    /// more than 72 hours after the first operation it nullifies.
    LateRecovery {
        /// The entry's position.
        index: usize,
    },
    /// An entry's `nullified` flag disagrees with the replay.
    NullifiedMismatch {
        /// The entry's position.
        index: usize,
    },
}

impl fmt::Display for PlcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPlc => f.write_str("DID method is not plc"),
            Self::Json(_) => f.write_str("PLC audit log is not a JSON array of entries"),
            Self::EmptyLog => f.write_str("PLC audit log is empty"),
            Self::WrongDid { index } => write!(f, "PLC audit log entry {index} is for another DID"),
            Self::InvalidTimestamp { index } => {
                write!(f, "PLC audit log entry {index} has a malformed createdAt")
            }
            Self::NonMonotonic { index } => {
                write!(
                    f,
                    "PLC audit log entry {index} is older than its predecessor"
                )
            }
            Self::MalformedOperation { index } => {
                write!(f, "PLC audit log entry {index} has a malformed operation")
            }
            Self::CidMismatch { index } => {
                write!(
                    f,
                    "PLC audit log entry {index} cid does not match its operation"
                )
            }
            Self::UnsupportedRotationKey { index } => write!(
                f,
                "PLC operation {index} has a rotation key that is not a P-256 or secp256k1 did:key"
            ),
            Self::InvalidGenesis => {
                f.write_str("PLC genesis operation is invalid or does not hash to the DID")
            }
            Self::Misordered { index } => {
                write!(f, "PLC operation {index} does not extend the valid history")
            }
            Self::InvalidSignature { index } => {
                write!(
                    f,
                    "PLC operation {index} is not signed by a permitted rotation key"
                )
            }
            Self::LateRecovery { index } => write!(
                f,
                "PLC operation {index} nullifies history outside the 72-hour recovery window"
            ),
            Self::NullifiedMismatch { index } => write!(
                f,
                "PLC audit log entry {index} nullified flag disagrees with the replay"
            ),
        }
    }
}

impl std::error::Error for PlcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditEntry {
    did: String,
    operation: Value,
    cid: String,
    nullified: bool,
    created_at: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Operation {
    #[serde(rename = "plc_operation", rename_all = "camelCase")]
    Regular {
        rotation_keys: Vec<String>,
        verification_methods: BTreeMap<String, String>,
        also_known_as: Vec<String>,
        services: BTreeMap<String, PlcService>,
        prev: Option<String>,
    },
    #[serde(rename = "plc_tombstone")]
    Tombstone { prev: String },
    #[serde(rename = "create", rename_all = "camelCase")]
    Create {
        signing_key: String,
        recovery_key: String,
        handle: String,
        service: String,
    },
}

enum RotationKey {
    K256(k256::ecdsa::VerifyingKey),
    P256(p256::ecdsa::VerifyingKey),
}

impl RotationKey {
    fn parse(did_key: &str) -> Option<Self> {
        let key = DidKey::from_did(&Did::parse(did_key).ok()?).ok()?;
        match key.key_type() {
            KeyType::Secp256k1 => k256::ecdsa::VerifyingKey::from_sec1_bytes(key.public_key())
                .ok()
                .map(Self::K256),
            KeyType::P256 => p256::ecdsa::VerifyingKey::from_sec1_bytes(key.public_key())
                .ok()
                .map(Self::P256),
            _ => None,
        }
    }

    /// Verifies a 64-byte `r || s` signature, rejecting high-S values
    /// ("non-canonical encodings or 'High-S' values rejected as
    /// invalid").
    fn verifies(&self, msg: &[u8], sig: &[u8]) -> bool {
        use k256::ecdsa::signature::Verifier as _;
        match self {
            Self::K256(key) => k256::ecdsa::Signature::from_slice(sig)
                .is_ok_and(|s| s.normalize_s().is_none() && key.verify(msg, &s).is_ok()),
            Self::P256(key) => p256::ecdsa::Signature::from_slice(sig)
                .is_ok_and(|s| s.normalize_s().is_none() && key.verify(msg, &s).is_ok()),
        }
    }
}

/// One operation in the valid history.
struct Link {
    index: usize,
    cid: String,
    created_at_ms: i64,
    /// Index of the signing key in the previous link's rotation keys.
    signer: usize,
    /// `None` for a tombstone.
    state: Option<(PlcState, Vec<RotationKey>)>,
}

/// Verifies a `did:plc` audit log and returns the DID's current state.
///
/// `audit_log` is the JSON body of the directory's `/:did/log/audit`
/// endpoint: an array of `{did, operation, cid, nullified, createdAt}`
/// entries in directory order, nullified entries included.
///
/// The operation-size, key-count and duplicate-key limits that the
/// directory enforces on submission are not re-checked: the directory
/// holds operations that predate some of them.
///
/// # Errors
///
/// Returns a [`PlcError`] naming the first entry that breaks a did:plc
/// rule; see the variants.
pub fn verify_plc_audit_log(did: &Did, audit_log: &[u8]) -> Result<PlcStatus, PlcError> {
    if did.method() != "plc" {
        return Err(PlcError::NotPlc);
    }
    let entries: Vec<AuditEntry> = serde_json::from_slice(audit_log).map_err(PlcError::Json)?;
    if entries.is_empty() {
        return Err(PlcError::EmptyLog);
    }
    let mut chain: Vec<Link> = Vec::new();
    let mut nullified = vec![false; entries.len()];
    let mut last_ms = i64::MIN;
    for (index, entry) in entries.iter().enumerate() {
        if entry.did != did.as_str() {
            return Err(PlcError::WrongDid { index });
        }
        let created_at_ms =
            parse_created_at(&entry.created_at).ok_or(PlcError::InvalidTimestamp { index })?;
        if created_at_ms < last_ms {
            return Err(PlcError::NonMonotonic { index });
        }
        last_ms = created_at_ms;

        let Value::Object(fields) = &entry.operation else {
            return Err(PlcError::MalformedOperation { index });
        };
        let signed = dag_cbor(&entry.operation).ok_or(PlcError::MalformedOperation { index })?;
        let cid = cid_string(&Sha256::digest(&signed));
        if cid != entry.cid {
            return Err(PlcError::CidMismatch { index });
        }
        let sig = fields
            .get("sig")
            .and_then(Value::as_str)
            .ok_or(PlcError::MalformedOperation { index })?;
        let mut unsigned_fields = fields.clone();
        unsigned_fields.remove("sig");
        let unsigned = dag_cbor(&Value::Object(unsigned_fields))
            .ok_or(PlcError::MalformedOperation { index })?;
        let operation = Operation::deserialize(&entry.operation)
            .map_err(|_| PlcError::MalformedOperation { index })?;
        let state = state_of(did, &operation)
            .map(|state| {
                let keys = state
                    .rotation_keys
                    .iter()
                    .map(|k| RotationKey::parse(k))
                    .collect::<Option<Vec<_>>>()
                    .ok_or(PlcError::UnsupportedRotationKey { index })?;
                Ok((state, keys))
            })
            .transpose()?;
        let signer_in = |keys: &[RotationKey]| {
            URL_SAFE_NO_PAD
                .decode(sig)
                .ok()
                .and_then(|sig| keys.iter().position(|k| k.verifies(&unsigned, &sig)))
                .ok_or(PlcError::InvalidSignature { index })
        };

        if chain.is_empty() {
            let (_, keys) = state.as_ref().ok_or(PlcError::InvalidGenesis)?;
            if fields.get("prev") != Some(&Value::Null)
                || !did_identifier_matches(did, &Sha256::digest(&signed))
            {
                return Err(PlcError::InvalidGenesis);
            }
            let signer = signer_in(keys)?;
            chain.push(Link {
                index,
                cid,
                created_at_ms,
                signer,
                state,
            });
            continue;
        }

        let prev = match &operation {
            Operation::Regular { prev: Some(p), .. } | Operation::Tombstone { prev: p } => p,
            _ => return Err(PlcError::Misordered { index }),
        };
        let fork = chain
            .iter()
            .position(|link| &link.cid == prev)
            .ok_or(PlcError::Misordered { index })?;
        let (_, keys) = chain[fork]
            .state
            .as_ref()
            .ok_or(PlcError::Misordered { index })?;
        let signer = match chain.get(fork + 1) {
            None => signer_in(keys)?,
            Some(first_nullified) => {
                let signer = signer_in(&keys[..first_nullified.signer])?;
                if created_at_ms - first_nullified.created_at_ms > RECOVERY_WINDOW_MS {
                    return Err(PlcError::LateRecovery { index });
                }
                signer
            }
        };
        for link in chain.drain(fork + 1..) {
            nullified[link.index] = true;
        }
        chain.push(Link {
            index,
            cid,
            created_at_ms,
            signer,
            state,
        });
    }
    if let Some(index) = (0..entries.len()).find(|&i| nullified[i] != entries[i].nullified) {
        return Err(PlcError::NullifiedMismatch { index });
    }
    Ok(match chain.pop().and_then(|link| link.state) {
        Some((state, _)) => PlcStatus::Active(state),
        None => PlcStatus::Deactivated,
    })
}

/// The DID state an operation sets, with legacy `create` operations
/// converted to the regular form; `None` for a tombstone.
fn state_of(did: &Did, operation: &Operation) -> Option<PlcState> {
    match operation {
        Operation::Regular {
            rotation_keys,
            verification_methods,
            also_known_as,
            services,
            ..
        } => Some(PlcState {
            did: did.clone(),
            verification_methods: verification_methods.clone(),
            rotation_keys: rotation_keys.clone(),
            also_known_as: also_known_as.clone(),
            services: services.clone(),
        }),
        Operation::Tombstone { .. } => None,
        Operation::Create {
            signing_key,
            recovery_key,
            handle,
            service,
        } => {
            let handle = handle
                .strip_prefix("at://")
                .or_else(|| handle.strip_prefix("https://"))
                .or_else(|| handle.strip_prefix("http://"))
                .unwrap_or(handle);
            let endpoint = if service.starts_with("https://") || service.starts_with("http://") {
                service.clone()
            } else {
                format!("https://{service}")
            };
            Some(PlcState {
                did: did.clone(),
                verification_methods: BTreeMap::from([("atproto".to_owned(), signing_key.clone())]),
                rotation_keys: vec![recovery_key.clone(), signing_key.clone()],
                also_known_as: vec![format!("at://{handle}")],
                services: BTreeMap::from([(
                    "atproto_pds".to_owned(),
                    PlcService {
                        service_type: "AtprotoPersonalDataServer".to_owned(),
                        endpoint,
                    },
                )]),
            })
        }
    }
}

/// "the bytes are hashed with SHA-256; the hash bytes are base32-encoded
/// ... and that string is truncated to 24 chars".
fn did_identifier_matches(did: &Did, genesis_hash: &[u8]) -> bool {
    base32_lower(genesis_hash).get(..24) == Some(did.method_specific_id())
}

/// Parses `YYYY-MM-DDTHH:mm:ss.sssZ` to milliseconds since the epoch.
fn parse_created_at(s: &str) -> Option<i64> {
    let (seconds, millis) = s.strip_suffix('Z')?.split_once('.')?;
    if millis.len() != 3 || !millis.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds = UTCDate::new_validated(format!("{seconds}Z"))
        .ok()?
        .to_epoch_seconds()
        .ok()?;
    Some(seconds * 1000 + millis.parse::<i64>().ok()?)
}

/// The multibase `b` string of the dag-cbor sha2-256 CIDv1 of `digest`.
fn cid_string(digest: &[u8]) -> String {
    let mut bytes = CID_PREFIX.to_vec();
    bytes.extend_from_slice(digest);
    format!("b{}", base32_lower(&bytes))
}

/// RFC 4648 §6 base32, lowercase, without padding.
fn base32_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer = 0u16;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u16::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(
                BASE32_LOWER[usize::from((buffer >> bits) & 0x1f)],
            ));
        }
    }
    if bits > 0 {
        out.push(char::from(
            BASE32_LOWER[usize::from((buffer << (5 - bits)) & 0x1f)],
        ));
    }
    out
}

/// DAG-CBOR encoding of a JSON value: shortest-form heads, map keys
/// sorted by length then bytewise. `None` for a floating-point number.
fn dag_cbor(value: &Value) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    encode(value, &mut out)?;
    Some(out)
}

fn encode(value: &Value, out: &mut Vec<u8>) -> Option<()> {
    match value {
        Value::Null => out.push(0xf6),
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::Number(n) => match (n.as_u64(), n.as_i64()) {
            (Some(u), _) => head(0, u, out),
            (None, Some(i)) => head(1, i.unsigned_abs() - 1, out),
            (None, None) => return None,
        },
        Value::String(s) => {
            head(3, s.len() as u64, out);
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(items) => {
            head(4, items.len() as u64, out);
            for item in items {
                encode(item, out)?;
            }
        }
        Value::Object(map) => {
            head(5, map.len() as u64, out);
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
            for key in keys {
                head(3, key.len() as u64, out);
                out.extend_from_slice(key.as_bytes());
                encode(&map[key], out)?;
            }
        }
    }
    Some(())
}

/// A CBOR head (RFC 8949 §3) in its shortest form.
fn head(major: u8, n: u64, out: &mut Vec<u8>) {
    let major = major << 5;
    match n {
        0..=23 => out.push(major | n as u8),
        24..=0xff => out.extend_from_slice(&[major | 24, n as u8]),
        0x100..=0xffff => {
            out.push(major | 25);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(major | 26);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
        _ => {
            out.push(major | 27);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(verification_methods: &[(&str, &str)], services: &[(&str, &str, &str)]) -> PlcState {
        PlcState {
            did: Did::parse("did:plc:7iza6de2dwap2sbkpav7c6c6").unwrap(),
            verification_methods: verification_methods
                .iter()
                .map(|(id, key)| ((*id).to_owned(), (*key).to_owned()))
                .collect(),
            rotation_keys: vec![
                "did:key:zDnaedvvAsDE6H3BDdBejpx9ve2Tz95cymyCAKF66JbyMh1Lt".to_owned(),
                "did:key:zDnaeh9v2RmcMo13Du2d6pjUf5bZwtauYxj3n9dYjw4EZUAR7".to_owned(),
            ],
            also_known_as: vec!["at://alice.test".to_owned()],
            services: services
                .iter()
                .map(|(id, service_type, endpoint)| {
                    (
                        (*id).to_owned(),
                        PlcService {
                            service_type: (*service_type).to_owned(),
                            endpoint: (*endpoint).to_owned(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// did:plc v0.3.0 §Presentation as DID Document: "The following
    /// data: ... Will be presented as the following DID document". Two
    /// fields differ from the printed example, each where the example
    /// contradicts another source: the verification method `id` is the
    /// full `did:plc:...#atproto` that the reference `formatDidDoc`
    /// builds (`${data.did}#${keyid}`) and plc.directory serves, not the
    /// example's bare `#atproto`; `serviceEndpoint` is the input's
    /// `https://example.test`, not the example's `https://example2.com`.
    #[test]
    fn spec_example_presentation() {
        let state = state(
            &[(
                "atproto",
                "did:key:zDnaeh9v2RmcMo13Du2d6pjUf5bZwtauYxj3n9dYjw4EZUAR7",
            )],
            &[(
                "atproto_pds",
                "AtprotoPersonalDataServer",
                "https://example.test",
            )],
        );
        assert_eq!(
            state.document(),
            json!({
                "@context": [
                    "https://www.w3.org/ns/did/v1",
                    "https://w3id.org/security/multikey/v1",
                    "https://w3id.org/security/suites/ecdsa-2019/v1"
                ],
                "id": "did:plc:7iza6de2dwap2sbkpav7c6c6",
                "alsoKnownAs": ["at://alice.test"],
                "verificationMethod": [{
                    "id": "did:plc:7iza6de2dwap2sbkpav7c6c6#atproto",
                    "type": "Multikey",
                    "controller": "did:plc:7iza6de2dwap2sbkpav7c6c6",
                    "publicKeyMultibase": "zDnaeh9v2RmcMo13Du2d6pjUf5bZwtauYxj3n9dYjw4EZUAR7"
                }],
                "service": [{
                    "id": "#atproto_pds",
                    "type": "AtprotoPersonalDataServer",
                    "serviceEndpoint": "https://example.test"
                }]
            })
        );
    }

    /// Reference `formatDidDoc`: a suite context is pushed only
    /// `if (info.context && !context.includes(info.context))`, and a key
    /// type it does not recognize gets none ("we can't specify a context
    /// for a key type we don't recognize"). The Ed25519 key is the did:key
    /// spec's Create example.
    #[test]
    fn suite_contexts_once_each_and_none_for_other_keys() {
        let state = state(
            &[
                (
                    "a",
                    "did:key:zQ3shunBKsXixLxKtC5qeSG9E4J5RkGN57im31pcTzbNQnm5w",
                ),
                (
                    "b",
                    "did:key:zDnaeh9v2RmcMo13Du2d6pjUf5bZwtauYxj3n9dYjw4EZUAR7",
                ),
                (
                    "c",
                    "did:key:zQ3shbTzUCq5zuk7oSj5zaJndqWhjwGDaGuvBXpjg8C19qssW",
                ),
                (
                    "d",
                    "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
                ),
            ],
            &[],
        );
        let document = state.document();
        assert_eq!(
            document["@context"],
            json!([
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/multikey/v1",
                "https://w3id.org/security/suites/secp256k1-2019/v1",
                "https://w3id.org/security/suites/ecdsa-2019/v1"
            ])
        );
        assert_eq!(
            document["verificationMethod"][3],
            json!({
                "id": "did:plc:7iza6de2dwap2sbkpav7c6c6#d",
                "type": "Multikey",
                "controller": "did:plc:7iza6de2dwap2sbkpav7c6c6",
                "publicKeyMultibase": "z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
            })
        );
        assert_eq!(document["service"], json!([]));
    }
}
