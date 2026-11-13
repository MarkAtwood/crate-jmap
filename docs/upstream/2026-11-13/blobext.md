<!--
DRAFT, not filed.
Target: GitHub issue on https://github.com/brong/draft-gondwana-jmap-blobext (named in -03 §12 "Changes";
confirmed read-only with gh: issues enabled, no existing issues; source draft-gondwana-jmap-blobext.mkd,
editor's copy still docname -03, commit 83f54e97). Each point checked against that editor's copy too: all still open.
crate-jmap decisions: 15 (gap rulings (a)-(h)), 21(i) (blob + blob2), 19(2) (conditional on Blob/convert).
Could be split per point if the author prefers.
-->

**Title:** Gaps found implementing -03: eight small rulings, blob+blob2 on the server, conditional on Blob/convert

While implementing -03 in crate-jmap (https://github.com/MarkAtwood/crate-jmap) we had to make the calls below where the draft is silent or the examples disagree with the text. Each says what crate-jmap's implementation (in progress) does; suggested text where it's more than an example fix.

1. **Examples leave out oldState/newState/expires.** §4 says the Blob/set "response has oldState and newState both null", and §7 says the Blob/set and Blob/convert responses "include the following property on each BlobObject", namely expires. The §9.1 Blob/set response and the §9.4-§9.7 Blob/convert responses have neither. crate-jmap's server emits them (null where there's no value); its client accepts them absent or null. Suggest adding them to the examples.

2. **Blob/convert "same structure as Blob/set".** §8: "The response has the same structure as Blob/set: a created map ... and a notCreated map". Blob/set's response also has oldState/newState/updated/destroyed and their not* maps, but the examples show only accountId/created/notCreated. crate-jmap emits accountId/created/notCreated only. Suggest: "The response has the following properties: accountId, created, and notCreated, as for Blob/set."

3. **Blob/get with ids null.** §5 says Blob/get "is a standard Foo/get method", and RFC 8620 §5.1 returns all objects for null ids, but blobs can't be enumerated. crate-jmap answers invalidArguments. Suggest: "The ids argument MUST NOT be null; the server rejects a null ids with an "invalidArguments" error."

4. **A dependency cycle in Blob/set.** §8 says, for Blob/convert, "If a dependency cycle is detected, all members of the cycle MUST be rejected with an "invalidProperties" error." Blob/set creates can refer to each other by `#` too, and §4 doesn't say. crate-jmap applies the same rule with properties ["data"]. Suggest the §8 sentence in §4.

5. **A `#` reference to a failed or unknown creation.** §3 says "If a blobId does not exist, or the user cannot access it, the server MUST reject the creation with a "notFound" SetError", but not what a `#ref` that doesn't resolve gets (other types' servers commonly use invalidProperties). crate-jmap uses notFound, matching §3 and the §8 deferred-blob wording ("fails with a "notFound" SetError"). Suggest saying so in §3.

6. **A null data key in a DataSourceObject.** §3: "It MUST contain exactly one of either data:asText or data:asBase64 (for inline data) or blobId", but both are typed "String|null". crate-jmap counts a key whose value is null as absent for the "exactly one" rule. Suggest: "A property with a null value counts as absent."

7. **offset/length without properties.** §5: "If neither offset nor length are specified (i.e., both null), the default properties are data and size.  Otherwise, the client MUST specify the properties to return." The server side isn't stated. crate-jmap answers invalidArguments. Suggest: "...; if it does not, the server rejects the call with an "invalidArguments" error."

8. **What invalidProperties names in Blob/convert.** The recipe errors (§8.1-§8.8) say "invalidProperties" but not what goes in `properties`, and not what a recipe gets when its capability list is null ("the server does not support image conversion", §2.1). crate-jmap names JSON-pointer paths into the creation (`imageConvert/type`, `archive/entries/3/name`), and answers a recipe whose list is null with invalidProperties `["<recipeKey>"]`. Suggest a sentence in §8 saying so.

9. **Both blob and blob2 in using, server side.** §2.1: "A server MAY also advertise urn:ietf:params:jmap:blob (as defined in [JMAP-BLOB]) to support older clients, but a client MUST NOT include both urn:ietf:params:jmap:blob and urn:ietf:params:jmap:blob2 in the using array of the same request." Nothing says what the server does if one does. crate-jmap serves both capabilities and answers every Blob/* call in such a request with invalidArguments. Suggest adding: "A server that receives both MUST reject each Blob method call in that request with an "invalidArguments" error."

10. **Does draft-ietf-jmap-conditional apply to Blob/convert?** Blob/set is a Foo/set, so it takes ifUnchangedBy and atomic. Conditional-02 §3 adds its arguments "to the standard Foo/set method", and Blob/convert isn't one, but it has a create map, `#` dependencies between creations and partial failure, so a client could reasonably send `"atomic": true` and expect all-or-nothing. crate-jmap answers Blob/convert with `atomic: true` or a non-empty ifUnchangedBy with invalidArguments, rather than ignoring them and partly applying a pipeline the client asked to be atomic. Suggest stating either that (conditional arguments are not accepted by Blob/convert) or that atomic applies to it.

Related, for RFC 9404 (which -03 obsoletes): we're sending two errata, on the §4.3.1 Blob/lookup example's `notFound` (-03 §6 and §9.3 already get this right) and on the §4.1.2/§4.2.2 Blob/upload examples carrying `oldState`.

Found implementing -03 in crate-jmap. Sections are -03; the same text is in the editor's copy at 83f54e97 (draft-gondwana-jmap-blobext.mkd L99, L315, L611, L785, L831).
