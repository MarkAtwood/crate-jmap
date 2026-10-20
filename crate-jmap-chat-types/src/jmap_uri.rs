//! The private-use `jmap:` object-reference URI
//! (draft-atwood-jmap-chat-calendars-00 §CalendarEvent in Chat Context).
//!
//! The calendars draft defines the syntax as
//! `jmap:<type>:<accountId>:<objectId>`, where `<type>` is a registered
//! JMAP data type name (lowercased), `<accountId>` is a JMAP account
//! identifier and `<objectId>` is the object's `id`. The tasks draft
//! (draft-atwood-jmap-chat-tasks-00 §MessageAction format) reuses the
//! scheme with one extra segment, `jmap:task:<accountId>:<taskListId>:<taskId>`,
//! so [`JmapUri`] carries one or more object-id segments after the account.
//!
//! Every segment after the type is an RFC 8620 §1.2 `Id`, whose alphabet
//! excludes `:`, so splitting on `:` is unambiguous.

use jmap_types::{Id, ValidationError};
use std::fmt;
use std::str::FromStr;

/// A parsed `jmap:<type>:<accountId>:<objectId>[:<objectId>...]` URI.
///
/// Build one with [`JmapUri::new`] or parse one with [`str::parse`];
/// [`Display`](fmt::Display) renders it back to the wire string. The URI
/// arrives in peer-supplied `MessageAction.uri` values, so parsing
/// validates every segment.
///
/// ```rust
/// use jmap_chat_types::JmapUri;
///
/// let uri: JmapUri = "jmap:calendarevent:a1:e42".parse().unwrap();
/// assert_eq!(uri.data_type(), "calendarevent");
/// assert_eq!(uri.account_id().as_ref(), "a1");
/// assert_eq!(uri.object_id().as_ref(), "e42");
/// assert_eq!(uri.to_string(), "jmap:calendarevent:a1:e42");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JmapUri {
    data_type: String,
    account_id: Id,
    object_ids: Vec<Id>,
}

/// Why a string or set of parts is not a valid [`JmapUri`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum JmapUriError {
    /// The URI scheme is not `jmap`.
    NotJmapScheme,
    /// The `<type>` segment is empty or not lowercase ASCII alphanumeric.
    InvalidDataType,
    /// The `<accountId>` segment is missing or not a valid `Id`.
    InvalidAccountId(Option<ValidationError>),
    /// No `<objectId>` segment follows the account.
    MissingObjectId,
    /// An `<objectId>` segment is not a valid `Id`.
    InvalidObjectId(ValidationError),
}

impl fmt::Display for JmapUriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJmapScheme => f.write_str("URI scheme is not `jmap`"),
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
    /// `object_ids` holds the segments after the account: one for the
    /// calendars form, two (`taskListId`, `taskId`) for the tasks form.
    ///
    /// # Errors
    ///
    /// Returns [`JmapUriError`] when `data_type` is not lowercase ASCII
    /// alphanumeric, `object_ids` is empty, or any id fails RFC 8620 §1.2
    /// validation (an unvalidated [`Id::from`] could otherwise smuggle a
    /// `:` into the rendered URI).
    pub fn new(
        data_type: impl Into<String>,
        account_id: Id,
        object_ids: Vec<Id>,
    ) -> Result<Self, JmapUriError> {
        let data_type = data_type.into();
        validate_data_type(&data_type)?;
        Id::new_validated(account_id.as_ref())
            .map_err(|e| JmapUriError::InvalidAccountId(Some(e)))?;
        if object_ids.is_empty() {
            return Err(JmapUriError::MissingObjectId);
        }
        for id in &object_ids {
            Id::new_validated(id.as_ref()).map_err(JmapUriError::InvalidObjectId)?;
        }
        Ok(Self {
            data_type,
            account_id,
            object_ids,
        })
    }

    /// The lowercased JMAP data type name, e.g. `calendarevent`.
    pub fn data_type(&self) -> &str {
        &self.data_type
    }

    /// The account the object lives on.
    pub fn account_id(&self) -> &Id {
        &self.account_id
    }

    /// The final segment: the referenced object's `id`.
    pub fn object_id(&self) -> &Id {
        self.object_ids
            .last()
            .expect("constructors reject an empty object_ids")
    }

    /// Every segment after the account, in order; for the tasks form this
    /// is `[taskListId, taskId]`.
    pub fn object_ids(&self) -> &[Id] {
        &self.object_ids
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
        let mut parts = s.split(':');
        let scheme = parts.next().unwrap_or_default();
        if !scheme.eq_ignore_ascii_case("jmap") {
            return Err(JmapUriError::NotJmapScheme);
        }
        let data_type = parts.next().ok_or(JmapUriError::InvalidDataType)?;
        validate_data_type(data_type)?;
        let account_id = parts.next().ok_or(JmapUriError::InvalidAccountId(None))?;
        let account_id =
            Id::new_validated(account_id).map_err(|e| JmapUriError::InvalidAccountId(Some(e)))?;
        let object_ids = parts
            .map(Id::new_validated)
            .collect::<Result<Vec<_>, _>>()
            .map_err(JmapUriError::InvalidObjectId)?;
        Self::new(data_type, account_id, object_ids)
    }
}

impl fmt::Display for JmapUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "jmap:{}:{}", self.data_type, self.account_id)?;
        self.object_ids.iter().try_for_each(|id| write!(f, ":{id}"))
    }
}
