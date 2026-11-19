# Agent Instructions — jmap-object-history

Umbrella facade for JMAP Object History (draft-ietf-jmap-object-history-03): re-exports `jmap-object-history-types`
(always), `jmap-object-history-client` (`client` feature) and `jmap-object-history-server`
(`server`), plus `CAPABILITY`, the one item the 0.0.1 placeholder published (the capability URI). It holds no logic: change the component crates,
not this one. It copies the shape of `jmap-quotas`, the workspace's first
umbrella, and replaces the 0.0.1 name-claim placeholder (bd JMAP-gs76).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-object-history
cargo test -p jmap-object-history --features client
cargo test -p jmap-object-history --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
