// jmap-chat-client — auth-agnostic JMAP Chat HTTP client with WebSocket and SSE support.
// See PLAN.md for the full implementation plan.

/// Extension trait adding JMAP Chat methods to `JmapClient` (from `jmap-client`).
///
/// Import this trait to use: `use jmap_chat_client::JmapChatExt;`
///
/// The `impl JmapChatExt for JmapClient` block will be added once `jmap-client`
/// is wired as a dependency (see PLAN.md "Key Design Decisions", item 3).
pub trait JmapChatExt {
    // Methods will be added in implementation beads.
}
