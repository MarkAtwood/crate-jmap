//! Slack reaction names and JMAP Reaction `emoji` values.
//!
//! Slack names reactions by shortcode (`thumbsup`, no colons;
//! <https://docs.slack.dev/reference/methods/reactions.add/> `name`). A JMAP
//! Reaction's `emoji` is "Typically a Unicode emoji sequence or a
//! deployment-defined token" (draft-atwood-jmap-chat-00 §Reaction). The
//! common shortcodes below map to Unicode both ways; every other name,
//! custom workspace emoji included, is carried as the token `:name:`.

/// (Slack name, Unicode). Names from Slack's standard emoji set.
const TABLE: &[(&str, &str)] = &[
    ("+1", "\u{1F44D}"),
    ("thumbsup", "\u{1F44D}"),
    ("-1", "\u{1F44E}"),
    ("thumbsdown", "\u{1F44E}"),
    ("heart", "\u{2764}\u{FE0F}"),
    ("joy", "\u{1F602}"),
    ("smile", "\u{1F604}"),
    ("laughing", "\u{1F606}"),
    ("tada", "\u{1F389}"),
    ("eyes", "\u{1F440}"),
    ("pray", "\u{1F64F}"),
    ("fire", "\u{1F525}"),
    ("white_check_mark", "\u{2705}"),
    ("rocket", "\u{1F680}"),
    ("100", "\u{1F4AF}"),
    ("wave", "\u{1F44B}"),
    ("clap", "\u{1F44F}"),
    ("thinking_face", "\u{1F914}"),
    ("cry", "\u{1F622}"),
    ("open_mouth", "\u{1F62E}"),
];

/// The JMAP `emoji` for Slack reaction `name`.
pub(crate) fn from_slack(name: &str) -> String {
    // Skin-tone variants (`+1::skin-tone-2`) keep their base mapping only
    // when the base is in the table; otherwise the whole name is a token.
    TABLE
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| format!(":{name}:"), |(_, u)| (*u).to_owned())
}

/// The Slack reaction name for JMAP `emoji`, or `None` when it has none.
pub(crate) fn to_slack(emoji: &str) -> Option<String> {
    if let Some(name) = emoji
        .strip_prefix(':')
        .and_then(|e| e.strip_suffix(':'))
        .filter(|n| !n.is_empty() && !n.contains(char::is_whitespace))
    {
        return Some(name.to_owned());
    }
    let bare = emoji.trim_end_matches('\u{FE0F}');
    TABLE
        .iter()
        .find(|(_, u)| u.trim_end_matches('\u{FE0F}') == bare)
        .map(|(n, _)| (*n).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(from_slack("thumbsup"), "\u{1F44D}");
        assert_eq!(to_slack("\u{1F44D}").as_deref(), Some("+1"));
        assert_eq!(from_slack("partyparrot"), ":partyparrot:");
        assert_eq!(to_slack(":partyparrot:").as_deref(), Some("partyparrot"));
        // U+2764 with and without the emoji presentation selector.
        assert_eq!(to_slack("\u{2764}").as_deref(), Some("heart"));
        assert_eq!(to_slack("\u{1F9A9}"), None);
        assert_eq!(to_slack("::"), None);
    }
}
