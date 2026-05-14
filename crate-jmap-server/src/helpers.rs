//! Shared helper utilities for JMAP method handlers.

use jmap_types::{Id, JmapError};
use serde_json::{Map, Value};

/// Serialize any [`serde::Serialize`] type to a [`serde_json::Value`],
/// mapping serialization errors to [`JmapError::server_fail`].
pub fn ser<T: serde::Serialize>(val: T) -> Result<serde_json::Value, JmapError> {
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

/// Extract `accountId` from a JMAP method arguments envelope and return both
/// the extracted [`Id`] and the remaining argument map.
///
/// The caller passes the full `args: Value` from the method invocation by
/// value; this function destructures it once, so handlers do not have to
/// repeat the `let Value::Object(mut args) = args else { ... }` pattern after
/// every call.
///
/// Returns `invalidArguments` with the message "arguments must be an object
/// containing accountId" when `args` is not a JSON object, and the same error
/// type with the message "accountId is required" when the field is missing or
/// not a string.
pub fn extract_account_id(args: Value) -> Result<(Id, Map<String, Value>), JmapError> {
    let Value::Object(args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be an object containing accountId",
        ));
    };
    match args.get("accountId").and_then(|v| v.as_str()) {
        Some(s) => {
            let id = Id::from(s);
            Ok((id, args))
        }
        None => Err(JmapError::invalid_arguments("accountId is required")),
    }
}

/// Return the current UTC instant formatted as an RFC 3339 string with
/// millisecond precision (`YYYY-MM-DDTHH:MM:SS.mmmZ`).
///
/// Uses `std::time::SystemTime` so no external dependency is needed.
///
/// Pre-epoch handling: if `duration_since(UNIX_EPOCH)` fails (system clock
/// drifted before the epoch), this function uses the absolute duration from
/// `UNIX_EPOCH.duration_since(now)` but negates the seconds — producing a
/// timestamp in the range 1969-12-31T… through 1970-01-01T00:00:00Z. This
/// is still monotonically increasing for subsequent calls and never silently
/// produces 1970-01-01T00:00:00.000Z for a clock that is merely slightly behind.
///
/// Clock-overflow handling: if `SystemTime::now()` reports a Duration whose
/// `as_secs()` value exceeds `i64::MAX` (~9.2e18 seconds, ~292 billion years
/// past or before epoch — only reachable on a corrupted clock), this
/// function returns the sentinel string `"clock-out-of-range"`. The `as`
/// truncating-cast that this branch replaces (bd:JMAP-wlip.27) would have
/// silently wrapped to a negative second count and produced a bizarre
/// in-range date string instead.
pub fn now_utc_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now();
    let (secs, millis): (i64, u32) = match now.duration_since(UNIX_EPOCH) {
        Ok(d) => match i64::try_from(d.as_secs()) {
            Ok(s) => (s, d.subsec_millis()),
            // u64 seconds exceeds i64::MAX — corrupted clock sentinel.
            Err(_) => return "clock-out-of-range".to_owned(),
        },
        Err(e) => {
            // Clock is before the Unix epoch — negate so we get a real (negative)
            // epoch offset rather than silently returning 1970-01-01T00:00:00Z.
            let d = e.duration();
            match i64::try_from(d.as_secs()) {
                // Negate after the fallible widen so we don't underflow at
                // i64::MIN. .checked_neg() returns None only for i64::MIN,
                // which try_from cannot produce (its output range is
                // [0, i64::MAX]). The branch is therefore unreachable on
                // valid u64 → i64 widening; the .unwrap_or path falls through
                // to the sentinel for defence in depth.
                Ok(s) => match s.checked_neg() {
                    Some(neg) => (neg, d.subsec_millis()),
                    None => return "clock-out-of-range".to_owned(),
                },
                Err(_) => return "clock-out-of-range".to_owned(),
            }
        }
    };

    let s = secs.rem_euclid(60);
    let m = (secs / 60).rem_euclid(60);
    let h = (secs / 3600).rem_euclid(24);
    let days = secs.div_euclid(86400);
    let (year, month, day) = civil_from_days(days);

    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

/// Convert a count of days since the Unix epoch (1970-01-01) to a proleptic
/// Gregorian (year, month, day) triple.
///
/// Algorithm by Howard Hinnant (public domain):
/// <https://howardhinnant.github.io/date_algorithms.html>
///
/// The output narrowing casts use `try_from(...).expect(...)` to document
/// the algorithm-guaranteed ranges (bd:JMAP-wlip.27): year fits in i32 for
/// any input within i64's full range (z is added to 719_468 and divided by
/// 146_097 in `era`, so y is bounded by i64::MAX / 146_097 ≈ ±6.3e13 which
/// fits in i32 only for inputs already constrained to ±2.1e9 years —
/// callers driven by SystemTime cannot exceed that, so the expect documents
/// an invariant rather than a real failure mode); month is in [1, 12] and
/// day is in [1, 31] by the algorithm's modular structure.
fn civil_from_days(z: i64) -> (i32, u8, u8) {
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
    (
        i32::try_from(yr).expect("year bounded by SystemTime-driven input to i32"),
        u8::try_from(mo).expect("month bounded by algorithm to [1, 12]"),
        u8::try_from(d).expect("day bounded by algorithm to [1, 31]"),
    )
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
pub const MAX_MERGE_PATCH_DEPTH: usize = 32;

/// Error returned by [`json_merge_patch`] when a patch cannot be applied.
///
/// Marked `#[non_exhaustive]` so future RFC 7396 failure modes (e.g. a
/// size cap in addition to the depth cap) can be added without an API
/// break.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergePatchError {
    /// The patch nests deeper than [`MAX_MERGE_PATCH_DEPTH`] levels.
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
/// deeper than [`MAX_MERGE_PATCH_DEPTH`] levels (DoS guard added in
/// bd:JMAP-sc1b.97, made non-silent in bd:JMAP-wlip.1). Below the cap
/// the behaviour is exactly RFC 7396 and the call always returns
/// `Ok(())`.
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
        civil_from_days, json_merge_patch, now_utc_string, MergePatchError, MAX_MERGE_PATCH_DEPTH,
    };

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
                expected,
                "civil_from_days({days}) mismatch"
            );
        }
    }

    #[test]
    fn now_utc_string_format() {
        let s = now_utc_string();
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
