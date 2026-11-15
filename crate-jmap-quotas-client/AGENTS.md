# Agent Instructions — jmap-quotas-client

## 🧬 Sibling of the canonical jmap-mail-client — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-client`
extension-client template**. Module layout, `SessionClient` shape,
extension-trait pattern, error handling, and test layout must mirror `jmap-mail-client`. Differences are *only* the spec
content (RFC 9425 here; RFC 8621 in mail-client).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape this crate in a way that diverges from
  `jmap-mail-client`, **change `jmap-mail-client` first, then
  propagate** to every other extension-client sibling in the same pass.
- If `jmap-mail-client` changes, propagate the change here in the same
  pass (or file a follow-up sweep bead before merging).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — module layout
2. Read the relevant RFC 9425 section for the method you are changing
3. Cross-check `~/PROJECT/crate-jmap/crate-jmap-mail-client/` for the
   client pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Quotas client (RFC 9425): `JmapQuotasExt::with_quotas_session` and
typed `quota_get`, `quota_get_properties`, `quota_changes`, `quota_query`
and `quota_query_changes` on `SessionClient`, over `jmap-base-client`.

`using` is `core`, `quota` and the type capabilities: every Session
capability by default, or those given to `with_type_capabilities`. RFC 9425
section 4.1 hides a Quota whose types the request does not cover, so a
`using` of only `core` and `quota` would show none.

## Crate Family Context

```
jmap-base-client
    └── jmap-quotas-client  ← this crate (sibling of canonical jmap-mail-client)
jmap-quotas-types           data types
```

## Spec Reference

```
~/PROJECT/IETF/specs/rfc9425.txt    ← normative (JMAP Quotas)
~/PROJECT/IETF/specs/rfc8620.txt    ← base protocol
```

## Non-Interactive Shell Commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-quotas-client --all-targets -- -D warnings
cargo test -p jmap-quotas-client
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-quotas-client --all-features --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, jmap-quotas-types, jmap-base-client, serde, serde_json; tokio and wiremock dev-only |
| Tests | wiremock, with RFC 9425 section 5 exchanges as oracles |

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond those listed above
- Do not add `Quota/set` or other methods not in RFC 9425 unless explicitly directed
