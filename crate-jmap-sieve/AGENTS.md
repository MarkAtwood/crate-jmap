# Agent Instructions — jmap-sieve

Facade for JMAP for Sieve Scripts (RFC 9661): enables the `sieve` feature
of `jmap-mail-types` and `jmap-mail-server` and re-exports exactly its
items: the wire types as `types` (always) and `SieveBackend`, the
`SieveScript/*` handlers and their registration functions as `server`
(`server` feature), plus `CAPABILITY`, the one item the 0.0.1 placeholder
published. No `client`: `jmap-mail-client` has no Sieve bindings. Sieve is
a feature of the mail crates (DECISIONS.md decision 50), not a crate
family. It holds no logic: change the mail crates, not this one. It copies
the shape of `jmap-quotas`, the workspace's first umbrella, and replaces
the 0.0.1 name-claim placeholder (bd JMAP-gs76.14).

When the mail crates add or remove a `sieve`-gated item, update the lists
here in the same change.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-sieve
cargo test -p jmap-sieve --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
