# Agent Instructions — jmap-metadata

Umbrella facade for JMAP Object Metadata (draft-ietf-jmap-metadata-05): re-exports `jmap-metadata-types`
(always), `jmap-metadata-client` (`client` feature) and `jmap-metadata-server`
(`server`). It holds no logic: change the component crates,
not this one. It copies the shape of `jmap-quotas`, the workspace's first
umbrella, and replaces the 0.0.1 name-claim placeholder (bd JMAP-gs76).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-metadata
cargo test -p jmap-metadata --features client
cargo test -p jmap-metadata --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
