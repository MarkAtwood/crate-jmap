//! Slack mrkdwn and JMAP Chat bodies.
//!
//! Oracles:
//! - Slack "Formatting message text"
//!   (<https://docs.slack.dev/messaging/formatting-message-text/>): `*bold*`,
//!   `_italic_`, `~strike~`, `` `code` ``, triple-backtick code blocks,
//!   `>` blockquotes, `<url|text>` links, `<@U…>` user mentions, `<#C…>`
//!   channel links, `<!here>` / `<!channel>` / `<!everyone>`,
//!   `<!subteam^ID>`, `<!date^…|fallback>`; "Replace `&` with `&amp;`, `<`
//!   with `&lt;`, and `>` with `&gt;`" and escape only those three.
//! - draft-atwood-jmap-chat-00 §Rich Body Format: span types `text`,
//!   `bold`, `italic`, `bold-italic`, `code`, `codeblock`, `blockquote`,
//!   `mention` (`userId`), `broadcast` (`scope`), `link` (`uri`, which MUST
//!   NOT be a JMAP object URI); "only those arrays take effect" for
//!   `mentions` / `broadcastMentions`.
//!
//! What does not map: strikethrough becomes plain text (no span type);
//! `<#C…>` channel links, user groups and dates become their display text;
//! Block Kit `blocks` and legacy `attachments` are not read.

use jmap_chat_types::{BodyType, BroadcastScope, RichBody, SpanType};
use jmap_types::Id;
use serde_json::{Value, json};

/// Name lookups the conversion needs; the bridge implements it over its
/// mapping.
pub trait Directory {
    /// The `ChatContact` id (or the accountId for the owner) and display
    /// name of Slack user `slack_user`.
    fn contact(&self, slack_user: &str) -> Option<(Id, String)>;
    /// The Slack user id of `ChatContact` (or accountId) `id`.
    fn slack_user(&self, id: &Id) -> Option<String>;
    /// The name of Slack channel `channel`.
    fn channel_name(&self, channel: &str) -> Option<String> {
        let _ = channel;
        None
    }
}

/// A Slack message text as a JMAP Message body.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Converted {
    /// `body`: the plain text, or the JSON-encoded [`RichBody`].
    pub body: String,
    /// `bodyType`: `text/plain` when the text has no markup, mention or
    /// link; `application/jmap-chat-rich` otherwise.
    pub body_type: BodyType,
    /// `mentions`: the `ChatContact` ids of the `<@U…>` users the
    /// [`Directory`] knows, in order, without repeats.
    pub mentions: Vec<Id>,
    /// `broadcastMentions`: `<!here>` → `here`; `<!channel>` and
    /// `<!everyone>` → `everyone`.
    pub broadcast_mentions: Vec<BroadcastScope>,
}

/// Undo Slack's three entity escapes.
pub fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Escape `&`, `<` and `>` for literal display in Slack text.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[derive(Default)]
struct Out {
    spans: Vec<Value>,
    mentions: Vec<Id>,
    broadcasts: Vec<BroadcastScope>,
}

impl Out {
    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut()
            && last["type"] == "text"
            && let Some(t) = last["text"].as_str()
        {
            let joined = format!("{t}{s}");
            last["text"] = Value::String(joined);
            return;
        }
        self.spans.push(json!({ "type": "text", "text": s }));
    }

    fn span(&mut self, kind: &str, text: &str) {
        self.spans.push(json!({ "type": kind, "text": text }));
    }

    /// Drop one trailing newline from the last text span, before a block.
    fn trim_trailing_newline(&mut self) {
        if let Some(last) = self.spans.last_mut()
            && last["type"] == "text"
            && let Some(t) = last["text"].as_str()
            && let Some(stripped) = t.strip_suffix('\n')
        {
            if stripped.is_empty() {
                self.spans.pop();
            } else {
                last["text"] = Value::String(stripped.to_owned());
            }
        }
    }

    fn mention(&mut self, id: Id) {
        if !self.mentions.contains(&id) {
            self.mentions.push(id);
        }
    }

    fn broadcast(&mut self, scope: BroadcastScope) {
        if !self.broadcasts.contains(&scope) {
            self.broadcasts.push(scope);
        }
    }
}

