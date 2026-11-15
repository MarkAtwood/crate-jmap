//! [`BlobRef`]: a blobId argument that may be a creation-id reference.

use std::cmp::Ordering;
use std::fmt;

use jmap_types::{Id, ValidationError};
use serde::{Deserialize, Serialize};

/// A blob given as input: a literal blobId (`"B1"`) or a reference to a
/// blob created earlier in the same request (`"#c1"`, RFC 8620 §5.3).
///
/// [`Id`]'s `Deserialize` rejects `#`, so input blobIds use this type.
/// Ordering compares the wire string, so a `BTreeMap<BlobRef, _>` keys
/// serialize in wire order (every `#` reference sorts before every literal
/// id).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlobRef {
    /// A literal blobId.
    Id(Id),
    /// `#<creationId>`: the blob created under that creation id.
    Creation(Id),
}

impl BlobRef {
    /// Parse a wire string: `#` followed by a valid Id is
    /// [`Creation`](Self::Creation), any other valid Id is
    /// [`Id`](Self::Id).
    ///
    /// # Errors
    ///
    /// [`ValidationError`] when the string (after any `#`) is not a valid
    /// RFC 8620 §1.2 Id, including a bare `#`.
    pub fn parse(s: &str) -> Result<Self, ValidationError> {
        match s.strip_prefix('#') {
            Some(cid) => Id::new_validated(cid).map(Self::Creation),
            None => Id::new_validated(s).map(Self::Id),
        }
    }

    /// The literal blobId, or `None` for a creation reference.
    pub fn as_id(&self) -> Option<&Id> {
        match self {
            Self::Id(id) => Some(id),
            Self::Creation(_) => None,
        }
    }
}

impl From<Id> for BlobRef {
    fn from(id: Id) -> Self {
        Self::Id(id)
    }
}

impl fmt::Display for BlobRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(id) => f.write_str(id.as_ref()),
            Self::Creation(cid) => write!(f, "#{}", cid.as_ref()),
        }
    }
}

impl PartialOrd for BlobRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BlobRef {
    fn cmp(&self, other: &Self) -> Ordering {
        // '#' (0x23) sorts before every Id character, so this is the
        // order of the wire strings.
        match (self, other) {
            (Self::Creation(a), Self::Creation(b)) | (Self::Id(a), Self::Id(b)) => a.cmp(b),
            (Self::Creation(_), Self::Id(_)) => Ordering::Less,
            (Self::Id(_), Self::Creation(_)) => Ordering::Greater,
        }
    }
}

impl Serialize for BlobRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for BlobRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}
