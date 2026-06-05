//! Shared helper utilities for JMAP method handlers.

use jmap_types::{Id, JmapError, UTCDate};
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

/// Deprecated alias for [`serialize_value`] (bd:JMAP-wlip.21).
///
/// The opaque 3-letter name was hard to read in consumer code
/// (`let v = ser(x)?;` left readers grepping three crates to learn
/// what `ser` did) and collided with the common local-variable name
/// `ser`. Use [`serialize_value`] instead. This alias is preserved
/// for one release as a deprecation runway; it will be removed in
/// the next major.
// bd:JMAP-jfia.6 — the `since` field was previously set to "0.1.3"
// while the crate version was still 0.1.2, which rendered as
// "deprecated in the FUTURE" in cargo doc / docs.rs. Drop `since`
// until the release that ships the renaming actually goes out; the
// version-pinned form will be reintroduced when 0.1.3 is published.
#[deprecated(note = "renamed to serialize_value (bd:JMAP-wlip.21)")]
pub fn ser<T: serde::Serialize>(val: T) -> Result<serde_json::Value, JmapError> {
    serialize_value(val)
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
fn signed_seconds_since_epoch(now: std::time::SystemTime) -> Option<(i64, u32)> {
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

/// Resolve a request-side `position` argument (RFC 8620 §5.5) into a
/// 0-based start index suitable for slicing the full ordered result
/// list (bd:JMAP-qz9v.48).
///
/// `position` is the signed offset from the `/query` request. The
/// foundation handler at [`crate::handlers::handle_query`] parses it
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

/// Enforce RFC 8620 §5.3 `maxObjectsInSet` cap at the top of a `/set`
/// handler (bd:JMAP-ayoz.41.1).
///
/// Counts entries in the wire `create` (object), `update` (object), and
/// `destroy` (array) arguments. Returns
/// [`JmapError::limit("maxObjectsInSet")`][JmapError::limit] — which maps
/// to HTTP 400 + wire `type: "limit"` via [`crate::response::error_status`]
/// — when the sum exceeds `max`.
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
/// Returns `JmapError::limit("maxObjectsInSet")` when the sum exceeds
/// `max`. The HTTP layer maps this to a 400 response with the limit
/// name in the RFC 7807 `"limit"` field.
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
        return Err(JmapError::limit("maxObjectsInSet"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        civil_from_days, enforce_max_objects_in_set, extract_account_id, json_merge_patch,
        now_utc_string, resolve_query_offset, MergePatchError, MAX_MERGE_PATCH_DEPTH,
    };
    use serde_json::json;

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
        assert_eq!(err.error_type.as_str(), "invalidArguments");

        // Contains a space (0x20 — outside SAFE-CHAR's 0x21+ lower bound).
        let err = extract_account_id(json!({ "accountId": "my account" }))
            .expect_err("space in accountId must fail validation");
        assert_eq!(err.error_type.as_str(), "invalidArguments");

        // Contains a DQUOTE (0x22 — explicitly excluded by SAFE-CHAR).
        let err = extract_account_id(json!({ "accountId": "a\"b" }))
            .expect_err("DQUOTE in accountId must fail validation");
        assert_eq!(err.error_type.as_str(), "invalidArguments");

        // 256 bytes — exceeds the 255 cap.
        let long: String = "a".repeat(256);
        let err = extract_account_id(json!({ "accountId": long }))
            .expect_err("over-long accountId must fail validation");
        assert_eq!(err.error_type.as_str(), "invalidArguments");
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

    /// Oracle (RFC 8620 §5.3 + §3.6.1): one entry over the cap MUST
    /// return JmapError of type "limit" with description "maxObjectsInSet".
    /// The wire shape is verified by hand-building the expected
    /// JmapError independently of the helper.
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
        let expected = jmap_types::JmapError::limit("maxObjectsInSet");
        assert_eq!(
            err.error_type.as_str(),
            expected.error_type.as_str(),
            "type must be \"limit\""
        );
        assert_eq!(
            err.description.as_deref(),
            Some("maxObjectsInSet"),
            "description must name the exceeded cap"
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
        assert_eq!(err.error_type.as_str(), "limit");
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
}
