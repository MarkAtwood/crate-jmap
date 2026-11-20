# Spec patch: object-uri wording (bd JMAP-ew00, decision 56)

`object-uri-wording.patch` is a unified diff against jmap-chat-spec at
`125749aefb4b438f8f470e01f7787cedc78ee737` ("vtc: recording pause
capability, push schemes and stream key, layouts"). Apply it from the root
of the jmap-chat-spec repository with `git apply object-uri-wording.patch`.
It is a draft for Mark's review and has not been committed to the spec
repository.

Line numbers below refer to that HEAD.

## 1. chat-federation-00: the strip rule covers every `jmap` URI

`draft-atwood-jmap-chat-federation-00.md:428` says:

> A receiving server removes every MessageAction and every Endpoint whose
> `uri` is a JMAP object URI (a URI with the scheme `jmap`)

object-uri-00:100 reserves `jmap://` forms that "never match
`jmap-object-uri`", so a JMAP object URI and a URI with the scheme `jmap`
are not the same set. The patch says "a `jmap` URI (any URI with the scheme
`jmap`, whether or not it is a JMAP object URI)". It also adds one sentence
explaining why the rule covers the other forms: the forms are reserved for
future definition, and the receiver cannot tell whether they mean anything
away from their origin.

Restatements of the same rule changed to match:

- federation-00:428, last sentence: link spans "never carry JMAP object
  URIs ... and encrypted content does not either" now reads: link spans
  never carry `jmap` URIs, and clients do not put JMAP object URIs in
  encrypted content. That second clause matches chat-00:1872, which only
  says clients SHOULD NOT. The scope of that rule is not changed.
- federation-00:461 (`Peer/deliver` `actions`): "whose `uri` is a JMAP
  object URI are removed" now reads "is a `jmap` URI".
- chat-00:418 (MessageAction): "every MessageAction whose `uri` is a JMAP
  object URI ({{JMAP-OBJECT-URI}})" now reads "is a `jmap` URI".

Left unchanged: chat-tasks-00:256 and chat-calendars-00:288 say that "a
MessageAction whose `uri` is a JMAP object URI is removed". That stays true,
because a JMAP object URI is one kind of `jmap` URI.

## 2. chat-00: link spans exclude every `jmap` URI

`draft-atwood-jmap-chat-00.md:1620` (rich-body span table):

> `uri` SHOULD be treated as untrusted, and MUST NOT be a JMAP object URI
> ({{JMAP-OBJECT-URI}}), because `body` is relayed between servers unchanged

As written, this rule allows a `jmap://` link. The patch changes it to "MUST
NOT be a `jmap` URI of any form, including a JMAP object URI", which matches
item 1. Federation can then rely on link spans never carrying any `jmap`
URI.

## 3. chat-tasks-00: the registrations depend on Task and TaskList

`draft-atwood-jmap-chat-tasks-00.md:311-317` asks IANA to register
`tasklist` (TaskList) and `task` (Task) in "JMAP Object URI Types". The
designated-expert rule is at object-uri-00:144:

> The designated expert checks that the Data Type is registered in the "JMAP
> Data Types" registry ({{RFC9404, Section 6.3}})

No document registers Task or TaskList there. draft-ietf-jmap-tasks-06,
cited as JMAP-TASKS, has IANA sections for capabilities only. I checked this
in the editor's copy at ~/GIT/jmapio-jmap/spec/tasks/ianaconsiderations.mdown.
crate-jmap-types `DATA_TYPES` says the same thing (crate-jmap-types/src/data_types.rs:75-78).
The registry has no "provisional" status, so the patch does not use that
word. After the table, it adds a paragraph that:

- restates the expert check;
- states that JMAP-TASKS does not register Task or TaskList, and no other
  document does yet;
- asks IANA to add the two entries once a document registers Task and
  TaskList as JMAP data types.

It also adds `RFC9404` to the informative references, because the draft did
not cite it before. The draft does not name the new IETF task-lists effort
(decision 52), because there is no document to cite yet.

## 4. graph-projection-00: VacationResponse (and SearchSnippet)

