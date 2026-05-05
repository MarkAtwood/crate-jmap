//! Shared macro for string-backed enums with a catch-all `Other(String)` variant.

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
