//! RFC 8620 §7 ResultReference — back-references between method calls in a batch.
//!
//! Provides [`ResultReference`] and [`Argument<T>`].

use serde::{Deserialize, Serialize};

/// Sealed marker for types that are safe to use with [`Argument<T>`].
///
/// This trait is intentionally sealed: it is defined in a private module and
/// can only be implemented inside this crate. The current sealed set is:
///
/// | Type | Rationale |
/// |---|---|
/// | `String` | RFC 8620 §3.7 — generic id / token argument |
/// | `Vec<String>` | id-list argument, e.g. `Email/get { ids }` |
/// | [`crate::Id`] | typed id argument |
/// | `Vec<`[`crate::Id`]`>` | typed id-list argument, e.g. `Email/set { destroy }` |
/// | [`crate::Date`] | calendar-date argument (RFC 8984 / JMAP Calendars) |
/// | `Vec<`[`crate::Date`]`>` | date-list argument |
/// | [`crate::UTCDate`] | UTC-timestamp argument, e.g. `Email/query { before, after }` (RFC 8621 §4.4.1) |
/// | `Vec<`[`crate::UTCDate`]`>` | timestamp-list argument |
/// | [`crate::State`] | state-token argument, e.g. `Email/changes { sinceState }` (RFC 8620 §5.2) |
/// | `Vec<`[`crate::State`]`>` | state-list argument |
/// | `u32`, `u64` | counts, indexes, limits (RFC 8620 §5.5) |
/// | `bool` | flag argument |
///
/// `crate::Date`, `crate::UTCDate`, and `crate::State` are
/// `#[serde(transparent)]` newtypes over `String`. They serialize and
/// deserialize as JSON strings (never as objects), so they satisfy the
/// [`Argument`] `# Invariant` and are safe to seal.
///
/// **Types NOT in the sealed set** (and the reason): every JMAP wire
/// data-object type (`Email`, `Mailbox`, `Calendar`, `ContactCard`,
/// `Thread`, etc.) and every per-extension argument struct. These types
/// deserialize from JSON objects; with `#[serde(untagged)]` on
/// [`Argument`], the `T` variant would match before `Ref` and silently
/// swallow any `ResultReference` payload. `serde_json::Value` is
/// excluded for the same reason — see the `# Type safety` section on
/// [`Argument`].
///
/// **External contributors**: you cannot implement `Sealed` outside this crate.
/// To add a new type to the sealed set, open a PR to `crate-jmap-types` and
/// add the impl here. Before doing so, read the `# Invariant` section on
/// [`Argument`] — the new type must not deserialize from a JSON object.
mod sealed {
    pub trait Sealed {}

    impl Sealed for String {}
    impl Sealed for Vec<String> {}
    impl Sealed for crate::Id {}
    impl Sealed for Vec<crate::Id> {}
    impl Sealed for crate::Date {}
    impl Sealed for Vec<crate::Date> {}
    impl Sealed for crate::UTCDate {}
    impl Sealed for Vec<crate::UTCDate> {}
    impl Sealed for crate::State {}
    impl Sealed for Vec<crate::State> {}
    impl Sealed for u32 {}
    impl Sealed for u64 {}
    impl Sealed for bool {}
}

/// A reference to the result of a previous invocation in the same JMAP request batch.
/// Used in method arguments with a "#" prefix on the JSON key (RFC 8620 §9).
///
/// Example JSON:
/// ```json
/// {"resultOf": "0", "name": "ChatContact/get", "path": "/list/0/id"}
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResultReference {
    /// The call-id of the prior method invocation being referenced.
    #[serde(rename = "resultOf")]
    pub result_of: String,
    /// The method name of that prior invocation (e.g. "ChatContact/get").
    pub name: String,
    /// JSON Pointer (RFC 6901) into the result, e.g. "/list/0/id".
    pub path: String,
}

impl ResultReference {
    /// Construct a `ResultReference` from its three required fields.
    ///
    /// `result_of`: call-id of the prior method invocation
    /// `name`: method name of that invocation (e.g. `"Foo/get"`)
    /// `path`: RFC 6901 JSON Pointer into the result (e.g. `"/list/*/id"`)
    pub fn new(
        result_of: impl Into<String>,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            result_of: result_of.into(),
            name: name.into(),
            path: path.into(),
        }
    }
}

