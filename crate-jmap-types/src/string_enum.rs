//! Shared macro for string-backed enums with a catch-all `Other(String)` variant.
//!
//! This module is the single canonical home for the [`impl_string_enum!`] macro
//! used across the `jmap-*-types` crate family. JMAP wire-protocol enums often
//! allow vendor-specific extensions (per RFC 8620 §1.6 and the various JMAP
//! extension specs), so each enum carries an `Other(String)` variant that
//! preserves unknown values for round-trip fidelity.
//!
//! Previously every `jmap-*-types` crate carried a byte-identical copy of this
//! macro; this module consolidates them. See bd issue JMAP-wk77.

/// Implement [`serde::Serialize`], [`serde::Deserialize`], and [`std::fmt::Display`]
/// for a string-backed enum with a catch-all `Other(String)` variant.
///
/// # Requirements
///
/// The enum **must** have a variant `Other(String)` that stores the raw wire string
/// for any value not listed in the known-value mapping.  The macro generates match
/// arms only for the listed variants; the `Other` catch-all is generated automatically.
///
/// # Usage
///
/// ```ignore
/// impl_string_enum!(MyEnum, "a my-enum value",
///     "wire-string-1" => Variant1,
///     "wire-string-2" => Variant2,
/// );
/// ```
///
/// This emits:
/// - `impl Serialize` — serialises each listed variant to its wire string; `Other(v)`
///   serialises to the inner string.
/// - `impl Deserialize` — deserialises a JSON string using a `Visitor`; unknown values
///   become `Other(v.to_owned())`.
/// - `impl Display` — formats using the same mapping as `Serialize`.
#[macro_export]
macro_rules! impl_string_enum {
    ($ty:ident, $expecting:literal, $( $s:literal => $variant:ident ),+ $(,)?) => {
        impl ::serde::Serialize for $ty {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(match self {
                    $( $ty::$variant => $s, )+
                    $ty::Other(v) => v.as_str(),
                })
            }
        }
        impl<'de> ::serde::Deserialize<'de> for $ty {
            fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct Visitor;
                impl ::serde::de::Visitor<'_> for Visitor {
                    type Value = $ty;
                    fn expecting(
                        &self,
                        f: &mut ::std::fmt::Formatter<'_>,
                    ) -> ::std::fmt::Result {
                        write!(f, $expecting)
                    }
                    fn visit_str<E: ::serde::de::Error>(self, v: &str) -> Result<$ty, E> {
                        Ok(match v {
                            $( $s => $ty::$variant, )+
                            _ => $ty::Other(v.to_owned()),
                        })
                    }
                }
                d.deserialize_str(Visitor)
            }
        }
        impl ::std::fmt::Display for $ty {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(match self {
                    $( $ty::$variant => $s, )+
                    $ty::Other(v) => v.as_str(),
                })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    // Test enum exercising the catch-all `Other(String)` pattern. The test
    // oracle is hand-written JSON (independent of the code under test): we
    // assert the macro emits the exact wire bytes a JMAP client would receive
    // and that an unknown wire value round-trips losslessly via `Other`.
    #[derive(Debug, PartialEq)]
    enum TestKind {
        Foo,
        Bar,
        Other(String),
    }

    impl_string_enum!(
        TestKind,
        "a test kind string",
        "foo" => Foo,
        "bar" => Bar,
    );

    #[test]
    fn known_variant_serializes_to_wire_string() {
        // Hand-written oracle: the JSON form of TestKind::Foo is the literal
        // string "foo" (3 bytes plus surrounding quotes = 5 bytes total).
        assert_eq!(serde_json::to_string(&TestKind::Foo).unwrap(), r#""foo""#);
        assert_eq!(serde_json::to_string(&TestKind::Bar).unwrap(), r#""bar""#);
    }

    #[test]
    fn known_wire_string_deserializes_to_variant() {
        // Inverse oracle: feeding the canonical wire bytes yields the typed variant.
        let foo: TestKind = serde_json::from_str(r#""foo""#).unwrap();
        let bar: TestKind = serde_json::from_str(r#""bar""#).unwrap();
        assert_eq!(foo, TestKind::Foo);
        assert_eq!(bar, TestKind::Bar);
    }

    #[test]
    fn unknown_wire_string_round_trips_via_other() {
        // The defining JMAP property: a value the spec does not know must be
        // preserved verbatim through deserialize -> serialize. This is what
        // `Other(String)` exists to guarantee.
        let original = r#""vendor-extension-value""#;
        let parsed: TestKind = serde_json::from_str(original).unwrap();
        assert_eq!(parsed, TestKind::Other("vendor-extension-value".to_owned()));
        let reserialized = serde_json::to_string(&parsed).unwrap();
        assert_eq!(reserialized, original);
    }

    #[test]
    fn display_matches_serialize_form() {
        // Display must emit the same wire string as Serialize. A divergence
        // here would silently corrupt log output relative to JSON output.
        assert_eq!(format!("{}", TestKind::Foo), "foo");
        assert_eq!(format!("{}", TestKind::Bar), "bar");
        assert_eq!(
            format!("{}", TestKind::Other("custom".to_owned())),
            "custom",
        );
    }

    #[test]
    fn empty_string_round_trips_as_other() {
        // Empty wire string is unknown to the registered list, so it lands in
        // `Other("")`. The macro must not panic or treat it specially.
        let parsed: TestKind = serde_json::from_str(r#""""#).unwrap();
        assert_eq!(parsed, TestKind::Other(String::new()));
        assert_eq!(serde_json::to_string(&parsed).unwrap(), r#""""#);
    }

    #[test]
    fn deserialize_rejects_non_string_json() {
        // Visitor only implements visit_str. Numbers, booleans, objects must
        // fail with a serde error rather than silently coercing.
        assert!(serde_json::from_str::<TestKind>("42").is_err());
        assert!(serde_json::from_str::<TestKind>("true").is_err());
        assert!(serde_json::from_str::<TestKind>("null").is_err());
        assert!(serde_json::from_str::<TestKind>("{}").is_err());
    }
}
