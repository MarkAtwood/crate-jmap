//! Shared helper utilities for JMAP method handlers.

use std::borrow::Cow;

use jmap_types::{Id, JmapError, UTCDate};

use crate::backend::{SetError, SetErrorType};
use serde_json::{Map, Value};

/// Serialize any [`serde::Serialize`] type to a [`serde_json::Value`],
/// mapping serialization errors to [`JmapError::server_fail`].
///
/// Used by every `*-server` handler to project a typed domain object
/// (e.g. `Email`, `Mailbox`, `Chat`) into the wire-format `list` /
/// `created` payload.
pub fn serialize_value<T: serde::Serialize>(val: T) -> Result<serde_json::Value, JmapError> {
    serde_json::to_value(val).map_err(|e| JmapError::server_fail(e.to_string()))
}

/// Convert a slice of [`Id`]s to a JSON `notFound` value.
///
/// RFC 8620 §5.1 specifies `notFound` as `Id[]` — always an array, never
/// `null`. Returns an empty array when all requested ids were found.
///
/// Equivalent to `serde_json::to_value(ids)` but threads through
/// `Value::Array` directly so the call site is infallible (bd:JMAP-wlip.28).
pub fn not_found_json(ids: &[Id]) -> Value {
    Value::Array(
        ids.iter()
            .map(|id| Value::String(id.as_ref().to_owned()))
            .collect(),
    )
}

/// Extract an optional, deserializable argument from a method-arguments
/// envelope (bd:JMAP-wlip.32).
///
/// Looks up `name` in `args`, removing it. The result is:
///
/// - `Ok(None)` if the key is absent OR is present with `Value::Null`
///   (RFC 8620 §3.3 treats absent and explicit-null the same for
///   optional fields).
/// - `Ok(Some(value))` if the key is present and the value
///   deserializes successfully into `T`.
/// - `Err(invalid_arguments_with(name))` if the value is present but
///   fails to deserialize. The error message is built by the caller-
///   supplied `invalid_arguments_with` so the resulting `JmapError`
///   carries a domain-specific description ("ids must be an Id
///   array", "filter must be a Filter object", etc.).
///
/// Collapses six near-identical
/// `match args.remove(...).unwrap_or(Value::Null) { Value::Null => None,
/// v => Some(serde_json::from_value(v)...) }` blocks in `handlers.rs`
/// into one-liners.
///
/// # Interaction with `ResultReference` resolution (bd:JMAP-jfia.15)
///
/// [`crate::parse::resolve_args`] replaces a `#key` with the value
/// produced by walking the JSON Pointer path against a prior
/// response. RFC 6901 §4 makes no distinction between "key absent"
/// and "key present with `null`", so the resolved value CAN be
/// `Value::Null`. This function then treats that resolved-null
/// identically to "key was not sent". The behaviour is
/// RFC-compliant — both are "optional argument not provided" — but
/// the asymmetry between an explicit `null` argument and a
/// `#key`-resolved `null` argument may surprise a handler author
/// who expects "I asked for the value via a ResultReference, so it
/// must have been there". If a handler needs to distinguish
/// resolved-null from unsent, it has to read the raw `args` map
/// before calling this helper.
pub fn optional_arg<T>(
    args: &mut Map<String, Value>,
    name: &str,
    invalid_arguments_with: impl FnOnce() -> JmapError,
) -> Result<Option<T>, JmapError>
where
    T: serde::de::DeserializeOwned,
{
    match args.remove(name).unwrap_or(Value::Null) {
        Value::Null => Ok(None),
        v => Ok(Some(
            serde_json::from_value(v).map_err(|_| invalid_arguments_with())?,
        )),
    }
}

/// Remove and decode the optional `filter` argument of a hand-written
/// `/query` or `/queryChanges` handler (RFC 8620 §5.5).
///
/// Absent or `null` yields `None`. A value that is not a JSON object is
/// not a filter at all and yields `invalidArguments`. An object that does
/// not decode as `T` yields `unsupportedFilter` ("The "filter" is
/// syntactically valid, but the server cannot process it") with the serde
/// message as its description. With `deny_unknown_fields` on the filter
/// types, that covers an unknown or misspelt condition property at any
/// depth and an operator tree sent to an object whose filter is a bare
/// condition.
pub fn filter_arg<T>(args: &mut Map<String, Value>) -> Result<Option<T>, JmapError>
where
    T: serde::de::DeserializeOwned,
{
    match args.remove("filter").unwrap_or(Value::Null) {
        Value::Null => Ok(None),
        v @ Value::Object(_) => serde_json::from_value(v).map(Some).map_err(|e| {
            let mut err = JmapError::unsupported_filter();
            err.description = Some(format!("filter: {e}"));
            err
        }),
        v => Err(JmapError::invalid_arguments(format!(
            "filter: expected an object, got {v}"
        ))),
    }
}

/// Read an optional boolean argument from a method-arguments envelope without
/// removing it (bd:JMAP-ic0j.39).
///
/// Returns `default` if `key` is absent, if the value is not a boolean, or if
/// the value is `Value::Null`. This is the canonical workspace helper for
/// JMAP's "permissive parsing" convention on optional boolean controls
/// (RFC 8620 §3.6.1 — unknown / non-typed values coerce to the spec default).
///
/// Use [`take_bool_arg`] if the caller wants to consume the entry as part of
/// a "walk the args map removing as we go" pattern.
///
/// Collapses ~50 near-identical
/// `args.get(K).and_then(|v| v.as_bool()).unwrap_or(default)` blocks across
/// the extension-server family into single-line calls.
pub fn bool_arg(args: &Map<String, Value>, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Read an optional boolean argument from a method-arguments envelope,
/// removing it from the map (bd:JMAP-ic0j.39).
///
/// Returns `default` if `key` is absent, if the value is not a boolean, or if
/// the value is `Value::Null`. Mirrors the semantics of [`bool_arg`] but
/// consumes the entry from `args`. Matches the canonical mail-server
/// "remove-as-we-go" parsing pattern.
pub fn take_bool_arg(args: &mut Map<String, Value>, key: &str, default: bool) -> bool {
    args.remove(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// Extract `accountId` from a JMAP method arguments envelope and return both
/// the extracted [`Id`] and the remaining argument map.
///
/// The caller passes the full `args: Value` from the method invocation by
/// value; this function destructures it once, so handlers do not have to
/// repeat the `let Value::Object(mut args) = args else { ... }` pattern after
/// every call.
///
/// # Errors
///
/// Returns `invalidArguments` with:
///
/// - `"arguments must be an object containing accountId"` when `args` is
///   not a JSON object.
/// - `"accountId is required"` when the field is missing or not a string.
/// - `"accountId is not a valid Id: <reason>"` when the field is a string
///   but does not satisfy the RFC 8620 §1.2 Id grammar
///   (`Id::new_validated`). Catches empty strings, strings longer than
///   255 bytes, and strings containing characters outside the SAFE-CHAR
///   set (`%x21 / %x23-7E` — visible ASCII excluding `"`).
///   bd:JMAP-wlip.5 closed the previous silent-pass-through behaviour
///   where a malformed accountId reached the backend's `account_exists`
///   call and surfaced as either `notFound` or a storage-layer parse
///   error, depending on the backend.
pub fn extract_account_id(args: Value) -> Result<(Id, Map<String, Value>), JmapError> {
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be an object containing accountId",
        ));
    };
    // Remove (not get) the accountId entry so the returned args map no
    // longer carries it (bd:JMAP-jfia.9). This matches optional_arg's
    // remove-and-consume semantics and prevents downstream handlers
    // from re-parsing the validated id or seeing it as an unexpected
    // residual key.
    let raw = args
        .remove("accountId")
        .ok_or_else(|| JmapError::invalid_arguments("accountId is required"))?;
    let s = raw
        .as_str()
        .ok_or_else(|| JmapError::invalid_arguments("accountId is required"))?;
    let id = Id::new_validated(s)
        .map_err(|e| JmapError::invalid_arguments(format!("accountId is not a valid Id: {e}")))?;
    Ok((id, args))
}

/// Parse the optional `maxChanges` argument of `/changes` and
/// `/queryChanges`. RFC 8620 §5.2: "If supplied by the client, the value
/// MUST be a positive integer greater than 0. If a value outside of this
/// range is given, the server MUST reject the call with an
/// "invalidArguments" error." §5.6 gives no range of its own, so the same
/// rule applies to `/queryChanges`.
///
/// Pass the raw argument as taken from the args map (`args.get(..)` or
/// `args.remove(..).as_ref()`). Absent or `null` yields `Ok(None)`.
///
/// # Errors
///
/// Returns `invalidArguments` (`"maxChanges must be a positive integer"`)
/// for `0`, negative numbers, non-integers, and non-number values.
pub fn parse_max_changes(value: Option<&Value>) -> Result<Option<u64>, JmapError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_u64().filter(|&n| n > 0).map(Some).ok_or_else(|| {
                JmapError::invalid_arguments("maxChanges must be a positive integer")
            })
        }
    }
}

/// Return the current UTC instant as an [`UTCDate`] (RFC 3339,
/// millisecond precision, format `YYYY-MM-DDTHH:MM:SS.mmmZ`).
///
/// Uses `std::time::SystemTime` so no external dependency is needed.
///
/// Returns a typed [`UTCDate`] rather than a `String` (bd:JMAP-wlip.20)
/// so callers do not need to wrap the result in
/// `UTCDate::from(now_utc_string().as_str())`. The function name is
/// retained for back-compat across the workspace's many call sites.
///
/// The string the [`UTCDate`] wraps does not pass
/// [`UTCDate::new_validated`] because that validator requires exactly
/// the 20-char `YYYY-MM-DDTHH:MM:SSZ` form (no millis) and this helper
/// emits the 24-char form with millis. The workspace convention is to
/// use the 24-char form on the wire — consumers wanting strict
/// validation should construct their own `UTCDate::new_validated` value
/// from a `chrono`-formatted source.
///
/// Pre-epoch handling: `SystemTime::now().duration_since(UNIX_EPOCH)`
/// fails on clocks drifted before the epoch. The function uses
/// `Err::duration()` to recover the magnitude and negates the seconds
/// before formatting; the result is a correct RFC 3339 timestamp
/// anywhere from the BCE proleptic Gregorian range up to
/// `1970-01-01T00:00:00.000Z`. The rem_euclid / div_euclid math at
/// the format step handles arbitrarily-large negative offsets, not
/// only sub-day drifts. Pre-epoch correctness is best-effort —
/// `SystemTime` is non-monotonic and the JMAP spec does not require
/// pre-epoch timestamps. (bd:JMAP-wlip.11 corrected the previous
/// docstring's incorrect "1969-12-31T… through 1970-01-01T00:00:00Z"
/// range claim.)
///
/// Clock-overflow handling: on a corrupted clock (system clock
/// reporting a Duration whose `as_secs()` exceeds `i64::MAX`,
/// `civil_from_days` reporting a year outside `i32`, or any other
/// `SystemTime` failure mode), this function **panics**. The
/// previous sentinel-string behaviour (`UTCDate::from("clock-out-of-range")`)
/// was an idiom-grade defect (bd:JMAP-jfia.30): the sentinel was not a
/// valid wire-format timestamp, had no in-band signal to distinguish
/// it from a real value, and could silently propagate into JSON
/// responses, audit logs, and database rows.
///
/// Callers that need to handle clock failure without panicking MUST
/// use [`now_utc_string_checked`], which returns `Option<UTCDate>`.
/// Long-running daemons, schedulers, and persistence layers SHOULD
/// prefer the checked variant; one-shot tools and request handlers
/// MAY accept the panic since clock corruption is unrecoverable and
/// the dispatcher's `task::spawn` isolation already converts the
/// panic into a `serverFail` invocation rather than crashing the
/// process.
///
/// # Panics
///
/// Panics if `SystemTime::now()` cannot be expressed as an RFC 3339
/// timestamp — the same conditions under which
/// [`now_utc_string_checked`] returns `None`.
pub fn now_utc_string() -> UTCDate {
    now_utc_string_checked().expect("system clock returned an out-of-range value (bd:JMAP-jfia.30)")
}

