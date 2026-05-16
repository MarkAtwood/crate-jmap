# Agent Instructions — jmap-chat-server

## 🧬 Sibling of the canonical jmap-mail-server — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-server`
extension-server template**. Module layout, `Backend` trait shape,
handler-registration pattern, `/set` boilerplate, error-mapping
helpers, and test-harness layout must mirror `jmap-mail-server`.
Differences are *only* the spec content (draft-atwood-jmap-chat-00
here; RFC 8621 in mail-server).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape this crate in a way that diverges from
  `jmap-mail-server`, **change `jmap-mail-server` first, then
  propagate** to every other extension-server sibling in the same
  pass.
- If `jmap-mail-server` changes, propagate the change here in the
  same pass (or file a follow-up sweep bead before merging).
- A divergent sibling without a matching change on the canonical is
  a regression of the cookie-cutter intent — review will catch it.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — `ChatBackend` trait, module layout, handler pattern
2. Read the relevant draft-atwood-jmap-chat-00 section for the method you are implementing
3. Cross-check the canonical sibling `~/PROJECT/crate-jmap/crate-jmap-mail-server/` for the handler/backend pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Chat extension method handlers (draft-atwood-jmap-chat-00). Plugs
into `jmap-server`'s `Dispatcher` via `register_chat_handlers`. Defines
`ChatBackend` trait; consumers provide the storage impl. A reference
in-memory implementation (`memory::MemoryBackend`) is gated behind
the `memory` feature for tests and demos.

## Crate Family Context

```
jmap-types
    ├── jmap-server          dispatcher
    └── jmap-chat-types      data types
            └── jmap-chat-server  ← this crate (sibling of canonical jmap-mail-server)
```

## Spec Reference

```
~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md         ← normative (core objects)
~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md    ← push payloads
~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md     ← WebSocket events
~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-federation-00.md
~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md
~/PROJECT/jmap-chat-spec/references/rfc8620.txt               ← base protocol
```

Before implementing any method, read its section in the relevant
draft. Method names, error conditions, and response shapes must
match exactly.

## Reference Pattern

Study `~/PROJECT/crate-jmap/crate-jmap-mail-server/` (the canonical
extension-server template) for:
- How the `Backend` trait is structured (analog of `ChatBackend`)
- How handlers are registered with `Dispatcher`
- How `MemoryBackend` serves as both harness and example

Do NOT copy mail-specific types or logic.

## Documented divergence from canonical: tests/common backend wrappers

`crate-jmap-chat-server/tests/common/mod.rs` defines three backend
wrappers — `FaultyBackend`, `TrackingBackend`, `IdentityBackend` —
whereas the canonical `jmap-mail-server/tests/common/mod.rs` defines
only `FaultyBackend`. This divergence is intentional and tracked here
so it does not get flagged as a canonical-template regression.

| Wrapper | Mail | Chat | Why chat needs it |
|---|---|---|---|
| `FaultyBackend` | yes | yes | Common: surfaces backend-error mapping in handlers. |
| `TrackingBackend` | no | yes | Call-count introspection for the chat-spec policy hooks `slow_mode_check`, `may_set_custom_emoji`, `is_contact_blocked` — gates RFC 8621 does not have. |
| `IdentityBackend` | no | yes | `CallerCtx = Id` (instead of `()`), so per-test principal_id resolves to a real `Id` and `apply_space_patch`'s identity-sensitive enforcement helpers are exercised. RFC 8621 currently has no identity-sensitive method on `MailBackend`. |

The divergence is justified by spec content, not by harness-design
drift: chat has identity-sensitive surface (role-hierarchy
permissions, `manage_members` / `ban` / `manage_space` /
`manage_channels` gates) that mail-server does not yet have. When
identity-sensitive surface lands in mail-server (e.g. RFC 8621
`$seen` on shared mailboxes via the sharing extension), the matching
wrappers should be added there at that time — not pre-positioned
speculatively now.

The pure-passthrough forwarding inside both wrappers (every method
except 3-4 customizations is a one-line forward to `inner`) is verbose
(~210 lines each) but uses no extra dependencies. A delegate /
ambassador proc-macro would shrink each wrapper to ~30 lines but
would add a dev-dependency; that trade-off is deliberately not made
here.

## Permission enforcement: backend canonical

Per the workspace AGENTS.md "Caller identity (foundation seam)"
section, **backends are canonical for permission enforcement**.
Handlers in this crate do NO permission checking. Defense-in-depth
handler-side pre-checks are allowed but the backend MUST re-verify
atomically with the mutation. A handler that "trusts" a handler-side
check and skips the backend re-check is a bug.

Caller identity is read via the foundation seam
`JmapBackend::principal_id(caller: &Self::CallerCtx) -> Option<&jmap_types::Id>`.
Backends that have not wired identity (test fixtures, single-user
dev servers) return `None`; such backends CANNOT correctly implement
chat role-hierarchy or per-user visibility scoping. Multi-user
production deployments MUST override the default impl.

The chat-specific implications: every Space mutation must be
authorized against the caller's effective permissions resolved
through the Space's role hierarchy. Any additional ownership /
admin model on top of role-based permissions is
implementation-defined (the JMAP Chat draft does not declare a
normative ownership field on `Space`). The handler computes the
candidate mutation; the backend's `apply_space_patch` (or
equivalent) is the canonical point of enforcement.

## Non-Interactive Shell Commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Use `-o BatchMode=yes` for scp/ssh. Use `-y` for apt-get.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-server -- -D warnings
cargo test -p jmap-chat-server
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-chat-server --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Async | Always async (tokio) |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Auth | Not in handlers — caller's responsibility before `dispatch()`; permission enforcement lives in the backend per the section above |
| Dependencies | jmap-types, jmap-chat-types, jmap-server, serde, serde_json, subtle, getrandom (optional, gated behind `memory` feature) |
| Method names | Must match draft-atwood-jmap-chat-00 exactly (`Chat/get`, `Message/set`, `Space/set`, etc.) |
| Test harness | `MemoryBackend` gated behind `feature = "memory"` — HashMap-based, no external deps. Not production. |
| Constant-time secret compares | Invite-code and similar credential comparisons MUST use `subtle::ConstantTimeEq::ct_eq`, never `==` |

## Subagent Guidance

- Spawn subagents for parallel handler work on independent methods (`chat.rs` vs `space.rs`)
- Never spawn two subagents editing the same file
- Give each subagent the relevant draft section and the `ChatBackend` trait explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond: jmap-types, jmap-chat-types, jmap-server, serde, serde_json, subtle, getrandom (optional)
- Do not add auth logic or role checks inside handlers (backend canonical — see section above)
- Do not implement methods not in draft-atwood-jmap-chat-00 unless explicitly directed