/// A JMAP method argument that can be either a direct value or a ResultReference.
///
/// In JMAP JSON, a ResultReference is indicated by a "#" prefix on the key:
///   `"ids": [...]`  →  Argument::Value([...])
///   `"#ids": {...}` →  Argument::Ref(ResultReference { ... })
///
/// The dispatcher in `jmap-server` evaluates `Ref` variants before
/// invoking the method handler.
///
/// # Deserialization note
/// Uses `#[serde(untagged)]` which tries to deserialize as T first.
/// If T and ResultReference share field names, T is preferred.
/// Callers must handle the `#` key prefix before deserializing.
///
/// # Invariant
///
/// `T` must not deserialize from a JSON object with keys `resultOf`, `name`,
/// and `path`. If `T` accepts arbitrary JSON objects, `#[serde(untagged)]`
/// will match `T` before trying `Ref`, silently swallowing any
/// `ResultReference` payload. The sealed set is chosen specifically to exclude
/// struct types for this reason. Never implement `Sealed` for a type that
/// deserializes from a JSON object.
///
/// # Type safety
/// `T` is constrained to `sealed::Sealed` types. `serde_json::Value` does NOT
/// implement `Sealed` — `Argument<Value>` therefore fails to compile. This
/// prevents a silent bug: `Value` accepts any JSON, so with `T = Value` the
/// `Ref` variant can never be reached via serde; every ResultReference JSON
/// would deserialize as `Argument::Value(json_object)` and the reference would
/// be silently ignored.
///
/// ```compile_fail
/// use jmap_types::resultref::Argument;
/// // serde_json::Value does not implement Sealed — this must not compile.
/// let _: Argument<serde_json::Value> = Argument::Value(serde_json::Value::Null);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument<T: sealed::Sealed> {
    /// A direct, inlined argument value supplied by the caller.
    Value(T),
    /// A result-reference back to a previous method response in the same
    /// request (RFC 8620 §3.7), resolved by the dispatcher before invoking
    /// the method handler.
    Ref(ResultReference),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_reference_round_trip() {
        // Independent oracle: RFC 8620 §9 example
        let rr = ResultReference {
            result_of: "0".into(),
            name: "ChatContact/get".into(),
            path: "/list/0/id".into(),
        };
        let json_str = serde_json::to_string(&rr).unwrap();
        let rr2: ResultReference = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rr, rr2);
    }

    #[test]
    fn result_reference_field_names() {
        let rr = ResultReference {
            result_of: "req-1".into(),
            name: "Chat/get".into(),
            path: "/id".into(),
        };
        let json_str = serde_json::to_string(&rr).unwrap();
        assert!(
            json_str.contains("\"resultOf\""),
            "must use camelCase resultOf"
        );
        assert!(json_str.contains("\"name\""));
        assert!(json_str.contains("\"path\""));
        assert!(
            !json_str.contains("\"result_of\""),
            "must not use snake_case"
        );
    }

    #[test]
    fn argument_value_serializes_as_inner_type() {
        let arg: Argument<u32> = Argument::Value(42);
        let json_str = serde_json::to_string(&arg).unwrap();
        assert_eq!(json_str, "42");
    }

    #[test]
    fn argument_ref_serializes_as_result_reference() {
        let rr = ResultReference {
            result_of: "0".into(),
            name: "ChatContact/get".into(),
            path: "/list/0/id".into(),
        };
        let arg: Argument<Vec<String>> = Argument::Ref(rr);
        let json_str = serde_json::to_string(&arg).unwrap();
        assert!(json_str.contains("\"resultOf\""));
        assert!(json_str.contains("\"ChatContact/get\""));
    }

    #[test]
    fn argument_value_vec_string_deserializes() {
        let json_str = r#"["alice","bob"]"#;
        let arg: Argument<Vec<String>> = serde_json::from_str(json_str).unwrap();
        match arg {
            Argument::Value(v) => assert_eq!(v, vec!["alice", "bob"]),
            Argument::Ref(_) => panic!("expected Value variant"),
        }
    }

    // Oracle: tests/fixtures/rfc8620-result-reference.json (RFC 8620 §9)
    #[test]
    fn result_reference_deserializes_from_fixture() {
        let raw = include_str!("../tests/fixtures/rfc8620-result-reference.json");
        let rr: ResultReference = serde_json::from_str(raw).expect("deserialize ResultReference");
        assert_eq!(rr.result_of, "t0");
        assert_eq!(rr.name, "Foo/changes");
        assert_eq!(rr.path, "/created");
    }

    // Oracle: #[serde(untagged)] — ResultReference JSON object deserializes as Argument::Ref.
    #[test]
    fn argument_ref_deserializes() {
        let j = r#"{"resultOf":"0","name":"X/get","path":"/ids"}"#;
        let arg: Argument<Vec<String>> = serde_json::from_str(j).expect("deser");
        match arg {
            Argument::Ref(rr) => {
                assert_eq!(rr.result_of, "0");
                assert_eq!(rr.name, "X/get");
            }
            Argument::Value(_) => panic!("expected Ref"),
        }
    }

    // Oracle: bd:JMAP-6xs8.27 — the sealed set now includes Date, UTCDate,
    // and State (plus their Vec forms). These are #[serde(transparent)]
    // String newtypes that satisfy the Argument<T> invariant (they
    // deserialize as JSON strings, never as objects), so they are safe
    // to use as the inner type of an Argument. This test exercises both
    // variants for each, ensuring the sealed-set additions actually
    // compile and round-trip.
    #[test]
    fn argument_string_newtypes_compile_and_round_trip() {
        use crate::{Date, State, UTCDate};

        // Date
        let arg_date: Argument<Date> = Argument::Value(Date::from("2024-06-15"));
        let j_date = serde_json::to_string(&arg_date).expect("serialize Date Value");
        assert_eq!(j_date, "\"2024-06-15\"");
        let back_date: Argument<Date> =
            serde_json::from_str(&j_date).expect("deserialize Date Value");
        assert_eq!(arg_date, back_date);

        let arg_date_list: Argument<Vec<Date>> = Argument::Value(vec![Date::from("2024-06-15")]);
        let _ = serde_json::to_string(&arg_date_list).expect("Vec<Date> Value");

        // UTCDate
        let arg_utc: Argument<UTCDate> = Argument::Value(UTCDate::from("2024-06-15T09:00:00Z"));
        let j_utc = serde_json::to_string(&arg_utc).expect("serialize UTCDate Value");
        assert_eq!(j_utc, "\"2024-06-15T09:00:00Z\"");
        let back_utc: Argument<UTCDate> =
            serde_json::from_str(&j_utc).expect("deserialize UTCDate Value");
        assert_eq!(arg_utc, back_utc);

        let arg_utc_list: Argument<Vec<UTCDate>> = Argument::Value(vec![]);
        let _ = serde_json::to_string(&arg_utc_list).expect("Vec<UTCDate> Value");

        // State
        let arg_state: Argument<State> = Argument::Value(State::from("s-42"));
        let j_state = serde_json::to_string(&arg_state).expect("serialize State Value");
        assert_eq!(j_state, "\"s-42\"");
        let back_state: Argument<State> =
            serde_json::from_str(&j_state).expect("deserialize State Value");
        assert_eq!(arg_state, back_state);

        let arg_state_list: Argument<Vec<State>> = Argument::Value(vec![]);
        let _ = serde_json::to_string(&arg_state_list).expect("Vec<State> Value");

        // The Ref variant still works for the newly-added inner types.
        let rr = ResultReference::new("0", "Email/changes", "/newState");
        let arg_state_ref: Argument<State> = Argument::Ref(rr.clone());
        let j_state_ref = serde_json::to_string(&arg_state_ref).expect("serialize Ref");
        assert!(j_state_ref.contains("\"resultOf\""));
        assert!(j_state_ref.contains("\"Email/changes\""));
        let back_ref: Argument<State> =
            serde_json::from_str(&j_state_ref).expect("deserialize Ref");
        match back_ref {
            Argument::Ref(rr2) => {
                assert_eq!(rr2.result_of, "0");
                assert_eq!(rr2.name, "Email/changes");
                assert_eq!(rr2.path, "/newState");
            }
            Argument::Value(_) => panic!("expected Ref"),
        }
    }
}
