//! Which blob specification a request speaks (decision 11).

use jmap_blob_types::{JMAP_BLOB_URI, JMAP_BLOB2_URI};

/// The blob specification a request selects through its `using`.
///
/// A server may advertise both `urn:ietf:params:jmap:blob` (RFC 9404) and
/// `urn:ietf:params:jmap:blob2` (draft-ietf-jmap-blobext-03), "but a client
/// MUST NOT include both ... in the using array of the same request"
/// (blobext-03 §2.1).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobMode {
    /// blobext-03 semantics: `blob2` in `using`, or neither URI.
    Blobext,
    /// RFC 9404 semantics: `blob` in `using` and `blob2` not.
    Rfc9404,
    /// Both URIs in `using`: every `Blob/*` method answers
    /// `invalidArguments`.
    Conflict,
}

impl BlobMode {
    /// The mode of a request whose `using` is `using`.
    pub fn from_using(using: &[String]) -> Self {
        let has = |uri: &str| using.iter().any(|u| u == uri);
        match (has(JMAP_BLOB_URI), has(JMAP_BLOB2_URI)) {
            (true, true) => Self::Conflict,
            (true, false) => Self::Rfc9404,
            (false, _) => Self::Blobext,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn using(uris: &[&str]) -> Vec<String> {
        let mut v = vec!["urn:ietf:params:jmap:core".to_owned()];
        v.extend(uris.iter().map(|u| (*u).to_owned()));
        v
    }

    #[test]
    fn modes() {
        let blob = "urn:ietf:params:jmap:blob";
        let blob2 = "urn:ietf:params:jmap:blob2";
        assert_eq!(BlobMode::from_using(&using(&[blob2])), BlobMode::Blobext);
        assert_eq!(BlobMode::from_using(&using(&[blob])), BlobMode::Rfc9404);
        assert_eq!(
            BlobMode::from_using(&using(&[blob, blob2])),
            BlobMode::Conflict
        );
        assert_eq!(BlobMode::from_using(&using(&[])), BlobMode::Blobext);
        // A prefix or near-miss is not the URI.
        assert_eq!(
            BlobMode::from_using(&using(&["urn:ietf:params:jmap:blob3"])),
            BlobMode::Blobext
        );
    }
}