/// Return the current UTC instant as an [`UTCDate`] (RFC 3339,
/// millisecond precision, format `YYYY-MM-DDTHH:MM:SS.mmmZ`), or
/// `None` if the system clock cannot be expressed as an RFC 3339
/// timestamp.
///
/// Added in bd:JMAP-jfia.30 to replace the previous sentinel-string
/// failure mode of [`now_utc_string`] with a typed `Option` shape.
/// Callers that want to react to a clock fault (audit-log
/// timestamps, last-seen markers, retention sweeps) SHOULD use this
/// variant; callers for whom a panic at the first sign of clock
/// corruption is acceptable MAY use [`now_utc_string`] directly.
///
/// Returns `None` when:
/// - `SystemTime::now().duration_since(UNIX_EPOCH).as_secs()`
///   exceeds `i64::MAX` (only reachable on a corrupted clock —
///   approx ±292 billion years from epoch).
/// - The negation of a pre-epoch duration overflows `i64`
///   (unreachable on a `try_from`-validated input but checked
///   defensively).
/// - `civil_from_days` reports a year outside `i32`
///   (bd:JMAP-jfia.2 — between the i32-year boundary and the
///   i64::MAX-secs cap).
///
/// # Why `Option<UTCDate>` and not `Result<UTCDate, ClockError>` (bd:JMAP-jfia.38)
///
/// The three failure modes are all "corrupted clock" — each one is
/// physically unreachable on a sane host (years 5.7M-to-292B,
/// `try_from`-impossible negation, `i32`-overflowing year). A caller
/// that wants to branch on which physical mechanism fired would be
/// branching on states that don't happen. The shapes the workspace
/// uses elsewhere for typed-variant-per-mode (`SetError`,
/// `BackendChangesError`, `BackendSetError`, `MergePatchError`) all
/// carry failure modes that DO occur in normal operation —
/// `notFound`, `tooManyChanges`, `invalidPatch`, `depthExceeded`.
/// The clock-corruption modes are different in kind. Erasing the
/// discriminator here trades a hypothetical observability win for a
/// cleaner contract: "the clock is unusable for RFC 3339, abandon
/// timestamping." A future need for per-mode telemetry can be added
/// non-breakingly as a parallel helper (e.g. `now_utc_string_diagnose
/// -> Result<UTCDate, ClockError>`) without disturbing this shape.
pub fn now_utc_string_checked() -> Option<UTCDate> {
    use std::time::SystemTime;

    let (secs, millis) = signed_seconds_since_epoch(SystemTime::now())?;

    let s = secs.rem_euclid(60);
    let m = (secs / 60).rem_euclid(60);
    let h = (secs / 3600).rem_euclid(24);
    let days = secs.div_euclid(86400);
    let (year, month, day) = civil_from_days(days)?;

    Some(UTCDate::from(format!(
        "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z"
    )))
}

/// Convert a [`SystemTime`] into `(signed_seconds_since_epoch, millis)`,
/// returning `None` when the system clock cannot be expressed as an
/// `i64` second count.
///
/// Extracted from [`now_utc_string_checked`] in bd:JMAP-jfia.11 to
/// flatten the previous three-level nested match
/// (`duration_since` → `try_from` → `checked_neg`) into one
/// linear function the reader can scan top-to-bottom. The
/// `.checked_neg()` branch is unreachable on a `try_from`-validated
/// input (its output is `[0, i64::MAX]` so negation cannot underflow)
/// but is kept for defence in depth — the failure path stays total.
pub(crate) fn signed_seconds_since_epoch(now: std::time::SystemTime) -> Option<(i64, u32)> {
    use std::time::UNIX_EPOCH;
    match now.duration_since(UNIX_EPOCH) {
        Ok(d) => {
            let s = i64::try_from(d.as_secs()).ok()?;
            Some((s, d.subsec_millis()))
        }
        Err(e) => {
            // Pre-epoch clock — negate after the fallible widen so we
            // can return a real (negative) epoch offset rather than
            // silently snapping to 1970-01-01T00:00:00Z.
            let d = e.duration();
            let s = i64::try_from(d.as_secs()).ok()?;
            let neg = s.checked_neg()?;
            Some((neg, d.subsec_millis()))
        }
    }
}

/// Format `secs` seconds since the Unix epoch as a seconds-precision
/// [`UTCDate`] (`YYYY-MM-DDTHH:MM:SSZ`, RFC 8620 §1.4), or `None` if the
/// year falls outside `0000..=9999`, which that four-digit form cannot
/// express.
///
/// This is the inverse of [`UTCDate::to_epoch_seconds`], which accepts only
/// this form. [`now_utc_string`] adds milliseconds, so its output does not
/// round-trip through `to_epoch_seconds`.
pub fn utc_date_from_epoch_seconds(secs: i64) -> Option<UTCDate> {
    let time_of_day = secs.rem_euclid(86400);
    let (h, m, s) = (
        time_of_day / 3600,
        time_of_day % 3600 / 60,
        time_of_day % 60,
    );
    let (year, month, day) = civil_from_days(secs.div_euclid(86400))?;
    if !(0..=9999).contains(&year) {
        return None;
    }
    Some(UTCDate::from(format!(
        "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z"
    )))
}

/// Convert a count of days since the Unix epoch (1970-01-01) to a proleptic
/// Gregorian (year, month, day) triple, or `None` if the resulting year
/// does not fit in `i32`.
///
/// Algorithm by Howard Hinnant (public domain):
/// <https://howardhinnant.github.io/date_algorithms.html>
///
/// The year-narrowing cast is fallible because the algorithm's intermediate
/// `y` value is bounded by `i64::MAX / 146_097 ≈ ±6.3e13`, only a subset
/// of which fits in `i32` (~±2.1e9 years). For a sane `SystemTime`-derived
/// input we stay well inside `i32::MIN..=i32::MAX`, but the outer
/// `now_utc_string` only rejects `u64` seconds exceeding `i64::MAX`
/// (bd:JMAP-wlip.27 sentinel) — inputs between the i32-year boundary
/// (~5.7e6 years from epoch) and that sentinel reach this function and
/// previously panicked the dispatcher worker (bd:JMAP-jfia.2). Returning
/// `None` lets the caller fall through to its own sentinel rather than
/// taking down the task.
///
/// Month and day cannot fail: the algorithm's modular structure pins them
/// to `[1, 12]` and `[1, 31]` respectively, narrow casts handled with
/// `try_from(...).expect(...)` documenting the invariant.
fn civil_from_days(z: i64) -> Option<(i32, u8, u8)> {
    let z = z + 719_468;
    let era: i64 = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let mo = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let yr = if mo <= 2 { y + 1 } else { y };
    Some((
        i32::try_from(yr).ok()?,
        u8::try_from(mo).expect("month bounded by algorithm to [1, 12]"),
        u8::try_from(d).expect("day bounded by algorithm to [1, 31]"),
    ))
}

/// Maximum recursion depth for [`json_merge_patch`] application.
///
/// Beyond this depth [`json_merge_patch`] returns
/// [`MergePatchError::DepthExceeded`] without applying any further levels.
/// Mitigates stack DoS from adversarial `PatchObject` values
/// (bd:JMAP-sc1b.97). 32 levels comfortably exceeds any legitimate JMAP
/// `/set update` shape — the deepest standard JMAP `/set update` shape
/// (Email with nested `bodyStructure`) tops out around 6 levels, so the
/// cap fires only on adversarial input.
///
/// Crate-private (bd:JMAP-wlip.4): consumers see the cap-exceeded
/// behaviour via [`MergePatchError::DepthExceeded`], not by reading the
/// constant directly. The crate reserves the right to tighten the
/// value (e.g. 32 → 16) without a major-version bump because the
/// contract is "the function may return DepthExceeded", not "the cap
/// is exactly N".
pub(crate) const MAX_MERGE_PATCH_DEPTH: usize = 32;

/// Error returned by [`json_merge_patch`] when a patch cannot be applied.
///
/// Marked `#[non_exhaustive]` so future RFC 7396 failure modes (e.g. a
/// size cap in addition to the depth cap) can be added without an API
/// break.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergePatchError {
    /// The patch nests deeper than the crate's `MAX_MERGE_PATCH_DEPTH`
    /// DoS-guard cap.
    ///
    /// Callers SHOULD map this to
    /// [`SetError`](crate::SetError) with
    /// [`SetErrorType::InvalidPatch`](crate::SetErrorType::InvalidPatch)
    /// and MUST discard any partially-mutated `target` rather than
    /// persisting it — see the contract on [`json_merge_patch`].
    ///
    /// # Why no `reached` / `max` payload (bd:JMAP-jfia.39)
    ///
    /// The variant is intentionally unit-shaped. The cap value is
    /// available in the `Display` impl (which interpolates
    /// `MAX_MERGE_PATCH_DEPTH`) for log-line and user-message use.
    /// A typed payload `{ reached: usize, max: usize }` was
    /// considered and rejected for now because (a) no current
    /// consumer in the workspace destructures the reached depth or
    /// the cap programmatically — all 6 call sites map to
    /// `SetErrorType::InvalidPatch` with a generic description; and
    /// (b) changing a unit variant to a struct variant is a breaking
    /// pattern-match change even on a `#[non_exhaustive]` enum (the
    /// existing `if let Err(MergePatchError::DepthExceeded)` pattern
    /// would stop compiling). If a future consumer needs
    /// programmatic access to the depth, the additive evolution is
    /// to add a new variant alongside (e.g.
    /// `DepthExceededDetail { reached: usize, max: usize }`) and
    /// emit the new variant from `json_merge_patch`. The unit
    /// variant would then become unreachable but remain valid
    /// pattern syntax for any pinned-version consumer.
    DepthExceeded,
}

impl std::fmt::Display for MergePatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DepthExceeded => write!(
                f,
                "merge patch nesting exceeds {MAX_MERGE_PATCH_DEPTH} levels"
            ),
        }
    }
}

impl std::error::Error for MergePatchError {}

/// Apply a JSON Merge Patch (RFC 7396) to `target` in-place.
///
/// Used by every `*-server` backend's `update_object` implementation
/// to merge a sparse `/set update` patch into the stored serialized
/// object. Extracted from per-crate copies in bd:JMAP-sc1b.103 — keep
/// edits here so all five reference backends stay byte-identical.
///
/// # Errors
///
/// Returns [`MergePatchError::DepthExceeded`] when the patch nests
/// deeper than the crate's internal `MAX_MERGE_PATCH_DEPTH` DoS-guard
/// cap (added in bd:JMAP-sc1b.97, made non-silent in bd:JMAP-wlip.1).
/// The exact value is intentionally not exposed; consumers see the
/// behaviour via the typed error rather than reading the constant.
/// Below the cap the behaviour is exactly RFC 7396 and the call always
/// returns `Ok(())`.
///
/// # Partial-mutation contract
///
/// On `Err(DepthExceeded)`, `target` may have been mutated up to the
/// level where the cap fired — RFC 7396 merging is applied recursively
/// in place and the function does not roll back on error. Callers MUST
/// discard `target` rather than persist it. The standard pattern in
/// every `*-server` `update_object` impl is to operate on a `.clone()`
/// of the stored value and only `insert(...)` it back on `Ok(())`; that
/// pattern is naturally safe because the stored value is left untouched
/// on error.
pub fn json_merge_patch(target: &mut Value, patch: Value) -> Result<(), MergePatchError> {
    json_merge_patch_inner(target, patch, 0)
}

fn json_merge_patch_inner(
    target: &mut Value,
    patch: Value,
    depth: usize,
) -> Result<(), MergePatchError> {
    if depth > MAX_MERGE_PATCH_DEPTH {
        return Err(MergePatchError::DepthExceeded);
    }
    match patch {
        Value::Object(patch_map) => {
            // Per RFC 7396 §2: "If the target value is not a JSON object,
            // the resulting value will be the merge patch." We therefore
            // reset a non-Object target to an empty Object before merging
            // — this is reachable when a Patch creates a nested field that
            // is absent from the target (the parent recursion frame inserted
            // Value::Null as a placeholder).
            if !target.is_object() {
                *target = Value::Object(Map::new());
            }
            let target_map = target
                .as_object_mut()
                .expect("target was just set to Value::Object above");
            for (key, patch_val) in patch_map {
                if patch_val.is_null() {
                    target_map.remove(&key);
                } else {
                    let entry = target_map.entry(key).or_insert(Value::Null);
                    json_merge_patch_inner(entry, patch_val, depth + 1)?;
                }
            }
        }
        other => *target = other,
    }
    Ok(())
}