`draft-atwood-jmap-graph-projection-00.md:153`:

> This document registers type names for the data types of {{RFC8621}} and
> {{RFC9610}} ({{iana}}).

The table at 321-329 registers seven types: Mailbox, Thread, Email,
Identity, EmailSubmission, AddressBook and ContactCard. It omits
VacationResponse, and it also omits SearchSnippet.

The patch narrows the claim instead of adding a registration. The draft
projects id references between objects (lines 92 and 98). SearchSnippet has
no `id`. VacationResponse is a per-account singleton that refers to no other
object, and no object refers to it, so as a node it would have no edges.
Line 345 already says how such a type becomes projectable later: "A data
type without a registered type name becomes projectable when a document
registers one."

This matches crate-jmap. crate-jmap-graph-projection-server implements
`Projectable` for exactly those seven types
(crate-jmap-graph-projection-server/src/lib.rs:28-31, 124-206), and
crate-jmap-object-uri `URI_TYPES` has no `vacationresponse` row.

If Mark would rather register `vacationresponse`, the alternative is to add
a table row and drop the VacationResponse clause. That would also need a
`URI_TYPES` row and, optionally, a `Projectable` impl.

## 5. object-uri-00: type-name is limited to 255 octets

`draft-atwood-jmap-object-uri-00.md:76`:

> `type-name       = 1*( %x61-7A / DIGIT )`

There is no upper bound, while `account-id` and `object-id` (lines 78-79)
are `1*255id-char`. The patch changes the rule to
`1*255( %x61-7A / DIGIT )`. In the prose at line 84 it adds that the type
name, like the ids, is at most 255 octets, the RFC 8620 §1.2 Id limit, so a
JMAP object URI is at most 772 octets (5 + 3 × 255 + 2). The registry rules
(lines 144 and 149) refer to the `type-name` rule, so they pick up the limit
without further changes.

### Code impact

crate-jmap does not enforce this limit today. `validate_data_type` in
crate-jmap-types/src/jmap_uri.rs:132-141 checks only that the type name is
non-empty and contains only lowercase ASCII letters and digits. Both
`JmapUri::new` and `FromStr` go through it. If the patch is accepted:

- add `&& s.len() <= 255` to `validate_data_type`. The error stays
  `JmapUriError::InvalidDataType`; its Display text could mention the
  length;
- add a test in crate-jmap-object-uri/tests/spec.rs:
  `"a".repeat(255)` parses and `"a".repeat(256)` fails with
  `InvalidDataType`. These sit next to the existing 255/256 id tests at
  lines 94-178. Also extend `registry_rows_satisfy_the_expert_review_rules`
  to check `type_name.len() <= 255`.

The change is in crate-jmap-types; crate-jmap-object-uri only re-exports
`JmapUri`. No registered type name comes close to the limit, so nothing
breaks for registered types.

Items 1-4 need no code changes. The federation strip already removes every
`jmap` URI: `jmap_chat_federation_types::is_jmap_object_uri` delegates to
`jmap_object_uri::is_jmap_uri`, which tests the scheme alone
(crate-jmap-chat-federation-types/src/peer.rs:1019-1020). After item 1, the
name `is_jmap_object_uri` is slightly misleading; renaming it to
`is_jmap_uri` would be an optional cleanup.

## Render check

I exported HEAD with `git archive` into two scratch copies, one unpatched
and one patched with `git apply`, and gave each its own copy of the repo's
`.refcache`. In each copy I ran `kramdown-rfc` and then
`xml2rfc --text --html` for the five touched drafts:

- Both tools exited 0 for all five drafts.
- `kramdown-rfc` printed no output.
- The `xml2rfc` logs were identical between the two copies.
- `idnits` on the rendered text showed the same warning and comment counts
  before and after: object-uri 2/1, chat 19/3, chat-federation 11/2,
  chat-tasks 7/2, graph-projection 10/1. The only differences were line
  numbers. chat-tasks also has one more RFC link in the existing
  RFC_REFERENCE_URL_FORMAT comment, because it now cites RFC 9404.

The scratch copies have been removed.
