//! The `jmap:` JMAP object URI
//! (draft-atwood-jmap-object-uri-00 §JMAP Object URIs).
//!
//! The draft defines the syntax as
//! `jmap-object-uri = "jmap:" type-name ":" account-id ":" object-id`,
//! where `type-name` is `1*( %x61-7A / DIGIT )` and `account-id` and
//! `object-id` are `1*255id-char`, the RFC 8620 §1.2 `Id` alphabet. That
//! alphabet excludes `:`, so splitting on `:` is unambiguous, and a URI
//! with any further segment matches no form this crate implements
//! (§Other Forms: "A receiver treats a `jmap` URI that matches no form it
//! implements as unresolvable").

use crate::{Id, ValidationError};
use std::fmt;
use std::str::FromStr;

/// A parsed `jmap:<type>:<accountId>:<objectId>` URI.
///
/// Build one with [`JmapUri::new`] or parse one with [`str::parse`];
/// [`Display`](fmt::Display) renders it back to the wire string. The URI
/// arrives in peer-supplied values, so parsing validates every segment.
///
/// ```rust
/// use jmap_types::JmapUri;
///
/// let uri: JmapUri = "jmap:calendarevent:a1:e42".parse().unwrap();
/// assert_eq!(uri.data_type(), "calendarevent");
/// assert_eq!(uri.account_id().as_ref(), "a1");
/// assert_eq!(uri.object_id().as_ref(), "e42");
/// assert_eq!(uri.to_string(), "jmap:calendarevent:a1:e42");
/// ```
///
/// It serializes as its wire string and deserializes by parsing one, so a
/// malformed value fails deserialization. Ordering compares the type, then
/// the account id, then the object id, consistently with equality (the
/// scheme is case-insensitive and not stored).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JmapUri {
    data_type: String,
    account_id: Id,
    object_id: Id,
}

/// Why a string or set of parts is not a valid [`JmapUri`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JmapUriError {
    /// The URI scheme is not `jmap`, or the string has no scheme at all.
    NotJmapScheme,
    /// A `jmap://` authority form, which §Other Forms reserves and which is
    /// not a JMAP object URI.
    ReservedForm,
    /// The `<type>` segment is empty or not lowercase ASCII alphanumeric.
    InvalidDataType,
    /// The `<accountId>` segment is missing or not a valid `Id`.
    InvalidAccountId(Option<ValidationError>),
    /// No `<objectId>` segment follows the account.
    MissingObjectId,
    /// The `<objectId>` segment is not a valid `Id`, including when
    /// further `:` segments follow it.
    InvalidObjectId(ValidationError),
}

impl fmt::Display for JmapUriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJmapScheme => f.write_str("URI scheme is not `jmap`"),
            Self::ReservedForm => {
                f.write_str("jmap:// URI is a reserved form, not a JMAP object URI")
            }
            Self::InvalidDataType => {
                f.write_str("jmap: URI type is not a lowercased JMAP data type name")
            }
            Self::InvalidAccountId(_) => f.write_str("jmap: URI accountId is missing or invalid"),
            Self::MissingObjectId => f.write_str("jmap: URI has no objectId"),
            Self::InvalidObjectId(_) => f.write_str("jmap: URI objectId is invalid"),
        }
    }
}

impl std::error::Error for JmapUriError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidAccountId(Some(e)) | Self::InvalidObjectId(e) => Some(e),
            _ => None,
        }
    }
}

impl JmapUri {
    /// Build a URI from its parts.
    ///
    /// # Errors
    ///
    /// Returns [`JmapUriError`] when `data_type` is not lowercase ASCII
    /// alphanumeric or either id fails RFC 8620 §1.2 validation (an
    /// unvalidated [`Id::from`] could otherwise smuggle a `:` into the
    /// rendered URI).
    pub fn new(
        data_type: impl Into<String>,
        account_id: Id,
        object_id: Id,
    ) -> Result<Self, JmapUriError> {
        let data_type = data_type.into();
        validate_data_type(&data_type)?;
        Id::new_validated(account_id.as_ref())
            .map_err(|e| JmapUriError::InvalidAccountId(Some(e)))?;
        Id::new_validated(object_id.as_ref()).map_err(JmapUriError::InvalidObjectId)?;
        Ok(Self {
            data_type,
            account_id,
            object_id,
        })
    }

    /// The lowercased type name, e.g. `calendarevent`.
    pub fn data_type(&self) -> &str {
        &self.data_type
    }

    /// The account the object lives on.
    pub fn account_id(&self) -> &Id {
        &self.account_id
    }

    /// The referenced object's `id`.
    pub fn object_id(&self) -> &Id {
        &self.object_id
    }
}

fn validate_data_type(s: &str) -> Result<(), JmapUriError> {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        Ok(())
    } else {
        Err(JmapUriError::InvalidDataType)
    }
}

impl FromStr for JmapUri {
    type Err = JmapUriError;

    /// Parse a `jmap:` URI. The scheme matches case-insensitively
    /// (RFC 3986 §3.1); every other segment is case-sensitive.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // At most four parts: anything after the object-id stays in it and
        // fails `Id` validation, since `:` is not an `id-char`.
        let mut parts = s.splitn(4, ':');
        let scheme = parts.next().unwrap_or_default();
        // No `:` at all: not a URI with the `jmap` scheme.
        let Some(data_type) = parts.next().filter(|_| scheme.eq_ignore_ascii_case("jmap")) else {
            return Err(JmapUriError::NotJmapScheme);
        };
        if data_type.starts_with("//") {
            return Err(JmapUriError::ReservedForm);
        }
        validate_data_type(data_type)?;
        let account_id = parts.next().ok_or(JmapUriError::InvalidAccountId(None))?;
        let account_id =
            Id::new_validated(account_id).map_err(|e| JmapUriError::InvalidAccountId(Some(e)))?;
        let object_id = parts.next().ok_or(JmapUriError::MissingObjectId)?;
        let object_id = Id::new_validated(object_id).map_err(JmapUriError::InvalidObjectId)?;
        Self::new(data_type, account_id, object_id)
    }
}

impl fmt::Display for JmapUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "jmap:{}:{}:{}",
            self.data_type, self.account_id, self.object_id
        )
    }
}

impl serde::Serialize for JmapUri {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for JmapUri {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