/// Error returned by [`apply_jmap_patch`] and [`apply_patch_object`] when
/// a PatchObject cannot be applied.
///
/// Every variant is an RFC 8620 §5.3 violation for which "the update
/// MUST be rejected with an "invalidPatch" error". Callers SHOULD map
/// any variant to [`SetError`](crate::SetError) with
/// [`SetErrorType::InvalidPatch`](crate::SetErrorType::InvalidPatch);
/// the `Display` form is suitable as its description.
///
/// Marked `#[non_exhaustive]` so further §5.3 checks can be added
/// without an API break.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchPathError {
    /// A part before the last does not exist on the patched object.
    ParentMissing,
    /// A part before the last names an array, so the pointer would
    /// reference inside an array.
    InsideArray,
    /// A part before the last names a value that is neither an object nor
    /// an array (string, number, boolean or null), which has no members.
    ParentNotObject,
    /// A `~` in the path is not followed by `0` or `1`, so the path is
    /// not a valid RFC 6901 JSON Pointer.
    InvalidEscape,
    /// The pointer of one patch is a prefix of the pointer of another in
    /// the same PatchObject (e.g. `"alerts/1/offset"` and `"alerts"`).
    PrefixConflict,
    /// The path's segment count plus the value's object/array nesting
    /// exceeds the crate's `MAX_MERGE_PATCH_DEPTH` DoS-guard cap.
    ///
    /// Not an RFC 8620 §5.3 rule: it is this crate's server limit
    /// (RFC 8620 §3.6.1 lets a server reject requests that exceed its
    /// limits), so repeated path patches cannot grow a stored value's
    /// depth without bound. It is still reported as invalidPatch.
    DepthExceeded,
}

impl std::fmt::Display for PatchPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParentMissing => f.write_str("patch path parent does not exist"),
            Self::InsideArray => f.write_str("patch path references inside an array"),
            Self::ParentNotObject => f.write_str("patch path parent is not an object"),
            Self::InvalidEscape => f.write_str("patch path has a '~' not followed by '0' or '1'"),
            Self::PrefixConflict => {
                f.write_str("patch has a path that is a prefix of another path")
            }
            Self::DepthExceeded => {
                write!(f, "patch nesting exceeds {MAX_MERGE_PATCH_DEPTH} levels")
            }
        }
    }
}

impl std::error::Error for PatchPathError {}

/// Apply one RFC 8620 §5.3 PatchObject entry (`path` → `value`) to
/// `target` in-place.
///
/// `path` is a PatchObject key: an RFC 6901 JSON Pointer with the
/// implicit leading `/` omitted (e.g. `"mailboxIds/abc123"`). Each
/// `/`-separated reference token is decoded per RFC 6901 §4 (`~1` → `/`,
/// then `~0` → `~`). Every token before the last must name an existing
/// object member; the last token names the member to set. A `null`
/// `value` removes that member (a no-op when it is absent); any other
/// value inserts or replaces it.
///
/// Extracted from per-crate copies in bd:JMAP-j6ab. Extension servers
/// MUST call this rather than reimplement it.
///
/// Empty tokens are legal: RFC 6901 §3 allows a zero-length
/// `reference-token`, and its §5 example evaluates `"/"` to the member
/// named `""`. The RFC 8620 §5.3 "null sets the property default" rule
/// is type-specific and left to the caller. The rule that no two pointers
/// in one PatchObject may be prefixes of each other is checked by
/// [`apply_patch_object`].
///
/// # Errors
///
/// Returns [`PatchPathError`] when a part before the last does not exist
/// ([`ParentMissing`](PatchPathError::ParentMissing)), is an array
/// ([`InsideArray`](PatchPathError::InsideArray)) or is another
/// non-object value ([`ParentNotObject`](PatchPathError::ParentNotObject)),
/// when `path` contains an invalid escape
/// ([`InvalidEscape`](PatchPathError::InvalidEscape)), or when the
/// number of `/`-separated segments in `path` plus the object/array
/// nesting of `value` (a scalar nests 0 levels) exceeds the crate's
/// `MAX_MERGE_PATCH_DEPTH` DoS-guard cap
/// ([`DepthExceeded`](PatchPathError::DepthExceeded)).
///
/// # Mutation contract
///
/// A single call is all-or-nothing: on `Err`, `target` is unchanged.
/// A PatchObject has several entries, and an error on a later entry
/// does not undo earlier ones, so callers applying a whole PatchObject
/// MUST use [`apply_patch_object`].
pub fn apply_jmap_patch(
    target: &mut Map<String, Value>,
    path: &str,
    value: Value,
) -> Result<(), PatchPathError> {
    let segments = path.split('/').count();
    if segments > MAX_MERGE_PATCH_DEPTH
        || nests_deeper_than(&value, MAX_MERGE_PATCH_DEPTH - segments)
    {
        return Err(PatchPathError::DepthExceeded);
    }
    let (parents, leaf) = match path.rsplit_once('/') {
        Some((parents, leaf)) => (Some(parents), leaf),
        None => (None, path),
    };
    let leaf = decode_pointer_token(leaf)?;
    let mut parent = target;
    for token in parents.into_iter().flat_map(|p| p.split('/')) {
        parent = match parent.get_mut(decode_pointer_token(token)?.as_ref()) {
            Some(Value::Object(map)) => map,
            Some(Value::Array(_)) => return Err(PatchPathError::InsideArray),
            Some(_) => return Err(PatchPathError::ParentNotObject),
            None => return Err(PatchPathError::ParentMissing),
        };
    }
    if value.is_null() {
        parent.remove(leaf.as_ref());
    } else {
        parent.insert(leaf.into_owned(), value);
    }
    Ok(())
}

/// Apply a whole RFC 8620 §5.3 PatchObject to `target`.
///
/// Each entry is applied with [`apply_jmap_patch`]. The value at each
/// pointer is set as given: a top-level key such as `"name"` replaces that
/// property wholesale, with no nested merge of object values (RFC 8620
/// §5.3: "The value to set for this property").
///
/// # Errors
///
/// Returns [`PatchPathError::PrefixConflict`] when the decoded pointer of
/// one key is a whole-segment prefix of another (`"alerts"` and
/// `"alerts/1/offset"` conflict; `"alert"` and `"alerts/x"` do not), and
/// otherwise any error [`apply_jmap_patch`] returns for an entry.
///
/// # Mutation contract
///
/// All-or-nothing: on `Err`, `target` is unchanged.
pub fn apply_patch_object(
    target: &mut Map<String, Value>,
    patch: &Map<String, Value>,
) -> Result<(), PatchPathError> {
    let mut pointers = patch
        .keys()
        .map(|k| {
            k.split('/')
                .map(decode_pointer_token)
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Sorted, a pointer that prefixes any other also prefixes its successor.
    pointers.sort_unstable();
    if pointers.windows(2).any(|w| w[1].starts_with(&w[0])) {
        return Err(PatchPathError::PrefixConflict);
    }
    let mut patched = target.clone();
    for (path, value) in patch {
        apply_jmap_patch(&mut patched, path, value.clone())?;
    }
    *target = patched;
    Ok(())
}

/// Whether `value` nests objects or arrays more than `limit` levels deep
/// (a scalar nests 0 levels). Recursion is bounded by `limit`, not by the
/// input.
fn nests_deeper_than(value: &Value, limit: usize) -> bool {
    match value {
        Value::Object(map) => limit == 0 || map.values().any(|v| nests_deeper_than(v, limit - 1)),
        Value::Array(items) => limit == 0 || items.iter().any(|v| nests_deeper_than(v, limit - 1)),
        _ => false,
    }
}

/// Decode one RFC 6901 reference token: `~1` → `/`, then `~0` → `~`
/// (RFC 6901 §4 order, so `~01` becomes `~1`, not `/`).
fn decode_pointer_token(token: &str) -> Result<Cow<'_, str>, PatchPathError> {
    if !token.contains('~') {
        return Ok(Cow::Borrowed(token));
    }
    let escapes_valid = token
        .match_indices('~')
        .all(|(i, _)| matches!(token.as_bytes().get(i + 1), Some(b'0' | b'1')));
    if !escapes_valid {
        return Err(PatchPathError::InvalidEscape);
    }
    Ok(Cow::Owned(token.replace("~1", "/").replace("~0", "~")))
}

/// Resolve a request-side `position` argument (RFC 8620 §5.5) into a
/// 0-based start index suitable for slicing the full ordered result
/// list (bd:JMAP-qz9v.48).
///
/// `position` is the signed offset from the `/query` request. The
/// foundation handler at [`handle_query`](crate::handle_query) parses it
/// via [`serde_json::Value::as_i64`], so the value reaching the
/// backend is any `i64` (including `i64::MIN`). Non-negative values
/// are absolute offsets from the start of the result list; negative
/// values are end-relative offsets per RFC 8620 §5.5 ("`-1` represents
/// the last entry, `-2` the second to last, and so on").
///
/// Returns a `usize` clamped to `[0, total]`. The clamp at the high
/// end matches the slice-safety property: `all_ids[start..]` is
/// guaranteed not to panic regardless of the input `position`.
///
/// # Edge cases this helper exists to centralize
///
/// 1. `position == 0` → `0`.
/// 2. `position > total` → `total` (yields an empty page, matching
///    the RFC's silent-clamp semantics — `/query` does not error on
///    out-of-range positions).
/// 3. `position == i64::MIN` → handled via [`i64::saturating_neg`],
///    which returns `i64::MAX` rather than overflowing. The resulting
///    `usize::MAX`-magnitude offset is then absorbed by
///    [`usize::saturating_sub`].
/// 4. `position` exceeding `usize::MAX` on a 32-bit target (`i64`
///    values above `2^32 - 1`) → clamped to `total` rather than
///    truncated. The previous workspace idiom (`position as usize`)
///    silently wrapped the high bits on 32-bit, returning a small
///    `start` index that did not match the caller's intent.
///
/// # Why this lives in the foundation
///
/// The pattern is identical across every extension server's `/query`
/// backend impl. Centralizing it eliminates the per-crate
/// `(position as usize)` / `(position.saturating_neg() as usize)`
/// idiom that clippy::pedantic flags as `cast_possible_truncation` +
/// `cast_sign_loss`, and prevents future siblings from re-introducing
/// the strictly-worse `(-position) as usize` variant (which panics on
/// `i64::MIN` in debug builds and wraps in release).
///
/// # Example
///
/// ```
/// use jmap_server::resolve_query_offset;
///
/// // Absolute offset.
/// assert_eq!(resolve_query_offset(0, 100), 0);
/// assert_eq!(resolve_query_offset(25, 100), 25);
///
/// // End-relative offset.
/// assert_eq!(resolve_query_offset(-1, 100), 99);
/// assert_eq!(resolve_query_offset(-100, 100), 0);
///
/// // Out-of-range clamps to total.
/// assert_eq!(resolve_query_offset(1_000, 100), 100);
/// assert_eq!(resolve_query_offset(-1_000, 100), 0);
///
/// // i64::MIN does not overflow.
/// assert_eq!(resolve_query_offset(i64::MIN, 100), 0);
/// ```
#[must_use]
pub fn resolve_query_offset(position: i64, total: usize) -> usize {
    if position >= 0 {
        // `i64 >= 0` → fits in `u64`. `usize::try_from` returns the
        // value on 64-bit targets and `Err` on 32-bit targets only
        // when the magnitude exceeds `usize::MAX`; saturating to
        // `usize::MAX` is then clamped to `total` by `.min`.
        usize::try_from(position).unwrap_or(usize::MAX).min(total)
    } else {
        // `saturating_neg` maps `i64::MIN` to `i64::MAX` instead of
        // overflowing. Every other negative `i64` negates exactly.
        let neg = usize::try_from(position.saturating_neg()).unwrap_or(usize::MAX);
        total.saturating_sub(neg)
    }
}

/// Parse the RFC 8620 §5.5 `/query` arguments `anchor` (`Id|null`) and
/// `anchorOffset` (`Int`, default `0`) from `args`.
///
/// # Errors
///
/// `invalidArguments` when `anchor` is neither a string nor `null`, or
/// `anchorOffset` is neither an integer nor `null`.
pub fn query_anchor_args(args: &Map<String, Value>) -> Result<(Option<Id>, i64), JmapError> {
    let anchor = match args.get("anchor") {
        None | Some(Value::Null) => None,
        // Id::from: wire-boundary validation deferred to JMAP-k9va; a
        // malformed id is never in the results, so it is anchorNotFound.
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {v}"
            )))
        }
    };
    let anchor_offset = match args.get("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("anchorOffset: expected an integer, got {v}"))
        })?,
    };
    Ok((anchor, anchor_offset))
}

