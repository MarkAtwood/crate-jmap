# Agent Instructions — jmap-emailpush

Facade for JMAP Email Delivery Push (draft-ietf-jmap-emailpush-05): enables
the `emailpush` feature of `jmap-mail-types`, `jmap-mail-client` and
`jmap-mail-server` and re-exports exactly its items: the wire types as
`types` (always), the subscription input and payload decoding as `client`
(`client` feature), `jmap_mail_server::emailpush` as `server` (`server`
feature), plus `CAPABILITY`, the one item the 0.0.1 placeholder published.
EmailPush is a feature of the mail crates (DECISIONS.md decisions 3 and
50), not a crate family. It holds no logic: change the mail crates, not
this one. It copies the shape of `jmap-quotas`, the workspace's first
umbrella, and replaces the 0.0.1 name-claim placeholder (bd JMAP-gs76.12).

When the mail crates add or remove an `emailpush`-gated item, update the
lists here in the same change.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-emailpush
cargo test -p jmap-emailpush --features client
cargo test -p jmap-emailpush --features server
cargo test -p jmap-emailpush --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