/// Convert Slack mrkdwn `text` (as Slack sends it, entity-escaped) to a
/// JMAP Chat body.
pub fn to_jmap(text: &str, dir: &dyn Directory) -> Converted {
    let mut out = Out::default();
    let mut rest = text;
    let mut after_block = false;
    loop {
        let Some(open) = rest.find("```") else {
            lines(strip_block_newline(rest, after_block), dir, &mut out);
            break;
        };
        let inner = &rest[open + 3..];
        let Some(close) = inner.find("```") else {
            lines(strip_block_newline(rest, after_block), dir, &mut out);
            break;
        };
        lines(
            strip_block_newline(&rest[..open], after_block),
            dir,
            &mut out,
        );
        out.trim_trailing_newline();
        let code = inner[..close].strip_prefix('\n').unwrap_or(&inner[..close]);
        let code = code.strip_suffix('\n').unwrap_or(code);
        out.span("codeblock", &unescape(code));
        rest = &inner[close + 3..];
        after_block = true;
    }
    finish(out)
}

fn strip_block_newline(s: &str, after_block: bool) -> &str {
    if after_block {
        s.strip_prefix('\n').unwrap_or(s)
    } else {
        s
    }
}

fn finish(out: Out) -> Converted {
    let plain = out.spans.iter().all(|s| s["type"] == "text");
    let (body, body_type) = if plain {
        let body: String = out
            .spans
            .iter()
            .filter_map(|s| s["text"].as_str())
            .collect();
        (body, BodyType::Plain)
    } else {
        (json!({ "spans": out.spans }).to_string(), BodyType::Rich)
    };
    Converted {
        body,
        body_type,
        mentions: out.mentions,
        broadcast_mentions: out.broadcasts,
    }
}

fn quote_line(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("&gt;")
        .or_else(|| line.strip_prefix('>'))?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn lines(s: &str, dir: &dyn Directory, out: &mut Out) {
    if s.is_empty() {
        return;
    }
    let all: Vec<&str> = s.split('\n').collect();
    let mut i = 0;
    while i < all.len() {
        if quote_line(all[i]).is_some() {
            let mut quoted = Vec::new();
            while let Some(q) = all.get(i).and_then(|l| quote_line(l)) {
                quoted.push(flatten(q, dir, out));
                i += 1;
            }
            out.trim_trailing_newline();
            out.span("blockquote", &quoted.join("\n"));
            // The newline that ended the quote belongs to the block.
            continue;
        }
        if i > 0 && quote_line(all[i - 1]).is_none() {
            out.text("\n");
        }
        inline(all[i], dir, out);
        i += 1;
    }
}

fn is_boundary(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || (c.is_ascii_punctuation() && c != '`'))
}

/// The byte index of the closing `marker` for an opening one at `open`.
fn closing(line: &str, open: usize, marker: u8) -> Option<usize> {
    let b = line.as_bytes();
    if b.get(open + 1).is_none_or(|c| c.is_ascii_whitespace()) {
        return None;
    }
    let mut j = open + 2;
    while j < b.len() {
        if b[j] == marker
            && !b[j - 1].is_ascii_whitespace()
            && is_boundary(line[j + 1..].chars().next())
        {
            return Some(j);
        }
        j += 1;
    }
    None
}