/// Start index of a `/query` page within the filtered, sorted `ids`
/// (RFC 8620 §5.5), for [`JmapBackend::query_objects`] implementations.
///
/// With an `anchor`, `position` is ignored and the start is the
/// anchor's index plus `anchor_offset`, clamped to `0..=ids.len()`.
/// Without one, `anchor_offset` is ignored and the start is
/// [`resolve_query_offset`]`(position, ids.len())`.
///
/// # Errors
///
/// [`JmapError::anchor_not_found`] when `anchor` is not in `ids`.
///
/// [`JmapBackend::query_objects`]: crate::JmapBackend::query_objects
pub fn resolve_query_start(
    ids: &[Id],
    position: i64,
    anchor: Option<&Id>,
    anchor_offset: i64,
) -> Result<usize, JmapError> {
    let Some(anchor) = anchor else {
        return Ok(resolve_query_offset(position, ids.len()));
    };
    let index = ids
        .iter()
        .position(|id| id == anchor)
        .ok_or_else(JmapError::anchor_not_found)?;
    let start = i64::try_from(index)
        .unwrap_or(i64::MAX)
        .saturating_add(anchor_offset)
        .max(0);
    Ok(resolve_query_offset(start, ids.len()))
}

/// Enforce RFC 8620 §5.3 `maxObjectsInSet` cap at the top of a `/set`
/// handler (bd:JMAP-ayoz.41.1).
///
/// Counts entries in the wire `create` (object), `update` (object), and
/// `destroy` (array) arguments. Returns the method-level error
/// [`JmapError::request_too_large`] (wire `type: "requestTooLarge"`,
/// `description: "maxObjectsInSet"`) when the sum exceeds `max`. Per
/// RFC 8620 §5.3 it is returned instead of the `Foo/set` response, so
/// the dispatcher emits an `error` invocation and later method calls
/// in the same request still run (bd:JMAP-yt1w.30).
///
/// Call at the top of every `handle_*_set` after the `account_exists`
/// gate and before any per-entry processing. A request carrying
/// megabytes of `/set` ops is rejected before the handler touches the
/// storage layer:
///
/// ```ignore
/// let (account_id, args) = extract_account_id(args)?;
/// if !backend.account_exists(caller, &account_id).await
///     .map_err(|e| server_fail_from_backend(&e))?
/// {
///     return Err(JmapError::account_not_found());
/// }
/// jmap_server::helpers::enforce_max_objects_in_set(
///     &args,
///     backend.max_objects_in_set(caller, &account_id),
/// )?;
/// ```
///
/// # Why this lives in the foundation
///
/// `maxObjectsInSet` is a RFC 8620 §5.3 base-protocol cap, not an
/// extension concept. Every `jmap-*-server` extension's `handle_*_set`
/// needs the same check; the helper is the single source of truth so a
/// future revision (different error shape, additional counting rule,
/// alternate semantics for non-object `create` / `update`) lands in one
/// place instead of being propagated through 28 handler sites.
///
/// # Counting rules
///
/// - `create` is counted as `args["create"].as_object()?.len()` —
///   missing key, `null`, or non-object types count as 0.
/// - `update` is counted the same way (RFC 8620 §5.3 `update` is
///   `Id[PatchObject]`).
/// - `destroy` is counted as `args["destroy"].as_array()?.len()` —
///   missing key, `null`, or non-array types count as 0.
///
/// Wire-shape validation of the individual `create` / `update` /
/// `destroy` arguments belongs to the per-handler argument parsing,
/// not to this cap-enforcement helper. A non-object `create` survives
/// the cap check (counts as 0) and is rejected by the handler's
/// downstream `args.remove("create")` match arm. Conversely, a
/// well-formed but over-limit `create` is rejected here before the
/// handler runs.
///
/// # Errors
///
/// Returns `JmapError::request_too_large()` with description
/// `"maxObjectsInSet"` when the sum exceeds `max` (RFC 8620 §5.3).
pub fn enforce_max_objects_in_set(args: &Map<String, Value>, max: u64) -> Result<(), JmapError> {
    let create_count = args
        .get("create")
        .and_then(|v| v.as_object())
        .map_or(0u64, |m| m.len() as u64);
    let update_count = args
        .get("update")
        .and_then(|v| v.as_object())
        .map_or(0u64, |m| m.len() as u64);
    let destroy_count = args
        .get("destroy")
        .and_then(|v| v.as_array())
        .map_or(0u64, |a| a.len() as u64);
    // u64 saturation: each count is bounded by serde_json's body parse
    // (Map::len / Vec::len are usize; on 64-bit targets usize == u64,
    // on 32-bit targets usize < u64). The sum cannot exceed
    // 3 * u32::MAX even on 32-bit hosts, well within u64 range.
    let count = create_count
        .saturating_add(update_count)
        .saturating_add(destroy_count);
    if count > max {
        let mut err = JmapError::request_too_large();
        err.description = Some("maxObjectsInSet".into());
        return Err(err);
    }
    Ok(())
}

/// Enforce RFC 8620 §5.1 `maxObjectsInGet` at the top of a `/get`
/// handler (bd:JMAP-yt1w.31).
///
/// Counts the entries in the wire `ids` argument. Returns the
/// method-level error [`JmapError::request_too_large`] (wire
/// `type: "requestTooLarge"`, `description: "maxObjectsInGet"`) when the
/// count exceeds `max`. Per RFC 8620 §5.1 it is returned instead of the
/// `Foo/get` response.
///
/// Call after the `account_exists` gate and before parsing `ids`:
///
/// ```ignore
/// jmap_server::helpers::enforce_max_objects_in_get(
///     &args,
///     backend.max_objects_in_get(caller, &account_id),
/// )?;
/// ```
///
/// # Counting rules
///
/// `ids` is counted as `args["ids"].as_array()?.len()`. A missing key,
/// `null` (fetch all), or a non-array value counts as 0: RFC 8620 §5.1
/// bounds "the number of ids requested by the client", and a null `ids`
/// requests none. Bounding a fetch-all by the number of records is the
/// backend's job, since only it knows how many records exist. Wire-shape
/// validation of `ids` belongs to the handler's argument parsing.
///
/// # Errors
///
/// Returns `JmapError::request_too_large()` with description
/// `"maxObjectsInGet"` when the count exceeds `max` (RFC 8620 §5.1).
pub fn enforce_max_objects_in_get(args: &Map<String, Value>, max: u64) -> Result<(), JmapError> {
    let count = args
        .get("ids")
        .and_then(|v| v.as_array())
        .map_or(0u64, |a| a.len() as u64);
    if count > max {
        let mut err = JmapError::request_too_large();
        err.description = Some("maxObjectsInGet".into());
        return Err(err);
    }
    Ok(())
}

/// Reject a `/set` create object that carries a DID-bearing property, or
/// an update PatchObject that changes one (draft-atwood-jmap-did-00
/// §The `did` Property).
///
/// `entry` is one value from the `create` or `update` argument and
/// `did_properties` names the data type's DID-bearing properties, which the
/// consuming specification defines. `current` is `None` for a create and
/// the stored object for an update.
///
/// "Clients omit DID-bearing properties when creating objects, as
/// {{RFC8620}} Section 1.1 requires for server-set properties; an update
/// can include one only with its current value ({{RFC8620}} Section 5.3).
/// Servers MUST reject a create that includes a DID-bearing property, or an
/// update that changes one, with a SetError of type `invalidProperties`"
/// (draft-atwood-jmap-did-00 L247). So a create that names the property is
/// rejected whatever the value, even `null`. An update key equal to the
/// property name passes when its value equals the stored value, where an
/// absent stored property counts as `null`, the value's other type. A
/// PatchObject key is a path, and a key whose first segment is the property
/// name but has further segments always changes it, so it is rejected.
/// Keeping the value unchanged on the server side, including across a DID
/// migration, is the backend's job.
///
/// Returns `None` when `entry` is acceptable. Otherwise returns an
/// `invalidProperties` [`SetError`] whose `properties` lists every rejected
/// DID-bearing property, in `did_properties` order.
#[must_use]
pub fn reject_did_bearing_properties(
    entry: &Map<String, Value>,
    did_properties: &[&str],
    current: Option<&Map<String, Value>>,
) -> Option<SetError> {
    // ponytail: compares the raw first segment, without RFC 6901 `~0`/`~1`
    // decoding. That is exact for property names with no `~` or `/`, which
    // covers every DID-bearing name in the drafts.
    let rejected: Vec<&str> = did_properties
        .iter()
        .copied()
        .filter(|prop| {
            entry.iter().any(|(k, v)| {
                k.split('/').next() == Some(*prop)
                    && !(k == prop
                        && current
                            .is_some_and(|stored| stored.get(*prop).unwrap_or(&Value::Null) == v))
            })
        })
        .collect();
    (!rejected.is_empty())
        .then(|| SetError::new(SetErrorType::InvalidProperties).with_properties(rejected))
}

