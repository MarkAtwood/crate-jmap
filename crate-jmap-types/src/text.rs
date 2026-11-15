//! Text handling for display text (draft-atwood-jmap-updates-00 §6).
//!
//! - [`truncate_graphemes`]: shorten text at an extended grapheme cluster
//!   boundary (UAX #29) within an octet limit (§6.5), for previews,
//!   notification bodies and push payloads.
//! - [`check_display_text`]: the display-text profile of §6.2 (the
//!   one- and two-dimensional Modern Network Unicode rules). It is a SHOULD
//!   on content and a MAY for rejection; the crates offer the check and do
//!   not enforce it by default (gap G4 in `docs/design/updates.md`).
//!
//! Servers SHOULD NOT normalize text they store and return (§6.3), and
//! SHOULD give display text no meaning (§6.4): nothing here normalizes.

use std::fmt;

use icu_segmenter::GraphemeClusterSegmenter;

/// The longest prefix of `s` that is at most `max_octets` bytes of UTF-8
/// and ends at an extended grapheme cluster boundary (UAX #29), so no code
/// point, combining sequence, emoji sequence or Hangul syllable is split
/// (§6.5). Returns `s` unchanged when it already fits; the empty string
/// when even the first cluster does not.
///
/// ```rust
/// use jmap_types::text::truncate_graphemes;
///
/// // "e" + U+0301 COMBINING ACUTE ACCENT is one cluster of 3 octets.
/// assert_eq!(truncate_graphemes("ae\u{301}b", 3), "a");
/// assert_eq!(truncate_graphemes("ae\u{301}b", 4), "ae\u{301}");
/// assert_eq!(truncate_graphemes("short", 100), "short");
/// ```
pub fn truncate_graphemes(s: &str, max_octets: usize) -> &str {
    if s.len() <= max_octets {
        return s;
    }
    let mut cut = 0;
    for boundary in GraphemeClusterSegmenter::new().segment_str(s) {
        if boundary > max_octets {
            break;
        }
        cut = boundary;
    }
    &s[..cut]
}

/// [`truncate_graphemes`] for a limit in characters (Unicode scalar
/// values) rather than octets, as the RFC 8621 §4.1.4 Email `preview`
/// ("up to 256 characters") states it: the longest prefix of at most
/// `max_chars` characters that ends at a grapheme cluster boundary.
///
/// ```rust
/// use jmap_types::text::truncate_graphemes_chars;
///
/// assert_eq!(truncate_graphemes_chars("ae\u{301}b", 2), "a");
/// assert_eq!(truncate_graphemes_chars("ae\u{301}b", 3), "ae\u{301}");
/// ```
pub fn truncate_graphemes_chars(s: &str, max_chars: usize) -> &str {
    let limit = s.char_indices().nth(max_chars).map_or(s.len(), |(i, _)| i);
    truncate_graphemes(s, limit)
}

/// Why a string breaks the display-text profile of
/// draft-atwood-jmap-updates-00 §6.2.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTextError {
    /// Byte offset of the offending character.
    pub index: usize,
    /// The offending character.
    pub ch: char,
}

impl fmt::Display for DisplayTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "display text has U+{:04X} at byte {}, which the display-text profile excludes",
            self.ch as u32, self.index
        )
    }
}

impl std::error::Error for DisplayTextError {}

/// Check `s` against the display-text profile of
/// draft-atwood-jmap-updates-00 §6.2: no control characters U+0000 to
/// U+001F and U+007F to U+009F (except U+000A LINE FEED when `multiline`,
/// so lines end with LF alone and CR is rejected), no U+2028 LINE
/// SEPARATOR or U+2029 PARAGRAPH SEPARATOR, no noncharacters U+FFFE and
/// U+FFFF, and no leading U+FEFF byte order mark.
///
/// A server MAY reject display text that fails with an
/// `invalidProperties` SetError naming the property. A specification may
/// require a stricter profile, as RFC 8621 does for Mailbox names.
///
/// # Errors
///
/// [`DisplayTextError`] naming the first offending character.
///
/// ```rust
/// use jmap_types::text::check_display_text;
///
/// assert!(check_display_text("Team contacts", false).is_ok());
/// assert!(check_display_text("line 1\nline 2", true).is_ok());
/// assert!(check_display_text("line 1\nline 2", false).is_err());
/// assert!(check_display_text("line 1\r\nline 2", true).is_err());
/// ```
pub fn check_display_text(s: &str, multiline: bool) -> Result<(), DisplayTextError> {
    for (index, ch) in s.char_indices() {
        let u = ch as u32;
        let bad = match u {
            0x0A => !multiline,
            0x00..=0x1F | 0x7F..=0x9F => true,
            0x2028 | 0x2029 | 0xFFFE | 0xFFFF => true,
            0xFEFF => index == 0,
            _ => false,
        };
        if bad {
            return Err(DisplayTextError { index, ch });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UAX #29 examples: an emoji ZWJ sequence (GB11), a regional-indicator
    /// flag (GB12/13) and a Hangul syllable of conjoining jamo (GB6-8) are
    /// each one cluster and are never split.
    #[test]
    fn never_splits_clusters() {
        // U+1F469 U+200D U+1F4BB (woman technologist): 4 + 3 + 4 octets.
        let tech = "\u{1F469}\u{200D}\u{1F4BB}";
        assert_eq!(truncate_graphemes(&format!("{tech}x"), 10), "");
        assert_eq!(truncate_graphemes(&format!("{tech}x"), 11), tech);
        // U+1F1E9 U+1F1EA (flag DE): 8 octets.
        let de = "\u{1F1E9}\u{1F1EA}";
        assert_eq!(truncate_graphemes(&format!("a{de}"), 8), "a");
        // U+1100 U+1161 U+11A8 (Hangul L V T): 9 octets.
        let gak = "\u{1100}\u{1161}\u{11A8}";
        assert_eq!(truncate_graphemes(&format!("{gak}{gak}"), 12), gak);
    }

    /// §6.5: limits are in octets; a CR LF pair is one cluster (GB3).
    #[test]
    fn crlf_is_one_cluster() {
        assert_eq!(truncate_graphemes("a\r\nb", 2), "a");
        assert_eq!(truncate_graphemes("a\r\nb", 3), "a\r\n");
    }

    #[test]
    fn zero_limit() {
        assert_eq!(truncate_graphemes("abc", 0), "");
    }

    /// §6.2 profile, character by character.
    #[test]
    fn display_text_profile() {
        for (s, multi) in [
            ("tab\there", true),
            ("nul\0", true),
            ("del\u{7F}", true),
            ("c1\u{85}", true),
            ("ls\u{2028}", true),
            ("ps\u{2029}", true),
            ("nc\u{FFFE}", true),
            ("nc\u{FFFF}", true),
            ("\u{FEFF}bom", true),
            ("cr\r", true),
        ] {
            assert!(check_display_text(s, multi).is_err(), "{s:?}");
        }
        // A U+FEFF that is not leading is a ZWNBSP, not a BOM.
        assert!(check_display_text("a\u{FEFF}b", false).is_ok());
        // Non-ASCII text and other format characters are fine.
        assert!(check_display_text("Équipe \u{200D} 東京", false).is_ok());
        let err = check_display_text("ab\u{2028}", false).unwrap_err();
        assert_eq!((err.index, err.ch), (2, '\u{2028}'));
    }
}
