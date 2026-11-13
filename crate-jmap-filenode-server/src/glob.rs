//! Glob matching for the `nameMatch` and `mediaTypeMatch` FileNode/query
//! conditions of [`crate::memory::MemoryBackend`].
//!
//! draft-ietf-jmap-filenode-20 §3.2.5 defines the syntax: "`*` matches
//! any sequence of characters, `?` matches any single character, and a
//! pair of brackets matches any single character in the bracketed set
//! (for example, `[abc]` matches "a", "b", or "c"; `[a-z]` matches any
//! letter from "a" to "z"; `[!abc]` or `[^abc]` matches any character not
//! in the set). All other characters match literally. The match is
//! case-insensitive." §1.1 makes every case-insensitive name comparison
//! the `i;unicode-casemap` collation, so literals compare in their RFC 5051
//! §2 prepared form ([`unicode_casemap_prepare`]).
//!
//! A character of the matched text is the unit for `?` and brackets, so
//! `?` matches "é" whether or not preparation decomposes it. A run of
//! literal pattern characters matches a run of whole text characters
//! whose prepared forms concatenate to the run's prepared form; this is
//! how a fullwidth "ｒｅｐｏｒｔ" matches the literal "report".

use jmap_metadata_server::unicode_casemap_prepare;

/// One element of a parsed pattern.
#[derive(Debug)]
enum Token {
    /// `*`: any sequence of text characters, including none.
    Star,
    /// `?`: exactly one text character.
    Any,
    /// `[...]`: one text character in (or, when negated, not in) the set.
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
    /// A run of literal characters, held in prepared form.
    Literal(String),
}

/// One member of a bracket expression.
#[derive(Debug)]
enum ClassItem {
    /// A single character, held in prepared form.
    Char(String),
    /// An inclusive code point range.
    Range(char, char),
}

impl ClassItem {
    /// Whether text character `c`, whose prepared form is `prepared`, is
    /// a member. A range is matched case-insensitively by also trying
    /// `c`'s single-character lower- and uppercase forms, so `[a-z]`
    /// matches "Q".
    fn contains(&self, c: char, prepared: &str) -> bool {
        match self {
            ClassItem::Char(member) => member == prepared,
            ClassItem::Range(lo, hi) => {
                let in_range = |x: char| *lo <= x && x <= *hi;
                in_range(c)
                    || single(c.to_lowercase()).is_some_and(in_range)
                    || single(c.to_uppercase()).is_some_and(in_range)
            }
        }
    }
}

/// The one item of `iter`, or `None` when it yields zero or several.
fn single(mut iter: impl Iterator<Item = char>) -> Option<char> {
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

fn prepare_char(c: char) -> String {
    let mut buf = [0_u8; 4];
    unicode_casemap_prepare(c.encode_utf8(&mut buf))
}

/// Parse `pattern` into tokens. A `[` with no closing `]` is a literal,
/// as is a `]` straight after the opening `[` or `[!`/`[^`; a `-` first or
/// last in a set is a member, not a range.
fn parse(pattern: &str) -> Vec<Token> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut i = 0;
    let flush = |literal: &mut String, tokens: &mut Vec<Token>| {
        if !literal.is_empty() {
            tokens.push(Token::Literal(unicode_casemap_prepare(literal)));
            literal.clear();
        }
    };
    while let Some(&c) = chars.get(i) {
        match c {
            '*' => {
                flush(&mut literal, &mut tokens);
                // Adjacent stars match the same as one.
                if !matches!(tokens.last(), Some(Token::Star)) {
                    tokens.push(Token::Star);
                }
                i += 1;
            }
            '?' => {
                flush(&mut literal, &mut tokens);
                tokens.push(Token::Any);
                i += 1;
            }
            '[' => match parse_class(&chars, i + 1) {
                Some((token, next)) => {
                    flush(&mut literal, &mut tokens);
                    tokens.push(token);
                    i = next;
                }
                None => {
                    literal.push(c);
                    i += 1;
                }
            },
            _ => {
                literal.push(c);
                i += 1;
            }
        }
    }
    flush(&mut literal, &mut tokens);
    tokens
}

/// Parse the bracket expression whose body starts at `start` (just past
/// the `[`). Returns the token and the index past the closing `]`, or
/// `None` when the set is never closed.
fn parse_class(chars: &[char], start: usize) -> Option<(Token, usize)> {
    let mut i = start;
    let negated = matches!(chars.get(i), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let body_start = i;
    let mut items = Vec::new();
    loop {
        let c = *chars.get(i)?;
        if c == ']' && i > body_start {
            return Some((Token::Class { negated, items }, i + 1));
        }
        match (chars.get(i + 1), chars.get(i + 2)) {
            (Some('-'), Some(&hi)) if hi != ']' => {
                items.push(ClassItem::Range(c, hi));
                i += 3;
            }
            _ => {
                items.push(ClassItem::Char(prepare_char(c)));
                i += 1;
            }
        }
    }
}

/// Whether `text` matches the glob `pattern`, case-insensitively under
/// `i;unicode-casemap` (see the module docs for the syntax).
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let tokens = parse(pattern);
    let chars: Vec<char> = text.chars().collect();
    let prepared: Vec<String> = chars.iter().map(|&c| prepare_char(c)).collect();
    let mut memo = vec![None; (tokens.len() + 1) * (chars.len() + 1)];
    Matcher {
        tokens: &tokens,
        chars: &chars,
        prepared: &prepared,
        memo: &mut memo,
    }
    .matches(0, 0)
}