/// Resolve an Id-typed value that may be a creation-id reference
/// (RFC 8620 §5.3).
///
/// A value starting with `#` names a record created earlier in the same
/// request; it resolves to that record's server-assigned id from
/// `created_ids` (pass [`crate::CallContext::created_ids`]). Any other
/// value is a literal id and is returned as is. `None` means a `#`
/// reference with no matching creation; the caller rejects the
/// create/update, typically with an `invalidProperties` SetError naming
/// the property.
///
/// References to creates in the *same* `/set` call are not in
/// `created_ids` yet; the handler orders and resolves those itself
/// (RFC 8620 §5.3).
pub fn resolve_creation_ref(
    raw: &str,
    created_ids: &std::collections::BTreeMap<Id, Id>,
) -> Option<Id> {
    match raw.strip_prefix('#') {
        Some(creation_id) => created_ids.get(&Id::from(creation_id)).cloned(),
        None => Some(Id::from(raw)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_jmap_patch, apply_patch_object, civil_from_days, enforce_max_objects_in_set,
        extract_account_id, filter_arg, json_merge_patch, now_utc_string, parse_max_changes,
        reject_did_bearing_properties, resolve_creation_ref, resolve_query_offset,
        resolve_query_start, utc_date_from_epoch_seconds, MergePatchError, PatchPathError,
        MAX_MERGE_PATCH_DEPTH,
    };
    use jmap_types::Id;
    use serde_json::json;

    /// Oracle: RFC 8620 §5.3 — "the client refers to the new record
    /// using its creation id prefixed with a '#'"; the RFC 8620 §5.3
    /// example request sets `"subTodoIds": [ "#k15" ]` after creating
    /// `k15`. A bare value is an ordinary id.
    #[test]
    fn resolve_creation_ref_maps_hash_refs_and_passes_literals() {
        let created = std::collections::BTreeMap::from([(Id::from("k15"), Id::from("T15"))]);
        assert_eq!(
            resolve_creation_ref("#k15", &created),
            Some(Id::from("T15"))
        );
        assert_eq!(resolve_creation_ref("k15", &created), Some(Id::from("k15")));
        assert_eq!(resolve_creation_ref("#k16", &created), None);
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct NameCond {
        name: Option<String>,
    }

    // draft-atwood-jmap-did-00 L247 (jmap-chat-spec 6bcd9d8): "Clients omit
    // DID-bearing properties when creating objects, as {{RFC8620}} Section
    // 1.1 requires for server-set properties; an update can include one only
    // with its current value ({{RFC8620}} Section 5.3). Servers MUST reject a
    // create that includes a DID-bearing property, or an update that changes
    // one, with a SetError of type `invalidProperties`, listing the
    // DID-bearing property name in the `properties` array."
    fn did_entry(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().cloned().expect("object")
    }

    fn invalid_did() -> serde_json::Value {
        json!({ "type": "invalidProperties", "properties": ["did"] })
    }

    #[test]
    fn did_create_with_did_property_is_invalid_properties() {
        let entry = did_entry(json!({
            "name": "x",
            "did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
        }));
        let err = reject_did_bearing_properties(&entry, &["did"], None).expect("L247: MUST reject");
        assert_eq!(
            serde_json::to_value(&err).expect("serialize"),
            invalid_did()
        );
    }

    #[test]
    fn did_create_with_null_did_is_still_rejected() {
        // L247: "a create that includes a DID-bearing property"; null is included.
        let entry = did_entry(json!({ "did": null }));
        assert!(reject_did_bearing_properties(&entry, &["did"], None).is_some());
    }

    #[test]
    fn did_lists_every_did_bearing_property_named() {
        // L247 rejects each included DID-bearing property. The two names
        // here are arbitrary DID-bearing names the caller declares; the
        // `properties` array follows `did_properties` order, not key order.
        let entry = did_entry(json!({
            "ownerDid": "did:web:example.com",
            "did": "did:web:example.org"
        }));
        let err = reject_did_bearing_properties(&entry, &["did", "ownerDid"], None)
            .expect("L247: MUST reject");
        assert_eq!(
            err.properties,
            Some(vec!["did".to_owned(), "ownerDid".to_owned()])
        );
    }

    #[test]
    fn did_update_with_current_value_passes() {
        // L247: "an update can include one only with its current value".
        let stored = did_entry(json!({ "id": "a", "did": "did:web:example.com" }));
        let entry = did_entry(json!({ "name": "y", "did": "did:web:example.com" }));
        assert_eq!(
            reject_did_bearing_properties(&entry, &["did"], Some(&stored)),
            None
        );

        let stored_null = did_entry(json!({ "id": "a", "did": null }));
        let entry_null = did_entry(json!({ "did": null }));
        assert_eq!(
            reject_did_bearing_properties(&entry_null, &["did"], Some(&stored_null)),
            None
        );
    }

    #[test]
    fn did_update_with_null_over_absent_did_passes() {
        // L247: "an update can include one only with its current value".
        // RFC 8620 §5.3: "null ... to remove it (or reset it to its
        // default value)", so an absent property's current value is null.
        let stored = did_entry(json!({ "id": "a" }));
        let entry = did_entry(json!({ "did": null }));
        assert_eq!(
            reject_did_bearing_properties(&entry, &["did"], Some(&stored)),
            None
        );
    }

    #[test]
    fn did_update_that_changes_did_is_rejected() {
        // L247: "Servers MUST reject ... an update that changes one". A
        // different value, null over a stored DID, and a path under the
        // property ("did/x", RFC 8620 §5.3 PatchObject path) all change it.
        let stored = did_entry(json!({ "id": "a", "did": "did:web:example.com" }));
        for entry in [
            did_entry(json!({ "did": "did:web:example.org" })),
            did_entry(json!({ "did": null })),
            did_entry(json!({ "did/x": 1 })),
        ] {
            let err = reject_did_bearing_properties(&entry, &["did"], Some(&stored))
                .expect("L247: update that changes one");
            assert_eq!(
                serde_json::to_value(&err).expect("serialize"),
                invalid_did()
            );
        }
    }

    #[test]
    fn did_entry_without_did_bearing_property_passes() {
        // A property whose name merely starts with "did" is not "did".
        let entry = did_entry(json!({ "name": "x", "didx": 1, "keywords/did": true }));
        assert_eq!(reject_did_bearing_properties(&entry, &["did"], None), None);
    }

    fn filter_of(v: serde_json::Value) -> Result<Option<NameCond>, jmap_types::JmapError> {
        let mut args = json!({ "filter": v }).as_object().cloned().expect("object");
        filter_arg::<NameCond>(&mut args)
    }

    /// Oracle: RFC 8620 §5.5 — "unsupportedFilter: The "filter" is
    /// syntactically valid, but the server cannot process it"; RFC 8620
    /// §3.6.2 — invalidArguments for an argument of the wrong type.
    /// Hand-written JSON inputs.
    #[test]
    fn filter_arg_maps_object_and_non_object_failures() {
        assert!(filter_of(json!(null)).expect("null").is_none());
        let cond = filter_of(json!({ "name": "x" })).expect("known key");
        assert_eq!(cond.and_then(|c| c.name).as_deref(), Some("x"));
        let err = filter_of(json!({ "label": "x" })).expect_err("unknown key");
        assert_eq!(err.error_type.to_string(), "unsupportedFilter");
        assert!(
            err.description
                .as_deref()
                .is_some_and(|d| d.contains("label")),
            "{err:?}"
        );
        let err = filter_of(json!(42)).expect_err("scalar");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
    }

    /// Oracle (bd:JMAP-wlip.5): a malformed accountId — empty string,
    /// containing forbidden ASCII characters, or exceeding 255 bytes —
    /// MUST surface as `invalidArguments`, not silently pass through to
    /// the backend's `account_exists` call.
    ///
    /// Test vectors hand-built from RFC 8620 §1.2's Id grammar
    /// (SAFE-CHAR = `%x21 / %x23-7E`).
    #[test]
    fn extract_account_id_rejects_malformed_id() {
        // Empty string.
        let err = extract_account_id(json!({ "accountId": "" }))
            .expect_err("empty accountId must fail validation");
        assert_eq!(err.error_type.to_string(), "invalidArguments");

        // Contains a space (0x20 — outside SAFE-CHAR's 0x21+ lower bound).
        let err = extract_account_id(json!({ "accountId": "my account" }))
            .expect_err("space in accountId must fail validation");
        assert_eq!(err.error_type.to_string(), "invalidArguments");

        // Contains a DQUOTE (0x22 — explicitly excluded by SAFE-CHAR).
        let err = extract_account_id(json!({ "accountId": "a\"b" }))
            .expect_err("DQUOTE in accountId must fail validation");
        assert_eq!(err.error_type.to_string(), "invalidArguments");

        // 256 bytes — exceeds the 255 cap.
        let long: String = "a".repeat(256);
        let err = extract_account_id(json!({ "accountId": long }))
            .expect_err("over-long accountId must fail validation");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
    }

    /// Oracle (bd:JMAP-wlip.5): a well-formed accountId passes
    /// validation and is returned intact. Positive control paired with
    /// the rejection test above.
    ///
    /// Also pins (bd:JMAP-jfia.9): the returned args map MUST NOT
    /// contain accountId. The helper consumes the field rather than
    /// leaving it as a residual key for downstream handlers to
    /// re-parse or surface as "unexpected key".
    #[test]
    fn extract_account_id_accepts_well_formed_id() {
        let (id, rest) = extract_account_id(json!({
            "accountId": "u123-abc_DEF",
            "ids": ["e1", "e2"]
        }))
        .expect("well-formed accountId must pass validation");
        assert_eq!(id.as_ref(), "u123-abc_DEF");
        // Remaining args still contain the unrelated keys.
        assert!(rest.contains_key("ids"));
        // accountId MUST have been removed from the args map
        // (bd:JMAP-jfia.9). matches optional_arg's consume semantics.
        assert!(
            !rest.contains_key("accountId"),
            "accountId must be removed from args after extraction"
        );
    }

    /// Test vectors derived independently with Python's `datetime.date` module.
    /// `days` is the count of days since 1970-01-01.
    #[test]
    fn civil_from_days_known_dates() {
        let cases: &[(i64, (i32, u8, u8))] = &[
            (0, (1970, 1, 1)),       // Unix epoch
            (365, (1971, 1, 1)),     // one year later (1970 is not a leap year)
            (10957, (2000, 1, 1)),   // Y2K
            (11016, (2000, 2, 29)),  // leap day in a century-divisible leap year
            (11017, (2000, 3, 1)),   // day after the leap day (era boundary in algorithm)
            (19358, (2023, 1, 1)),   // a recent non-leap year start
            (19722, (2023, 12, 31)), // end of 2023
            (19782, (2024, 2, 29)),  // leap day in 2024
            (19783, (2024, 3, 1)),   // day after 2024 leap day
        ];

        for &(days, expected) in cases {
            assert_eq!(
                civil_from_days(days),
                Some(expected),
                "civil_from_days({days}) mismatch"
            );
        }
    }

    /// Oracle (bd:JMAP-jfia.2): civil_from_days MUST return None rather
    /// than panic on inputs whose computed year overflows i32. The
    /// Hinnant algorithm's intermediate `y = yoe + era * 400` value is
    /// bounded by `i64::MAX / 146_097 ≈ ±6.3e13`, only a thin slice of
    /// which fits in i32. The outer `now_utc_string_checked`
    /// u64→i64 sentinel only catches u64 seconds exceeding `i64::MAX`,
    /// so corrupted-clock inputs between the i32-year boundary and
    /// i64::MAX reach this function. A panic here would surface as
    /// serverFail under dispatcher task::spawn isolation — degraded,
    /// but a contract violation versus the function's "fallible
    /// without panicking" contract.
    ///
    /// Test vectors: the maximum days-from-epoch derived from
    /// i64::MAX seconds (the regime that reaches civil_from_days
    /// after passing the outer u64→i64 sentinel), and the symmetric
    /// negative case. These years are deep enough into the i64
    /// algorithm range to definitely overflow i32. Plus a
    /// non-overflowing positive control just below the threshold to
    /// prove the boundary check fires only when warranted.
    #[test]
    fn civil_from_days_returns_none_on_year_overflow() {
        // i64::MAX / 86400 ≈ 1.07e14 days → year ≈ 2.92e11, well past i32::MAX (~2.15e9).
        let max_days = i64::MAX / 86400;
        assert_eq!(
            civil_from_days(max_days),
            None,
            "i64::MAX / 86400 days must overflow i32 year"
        );

        // i64::MIN / 86400 ≈ -1.07e14 days — symmetric negative case.
        let min_days = i64::MIN / 86400;
        assert_eq!(
            civil_from_days(min_days),
            None,
            "i64::MIN / 86400 days must overflow i32 year"
        );

        // Positive control: a far-future but i32-fitting year should
        // still succeed. Year ~58_798_075 (from 10 * i32::MAX days)
        // fits in i32 with room to spare, so this MUST return Some.
        let year_58m_days = 10_i64 * i64::from(i32::MAX);
        let result = civil_from_days(year_58m_days);
        assert!(
            result.is_some(),
            "i32-fitting year must return Some; got {result:?}"
        );
    }

    /// Oracle (bd:JMAP-jfia.30): now_utc_string_checked MUST return
    /// `Some(UTCDate)` on a sane clock (every host the test runs on
    /// in practice). The civil_from_days_returns_none_on_year_overflow
    /// test above pins the underlying None-on-corrupted-clock
    /// behaviour at the algorithm level; this test pins the
    /// happy-path contract: a sane clock yields the typed wire format,
    /// not a sentinel string.
    #[test]
    fn now_utc_string_checked_returns_some_on_sane_clock() {
        use super::now_utc_string_checked;
        let dt = now_utc_string_checked().expect(
            "test host clock must be reasonable enough for now_utc_string_checked to succeed",
        );
        let s: &str = dt.as_ref();
        assert_eq!(s.len(), 24, "wire shape must be 24 chars: {s:?}");
        assert!(s.ends_with('Z'), "must end with Z: {s:?}");
    }

    /// Oracle (bd:JMAP-jfia.30): now_utc_string (the panicking variant)
    /// MUST agree with now_utc_string_checked on a sane clock — i.e.
    /// the .expect() in now_utc_string does not introduce a wire-format
    /// discrepancy versus the Option-returning variant.
    #[test]
    fn now_utc_string_matches_checked_variant_on_sane_clock() {
        use super::now_utc_string_checked;
        let panicky = now_utc_string();
        let checked = now_utc_string_checked().expect("test host clock must be reasonable");
        // Both calls observe SystemTime::now() at slightly different
        // instants; the seconds part can differ if the test runs across
        // a second boundary. Compare the prefix up to the seconds
        // resolution.
        let panicky_s: &str = panicky.as_ref();
        let checked_s: &str = checked.as_ref();
        assert_eq!(
            panicky_s.len(),
            checked_s.len(),
            "wire-format lengths must match: panicky={panicky_s:?} checked={checked_s:?}"
        );
        // Compare the date portion only (YYYY-MM-DD, 10 chars).
        assert_eq!(
            &panicky_s[..10],
            &checked_s[..10],
            "date portions must match: panicky={panicky_s:?} checked={checked_s:?}"
        );
    }

    /// Oracle: GNU `date -u -d @<secs> +%FT%TZ` (coreutils), plus the
    /// RFC 8620 §7.2.3 example expiry (rfc8620.txt:3879) and the RFC 8620
    /// §1.4 example date. -62167219200 is 0000-01-01T00:00:00Z; one second
    /// earlier is year -1, which the four-digit form cannot express.
    #[test]
    fn utc_date_from_epoch_seconds_known_values() {
        for (secs, expected) in [
            (0, Some("1970-01-01T00:00:00Z")),
            (-1, Some("1969-12-31T23:59:59Z")),
            (1_414_649_520, Some("2014-10-30T06:12:00Z")),
            (1_531_448_069, Some("2018-07-13T02:14:29Z")),
            (253_402_300_799, Some("9999-12-31T23:59:59Z")),
            (253_402_300_800, None),
            (-62_167_219_200, Some("0000-01-01T00:00:00Z")),
            (-62_167_219_201, None),
        ] {
            assert_eq!(
                utc_date_from_epoch_seconds(secs)
                    .as_ref()
                    .map(AsRef::as_ref),
                expected,
                "secs = {secs}"
            );
        }
    }

    #[test]
    fn now_utc_string_format() {
        // bd:JMAP-wlip.20 — return type is UTCDate; AsRef<str> gives
        // the underlying wire-format string for shape assertions.
        let dt = now_utc_string();
        let s: &str = dt.as_ref();
        // Must match YYYY-MM-DDTHH:MM:SS.mmmZ (24 chars)
        assert_eq!(s.len(), 24, "unexpected length: {s}");
        assert!(s.ends_with('Z'), "must end with Z: {s}");
        assert_eq!(&s[4..5], "-", "missing year-month separator: {s}");
        assert_eq!(&s[7..8], "-", "missing month-day separator: {s}");
        assert_eq!(&s[10..11], "T", "missing date-time separator: {s}");
        assert_eq!(&s[13..14], ":", "missing hour-minute separator: {s}");
        assert_eq!(&s[16..17], ":", "missing minute-second separator: {s}");
        assert_eq!(&s[19..20], ".", "missing decimal point before millis: {s}");
        // milliseconds are 3 decimal digits
        assert!(
            s[20..23].chars().all(|c| c.is_ascii_digit()),
            "milliseconds must be 3 digits: {s}"
        );
        assert!(
            s.starts_with("20"),
            "year should start with 20 in 21st century: {s}"
        );
    }

    // -----------------------------------------------------------------------
    // json_merge_patch (RFC 7396)
    //
    // Test oracles are hand-built JSON values derived from RFC 7396 §2 and §3
    // examples, plus the regression case from bd:JMAP-sc1b.87. No oracle is
    // computed by the function under test (test-integrity rule from
    // workspace AGENTS.md).
    // -----------------------------------------------------------------------

    /// Oracle: bd:JMAP-sc1b.97 — a 1000-deep merge patch must NOT crash
    /// via stack overflow. The depth cap returns
    /// [`MergePatchError::DepthExceeded`] beyond [`MAX_MERGE_PATCH_DEPTH`]
    /// rather than recursing further.
    ///
    /// The test does not use the function as its own oracle: the input
    /// is hand-built (a 1000-deep `{ "a": { "a": ... { "a": {} } } }`
    /// chain where every level is Object, matching the structural
    /// shape of a real PatchObject — the documented latent panic from
    /// bd:JMAP-sc1b.87 only fires on non-Object leaves, which a typed
    /// PatchObject cannot produce). The assertion checks that the
    /// call completes without panicking AND that it surfaces the
    /// depth-exceeded error rather than silently succeeding (the
    /// pre-bd:JMAP-wlip.1 silent-truncation bug).
    #[test]
    fn json_merge_patch_does_not_stack_overflow() {
        const DEPTH: usize = 1000;
        let mut target = serde_json::json!({});
        for _ in 0..DEPTH {
            target = serde_json::json!({ "a": target });
        }
        let mut patch = serde_json::json!({});
        for _ in 0..DEPTH {
            patch = serde_json::json!({ "a": patch });
        }
        let err = json_merge_patch(&mut target, patch)
            .expect_err("deep patch must surface DepthExceeded, not silently truncate");
        assert_eq!(
            err,
            MergePatchError::DepthExceeded,
            "deep patch must return MergePatchError::DepthExceeded"
        );
    }

    /// Oracle: bd:JMAP-wlip.1 — a patch at exactly [`MAX_MERGE_PATCH_DEPTH`]
    /// levels (the deepest legal patch) MUST apply successfully. The cap
    /// fires only when the patch tries to recurse one level beyond. The
    /// expected target shape is hand-built level-by-level from the same
    /// counter the patch uses, so the oracle is independent of the
    /// recursion under test.
    #[test]
    fn json_merge_patch_at_exact_cap_applies() {
        // Build a patch nested exactly MAX_MERGE_PATCH_DEPTH levels deep.
        // Outermost level is depth=1; innermost leaf-Object is at depth
        // MAX_MERGE_PATCH_DEPTH. The first depth-cap check fires at
        // depth == MAX_MERGE_PATCH_DEPTH + 1, so this is the deepest
        // patch that still applies.
        let mut patch = serde_json::json!({ "leaf": "value" });
        for _ in 0..(MAX_MERGE_PATCH_DEPTH - 1) {
            patch = serde_json::json!({ "a": patch });
        }
        let mut target = serde_json::json!({});
        json_merge_patch(&mut target, patch).expect("patch at the cap must apply");
        // Walk the resulting target down its 'a' chain to verify the
        // leaf field landed.
        let mut cursor = &target;
        for _ in 0..(MAX_MERGE_PATCH_DEPTH - 1) {
            cursor = cursor.get("a").expect("each level must have 'a'");
        }
        assert_eq!(
            cursor.get("leaf"),
            Some(&serde_json::Value::String("value".to_owned())),
            "the leaf field at exactly MAX_MERGE_PATCH_DEPTH must be applied"
        );
    }

    /// Oracle: a shallow merge patch under the cap still applies
    /// normally. Positive control paired with the stack-overflow test
    /// above to prove the depth cap only fires at the boundary, not on
    /// every call.
    #[test]
    fn json_merge_patch_shallow_applies_normally() {
        let mut target = serde_json::json!({ "a": 1, "b": { "c": 2 } });
        let patch = serde_json::json!({ "b": { "c": 99, "d": 7 }, "e": null });
        json_merge_patch(&mut target, patch).expect("shallow patch must succeed");
        assert_eq!(
            target,
            serde_json::json!({ "a": 1, "b": { "c": 99, "d": 7 } }),
            "RFC 7396 merge semantics broken at shallow depth"
        );
    }

    /// Regression: a Patch that adds a nested Object to a previously-
    /// absent field used to panic with `expect("merge patch target
    /// must be an object")` because the parent recursion frame
    /// inserted Value::Null as the placeholder, then recursed into
    /// Null with an Object patch.
    ///
    /// Per RFC 7396 §2 the correct behaviour is to reset the non-Object
    /// target to an empty Object and merge into it. Oracle is hand-
    /// derived from RFC 7396 §2's pseudocode:
    /// `Target[Name] = MergePatch(Target[Name], Value)` where
    /// MergePatch resets a non-Object target to `{}`.
    #[test]
    fn json_merge_patch_adds_nested_object_to_absent_field() {
        let mut target = serde_json::json!({ "a": 1 });
        let patch = serde_json::json!({ "b": { "c": 2 } });
        json_merge_patch(&mut target, patch).expect("nested-add patch must succeed");
        assert_eq!(
            target,
            serde_json::json!({ "a": 1, "b": { "c": 2 } }),
            "patch must add the nested object at the previously-absent field"
        );
    }

    /// Oracle: [`MergePatchError`] implements [`std::error::Error`] and
    /// has a stable Display form referencing the cap value. Pinning the
    /// Display string keeps the error message stable across refactors;
    /// the cap value is interpolated from the public constant so this
    /// test does not need updating if the cap changes.
    #[test]
    fn merge_patch_error_display() {
        let err = MergePatchError::DepthExceeded;
        let s = err.to_string();
        assert!(
            s.contains(&MAX_MERGE_PATCH_DEPTH.to_string()),
            "Display must mention the cap value; got {s:?}"
        );
        assert!(
            s.contains("merge patch"),
            "Display must identify the error source; got {s:?}"
        );
    }

    // ------------------------------------------------------------------
    // apply_jmap_patch (RFC 8620 §5.3 PatchObject paths, RFC 6901)
    //
    // Oracles are hand-built JSON values derived from the quoted RFC
    // text; the function under test is never its own oracle.
    // ------------------------------------------------------------------

    /// Build a `Map` from a JSON object literal.
    fn obj(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        match v {
            serde_json::Value::Object(m) => m,
            other => panic!("fixture must be a JSON object, got {other}"),
        }
    }

    /// Apply `path` → `value` to `before` and assert it fails with `want`
    /// and leaves the object unchanged.
    fn assert_patch_rejected(
        before: serde_json::Value,
        path: &str,
        value: serde_json::Value,
        want: PatchPathError,
    ) {
        let mut target = obj(before.clone());
        let err = apply_jmap_patch(&mut target, path, value).expect_err("patch must be rejected");
        assert_eq!(err, want, "wrong error for path {path:?}");
        assert_eq!(
            serde_json::Value::Object(target),
            before,
            "rejected patch {path:?} must leave the object unchanged"
        );
    }

    /// Oracle: RFC 8620 §5.3 — "Anything else: The value to set for this
    /// property (this may be a replacement or addition to the object
    /// being patched)."
    #[test]
    fn apply_jmap_patch_non_null_adds_or_replaces() {
        let mut target = obj(json!({ "a": 1 }));
        apply_jmap_patch(&mut target, "a", json!(2)).expect("replace");
        apply_jmap_patch(&mut target, "b", json!("x")).expect("add");
        assert_eq!(
            serde_json::Value::Object(target),
            json!({ "a": 2, "b": "x" })
        );
    }

    /// Oracle: RFC 8620 §5.3 — "If null, ... remove the property from the
    /// patched object.  If the key is not present in the parent, this a
    /// no-op."
    #[test]
    fn apply_jmap_patch_null_removes_and_absent_is_noop() {
        let mut target = obj(json!({ "a": 1, "m": { "k": true } }));
        apply_jmap_patch(&mut target, "a", serde_json::Value::Null).expect("remove");
        apply_jmap_patch(&mut target, "m/k", serde_json::Value::Null).expect("nested remove");
        apply_jmap_patch(&mut target, "m/absent", serde_json::Value::Null).expect("no-op");
        assert_eq!(serde_json::Value::Object(target), json!({ "m": {} }));
    }

    /// Oracle: RFC 8620 §5.3 — "The keys are a path in JSON Pointer
    /// format [RFC6901], with an implicit leading "/"". `mailboxIds/abc`
    /// sets member `abc` of the existing `mailboxIds` object and leaves
    /// its siblings alone.
    #[test]
    fn apply_jmap_patch_nested_path_sets_member_of_existing_object() {
        let mut target = obj(json!({ "mailboxIds": { "inbox": true }, "x": { "y": { "z": 1 } } }));
        apply_jmap_patch(&mut target, "mailboxIds/abc", json!(true)).expect("set");
        apply_jmap_patch(&mut target, "x/y/w", json!(2)).expect("deep set");
        assert_eq!(
            serde_json::Value::Object(target),
            json!({ "mailboxIds": { "inbox": true, "abc": true }, "x": { "y": { "z": 1, "w": 2 } } })
        );
    }

    /// Oracle: RFC 8620 §5.3 — "All parts prior to the last (i.e., the
    /// value after the final slash) MUST already exist on the object
    /// being patched." A missing parent is rejected, never created, for
    /// both non-null and null values.
    #[test]
    fn apply_jmap_patch_missing_parent_is_rejected() {
        let before = json!({ "a": { "b": {} } });
        for value in [json!("v"), serde_json::Value::Null] {
            assert_patch_rejected(
                before.clone(),
                "x/y",
                value.clone(),
                PatchPathError::ParentMissing,
            );
            assert_patch_rejected(
                before.clone(),
                "a/c/d",
                value,
                PatchPathError::ParentMissing,
            );
        }
    }

    /// Oracle: RFC 8620 §5.3 — "All parts prior to the last ... MUST
    /// already exist on the object being patched." A string, number,
    /// boolean or null has no members, so a part beneath one does not
    /// exist; the existing value must not be clobbered.
    #[test]
    fn apply_jmap_patch_non_object_parent_is_rejected() {
        let before = json!({ "s": "text", "n": 42, "b": true, "z": null });
        for parent in ["s", "n", "b", "z"] {
            assert_patch_rejected(
                before.clone(),
                &format!("{parent}/sub"),
                json!("would-clobber"),
                PatchPathError::ParentNotObject,
            );
        }
    }

    /// Oracle: RFC 8620 §5.3 — "The pointer MUST NOT reference inside an
    /// array (i.e., you MUST NOT insert/delete from an array; the array
    /// MUST be replaced in its entirety instead)." Applies to numeric
    /// tokens and to paths that would descend through an array element.
    #[test]
    fn apply_jmap_patch_inside_array_is_rejected() {
        let before = json!({ "list": ["a", { "k": 1 }] });
        assert_patch_rejected(
            before.clone(),
            "list/0",
            json!("x"),
            PatchPathError::InsideArray,
        );
        assert_patch_rejected(
            before.clone(),
            "list/1/k",
            json!(2),
            PatchPathError::InsideArray,
        );
        assert_patch_rejected(
            before,
            "list/0",
            serde_json::Value::Null,
            PatchPathError::InsideArray,
        );
    }

    /// Oracle: RFC 8620 §5.3 — "the array MUST be replaced in its entirety
    /// instead": setting the array-valued property itself is allowed.
    #[test]
    fn apply_jmap_patch_replaces_whole_array() {
        let mut target = obj(json!({ "list": ["a"] }));
        apply_jmap_patch(&mut target, "list", json!(["b", "c"])).expect("replace array");
        assert_eq!(
            serde_json::Value::Object(target),
            json!({ "list": ["b", "c"] })
        );
    }

    /// Oracle: RFC 6901 §4 — "first transforming any occurrence of the
    /// sequence '~1' to '/', and then transforming any occurrence of the
    /// sequence '~0' to '~' ... (the string '~01' correctly becomes '~1'
    /// after transformation)." Decoding is per token, after splitting on
    /// `/`, so `a~1b/c` addresses member `c` of member `a/b`.
    #[test]
    fn apply_jmap_patch_decodes_rfc6901_escapes() {
        let mut target = obj(json!({ "a/b": {}, "m~n": {} }));
        apply_jmap_patch(&mut target, "foo~1bar", json!(1)).expect("~1");
        apply_jmap_patch(&mut target, "foo~0bar", json!(2)).expect("~0");
        apply_jmap_patch(&mut target, "~01", json!(3)).expect("~01");
        apply_jmap_patch(&mut target, "a~1b/c", json!(4)).expect("escaped parent");
        apply_jmap_patch(&mut target, "m~0n/~1", json!(5)).expect("escaped parent and leaf");
        assert_eq!(
            serde_json::Value::Object(target),
            json!({
                "foo/bar": 1,
                "foo~bar": 2,
                "~1": 3,
                "a/b": { "c": 4 },
                "m~n": { "/": 5 }
            })
        );
    }

    /// Oracle: RFC 6901 §3 — `escaped = "~" ( "0" / "1" )` and "It is an
    /// error condition if a JSON Pointer value does not conform to this
    /// syntax". RFC 8620 §5.3 requires keys in JSON Pointer format, so a
    /// malformed escape in any token is rejected.
    #[test]
    fn apply_jmap_patch_invalid_escape_is_rejected() {
        let before = json!({ "a": {} });
        for path in ["~2", "x~", "a/~", "~x/b", "a/b~"] {
            assert_patch_rejected(
                before.clone(),
                path,
                json!(1),
                PatchPathError::InvalidEscape,
            );
        }
    }

    /// Oracle: RFC 6901 §3 — `reference-token = *( unescaped / escaped )`
    /// permits an empty token, and the §5 example evaluates `"/"` to the
    /// member named `""` (`"" : 0`). The PatchObject key `""` is the
    /// pointer `"/"`; `"a/"` is the pointer `"/a/"`.
    #[test]
    fn apply_jmap_patch_empty_token_names_empty_member() {
        let mut target = obj(json!({ "a": {} }));
        apply_jmap_patch(&mut target, "", json!(0)).expect("empty top-level token");
        apply_jmap_patch(&mut target, "a/", json!(1)).expect("empty nested token");
        assert_eq!(
            serde_json::Value::Object(target),
            json!({ "": 0, "a": { "": 1 } })
        );
    }

    /// Oracle: an empty token is an ordinary member name, so a missing
    /// empty-named parent is rejected like any other missing parent
    /// (RFC 8620 §5.3 "All parts prior to the last ... MUST already
    /// exist").
    #[test]
    fn apply_jmap_patch_missing_empty_parent_is_rejected() {
        assert_patch_rejected(
            json!({ "a": {} }),
            "a//b",
            json!(1),
            PatchPathError::ParentMissing,
        );
    }

    // ------------------------------------------------------------------
    // apply_patch_object (RFC 8620 §5.3 whole PatchObject)
    // ------------------------------------------------------------------

    /// Oracle: RFC 8620 §5.3 — "There MUST NOT be two patches in the
    /// PatchObject where the pointer of one is the prefix of the pointer
    /// of the other, e.g., "alerts/1/offset" and "alerts""; on violation
    /// "the update MUST be rejected with an "invalidPatch" error".
    #[test]
    fn apply_patch_object_prefix_conflict_is_rejected() {
        let before = json!({ "alerts": { "1": { "offset": 0 } } });
        let mut target = obj(before.clone());
        let patch = obj(json!({ "alerts/1/offset": 5, "alerts": {} }));
        let err = apply_patch_object(&mut target, &patch).expect_err("must be rejected");
        assert_eq!(err, PatchPathError::PrefixConflict);
        assert_eq!(serde_json::Value::Object(target), before);
    }

    /// Oracle: RFC 8620 §5.3 forbids only a pointer that is "the prefix of
    /// the pointer of the other"; JSON Pointers are compared by reference
    /// token (RFC 6901 §3), so "alert" is not a prefix of "alerts/x".
    #[test]
    fn apply_patch_object_near_prefix_is_accepted() {
        let mut target = obj(json!({ "alerts": {} }));
        let patch = obj(json!({ "alert": 1, "alerts/x": 2 }));
        apply_patch_object(&mut target, &patch).expect("no prefix conflict");
        assert_eq!(
            serde_json::Value::Object(target),
            json!({ "alert": 1, "alerts": { "x": 2 } })
        );
    }

    /// Oracle: RFC 8620 §5.3 — "if there is any violation, the update MUST
    /// be rejected"; the valid entry "a" must not be applied either.
    #[test]
    fn apply_patch_object_partial_failure_leaves_target_unchanged() {
        let before = json!({ "a": 1 });
        let mut target = obj(before.clone());
        let patch = obj(json!({ "a": 2, "m/k": 1 }));
        let err = apply_patch_object(&mut target, &patch).expect_err("must be rejected");
        assert_eq!(err, PatchPathError::ParentMissing);
        assert_eq!(serde_json::Value::Object(target), before);
    }

    /// A value nesting `levels` objects deep (`0` is the scalar `1`).
    fn nested(levels: usize) -> serde_json::Value {
        (0..levels).fold(json!(1), |inner, _| json!({ "k": inner }))
    }

    /// Oracle: this crate's DoS guard, not an RFC 8620 §5.3 rule (RFC 8620
    /// §3.6.1 lets a server reject requests beyond its limits). Path
    /// segments plus value nesting may not exceed `MAX_MERGE_PATCH_DEPTH`
    /// (32): 3 + 30 = 33 is rejected before any entry is applied.
    #[test]
    fn apply_patch_object_path_plus_value_over_depth_cap_is_rejected() {
        let before = json!({ "x": 1, "a": { "b": {} } });
        let mut target = obj(before.clone());
        let patch = obj(json!({ "x": 2, "a/b/c": nested(30) }));
        let err = apply_patch_object(&mut target, &patch).expect_err("must be rejected");
        assert_eq!(err, PatchPathError::DepthExceeded);
        assert_eq!(serde_json::Value::Object(target), before);
    }

    /// Oracle: the crate's DoS guard rejects only depth beyond
    /// `MAX_MERGE_PATCH_DEPTH`; exactly 3 segments + 29 levels = 32 applies.
    #[test]
    fn apply_patch_object_path_plus_value_at_depth_cap_applies() {
        let mut target = obj(json!({ "a": { "b": {} } }));
        let patch = obj(json!({ "a/b/c": nested(29) }));
        apply_patch_object(&mut target, &patch).expect("at the cap must apply");
        assert_eq!(
            serde_json::Value::Object(target),
            json!({ "a": { "b": { "c": nested(29) } } })
        );
    }

    /// Oracle: the crate's DoS guard; a key of 33 segments exceeds
    /// `MAX_MERGE_PATCH_DEPTH` even with a scalar value.
    #[test]
    fn apply_patch_object_path_over_depth_cap_is_rejected() {
        let before = json!({ "a": {} });
        let mut target = obj(before.clone());
        let path = format!("a{}", "/k".repeat(32));
        let patch = obj(json!({ path: 1 }));
        let err = apply_patch_object(&mut target, &patch).expect_err("must be rejected");
        assert_eq!(err, PatchPathError::DepthExceeded);
        assert_eq!(serde_json::Value::Object(target), before);
    }

    // ------------------------------------------------------------------
    // enforce_max_objects_in_set (bd:JMAP-ayoz.41.1) — RFC 8620 §5.3
    //
    // Oracles for all tests below are hand-built JmapError comparisons
    // and hand-built JSON args; the helper itself is never the oracle.
    // ------------------------------------------------------------------

    /// Oracle (RFC 8620 §5.3): an empty `/set` args envelope (no
    /// `create` / `update` / `destroy` keys) MUST pass any positive cap.
    #[test]
    fn enforce_max_objects_in_set_empty_args_is_ok() {
        let args = json!({}).as_object().unwrap().clone();
        enforce_max_objects_in_set(&args, 500).expect("empty args must be under any positive cap");
    }

    /// Oracle (RFC 8620 §5.3): at the exact boundary count == max, the
    /// helper MUST return Ok. The cap is enforced as strictly-greater
    /// (`count > max`), not greater-or-equal.
    #[test]
    fn enforce_max_objects_in_set_at_limit_is_ok() {
        // 5 + 5 + 5 = 15 entries against max=15 must pass.
        let mut create = serde_json::Map::new();
        let mut update = serde_json::Map::new();
        let mut destroy = Vec::new();
        for i in 0..5 {
            create.insert(format!("c{i}"), json!({}));
            update.insert(format!("u{i}"), json!({}));
            destroy.push(json!(format!("d{i}")));
        }
        let args = json!({
            "create": create,
            "update": update,
            "destroy": destroy,
        })
        .as_object()
        .unwrap()
        .clone();
        enforce_max_objects_in_set(&args, 15).expect("exact-boundary count must be allowed");
    }

    /// Oracle (RFC 8620 §5.3, bd:JMAP-yt1w.30): one entry over the cap
    /// MUST return the method error "requestTooLarge" — "The total number
    /// of objects to create, update, or destroy exceeds the maximum number
    /// the server is willing to process in a single method call." — with
    /// description "maxObjectsInSet". The wire shape is verified by
    /// hand-building the expected JmapError independently of the helper.
    #[test]
    fn enforce_max_objects_in_set_over_limit_returns_limit_error() {
        // 5 + 5 + 6 = 16 entries against max=15 must fail.
        let mut create = serde_json::Map::new();
        let mut update = serde_json::Map::new();
        let mut destroy = Vec::new();
        for i in 0..5 {
            create.insert(format!("c{i}"), json!({}));
            update.insert(format!("u{i}"), json!({}));
        }
        for i in 0..6 {
            destroy.push(json!(format!("d{i}")));
        }
        let args = json!({
            "create": create,
            "update": update,
            "destroy": destroy,
        })
        .as_object()
        .unwrap()
        .clone();
        let err = enforce_max_objects_in_set(&args, 15)
            .expect_err("16-entry args against max=15 must fail");
        // Independent oracle — hand-built JmapError, not derived from
        // the function under test.
        let expected = jmap_types::JmapError::request_too_large();
        assert_eq!(
            err.error_type.to_string(),
            expected.error_type.to_string(),
            "type must be \"requestTooLarge\""
        );
        assert_eq!(
            err.description.as_deref(),
            Some("maxObjectsInSet"),
            "description must name the exceeded cap"
        );
    }

    /// Oracle (bd:JMAP-yt1w.30): RFC 8620 §5.3 (rfc8620.txt L2166-2170):
    /// "The following additional errors may be returned instead of the
    /// "Foo/set" response: "requestTooLarge": The total number of objects
    /// to create, update, or destroy exceeds the maximum number the server
    /// is willing to process in a single method call." Exceeding
    /// maxObjectsInSet is therefore the method-level error
    /// `requestTooLarge`, not the request-level `limit` error.
    #[test]
    fn enforce_max_objects_in_set_over_limit_returns_request_too_large() {
        let mut create = serde_json::Map::new();
        for i in 0..3 {
            create.insert(format!("c{i}"), json!({}));
        }
        let args = json!({
            "create": create,
            "destroy": ["d0"],
        })
        .as_object()
        .unwrap()
        .clone();
        let err =
            enforce_max_objects_in_set(&args, 3).expect_err("4-entry args against max=3 must fail");
        assert_eq!(
            err.error_type.to_string(),
            "requestTooLarge",
            "RFC 8620 §5.3: over-cap /set must yield method error requestTooLarge"
        );
    }

    /// Oracle (RFC 8620 §5.3): a non-object `create` argument (e.g.
    /// `null` or a string) counts as 0 — wire-shape validation belongs
    /// to the per-handler argument parsing, not this cap helper. The
    /// handler will reject the malformed `create` downstream; this
    /// helper only counts well-formed entries.
    #[test]
    fn enforce_max_objects_in_set_ignores_non_object_create() {
        let args = json!({
            "create": null,
            "update": null,
            "destroy": ["id1"],
        })
        .as_object()
        .unwrap()
        .clone();
        // Only the 1 destroy entry counts.
        enforce_max_objects_in_set(&args, 1).expect("non-object create/update count as 0");
    }

    /// Oracle (RFC 8620 §5.3): a non-array `destroy` argument counts
    /// as 0. Same rationale as the create/update test above.
    #[test]
    fn enforce_max_objects_in_set_ignores_non_array_destroy() {
        let args = json!({
            "create": { "c0": {} },
            "destroy": "not-an-array",
        })
        .as_object()
        .unwrap()
        .clone();
        // Only the 1 create entry counts.
        enforce_max_objects_in_set(&args, 1).expect("non-array destroy counts as 0");
    }

    /// Oracle (RFC 8620 §5.3): the cap counts the SUM of all three
    /// sources. A request whose individual create/update/destroy
    /// counts are each below the cap can still trip the cap when
    /// summed.
    #[test]
    fn enforce_max_objects_in_set_sums_all_three() {
        // 3 + 3 + 3 = 9; against max=5, must fail because the SUM > max
        // even though each individual count <= max.
        let args = json!({
            "create": { "c0": {}, "c1": {}, "c2": {} },
            "update": { "u0": {}, "u1": {}, "u2": {} },
            "destroy": ["d0", "d1", "d2"],
        })
        .as_object()
        .unwrap()
        .clone();
        let err =
            enforce_max_objects_in_set(&args, 5).expect_err("sum-of-all-three must trip the cap");
        assert_eq!(err.error_type.to_string(), "requestTooLarge");
        assert_eq!(err.description.as_deref(), Some("maxObjectsInSet"));
    }

    /// Oracle (RFC 8620 §5.5 + bd:JMAP-qz9v.48): `position` is a signed
    /// offset where non-negative is absolute and negative is
    /// end-relative. All hand-computed values below come from reading
    /// the spec text, NOT from the implementation under test.
    #[test]
    fn resolve_query_offset_absolute() {
        // Non-negative position: absolute offset, clamped at total.
        assert_eq!(resolve_query_offset(0, 100), 0);
        assert_eq!(resolve_query_offset(1, 100), 1);
        assert_eq!(resolve_query_offset(50, 100), 50);
        assert_eq!(resolve_query_offset(99, 100), 99);
        assert_eq!(resolve_query_offset(100, 100), 100);
        // Past-the-end → clamped to total (RFC 8620 §5.5 silent-clamp).
        assert_eq!(resolve_query_offset(101, 100), 100);
        assert_eq!(resolve_query_offset(10_000, 100), 100);
        assert_eq!(resolve_query_offset(i64::MAX, 100), 100);
    }

    #[test]
    fn resolve_query_offset_end_relative() {
        // RFC 8620 §5.5: "-1 represents the last entry, -2 the second
        // to last". Implemented as `total - |position|`, saturating at
        // 0 from below.
        assert_eq!(resolve_query_offset(-1, 100), 99);
        assert_eq!(resolve_query_offset(-2, 100), 98);
        assert_eq!(resolve_query_offset(-99, 100), 1);
        assert_eq!(resolve_query_offset(-100, 100), 0);
        // Past-the-start → clamped to 0.
        assert_eq!(resolve_query_offset(-101, 100), 0);
        assert_eq!(resolve_query_offset(-10_000, 100), 0);
    }

    /// Oracle: with total=0 (empty result list), every position
    /// resolves to 0 (the only valid index, and a no-op slice).
    #[test]
    fn resolve_query_offset_empty_total() {
        assert_eq!(resolve_query_offset(0, 0), 0);
        assert_eq!(resolve_query_offset(1, 0), 0);
        assert_eq!(resolve_query_offset(-1, 0), 0);
        assert_eq!(resolve_query_offset(i64::MAX, 0), 0);
        assert_eq!(resolve_query_offset(i64::MIN, 0), 0);
    }

    /// Oracle: `i64::MIN` would overflow `-position` (the previous
    /// workspace idiom at mail-server `memory/mod.rs:980`) but
    /// `saturating_neg` returns `i64::MAX`. The resulting end-relative
    /// offset of `i64::MAX` is well past any realistic `total`, so for
    /// any realistic total the result is 0.
    ///
    /// For the synthetic case `total == usize::MAX`, the offset stays
    /// within the saturation range and the result is
    /// `usize::MAX - i64::MAX`, which on a 64-bit target is exactly
    /// `0x8000_0000_0000_0000`. The point of the regression guard is
    /// that the call does NOT panic (which it would under the prior
    /// `(-position) as usize` idiom in debug builds).
    #[test]
    fn resolve_query_offset_i64_min_does_not_overflow() {
        assert_eq!(resolve_query_offset(i64::MIN, 1), 0);
        assert_eq!(resolve_query_offset(i64::MIN, 100), 0);
        assert_eq!(resolve_query_offset(i64::MIN, i64::MAX as usize), 0);
        // Synthetic upper bound: the answer is computable but not 0.
        // What matters is no overflow / no panic, and start <= total.
        let start = resolve_query_offset(i64::MIN, usize::MAX);
        assert_eq!(start, usize::MAX - (i64::MAX as usize));
        // And `i64::MAX` on the positive side is also handled.
        assert_eq!(resolve_query_offset(i64::MAX, 0), 0);
        assert_eq!(resolve_query_offset(i64::MAX, 1), 1);
    }

    /// Oracle: the bead's specific concern was that `position as usize`
    /// truncates on 32-bit targets. On 64-bit (this test host) the
    /// `i64`-to-`usize` conversion is lossless within `[0, i64::MAX]`,
    /// so the truncation cannot be directly demonstrated; what we CAN
    /// verify is that the helper uses `usize::try_from` semantics, so
    /// a value above `i64::MAX` (only reachable here via direct cast
    /// from an unsigned constant) clamps to `total` rather than
    /// wrapping. The `i64::MIN` case in the prior test is the real
    /// regression guard.
    #[test]
    fn resolve_query_offset_truncation_resistant() {
        // The slice-safety property: `total..=total` is always a valid
        // start index for `slice[start..]` (yields empty slice). The
        // helper MUST return a value in `0..=total`.
        for &pos in &[i64::MIN, -1_000_000, -1, 0, 1, 1_000_000, i64::MAX] {
            for &total in &[0usize, 1, 100, 10_000] {
                let start = resolve_query_offset(pos, total);
                assert!(
                    start <= total,
                    "resolve_query_offset({pos}, {total}) = {start} violates start <= total"
                );
            }
        }
    }
    /// Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt):
    /// "If found, the "anchorOffset" is then added to its index.  If the
    /// resulting index is now negative, it is clamped to 0. ... If the
    /// anchor is not found, the call is rejected with an "anchorNotFound"
    /// error.  If an "anchor" is specified, any position argument supplied
    /// by the client MUST be ignored.  If no "anchor" is supplied, any
    /// "anchorOffset" argument MUST be ignored."
    ///
    /// `ids` is `[a, b, c, d]`, so anchor `c` is index 2.
    #[test]
    fn resolve_query_start_follows_rfc8620_anchor_rules() {
        let ids: Vec<jmap_types::Id> = ["a", "b", "c", "d"].map(jmap_types::Id::from).to_vec();
        let c = jmap_types::Id::from("c");
        let start = |position, anchor, offset| resolve_query_start(&ids, position, anchor, offset);
        assert_eq!(start(0, Some(&c), 1), Ok(3));
        assert_eq!(start(0, Some(&c), -1), Ok(1));
        assert_eq!(start(0, Some(&c), -5), Ok(0));
        assert_eq!(start(3, Some(&c), 0), Ok(2));
        assert_eq!(start(1, None, 2), Ok(1));
        let missing = jmap_types::Id::from("z");
        assert_eq!(
            start(0, Some(&missing), 0).map_err(|e| e.error_type.to_string()),
            Err("anchorNotFound".to_owned())
        );
    }

    /// RFC 8620 §5.2: "the value MUST be a positive integer greater than
    /// 0. If a value outside of this range is given, the server MUST
    /// reject the call with an "invalidArguments" error." The argument is
    /// `UnsignedInt|null`, so absent and `null` mean "not given".
    #[test]
    fn parse_max_changes_follows_rfc8620_5_2() {
        assert_eq!(parse_max_changes(None).ok(), Some(None));
        assert_eq!(parse_max_changes(Some(&json!(null))).ok(), Some(None));
        assert_eq!(parse_max_changes(Some(&json!(1))).ok(), Some(Some(1)));
        assert_eq!(parse_max_changes(Some(&json!(50))).ok(), Some(Some(50)));
        for bad in [json!(0), json!(-1), json!(1.5), json!("5"), json!(true)] {
            assert_eq!(
                parse_max_changes(Some(&bad)).map_err(|e| e.error_type.to_string()),
                Err("invalidArguments".to_owned()),
                "maxChanges = {bad} must be rejected"
            );
        }
    }
}
