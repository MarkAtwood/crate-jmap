# Upstream drafts from crate-jmap design (2026-11-13)

Implementation findings from designing crate-jmap support for object-history-03, conditional-02, emailpush-05 and blobext-03, plus two RFC 9404 errata. Nothing here has been filed, posted or sent; Mark reviews and files them.

| Draft | Goes to | Status | crate-jmap decisions |
|---|---|---|---|
| [object-history.md](object-history.md) | GitHub issue, https://github.com/brong/draft-gondwana-jmap-object-history | draft, not filed | 6(a), 6(c), 6(d) |
| [conditional-object-history.md](conditional-object-history.md) | GitHub issue cross-filed on https://github.com/brong/draft-gondwana-jmap-conditional and https://github.com/brong/draft-gondwana-jmap-object-history | draft, not filed | 6(b) |
| [emailpush.md](emailpush.md) | GitHub issue, https://github.com/jmapio/jmap | draft, not filed | 9, 10 |
| [blobext.md](blobext.md) | GitHub issue, https://github.com/brong/draft-gondwana-jmap-blobext | draft, not filed | 15 (gaps (a)-(h)), 21(i), 19(2) |
| [rfc9404-erratum-lookup.md](rfc9404-erratum-lookup.md) | RFC Editor errata report (RFC 9404 §4.3.1), Technical | draft, not filed | 21(ii) |
| [rfc9404-erratum-upload.md](rfc9404-erratum-upload.md) | RFC Editor errata report (RFC 9404 §4.1.2, §4.2.2), Editorial | draft, not filed | 21(iii) |
| [list-message.md](list-message.md) | JMAP WG mailing list (optional, after filing; links are placeholders) | draft, not filed | all of the above |

Notes:
- Repos come from each draft's own "Changes" section and were confirmed read-only with `gh` (issues enabled, no existing issues). No local clone of the brong repos exists. The object-history editor's copy is already at -04 (commit c4a87bda); the passages quoted are unchanged there. blobext (83f54e97) and conditional (4c6d2c01) editor's copies are still at -03 and -02.
- emailpush quotes are from the editor's copy, `spec/emaildelivery/draft-ietf-jmap-emailpush.xml` at f8f44fd on jmapio/jmap master.
- RFC 9404 had no errata on record as of 2026-11-13.
- File order suggestion: the two RFC 9404 errata and the blobext issue can go in any order; file conditional-object-history on both repos and link each copy to the other.
