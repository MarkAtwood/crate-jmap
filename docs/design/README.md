# Design notes

Design notes for JMAP extensions that span several crates, finalized
2026-11-13. Each note maps spec text to crate-jmap types, handlers and
backend hooks, records the decisions that shaped it, and ends with the
bead breakdown (§11) and the questions still open for Mark (§12). The
beads filed from each note carry `Design: docs/design/<note>.md` in
their description.

| Note | Spec | Epic |
|---|---|---|
| [conditional.md](conditional.md) | draft-ietf-jmap-conditional-02 (conditional and atomic `Foo/set`) | JMAP-y1yu |
| [emailpush.md](emailpush.md) | draft-ietf-jmap-emailpush-05 (`EmailPushConfig`, `EmailPush`) | JMAP-r44n |
| [blobext.md](blobext.md) | draft-ietf-jmap-blobext-03, plus RFC 9404 legacy serving | JMAP-tna1, server sub-epic JMAP-tna1.2 |
| [object-history.md](object-history.md) | draft-ietf-jmap-object-history-03 | JMAP-tna1.3 (sub-epic of JMAP-tna1) |

[DECISIONS.md](DECISIONS.md) lists Mark's decisions of 2026-11-13 that
the notes apply (cited in the notes as "(decision N)"), and the list of
spec gaps to raise with the draft authors.
