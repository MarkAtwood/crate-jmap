<!--
DRAFT, not filed.
Target: the same issue cross-filed on both drafts, each copy linking the other:
  - https://github.com/brong/draft-gondwana-jmap-conditional (source draft-ietf-jmap-conditional.mkd, editor's copy at -02, commit 4c6d2c01)
  - https://github.com/brong/draft-gondwana-jmap-object-history (source draft-gondwana-jmap-object-history.mkd, editor's copy at -04, commit c4a87bda)
Both repos confirmed read-only with gh (issues enabled). Neither editor's copy mentions the other extension.
crate-jmap decision: 6(b).
-->

**Title (conditional repo):** atomic and atomicFailure with object-history restores

**Title (object-history repo):** restore under conditional's atomic

(Cross-filed on conditional and object-history: <link to the other issue>.)

This is the interaction of two extensions, so it's filed on both; it probably only needs a sentence in each.

While implementing conditional-02 and object-history-03 together in crate-jmap (https://github.com/MarkAtwood/crate-jmap) we hit this: object-history adds a fourth kind of change to Foo/set, `restore` (object-history §5.1), with its own `notRestored` map, but conditional's atomic is written for exactly three.

conditional-02 §4: "When the "atomic" argument is true, the server MUST apply the Foo/set as a single unit: either every creation, update, and destroy the method requests takes effect, or none of them does."

conditional-02 §4.2: "any other SetError that would place an id in "notCreated", "notUpdated", or "notDestroyed" — the server MUST make no change and MUST reject the whole method with a method-level "atomicFailure" error." and "The "atomicFailure" error object MAY carry "notCreated", "notUpdated", and "notDestroyed" maps".

object-history-03 §5.1: "The server processes each restore before any create, update, or destroy in the same call, so these can refer to a restored object."

So with `"atomic": true` and a `restore` map, it isn't said whether a failed restore fails the method, whether a successful restore is rolled back when a later create fails, or where the failed restore's SetError goes. Since creates and updates can refer to a restored object, leaving restores out of the unit would let a client restore something and then have the creates that depended on it rolled back, which is the partial result atomic exists to prevent.

Questions:
1. Does atomic cover restores?
2. Should atomicFailure carry notRestored?

Proposal: yes to both, worded generally in conditional so it doesn't need a reference to object-history:
- conditional §4: "... either every creation, update, and destroy the method requests, and any other change an extension adds to Foo/set, takes effect, or none of them does."
- conditional §4.2: "or any other SetError that would place an id in "notCreated", "notUpdated", "notDestroyed", or a similar map defined by an extension" and "The "atomicFailure" error object MAY carry "notCreated", "notUpdated", and "notDestroyed" maps, and any similar map an extension adds to the Foo/set response, ..."
- object-history §5.1, one sentence: "When the Foo/set is atomic [CONDITIONAL], the restores are part of the single unit; a failed restore causes an "atomicFailure" error, which MAY carry a "notRestored" map."

crate-jmap's implementation (in progress) does exactly that: atomic covers restores, a notRestored entry fails the whole method with atomicFailure, the error carries `notRestored` alongside the other three maps (RFC 8620 §3.6.2 allows extra properties on a method error), and a backend that cannot restore inside the transaction answers cannotApplyAtomically. ifUnchangedBy is checked first, against the state at method start (conditional §3.1 rule 1), so a destroyed object about to be restored still counts as not existing for `{"id": null}`.

Found implementing conditional-02 with object-history-03 in crate-jmap. Editor's copies: conditional at 4c6d2c01 (draft-ietf-jmap-conditional.mkd L248, L273, L283-284); object-history at c4a87bda (L598-624).