fn inline(line: &str, dir: &dyn Directory, out: &mut Out) {
    let b = line.as_bytes();
    let mut plain = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'<' => {
                if let Some(end) = line[i + 1..].find('>') {
                    out.text(&unescape(&line[plain..i]));
                    entity(&line[i + 1..i + 1 + end], dir, out);
                    i += end + 2;
                    plain = i;
                    continue;
                }
            }
            b'`' => {
                if let Some(end) = line[i + 1..].find('`').filter(|&e| e > 0) {
                    out.text(&unescape(&line[plain..i]));
                    out.span("code", &unescape(&line[i + 1..i + 1 + end]));
                    i += end + 2;
                    plain = i;
                    continue;
                }
            }
            m @ (b'*' | b'_' | b'~') => {
                let prev = line[..i].chars().next_back();
                if is_boundary(prev)
                    && let Some(close) = closing(line, i, m)
                {
                    out.text(&unescape(&line[plain..i]));
                    let content = &line[i + 1..close];
                    styled(m, content, dir, out);
                    i = close + 1;
                    plain = i;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out.text(&unescape(&line[plain..]));
}

fn styled(marker: u8, content: &str, dir: &dyn Directory, out: &mut Out) {
    let other = match marker {
        b'*' => Some('_'),
        b'_' => Some('*'),
        _ => None,
    };
    let nested = other.and_then(|o| {
        content
            .strip_prefix(o)
            .and_then(|c| c.strip_suffix(o))
            .filter(|c| !c.is_empty())
    });
    match (marker, nested) {
        (b'*' | b'_', Some(inner)) => {
            let t = flatten(inner, dir, out);
            out.span("bold-italic", &t);
        }
        (b'*', None) => {
            let t = flatten(content, dir, out);
            out.span("bold", &t);
        }
        (b'_', None) => {
            let t = flatten(content, dir, out);
            out.span("italic", &t);
        }
        // Strikethrough has no span type: keep the words, lose the style.
        _ => {
            let t = flatten(content, dir, out);
            out.text(&t);
        }
    }
}

/// Plain display text of `s`, with entities replaced by their display text
/// (mentions still recorded).
fn flatten(s: &str, dir: &dyn Directory, out: &mut Out) -> String {
    let mut text = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        let Some(end) = rest[open + 1..].find('>') else {
            break;
        };
        text.push_str(&unescape(&rest[..open]));
        let mut scratch = Out::default();
        entity(&rest[open + 1..open + 1 + end], dir, &mut scratch);
        for span in &scratch.spans {
            text.push_str(span["text"].as_str().unwrap_or_default());
        }
        for m in scratch.mentions {
            out.mention(m);
        }
        for b in scratch.broadcasts {
            out.broadcast(b);
        }
        rest = &rest[open + 1 + end + 1..];
    }
    text.push_str(&unescape(rest));
    text
}

fn entity(content: &str, dir: &dyn Directory, out: &mut Out) {
    let (target, label) = match content.split_once('|') {
        Some((t, l)) => (t, Some(unescape(l))),
        None => (content, None),
    };
    if let Some(user) = target.strip_prefix('@') {
        match dir.contact(user) {
            Some((id, name)) => {
                out.spans.push(json!({
                    "type": "mention",
                    "text": format!("@{name}"),
                    "userId": id.as_ref(),
                }));
                out.mention(id);
            }
            None => out.text(&format!("@{}", label.as_deref().unwrap_or(user))),
        }
    } else if let Some(channel) = target.strip_prefix('#') {
        let name = label
            .or_else(|| dir.channel_name(channel))
            .unwrap_or_else(|| channel.to_owned());
        out.text(&format!("#{name}"));
    } else if let Some(special) = target.strip_prefix('!') {
        let scope = match special {
            "here" => Some((BroadcastScope::Here, "here")),
            "channel" => Some((BroadcastScope::Everyone, "channel")),
            "everyone" => Some((BroadcastScope::Everyone, "everyone")),
            _ => None,
        };
        match scope {
            Some((scope, word)) => {
                let scope_wire = match scope {
                    BroadcastScope::Here => "here",
                    _ => "everyone",
                };
                out.spans.push(json!({
                    "type": "broadcast",
                    "text": format!("@{word}"),
                    "scope": scope_wire,
                }));
                out.broadcast(scope);
            }
            // `<!subteam^ID|@handle>`, `<!date^…|fallback>`: the label.
            None => out.text(&label.unwrap_or_default()),
        }
    } else {
        let uri = unescape(target);
        let text = label.unwrap_or_else(|| uri.clone());
        if jmap_object_uri::is_jmap_uri(&uri) {
            // §Span Object: `uri` MUST NOT be a JMAP object URI.
            out.text(&text);
        } else {
            out.spans
                .push(json!({ "type": "link", "text": text, "uri": uri }));
        }
    }
}

/// Why a JMAP body cannot be sent to Slack.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// A `bodyType` other than `text/plain` and `application/jmap-chat-rich`.
    #[error("bodyType has no Slack form")]
    UnsupportedBodyType,
    /// An `application/jmap-chat-rich` body that is not a [`RichBody`].
    #[error("body is not a valid application/jmap-chat-rich object")]
    InvalidRichBody,
}

