# Agent Instructions — jmap-blob

Umbrella facade for JMAP Blob Management (draft-ietf-jmap-blobext-03,
with RFC 9404 legacy support): re-exports `jmap-blob-types` (always),
`jmap-blob-client` (`client` feature) and `jmap-blob-server` (`server`;
`memory` adds its reference backend), plus `CAPABILITY`, the
one item the 0.0.1 placeholder published (the RFC 9404 URI). It holds no
logic: change the component crates, not this one. It copies the shape of
`jmap-quotas`, the workspace's first umbrella (bd JMAP-tna1.10).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-blob
cargo test -p jmap-blob --features client
cargo test -p jmap-blob --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
