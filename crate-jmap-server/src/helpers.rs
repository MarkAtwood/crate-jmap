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
pub fn now_utc_string_checked() -> Option<UTCDate> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now();
    let (secs, millis): (i64, u32) = match now.duration_since(UNIX_EPOCH) {
        Ok(d) => {
            let s = i64::try_from(d.as_secs()).ok()?;
            (s, d.subsec_millis())
        }
        Err(e) => {
            // Clock is before the Unix epoch — negate so we get a real
            // (negative) epoch offset rather than silently returning
            // 1970-01-01T00:00:00Z. Negate after the fallible widen so
            // we don't underflow at i64::MIN. .checked_neg() returns
            // None only for i64::MIN, which try_from cannot produce
            // (its output range is [0, i64::MAX]); the branch is
            // therefore unreachable on valid u64 → i64 widening, but
            // returning None for defence in depth keeps the failure
            // path total.
            let d = e.duration();
            let s = i64::try_from(d.as_secs()).ok()?;
            let neg = s.checked_neg()?;
            (neg, d.subsec_millis())
        }
    };

    let s = secs.rem_euclid(60);
    let m = (secs / 60).rem_euclid(60);
    let h = (secs / 3600).rem_euclid(24);
    let days = secs.div_euclid(86400);
    let (year, month, day) = civil_from_days(days)?;

    Some(UTCDate::from(format!(
        "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z"
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

#[cfg(test)]
mod tests {
    use super::{
        civil_from_days, extract_account_id, json_merge_patch, now_utc_string, MergePatchError,
        MAX_MERGE_PATCH_DEPTH,
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
}