/// The Slack text for a JMAP body, and whether to send it with
/// `mrkdwn: true`.
///
/// `mentions` and `broadcast_mentions` are authoritative
/// (draft-atwood-jmap-chat-00 §Rich Body Format). A mention of a known
/// Slack user, or a `here` / `everyone` broadcast, that no span already
/// renders is prepended as its Slack token (`<@U…>`, `<!here>`,
/// `<!channel>`), so Slack notifies the same people. `admins` has no Slack
/// form and is dropped.
///
/// # Errors
///
/// [`RenderError`] for a body type or rich body Slack cannot carry.
pub fn from_jmap(
    body: &str,
    body_type: &BodyType,
    mentions: &[Id],
    broadcast_mentions: &[BroadcastScope],
    dir: &dyn Directory,
) -> Result<(String, bool), RenderError> {
    let mut rendered_users: Vec<String> = Vec::new();
    let mut rendered_scopes: Vec<BroadcastScope> = Vec::new();
    let (text, mrkdwn) = match body_type {
        BodyType::Plain => (escape(body), false),
        BodyType::Rich => {
            let rich: RichBody =
                serde_json::from_str(body).map_err(|_| RenderError::InvalidRichBody)?;
            let mut s = String::new();
            for span in &rich.spans {
                let t = span.text.as_str();
                match &span.kind {
                    SpanType::Bold => s.push_str(&format!("*{}*", escape(t))),
                    SpanType::Italic => s.push_str(&format!("_{}_", escape(t))),
                    SpanType::BoldItalic => s.push_str(&format!("*_{}_*", escape(t))),
                    SpanType::Code => s.push_str(&format!("`{}`", escape(t))),
                    SpanType::Codeblock => {
                        if !s.is_empty() && !s.ends_with('\n') {
                            s.push('\n');
                        }
                        s.push_str(&format!("```\n{}\n```\n", escape(t)));
                    }
                    SpanType::Blockquote => {
                        if !s.is_empty() && !s.ends_with('\n') {
                            s.push('\n');
                        }
                        for line in t.split('\n') {
                            s.push_str(&format!("> {}\n", escape(line)));
                        }
                    }
                    SpanType::Mention => {
                        let user = span
                            .user_id
                            .as_deref()
                            .and_then(|u| dir.slack_user(&Id::from(u)));
                        match user {
                            Some(u) => {
                                s.push_str(&format!("<@{u}>"));
                                rendered_users.push(u);
                            }
                            None => s.push_str(&escape(t)),
                        }
                    }
                    SpanType::Broadcast => match span.scope {
                        Some(BroadcastScope::Here) => {
                            s.push_str("<!here>");
                            rendered_scopes.push(BroadcastScope::Here);
                        }
                        Some(BroadcastScope::Everyone) => {
                            s.push_str("<!channel>");
                            rendered_scopes.push(BroadcastScope::Everyone);
                        }
                        _ => s.push_str(&escape(t)),
                    },
                    SpanType::Link => match span.uri.as_deref() {
                        Some(uri) if !uri.contains(['|', '<', '>']) && !uri.is_empty() => {
                            if t.is_empty() || t == uri {
                                s.push_str(&format!("<{uri}>"));
                            } else {
                                s.push_str(&format!("<{uri}|{}>", escape(t)));
                            }
                        }
                        _ => s.push_str(&escape(t)),
                    },
                    _ => s.push_str(&escape(t)),
                }
            }
            (s.trim_end_matches('\n').to_owned(), true)
        }
        _ => return Err(RenderError::UnsupportedBodyType),
    };
    let mut prefix = String::new();
    for id in mentions {
        if let Some(u) = dir.slack_user(id)
            && !rendered_users.contains(&u)
        {
            prefix.push_str(&format!("<@{u}> "));
            rendered_users.push(u);
        }
    }
    for scope in broadcast_mentions {
        if rendered_scopes.contains(scope) {
            continue;
        }
        match scope {
            BroadcastScope::Here => prefix.push_str("<!here> "),
            BroadcastScope::Everyone => prefix.push_str("<!channel> "),
            _ => continue,
        }
        rendered_scopes.push(scope.clone());
    }
    Ok((format!("{prefix}{text}"), mrkdwn))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir;
    impl Directory for Dir {
        fn contact(&self, u: &str) -> Option<(Id, String)> {
            (u == "U012AB3CD").then(|| (Id::from("contact-1"), "alice".to_owned()))
        }
        fn slack_user(&self, id: &Id) -> Option<String> {
            (id.as_ref() == "contact-1").then(|| "U012AB3CD".to_owned())
        }
    }

    fn spans(c: &Converted) -> Value {
        assert_eq!(c.body_type, BodyType::Rich, "{c:?}");
        let v: Value = serde_json::from_str(&c.body).expect("rich JSON");
        // The body parses as the jmap-chat-types RichBody.
        let _: RichBody = serde_json::from_value(v.clone()).expect("RichBody");
        v["spans"].clone()
    }

    #[test]
    fn plain_text_stays_plain_and_unescaped() {
        let c = to_jmap("a &lt;b&gt; &amp; c", &Dir);
        assert_eq!(c.body_type, BodyType::Plain);
        assert_eq!(c.body, "a <b> & c");
        assert!(c.mentions.is_empty());
    }

    #[test]
    fn styles_from_the_formatting_doc() {
        let c = to_jmap("*bold* _italic_ ~strike~ `code` *_both_*", &Dir);
        assert_eq!(
            spans(&c),
            json!([
                {"type": "bold", "text": "bold"},
                {"type": "text", "text": " "},
                {"type": "italic", "text": "italic"},
                {"type": "text", "text": " strike "},
                {"type": "code", "text": "code"},
                {"type": "text", "text": " "},
                {"type": "bold-italic", "text": "both"},
            ])
        );
    }

    #[test]
    fn markers_inside_words_are_text() {
        let c = to_jmap("snake_case_name and 2*3*4", &Dir);
        assert_eq!(c.body_type, BodyType::Plain, "{c:?}");
        assert_eq!(c.body, "snake_case_name and 2*3*4");
    }

    #[test]
    fn mentions_links_and_broadcasts() {
        let c = to_jmap(
            "<@U012AB3CD> see <https://example.com|the site> <!here> <#C123ABC456|general> <@U999>",
            &Dir,
        );
        assert_eq!(
            spans(&c),
            json!([
                {"type": "mention", "text": "@alice", "userId": "contact-1"},
                {"type": "text", "text": " see "},
                {"type": "link", "text": "the site", "uri": "https://example.com"},
                {"type": "text", "text": " "},
                {"type": "broadcast", "text": "@here", "scope": "here"},
                {"type": "text", "text": " #general @U999"},
            ])
        );
        assert_eq!(c.mentions, vec![Id::from("contact-1")]);
        assert_eq!(c.broadcast_mentions, vec![BroadcastScope::Here]);
    }

    #[test]
    fn channel_and_everyone_are_everyone() {
        let c = to_jmap("<!channel> and <!everyone>", &Dir);
        assert_eq!(c.broadcast_mentions, vec![BroadcastScope::Everyone]);
    }

    #[test]
    fn code_blocks_and_quotes() {
        let c = to_jmap(
            "look:\n```\nfn main() {}\n```\n&gt; quoted\n&gt; more\nafter",
            &Dir,
        );
        assert_eq!(
            spans(&c),
            json!([
                {"type": "text", "text": "look:"},
                {"type": "codeblock", "text": "fn main() {}"},
                {"type": "blockquote", "text": "quoted\nmore"},
                {"type": "text", "text": "after"},
            ])
        );
    }

    #[test]
    fn jmap_uris_are_not_links() {
        let c = to_jmap("<jmap:chat/1|x>", &Dir);
        assert_eq!(c.body_type, BodyType::Plain);
        assert_eq!(c.body, "x");
    }

    #[test]
    fn plain_bodies_are_escaped_without_mrkdwn() {
        let (t, mrkdwn) = from_jmap("1 < 2 & *x*", &BodyType::Plain, &[], &[], &Dir).expect("ok");
        assert_eq!(t, "1 &lt; 2 &amp; *x*");
        assert!(!mrkdwn);
    }

    #[test]
    fn rich_bodies_render_as_mrkdwn() {
        let body = json!({"spans": [
            {"type": "text", "text": "Hello "},
            {"type": "mention", "text": "@alice", "userId": "contact-1"},
            {"type": "text", "text": " and "},
            {"type": "broadcast", "text": "@here", "scope": "here"},
            {"type": "text", "text": ", see "},
            {"type": "code", "text": "foo()"},
            {"type": "bold", "text": "now"},
            {"type": "link", "text": "docs", "uri": "https://example.com/a"},
            {"type": "codeblock", "text": "fn main() {}", "lang": "rust"},
        ]})
        .to_string();
        let (t, mrkdwn) = from_jmap(
            &body,
            &BodyType::Rich,
            &[Id::from("contact-1")],
            &[BroadcastScope::Here],
            &Dir,
        )
        .expect("ok");
        assert!(mrkdwn);
        assert_eq!(
            t,
            "Hello <@U012AB3CD> and <!here>, see `foo()`*now*<https://example.com/a|docs>\n```\nfn main() {}\n```"
        );
    }

    #[test]
    fn unrendered_mentions_are_prepended() {
        let (t, _) = from_jmap(
            "hi",
            &BodyType::Plain,
            &[Id::from("contact-1"), Id::from("unknown")],
            &[BroadcastScope::Everyone, BroadcastScope::Admins],
            &Dir,
        )
        .expect("ok");
        assert_eq!(t, "<@U012AB3CD> <!channel> hi");
    }

    #[test]
    fn other_body_types_are_refused() {
        assert_eq!(
            from_jmap("x", &BodyType::Markdown, &[], &[], &Dir),
            Err(RenderError::UnsupportedBodyType)
        );
        assert_eq!(
            from_jmap("not json", &BodyType::Rich, &[], &[], &Dir),
            Err(RenderError::InvalidRichBody)
        );
    }
}
