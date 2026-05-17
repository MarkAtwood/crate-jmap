# Agent Instructions — jmap-chat-client

## 🧬 Sibling of the canonical jmap-mail-client — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-client`
extension-client template**. Module layout, `Jmap*Ext` extension-trait
shape, `SessionClient` shape, `with_*_session` accessor, `/get` and
`/set` builder patterns, Id-validation helpers, doc-comment style,
and test layout must mirror `jmap-mail-client`. Differences are
*only* the spec content (the JMAP Chat draft here; RFC 8621 in
mail-client).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape this crate in a way that diverges from
  `jmap-mail-client`, **change `jmap-mail-client` first, then
  propagate** to every other extension-client sibling in the same
  pass.
- If `jmap-mail-client` changes, propagate the change here in the
  same pass (or file a follow-up sweep bead before merging).
- A divergent sibling without a matching change on the canonical
  is a regression of the cookie-cutter intent — review will catch
  it.

## What This Is

JMAP Chat extension client methods (draft-atwood-jmap-chat-00) on
top of `jmap-base-client`. Implements `Chat/*`, `Message/*`, `Space/*`,
`ChatContact/*`, `ReadPosition/*`, push subscriptions, and the JMAP
Chat WebSocket transport.

## Crate Family Context

```
jmap-types
    └── jmap-base-client
            ├── jmap-mail-client  (canonical extension-client)
            └── jmap-chat-client  ← this crate (sibling)
                ↑ also depends on jmap-chat-types for wire types
```

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-chat-client --tests -- -D warnings
cargo test -p jmap-chat-client
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-chat-client --no-deps --all-features
```

Run all four before considering any work done.

## Source Material

- `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md` — normative
  for all method signatures, arguments, and response fields
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base
  get/set/changes/query request and response shapes (normative for
  structural fields)

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace
  AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add features not in PLAN.md or not explicitly directed
- Any change to this crate that diverges from the canonical
  `jmap-mail-client` shape requires landing the canonical change
  first and propagating to the six sibling extension-client crates
  (see workspace AGENTS.md "Canonical Templates" + the propagation
  rule above)
