# Agent Instructions — jmap-blob-client

## 🧬 Sibling of the canonical jmap-mail-client — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-client`
extension-client template**. Module layout, `SessionClient` shape,
extension-trait pattern, error handling, and test layout must mirror `jmap-mail-client`. Differences are *only* the spec
content (blobext-03 here; RFC 8621 in mail-client).

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
2. Read the relevant blobext-03 section for the method you are changing
3. Cross-check `~/PROJECT/crate-jmap/crate-jmap-mail-client/` for the
   client pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Blob Management client (draft-ietf-jmap-blobext-03):
`JmapBlobExt::with_blob_session` and typed `blob_set`, `blob_get`,
`blob_lookup` and `blob_convert` on `SessionClient`, over
`jmap-base-client`. Design: `docs/design/blobext.md` §8. Scaffold only
until bead B12.

`using` is `core` and `urn:ietf:params:jmap:blob2`, plus the type
capabilities of the `Blob/lookup` type names; never
`urn:ietf:params:jmap:blob` (RFC 9404 bindings are bead B24).

## Crate Family Context

```
jmap-base-client
    └── jmap-blob-client  ← this crate (sibling of canonical jmap-mail-client)
jmap-blob-types           data types
```

## Spec Reference

```
~/PROJECT/IETF/specs/draft-ietf-jmap-blobext-03.txt  ← normative (blob2)
~/PROJECT/IETF/specs/rfc9404.txt    ← normative (blob, served for older clients)
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
cargo clippy -p jmap-blob-client --all-targets -- -D warnings
cargo test -p jmap-blob-client
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-blob-client --all-features --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, jmap-blob-types, jmap-base-client (docs/design/blobext.md §3); tokio and wiremock dev-only when tests land |
| Tests | wiremock, with blobext-03 §9 exchanges as oracles |

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond those listed above
- Do not add methods not in blobext-03 unless explicitly directed
