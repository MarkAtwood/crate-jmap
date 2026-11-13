<!--
DRAFT, not filed.
Target: GitHub issue on https://github.com/brong/draft-gondwana-jmap-object-history
(the -03 "Changes" section names this repo; confirmed read-only with gh: issues enabled, no existing issues,
source file draft-gondwana-jmap-object-history.mkd, editor's copy already at -04, commit c4a87bda.
The -04 editor's copy keeps every passage quoted below.)
crate-jmap decisions: 6(a), 6(c), 6(d).
Could be split into three issues if the authors prefer; kept as one per the upstream list.
-->

**Title:** Restores vs maxObjectsInSet, queryChanges for onlyDestroyed, and a discarded final version

While implementing -03 in crate-jmap (https://github.com/MarkAtwood/crate-jmap) we hit three places where the draft doesn't say what a server does.

**1. Do restores count toward maxObjectsInSet?**

RFC 8620 §2 defines the limit over three operations only: maxObjectsInSet is "The maximum number of objects the client may send to create, update, or destroy in a single /set type method call", and §5.3's "requestTooLarge" is for "The total number of objects to create, update, or destroy". -03 §5.1 adds `restore` to Foo/set but doesn't mention the limit (it does settle maxObjectsInGet for Foo/get in §3). A restore is a write at least as expensive as an update, so a client could get round the limit with a large `restore` map.

Suggest adding to §5.1: "Each entry in restore counts as one object toward the maxObjectsInSet limit ([JMAP-CORE], Section 2), and the "requestTooLarge" error ([JMAP-CORE], Section 5.3) applies to the total of creates, updates, destroys, and restores."

crate-jmap's implementation (in progress) counts restores toward maxObjectsInSet and returns requestTooLarge when the total is over.

**2. How does Foo/queryChanges know a query was onlyDestroyed?**

§4 says, for onlyDestroyed: "Foo/queryChanges works as for any other query.  An object leaves the results when it is restored or discarded, and enters them when it is destroyed." But the draft adds onlyDestroyed, destroyedAfter and destroyedBefore only to Foo/query. RFC 8620 §5.6 Foo/queryChanges takes filter, sort and sinceQueryState, and filter/sort are "The filter argument that was used with "Foo/query"" / "The sort argument that was used with "Foo/query"". So a client following the spec sends a queryChanges for a destroyed query that looks exactly like one for a live query with the same filter and sort, and the destroyedAfter/destroyedBefore window is lost too.

Two ways to fix it:
- (a) Add the three arguments to Foo/queryChanges: "When this capability is included in the request's using array, Foo/queryChanges ([JMAP-CORE], Section 5.6) also accepts the onlyDestroyed, destroyedAfter, and destroyedBefore arguments. The client MUST pass the values it passed to Foo/query."
- (b) Say the server tells them apart by state: "A server MUST NOT return the same queryState for a query with onlyDestroyed true as for one without it; Foo/queryChanges determines which kind of query a sinceQueryState belongs to from the state itself."

We'd prefer (a): it's explicit, matches how filter and sort are already re-sent, and covers the date window. (The "dq7" queryState in the §4.1 example hints at (b).)

crate-jmap's implementation (in progress) does (b): every queryState from an onlyDestroyed query encodes the query kind and the destroyedAfter/destroyedBefore window, Foo/queryChanges routes on sinceQueryState, and a request whose window doesn't match the state gets cannotCalculateChanges (the §4 MAY).

**3. What if the final version of a destroyed object is discarded but older versions remain?**

§3.1 lets the server "prune old versions in any order", and §2.1 says it "MAY discard versions sooner". Several rules then key on any retained version:
- §3: with ids null and includeDestroyed, "this includes every destroyed object for which the server still retains any version."
- §4: onlyDestroyed "runs over the destroyed objects of this type for which the server still retains at least one version".
- §5.1: notFound is for an id that "is not the id of a destroyed object of this type for which the server retains any history."

But §5.1 restores from the final version: "The restored object takes the id of the destroyed object, and the values of the final version of that object (Section 3.1), with the PatchObject applied." And §3 says includeDestroyed without includeReplaced returns "only the final version before destruction". With the final version gone, the server has nothing to restore from, the query's destruction time ("the replaced value of its final version", §4) is undefined, and Foo/get has no final version to return, yet the object is still "a destroyed object ... for which the server retains any history".

Suggest the simple fix, in §3.1 after "prune old versions in any order": "However, the server MUST NOT discard the final version of a destroyed object while it retains any older version of that object; when it discards the final version, it discards the object's whole history." The alternative is to define that the newest retained version stands in for the final one, which needs changes in §3, §4 and §5.1.

crate-jmap's implementation (in progress) takes the suggested rule: its backend contract forbids discarding the final version while older versions remain, and its retention helper discards oldest first. Only a backend that breaks that contract would reach the undefined case; there it treats the object as gone (notFound for Foo/get and restore, left out of onlyDestroyed queries), which we know doesn't match the letter of §5.1's "retains any history".

Found implementing -03 in crate-jmap. Sections are -03; the same text is in the editor's copy at c4a87bda (draft-gondwana-jmap-object-history.mkd L226, L312, L497, L534, L633, L662).
