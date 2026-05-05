# Agent Instructions — jmap-server

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — full public API, module layout, settled design decisions, migration notes
2. Run `bd ready` — check for open issues before creating new ones
3. Grep kith source for the relevant type/function before implementing it from scratch

## What This Is

A backend-agnostic JMAP server framework library for Rust. Implements RFC 8620 wire protocol,
request parsing, ResultReference resolution, and `Dispatcher` dispatch machinery.

No auth, no method opinions, no capability URIs. Pure protocol layer.

## Crate Family Context

This crate sits at the base of a planned family (naming: `jmap-{extension}-{role}`):

```
jmap-types          (planned) serde/serde_json only — shared wire types
    └── jmap-server           this crate — dispatcher, parse, http response helpers
    └── jmap-mail-types       (planned)
    └── jmap-chat-types       (planned, replaces jmapchat-types)
```

Everything is fluid. See PLAN.md §Crate Family for the full graph.

The existing `crate-jmapchat-server` and `crate-jmapchat-client` will eventually be renamed
to `crate-jmap-chat-server` / `crate-jmap-chat-client` to match the convention.

## Source Material

All types and logic exist in kith — this crate is an extraction, not a greenfield impl.

| Item | Source location in kith |
|---|---|
| `JmapError` | `~/PROJECT/kith/crates/kith-core/src/error.rs` |
| `Invocation`, `JmapRequest`, `JmapResponse` | `~/PROJECT/kith/crates/kith-core/src/jmap.rs` |
| `ResultReference`, `Argument<T>` | `~/PROJECT/kith/crates/kith-core/src/resultref.rs` |
| `parse_request`, `resolve_args` | `~/PROJECT/kith/crates/kith-jmap/src/lib.rs` |
| `Dispatcher`, `JmapHandler` trait | `~/PROJECT/kith/crates/kith-jmap/src/lib.rs` |

**Do NOT copy kith-specific items** into this crate:
- `Role` (Owner/Peer) — Tailscale auth concept
- `Identity` — Tailscale WhoIs result
- Method-role ACL table (`METHOD_ROLES`)
- `AuthError`, `KithError`
- Session/capability structs (those stay in kith-jmap and stoa)

## Spec References

This crate implements **RFC 8620** (JMAP base protocol):

```
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← primary normative reference
~/PROJECT/jmap-chat-spec/references/rfc8621.txt   ← JMAP for Mail (structural analogue)
```

Additional IETF drafts are in `~/PROJECT/jmap-chat-spec/references/` (blobext, quotas, etc.).

The JMAP Chat extension drafts (for `kith`/`stoa` consumers, not this crate):

| Draft | Path |
|---|---|
| Core Chat objects | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md` |
| Push | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md` |
| WebSocket events | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md` |
| Federation | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-federation-00.md` |
| FileNode | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md` |
| CID scheme | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-cid-00.md` |

**Stale copies exist in** `jmap-chat-js/docs/`, `jmap-chat-jsbig/docs/`, and `~/GIT/ideas/`.
Always use `~/PROJECT/jmap-chat-spec/` as the authoritative source.

## Non-Interactive Shell Commands

Shell commands like `cp`, `mv`, `rm` may be aliased with `-i` on this system — use explicit
flags to avoid hanging on confirmation prompts:

```bash
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

Other commands that may prompt:
- `scp` / `ssh` — use `-o BatchMode=yes`
- `apt-get` — use `-y`

## Build & Test

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled — Do Not Revisit)

| Decision | Choice | Rationale |
|---|---|---|
| Async vs sync | Always async (tokio) | No sync path; `maybe-async` is dead weight |
| Auth | Not in dispatcher | Caller's responsibility — do it before `dispatch()` |
| `max_calls` | Parameter to `parse_request` | Each consumer sets its own limit |
| Unknown method | `unknownMethod` error invocation | Not a crash, not a 4xx |
| Panic isolation | `tokio::task::spawn` per handler | Panicking handler → `serverFail`, not crashed task |
| `createdIds` | Accumulated by dispatcher | Per RFC 8620 §3.4 |
| Unsafe code | Forbidden | `#[forbid(unsafe_code)]` at crate root |

## Cross-Crate Consistency

When this crate is consumed by kith or stoa:
- kith re-exports `JmapError`, `JmapRequest`, `JmapResponse`, `Invocation`, `ResultReference`
  from this crate; its own copies are deleted
- stoa deletes `crates/mail/src/jmap/types.rs` and rewrites its dispatcher using `Dispatcher<StoaCallerCtx>`

If you change a public API type or function signature, check that both consumers still compile.

## Subagent Guidance

- Spawn subagents for parallel extraction work on independent modules (`types.rs` vs `parse.rs`)
- Never spawn two subagents editing the same file — serialize those
- Each subagent reads only what it needs — grep for specific symbols; do not dump full files
- For consistency checks between kith source and extracted code, give the subagent both files explicitly
- If a subagent makes 3 failed attempts at the same error without progress, stop and escalate

## Restrictions

- Do not commit or push without explicit user approval
- Do not use TodoWrite or markdown task lists — use `bd create` for all tracking
- Do not add features not in PLAN.md or not explicitly directed
- Do not introduce dependencies beyond: serde, serde_json, tokio, http, thiserror
- Do not add auth logic, role checks, capability structs, or application-specific types
