# Agent Instructions — jmap-mail-client

## 🧬 Canonical extension-client template — siblings mirror this one

This crate is the **canonical template for the extension-client family**.
Every other `jmap-*-client` extension crate (chat, calendars, tasks, contacts,
filenode, sharing) is cookie-cut from this one — same module layout, same
`Jmap*Ext` extension-trait shape, same `SessionClient` shape, same
`with_*_session` accessor, same `/get` builder pattern, same `/set` builder
pattern, same Id-validation helper, same doc-comment style, same test
layout. Differences are *only* the spec content (RFC 8621 here; the
calendars draft in calendars-client; etc.).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape an extension-client sibling in a way that diverges from
  this crate, **change this crate first, then propagate** to every other
  extension-client sibling in the same pass.
- If you change this crate, **propagate the change to every extension-client
  sibling** in the same pass (or file a follow-up sweep bead before merging).
- A divergent sibling without a matching change here is a regression of
  the cookie-cutter intent — review will catch it.

## What This Is

RFC 8621 (JMAP for Mail) method implementations on top of `jmap-base-client`.
Implements the standard `/get`, `/changes`, `/set`, `/query`, `/queryChanges`,
`/copy` methods for `Email`, `Mailbox`, `Thread`, `Identity`, `EmailSubmission`,
and `SearchSnippet`, plus `Email/import`, `Email/parse`, and `VacationResponse/*`.

Blob upload / download for attachments composes with `jmap-base-client`'s
`JmapClient::upload` / `JmapClient::download_blob` — not re-exported here.

## Crate Family Context

```
jmap-types
    └── jmap-base-client
            └── jmap-mail-client  ← this crate (canonical extension-client)
                    │
                    ├─ siblings (mirror this crate's shape):
                    ├── jmap-chat-client
                    ├── jmap-calendars-client
                    ├── jmap-tasks-client
                    ├── jmap-contacts-client
                    ├── jmap-filenode-client
                    └── jmap-sharing-client
```

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-mail-client --tests -- -D warnings
cargo test -p jmap-mail-client
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-mail-client --no-deps --all-features
```

Run all four before considering any work done.

## Source Material

- `~/PROJECT/jmap-chat-spec/references/rfc8621.txt` — normative for all
  method signatures, arguments, and response fields
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base get/set/changes/query
  request and response shapes (normative for structural fields)

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add features not in PLAN.md or not explicitly directed
- Any change to this crate is a canonical-template change — propagate to
  the six sibling extension-client crates in the same pass (see workspace
  AGENTS.md "Canonical Templates" + the propagation rule above)
