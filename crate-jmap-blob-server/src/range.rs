//! Range selection and UTF-8 helpers for `Blob/get` (blobext-03 §5).

use std::ops::Range;

/// The octets a `Blob/get` `offset` / `length` selects in a blob of
/// `size` octets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selection {
    /// The octets to return, clamped to the blob.
    pub(crate) range: Range<u64>,
    /// `isTruncated`: the requested range extends past the end.
    pub(crate) is_truncated: bool,
}

/// Select `offset` (null = 0) and `length` (null = to the end) within a
/// blob of `size` octets.
///
/// blobext-03 §5 `isTruncated`: "Set to true if the selected range extends
/// past the end of the blob. In this case, only the bytes from offset to
/// the end of the blob (or an empty string if offset is past the end) are
/// returned. A null length selects everything from offset to the end, so it
/// sets isTruncated only if offset is past the end of the blob."
///
/// `offset + length` is computed with a checked add: an overflow extends
/// past every blob's end.
pub(crate) fn select_range(size: u64, offset: Option<u64>, length: Option<u64>) -> Selection {
    let offset = offset.unwrap_or(0);
    let start = offset.min(size);
    let (end, is_truncated) = match length {
        None => (size, offset > size),
        Some(length) => match offset.checked_add(length) {
            Some(want) if want <= size => (want, false),
            _ => (size, true),
        },
    };
    Selection {
        range: start..end,
        is_truncated,
    }
}

/// The octets as text when they are valid UTF-8 (`data:asText`), or
/// `None`, which the handler writes as `null` with `isEncodingProblem`
/// (blobext-03 §5). A range that cuts a multi-octet sequence is invalid.
pub(crate) fn utf8_text(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(size: u64, offset: Option<u64>, length: Option<u64>) -> (Range<u64>, bool) {
        let s = select_range(size, offset, length);
        (s.range, s.is_truncated)
    }

    /// The `isTruncated` table of blobext-03 §5.
    #[test]
    fn is_truncated_table() {
        // Whole blob.
        assert_eq!(sel(10, None, None), (0..10, false));
        // Null length: truncated only when offset is past the end.
        assert_eq!(sel(10, Some(4), None), (4..10, false));
        assert_eq!(sel(10, Some(10), None), (10..10, false));
        assert_eq!(sel(10, Some(11), None), (10..10, true));
        // Length within the blob.
        assert_eq!(sel(10, Some(4), Some(6)), (4..10, false));
        assert_eq!(sel(10, None, Some(0)), (0..0, false));
        // offset + length past the end.
        assert_eq!(sel(10, Some(4), Some(7)), (4..10, true));
        assert_eq!(sel(10, Some(20), Some(1)), (10..10, true));
        // Empty blob.
        assert_eq!(sel(0, None, None), (0..0, false));
        assert_eq!(sel(0, Some(1), None), (0..0, true));
    }

    #[test]
    fn overflowing_range_is_truncated() {
        assert_eq!(sel(10, Some(u64::MAX), Some(1)), (10..10, true));
        assert_eq!(sel(10, Some(1), Some(u64::MAX)), (1..10, true));
        assert_eq!(sel(u64::MAX, Some(0), Some(u64::MAX)), (0..u64::MAX, false));
    }

    /// RFC 9404 §4.2.1: offset 4, length 9 of the 45-octet fox blob.
    #[test]
    fn rfc9404_range_example() {
        let blob = b"The quick brown fox jumped over the lazy dog.";
        let s = select_range(45, Some(4), Some(9));
        let r = usize::try_from(s.range.start).unwrap()..usize::try_from(s.range.end).unwrap();
        assert_eq!(utf8_text(&blob[r]), Some("quick bro"));
        assert!(!s.is_truncated);
    }

    #[test]
    fn utf8_split_is_invalid() {
        let euro = "€".as_bytes(); // E2 82 AC
        assert_eq!(utf8_text(euro), Some("€"));
        assert_eq!(utf8_text(&euro[..2]), None);
        assert_eq!(utf8_text(b""), Some(""));
    }
}