/// Memoised matcher state: `memo[ti * (chars.len() + 1) + ci]` caches
/// whether `tokens[ti..]` matches `chars[ci..]`, which bounds the work at
/// `O(tokens * chars)` states.
struct Matcher<'a> {
    tokens: &'a [Token],
    chars: &'a [char],
    prepared: &'a [String],
    memo: &'a mut [Option<bool>],
}

impl Matcher<'_> {
    fn matches(&mut self, ti: usize, ci: usize) -> bool {
        let slot = ti * (self.chars.len() + 1) + ci;
        if let Some(Some(known)) = self.memo.get(slot) {
            return *known;
        }
        let result = self.compute(ti, ci);
        if let Some(entry) = self.memo.get_mut(slot) {
            *entry = Some(result);
        }
        result
    }

    fn compute(&mut self, ti: usize, ci: usize) -> bool {
        let Some(token) = self.tokens.get(ti) else {
            return ci == self.chars.len();
        };
        match token {
            Token::Star => (ci..=self.chars.len()).any(|next| self.matches(ti + 1, next)),
            Token::Any => ci < self.chars.len() && self.matches(ti + 1, ci + 1),
            Token::Class { negated, items } => {
                let (Some(&c), Some(prepared)) = (self.chars.get(ci), self.prepared.get(ci)) else {
                    return false;
                };
                let member = items.iter().any(|item| item.contains(c, prepared));
                member != *negated && self.matches(ti + 1, ci + 1)
            }
            Token::Literal(literal) => {
                let mut rest = literal.as_str();
                let mut next = ci;
                while !rest.is_empty() {
                    let Some(piece) = self.prepared.get(next) else {
                        return false;
                    };
                    match rest.strip_prefix(piece.as_str()) {
                        Some(after) => rest = after,
                        None => return false,
                    }
                    next += 1;
                }
                self.matches(ti + 1, next)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    /// §3.2.5: "* matches any sequence of characters, ? matches any
    /// single character".
    #[test]
    fn star_and_question_mark() {
        assert!(glob_match("report-*", "report-2026.pdf"));
        assert!(glob_match("report-*", "report-"));
        assert!(glob_match("*.pdf", "a.pdf"));
        assert!(!glob_match("*.pdf", "a.pdf.txt"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*a*b*", "xxaxxbxx"));
        assert!(!glob_match("*a*b*", "xxbxxaxx"));
    }

    /// "[abc] matches "a", "b", or "c"; [a-z] matches any letter from "a"
    /// to "z"; [!abc] or [^abc] matches any character not in the set".
    #[test]
    fn bracket_sets() {
        assert!(glob_match("[abc]x", "bx"));
        assert!(!glob_match("[abc]x", "dx"));
        assert!(glob_match("v[0-9]", "v7"));
        assert!(!glob_match("v[0-9]", "vx"));
        assert!(glob_match("[!abc]x", "dx"));
        assert!(!glob_match("[!abc]x", "ax"));
        assert!(glob_match("[^abc]x", "dx"));
        assert!(!glob_match("[^abc]x", "cx"));
    }

    /// "All other characters match literally": an unclosed `[` is a
    /// literal, `]` first in a set is a member, and `-` at either end of a
    /// set is a member.
    #[test]
    fn bracket_edge_cases_are_literal() {
        assert!(glob_match("a[b", "a[b"));
        assert!(!glob_match("a[b", "ab"));
        assert!(glob_match("[]]", "]"));
        assert!(glob_match("[!]]", "x"));
        assert!(!glob_match("[!]]", "]"));
        assert!(glob_match("[-a]", "-"));
        assert!(glob_match("[a-]", "-"));
    }

    /// "The match is case-insensitive", for literals and for sets.
    #[test]
    fn case_insensitive() {
        assert!(glob_match("REPORT-*", "report-1"));
        assert!(glob_match("[a-z]1", "Q1"));
        assert!(glob_match("[A-Z]1", "q1"));
        assert!(glob_match("[x]", "X"));
        assert!(!glob_match("[!x]", "X"));
    }

    /// §1.1 makes the comparison `i;unicode-casemap` (RFC 5051 §2:
    /// titlecase, then NFKD), not a plain lowercase fold: a fullwidth name
    /// matches its ASCII literal, and a decomposed and a precomposed "é"
    /// are the same character.
    #[test]
    fn unicode_casemap_not_lowercase() {
        assert!(glob_match(
            "report*",
            "\u{FF52}\u{FF45}\u{FF50}\u{FF4F}\u{FF52}\u{FF54}.pdf"
        ));
        assert!(glob_match("caf\u{E9}", "CAFE\u{301}"));
    }

    /// A text character is the unit for `?` and sets, even when its
    /// prepared form is several code points.
    #[test]
    fn question_mark_is_one_text_character() {
        assert!(glob_match("caf?", "caf\u{E9}"));
        assert!(glob_match("caf[\u{E9}]", "CAF\u{C9}"));
    }

    /// The memoised matcher stays fast on a pattern that defeats naive
    /// backtracking.
    #[test]
    fn many_stars_do_not_explode() {
        let text = "a".repeat(200);
        let pattern = format!("{}b", "*a".repeat(30));
        assert!(!glob_match(&pattern, &text));
    }
}
