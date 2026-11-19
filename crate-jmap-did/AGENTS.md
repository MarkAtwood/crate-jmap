# Agent Instructions — jmap-did

Umbrella facade for JMAP Decentralized Identifiers (draft-atwood-jmap-did-00):
re-exports `jmap-did-types` as `types`. The family has no `-client` or
`-server` crate, so there are no features. DID resolution stays in the
support crate `jmap-did-resolver`, which this crate does not re-export.
It holds no logic: change the component crate, not this one. It copies the
shape of `jmap-quotas`, the workspace's first umbrella, and replaces the
0.0.1 name-claim placeholder (bd JMAP-gs76).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-did
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
