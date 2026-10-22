//! RSVP helpers for `urn:ietf:params:jmap:chat:cap:calendar-event` MessageActions
//! (draft-atwood-jmap-chat-calendars-00 §RSVP Handling).
//!
//! The draft defines no RSVP method: the client patches
//! `participants/{participantKey}/participationStatus` on the referenced
//! CalendarEvent through the existing JMAP Calendars `CalendarEvent/set`.
//! These helpers build that patch and find the user's own participant key;
//! the caller submits the patch with its JMAP Calendars client.

use jmap_types::PatchObject;
use serde_json::{Map, Value};

/// A participation status a client may RSVP with.
///
/// draft-atwood-jmap-chat-calendars-00 limits the RSVP patch to one of
/// `"accepted"`, `"declined"`, `"tentative"` or `"delegated"`
/// (RFC 8984 §4.4.6 `participationStatus`).
///
/// `participationStatus` is an IANA-registry-extensible set (RFC 8984
/// §4.4.6 also defines `"needs-action"`), so this enum is
/// `#[non_exhaustive]` and a `match` outside this crate needs a wildcard
/// arm:
///
/// ```compile_fail,E0004
/// use jmap_chat_client::RsvpStatus;
/// fn f(s: RsvpStatus) -> u8 {
///     match s {
///         RsvpStatus::Accepted => 0,
///         RsvpStatus::Declined => 1,
///         RsvpStatus::Tentative => 2,
///         RsvpStatus::Delegated => 3,
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RsvpStatus {
    /// `"accepted"`: the participant will participate.
    Accepted,
    /// `"declined"`: the participant will not participate.
    Declined,
    /// `"tentative"`: the participant may participate.
    Tentative,
    /// `"delegated"`: the participant has delegated attendance (see the
    /// participant's `delegatedTo`).
    Delegated,
}

impl RsvpStatus {
    /// The RFC 8984 `participationStatus` wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::Tentative => "tentative",
            Self::Delegated => "delegated",
        }
    }
}

/// Build the `CalendarEvent/set` update patch that sets the participant
/// `participant_key`'s `participationStatus` to `status`.
///
/// The patch key is `participants/{participant_key}/participationStatus`.
/// RFC 8620 §5.3 patch keys are JSON Pointers, so `~` and `/` in
/// `participant_key` are escaped as `~0` and `~1` (RFC 6901 §3).
pub fn rsvp_patch(participant_key: &str, status: RsvpStatus) -> PatchObject {
    let escaped = participant_key.replace('~', "~0").replace('/', "~1");
    let mut map = Map::new();
    map.insert(
        format!("participants/{escaped}/participationStatus"),
        Value::String(status.as_str().to_owned()),
    );
    PatchObject::from_map(map)
}

/// Find the key of the user's own entry in a CalendarEvent `participants`
/// map.
///
/// `identity_addresses` are the `calendarAddress` values of the user's
/// ParticipantIdentity objects. A participant matches when its
/// `calendarAddress` equals one of them after RFC 3986 §6.2.2 syntax-based
/// normalisation (draft-ietf-jmap-calendars-26 §3). Returns the first
/// matching key in map order, or `None` when `participants` is not an
/// object or nothing matches.
pub fn own_participant_key<I>(participants: &Value, identity_addresses: I) -> Option<&str>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let own: Vec<String> = identity_addresses
        .into_iter()
        .map(|a| normalize_uri(a.as_ref()))
        .collect();
    participants.as_object()?.iter().find_map(|(key, p)| {
        let addr = normalize_uri(p.get("calendarAddress")?.as_str()?);
        own.contains(&addr).then_some(key.as_str())
    })
}

/// RFC 3986 §6.2.2 syntax-based normalisation: lowercase scheme and host,
/// percent-encoding normalisation, and dot-segment removal from the path.
/// A string without a valid scheme is returned percent-normalised only.
fn normalize_uri(uri: &str) -> String {
    let uri = normalize_percent(uri);
    let Some((scheme, rest)) = uri.split_once(':') else {
        return uri;
    };
    let valid_scheme = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid_scheme {
        return uri;
    }
    let mut out = scheme.to_ascii_lowercase();
    out.push(':');
    let mut rest = rest;
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        let (authority, tail) = after.split_at(end);
        let host_start = authority.rfind('@').map_or(0, |i| i + 1);
        out.push_str("//");
        out.push_str(&authority[..host_start]);
        out.push_str(&lowercase_outside_triplets(&authority[host_start..]));
        rest = tail;
    }
    let path_end = rest.find(['?', '#']).unwrap_or(rest.len());
    out.push_str(&remove_dot_segments(&rest[..path_end]));
    out.push_str(&rest[path_end..]);
    out
}

/// Uppercase percent-encoding hex digits and decode triplets that encode
/// an unreserved character (RFC 3986 §6.2.2.1, §6.2.2.2).
fn normalize_percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let byte = tail
            .get(1..3)
            .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match byte {
            Some(b) if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') => {
                out.push(char::from(b));
                rest = &tail[3..];
            }
            Some(_) => {
                out.push('%');
                out.push_str(&tail[1..3].to_ascii_uppercase());
                rest = &tail[3..];
            }
            None => {
                out.push('%');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Lowercase a host, leaving the hex digits of percent triplets uppercase.
fn lowercase_outside_triplets(s: &str) -> String {
    let mut skip = 0;
    s.chars()
        .map(|c| {
            if skip > 0 {
                skip -= 1;
                c
            } else if c == '%' {
                skip = 2;
                c
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// The remove_dot_segments algorithm of RFC 3986 §5.2.4.
fn remove_dot_segments(path: &str) -> String {
    fn pop_segment(out: &mut String) {
        match out.rfind('/') {
            Some(i) => out.truncate(i),
            None => out.clear(),
        }
    }
    let mut input = path;
    let mut out = String::with_capacity(path.len());
    while !input.is_empty() {
        if let Some(r) = input
            .strip_prefix("../")
            .or_else(|| input.strip_prefix("./"))
        {
            input = r;
        } else if input.starts_with("/./") {
            input = &input[2..];
        } else if input == "/." {
            input = "/";
        } else if input.starts_with("/../") {
            input = &input[3..];
            pop_segment(&mut out);
        } else if input == "/.." {
            input = "/";
            pop_segment(&mut out);
        } else if input == "." || input == ".." {
            input = "";
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = input[start..].find('/').map_or(input.len(), |i| i + start);
            out.push_str(&input[..end]);
            input = &input[end..];
        }
    }
    out
}
