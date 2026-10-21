# Agent Instructions — jmap-metadata-client

## 🧬 Sibling of the canonical jmap-mail-client — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-client`
extension-client template**. Doc-comment style, lint settings, and
test layout must mirror `jmap-mail-client`. Differences are *only*
the spec content (draft-ietf-jmap-metadata-04 here; RFC 8621 in
mail-client). draft-04 defines no methods of its own, so this crate
has no `Jmap*Ext` trait, `SessionClient`, `with_*_session` accessor,
or method bindings; that is a spec-mandated difference, not drift.

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

Pure client-side helpers for draft-ietf-jmap-metadata-04: property
name constants (§2, §3.1), the `ignoreMetadataOnlyChanges` argument
name and `updatedProperties` classifiers (§3.3), and builders for the
six `metadata*` / `privateMetadata*` filter conditions (§3.6). Used
alongside the typed extension clients' own `Foo/get`, `Foo/changes`,
and `Foo/query` calls.

## Crate Family Context

```
jmap-metadata-types
    └── jmap-metadata-client  ← this crate (sibling of canonical jmap-mail-client)
```

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-metadata-client --tests -- -D warnings
cargo test -p jmap-metadata-client
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-metadata-client --no-deps --all-features
```

Run all four before considering any work done.

## Source Material

- https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04
  — normative for property names, the `/changes` argument, and filter
  condition shapes
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base
  get/changes/query request shapes and filter operators

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace
  AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add features not in PLAN.md or not explicitly directed
- Do not add method bindings or transport (draft-04 has no
  `Metadata/*` methods)
- Any change to this crate that diverges from the canonical
  `jmap-mail-client` shape requires landing the canonical change
  first and propagating to the sibling extension-client crates
  (see workspace AGENTS.md "Canonical Templates" + the propagation
  rule above)
