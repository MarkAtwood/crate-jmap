# Design: JMAP Blob Management (draft-ietf-jmap-blobext-03, RFC 9404) in crate-jmap

Bead: JMAP-tna1 (children tna1.1 client update, tna1.2 server); epic JMAP-m4nc (RFC 9404 `jmap-blob`) is folded into it (decision 4).
Spec: `~/PROJECT/IETF/specs/draft-ietf-jmap-blobext-03.txt`; legacy `~/PROJECT/IETF/specs/rfc9404.txt` (decision 11).
Related: conditional-02 (JMAP-y1yu, `docs/design/conditional.md`), object-history-03 (JMAP-tna1.3, `docs/design/object-history.md`), emailpush-05 (JMAP-r44n, `docs/design/emailpush.md`).
Decisions: `docs/design/DECISIONS.md` (Mark, 2026-11-13). Every place a decision changed the draft is marked "(decision N)".

## 1. Summary

blobext-03 (capability `urn:ietf:params:jmap:blob2`, obsoletes RFC 9404 "if approved") adds `Blob/set` (inline / concatenated / range-copy creation, touch, destroy), `Blob/get` (ranges, digests, chunks, imageData), `Blob/lookup` (reverse reference index) and `Blob/convert` (image, archive, extract, compress, decompress, delta, patch).
The kit also serves RFC 9404's `urn:ietf:params:jmap:blob` (decision 11): RFC 9404 is the published standard and -03 §2.1 lets a server keep it "to support older clients". That adds `Blob/upload` and RFC 9404 behaviour for `Blob/get` and `Blob/lookup`, selected by `using` (§5).
Three new crates hold it: `jmap-blob-types` (wire types, `BlobRef`, digest computation), `jmap-blob-server` (five handlers, `BlobBackend` trait, `memory` backend) and `jmap-blob-client` (bindings). The crates.io placeholder `jmap-blob` (0.0.1, "RFC 9404") becomes the umbrella re-export crate over them (decision 4).
The JMAP Data Types registry is one shared table in `jmap-types`, read by blobext and object-history (decision 14).
Blob storage, the reverse lookup index, deferred generation and media/archive conversion are `BlobBackend` hooks. Validation, range/UTF-8/digest semantics, recipe checks, dependency order and error mapping stay in the crates.
Existing crates: chat-client blob methods get five -03 fixes now (tna1.1), become `#[deprecated]` wrappers over `jmap-blob-client` once it exists, and are removed in the next breaking release (decision 12). `BlobUploadResponse` gains `expires`, cid-types docs get new citations, and the sibling servers must start resolving `#creationId` in blobId-typed properties (none do today), or the §8 "later method calls" pipelines do not work.

## 2. Spec coverage

| -03 section / requirement | Where | Notes |
|---|---|---|
| §2.1 `blob2` in `capabilities` MUST be `{}`; present when any account has it | `blob-types::BlobCapability` (empty + `extra`) | The consumer builds the Session (kit has no session builder) |
| §2.1 account capability: 17 fields (`maxSizeBlobSet` … `maxImageDimension`) that the object "MUST contain" | `blob-types::BlobAccountCapability`; `BlobBackend::blob_capability` | Nullable fields serialize as `null`, never skipped. The same value feeds the Session and every handler check |
| §2.1 `maxDataSources` MUST be ≥ 64 | `BlobAccountCapability::check()` (consumer calls it when building the Session) + unit test | The handler enforces the advertised value as-is |
| §2.1 `supportedDigestAlgorithms` lowercase RFC 9530 names, preference order; SHOULD list only cheap ones | `blob-types::DigestAlgorithm` + `check()` (lowercase) | Names verified against RFC 9530 §7.2 |
| §2.1 client SHOULD use the first listed algorithm it supports; SHOULD NOT rely on `sha` for tamper detection | `blob-client::preferred_digest_algorithm()` + rustdoc | |
| §2.1 `supportedTypeNames`: clients MUST ignore unknown names and names whose capability is not in Session `capabilities` | `blob-client::lookup_type_names()` filters both | |
| §2.1 `uploadUrl` per-account; uploads return `expires`; null → SHOULD use session `uploadUrl`; servers MUST keep session `uploadUrl` | N/A server (transport); `base-client::BlobUploadResponse.expires`; `blob-client::upload_url()` | |
| §2.1 `chunkSize` hint; servers MUST allow other sizes | Handler never checks chunk sizes | |
| §2.1 server MAY also advertise `urn:ietf:params:jmap:blob` (decision 11) | `blob-types::Rfc9404AccountCapability` via `BlobAccountCapability::rfc9404()`; `blob-server` mode switch (§5) | Served, not just tolerated |
| §2.1 client MUST NOT send both `blob` and `blob2` in `using` | `blob-client` (using = core + blob2 + type capabilities only); server: every Blob/* method answers `invalidArguments` when both are present (§5, open question 3) | |
| §3 DSO: exactly one of `data:asText`/`data:asBase64`/`blobId`; valid base64; range not past end; `size`/`position`/`digest:*` match; else `invalidProperties` `["data"]` (MUST NOT guess) | `blob-server::set::validate_sources` | A source `digest:<alg>` not in `supportedDigestAlgorithms` cannot be checked, so it is also `["data"]` |
| §3 unknown/inaccessible `blobId` → `notFound` | `blob-server::set` via `blob_info`; backend re-verifies in `create_blob` | Includes a `#ref` with no creation (gap e) |
| §3/§5 chunk `size`/`position`/`digest:*` MUST be correct | `blob-server::get::chunks` computes `position` and digests itself | |
| §4 no state: non-null `ifInState` → `invalidArguments`; `oldState`/`newState` null | `blob-server::set` | Own `BlobSetResponse`: `jmap_types::SetResponse.new_state` is non-null `State` |
| §4 `BlobCreateObject` (`data`, `type`, `size`, `digest:<alg>`, `noPersist`) | `blob-types::BlobCreateObject`; `blob-server::set` | |
| §4 whole `size` mismatch / any `digest:*` mismatch / unsupported alg → `invalidProperties`, blob MUST NOT be created | `blob-server::set` verifies before `create_blob` | `properties`: `["size"]` / `["digest:<alg>"]` |
| §2.1/§4 `tooLarge` above `maxDataSources` or `maxSizeBlobSet` | `blob-server::set` | Null `maxSizeBlobSet`: backend MAY still return `tooLarge` |
| §4 update = touch `expires`; other props only at current values; missing id → `notUpdated` `notFound` | `blob-server::set::update`; `BlobBackend::touch_blob` | `updated[id]` is `null` when honoured, else `{"expires": …}` (a partial object) |
| RFC 8620 §5.3 `#creationId` in update keys / destroy ids / sources; same-call ordering | `blob-server::set` + `order.rs` | |
| §4 destroy MUST reject a referenced blob with `blobHasReference` | `BlobBackend::destroy_blob` contract | Backend owns the reference index |
| §4/§8 `noPersist`: MAY omit from `created` and `createdIds` only "if it did not create a referenceable blob" | `CreatedBlob::omit_from_response` contract | Dispatcher builds `createdIds` from `created` (`extract_created_ids_into`); an omitted blob is usable only inside its own method call |
| §4 `type` is a hint; server MAY override; §10.2 SHOULD NOT reject data for not matching its type | `create_blob` contract; handler never inspects content against `type` | |
| §4 BlobObject `id`,`type`,`size` (null = deferred), `expires` | `blob-types::BlobObject` | |
| §5 `offset`,`length`,`dataSourceProperties` (default `["blobId","offset","length"]`) | `blob-types::BlobGetArgs`; `blob-server::get` | |
| §5 properties; default `data`+`size` only when offset and length both null | `blob-server::get` | Offset/length without `properties` → `invalidArguments` (gap g); RFC 9404 mode differs (§5) |
| §5 unsupported `digest:*` in `properties`/`dataSourceProperties` → `invalidArguments` | `blob-server::get` | Checked even when `chunks` is not requested; other unknown names → `invalidArguments` (RFC 8620 §5.1); `id` is accepted (RFC 8620 §5.1, needed by the conditional evaluator, decision 1) |
| §5/§10.2 `isEncodingProblem`; MUST return null `data:asText`, never invalid UTF-8 | `blob-server::get` | |
| §5 `isTruncated` (null length: only when offset is past the end) | `blob-server::range::select_range` (pure, checked arithmetic) | |
| §5 digest over exactly the returned octets, incl. truncated range | `blob-server::get` (+ `stored_digest` fast path) | |
| §5 `size` total regardless of range; SHOULD be cheap | `BlobBackend::blob_info` contract | |
| §5 `imageData` / `ImageGPS`; SHOULD cache | `blob-types::ImageData`; `BlobBackend::image_data` (default `None` → `null`) | Media parsing and caching are the backend's |
| §5 `chunks`; non-chunked server returns one chunk covering the blob | `BlobBackend::blob_chunks` default = exactly that | |
| §5 response `accountId`, `list`, `notFound`; no `state` | `blob-types::BlobGetResponse` | |
| §6 `typeNames` ∈ `supportedTypeNames`, "Can Reference Blobs" Yes (or private), capability in `using`, else `unknownDataType` | `blob-server::lookup` + `jmap_types::data_types` registry (decision 14) + `BlobBackend::lookup_type_capability` | Method-level error |
| §6 one BlobInfo per id; `matchedIds` key per requested type, `[]` when none | `blob-server::lookup` | |
| §6 invisible/nonexistent blob → empty arrays; no undiscoverable object ids; Mailbox/Thread via Emails | `BlobBackend::blob_references` contract | |
| §7 `expires` on every BlobObject in Blob/set create and Blob/convert responses | `blob-server` always writes `"expires"` (null when none) | The §9 examples leave it out; the normative text wins |
| §8 `create`, `noPersist`, exactly one recipe key | `blob-types::ConvertRequest` / `Recipe`; `blob-server::convert` | Zero or several recipe keys → `invalidProperties` naming them |
| §8 `isIncomplete`, `description`; `conversionFailed` MUST | `ConvertOutcome` → response | |
| §8 `#` refs within the call and later calls; MUST order dependencies; cycle → all members `invalidProperties` | `blob-server::order::topo_order` | Later calls need sibling `#` resolution (B20, B21) |
| §8 deferred: size null; MUST generate on access; failed generation = not found everywhere; Blob/get `size` MUST materialize; blobId MUST be usable everywhere | `BlobBackend::convert` / `blob_info` contracts | The consumer's other hooks (`read_blob`, `blob_exists`, `blob_sha256`, `blob_size`) must honour deferred ids |
| §8.1 `type` ∈ write types (`invalidProperties`); source ∈ read types (`invalidProperties`); `maxImageDimension`/`maxConvertSize` → `tooLarge` | `blob-server::convert::check_recipe`; read-type check by the backend (it sniffs bytes) | |
| §8.2/§8.3 archive: type ∈ list; `blobId` iff file; dir name ends `/`; `linkTarget` iff symlink/hardlink; entryType per format; `maxArchiveEntries` → `tooLarge` | `blob-server::convert::check_archive` | Zip = file+directory, tar/cpio = the 7 defined types; other formats and `Other` entry types → backend |
| §8.3.x per-format field ignoring | backend | |
| §8.4 Extract: no blob created; result id/type/size are the archive's; `entries`; `noPersist` no effect; null type SHOULD auto-detect, else MUST `unknownFormat` | `blob-server::convert` + backend | |
| §8.5/§8.6 compress/decompress; level SHOULD clamp; xz checksum SHOULD sha-256 | backend; `blob-types::compress_level_range()` helper | |
| §8.7/§8.8 delta/patch; text/x-diff non-text MUST `unknownFormat` | backend | |
| §9 examples | Tests (§10) | |
| §10.1 MUST check access on every referenced blobId | `blob_info`/`read_blob_range`/`create_blob`/`convert` contracts (caller-scoped) | |
| §10.2 hardened parsers SHOULD | backend | |
| §10.3 limits, zip bombs, digest rate limiting SHOULD | advertised limits enforced in crate; `Dispatcher::set_guard` (per method+caller only) or throttling inside the backend hooks | |
| §10.4 SHOULD reject `..` entry names; clients extracting to disk MUST validate | `check_archive` → `invalidProperties`; client N/A (no filesystem extraction), rustdoc on `ArchiveEntry` | |
| §10.5 scan concatenated and converted blobs SHOULD | `create_blob`/`convert` contracts | |
| §11.1/11.2 capabilities | `JMAP_BLOB2_URI`, `JMAP_BLOB_URI` consts | Legacy `blob` is served (decision 11; overrides draft Q3) |
| §11.3/11.4 `unknownDataType`, `unknownFormat`, `blobHasReference`, `conversionFailed` | `blob-types::errors` consts; `SetError::new(..)` / `JmapError::custom` | `SetErrorType` docs forbid new extension variants; same rule as decision 7 for conditional's codes |
| §11.5 Data Types registry fields | `jmap_types::data_types::DataTypeEntry` (decision 14) | One table, also carrying object-history-03 §7.2 "Supports History" |

RFC 9404 coverage (decision 11):

| RFC 9404 section / requirement | Where | Notes |
|---|---|---|
| §3.1 `blob` capability: session `{}`; account object MUST contain `maxSizeBlobSet`, `maxDataSources` (≥ 64), `supportedTypeNames`, `supportedDigestAlgorithms` | `blob-types::Rfc9404AccountCapability`, built by `BlobAccountCapability::rfc9404()` | Same limits as `blob2`; `adler` left out (RFC 3230 spells it `adler32`) |
| §4.1 `Blob/upload`: create only; `created` `id`/`type`/`size`; `notCreated`; no state; MUST add every creation to `createdIds` | `blob-server::upload` (reuses the Blob/set create pipeline) | Every upload is persistent, so it is always in `created` |
| §4.1 DSO: exactly one data key or a blobId range; range past end, invalid base64 or UTF-8, invalid references → `notCreated` (MUST NOT guess) | `blob-server::set::validate_sources` | Same `invalidProperties` `["data"]` / `notFound` / `tooLarge` mapping as -03 |
| §4.2 `Blob/get`: properties `data`, `data:asText`, `data:asBase64`, `digest:<alg>`, `size`; default `data`+`size` whenever `properties` is absent | `blob-server::get` in `Rfc9404` mode | Gap (g) does not apply: §4.2.2 G4/G5 send offset/length without `properties` |
| §4.3 `Blob/lookup` | `blob-server::lookup` (same rules as -03 §6) | Unknown blob → empty arrays (normative text), not the §4.3.1 example's `notFound` (open question 3) |
| §5 security considerations | same contracts as -03 §10 | |

Gaps in -03 settled as written (decision 15: all eight accepted; all eight are on the upstream list for the blobext author in `docs/design/DECISIONS.md`):
(a) §9.1 and §9.4-9.7 responses leave out `oldState`/`newState`/`expires`: the server emits them; the client accepts absent or null.
(b) Blob/convert "same structure as Blob/set": emit `accountId`/`created`/`notCreated` only, as the examples do.
(c) `Blob/get` with `ids: null`: `invalidArguments` (blobs cannot be enumerated).
(d) A Blob/set create cycle: `invalidProperties` `["data"]`, by analogy with §8.
(e) A `#ref` to a failed or unknown creation: `notFound` (blobext §3/§8 wording), not the sibling `invalidProperties`.
(f) A DSO data key whose value is `null` counts as absent for the "exactly one" rule.
(g) A client that sends offset/length without `properties` (a client MUST): `invalidArguments`.
(h) Blob/convert `invalidProperties` names JSON-pointer paths into the request (`imageConvert/type`, `archive/entries/3/name`), the JMAP-r44n.5 precedent. A recipe whose capability list is null → `invalidProperties` `[<recipeKey>]`.

## 3. Crate placement

Crate names (decision 4): `jmap-blob-types` / `jmap-blob-server` / `jmap-blob-client` (free on crates.io), plus the umbrella `jmap-blob`.

| Crate | Kind | Depends on | Publish |
|---|---|---|---|
| `jmap-blob-types` (`crate-jmap-blob-types/`) | extension types, no async | `jmap-types`, serde, serde_json, base64 0.22, sha1 0.10, sha2 0.10 | yes, layer 2 |
| `jmap-blob-server` (`crate-jmap-blob-server/`) | extension server (handlers + `BlobBackend` + `memory`/`test-helpers` features) | `jmap-server`, `jmap-types`, `jmap-blob-types` | yes, layer 3 |
| `jmap-blob-client` (`crate-jmap-blob-client/`) | extension client | `jmap-base-client`, `jmap-types`, `jmap-blob-types` | yes, layer 4 |
| `jmap-blob` (`crate-jmap-blob/`) (decision 4) | umbrella facade: re-exports `jmap-blob-types` always, `jmap-blob-client` / `jmap-blob-server` behind `client` / `server` features | the three above | yes, layer 5; replaces the 0.0.1 placeholder on the shared `jmap-*` version line |

The JMAP Data Types registry lives in `jmap-types` (`data_types` module), not in `jmap-blob-types` (decision 14; overrides draft Q7(b)). Both blobext and object-history read it, and `jmap-types` is the only crate both already depend on.

Why new crates:
- Blob is a core data type, not mail or chat. `jmap-server` is the foundation and must not take five handlers, a backend trait, a hash stack and a memory store.
- `jmap-base-client` is RFC 8620-only; the extension-client family exists for this.
- Each crate mirrors its canonical (`mail-types`/`mail-server`/`mail-client`) and joins the sibling lists in AGENTS.md "Canonical Templates".
- `jmap-blob` follows the umbrella shape of the claimed 0.0.1 umbrellas (JMAP-amr2 `jmap-mail` and siblings): it is the first umbrella brought into the workspace, so it sets the shape they copy (decision 4).

No cycles: no existing crate depends on the new ones. B15 adds chat-client → blob-client (client to client), which moves `jmap-chat-client` from layer 4 to layer 5 in RELEASING.md; its only dependent is the unpublished testjig. The per-extension `blob_exists`/`read_blob`/`blob_size`/`blob_sha256` hooks stay; the consumer backs them all from one store. Hash deps sit in the types crate so server and client share one `Digester` (did-types already takes `sha2`; `sha1` 0.10.6 is already in Cargo.lock). Other RFC 9530 algorithms come from the backend's `digester` hook.

## 4. Wire types (`jmap-blob-types`, plus the registry in `jmap-types`)

Every struct is `#[non_exhaustive]` with `extra`, except builder-style `*Input` structs. Wire maps are `BTreeMap`. Wire names are quoted from -03 (or RFC 9404 for the `rfc9404` items).

```rust
pub const JMAP_BLOB2_URI: &str = "urn:ietf:params:jmap:blob2";   // §2.1
pub const JMAP_BLOB_URI:  &str = "urn:ietf:params:jmap:blob";    // §11.2, RFC 9404 §3.1 (served, decision 11)

pub struct BlobCapability { pub extra: Map }                     // session-level, MUST be {} (both URIs)

/// §2.1: all 17 keys MUST be present, so no skip_serializing_if on the Options.
pub struct BlobAccountCapability {
    pub max_size_blob_set: Option<u64>, pub max_data_sources: u64,
    pub supported_type_names: Vec<String>, pub supported_digest_algorithms: Vec<DigestAlgorithm>,
    pub upload_url: Option<String>, pub chunk_size: Option<u64>,
    pub supported_image_read_types: Option<Vec<String>>, pub supported_image_write_types: Option<Vec<String>>,
    pub supported_archive_types: Option<Vec<String>>,   pub supported_extract_types: Option<Vec<String>>,
    pub supported_compress_types: Option<Vec<String>>,  pub supported_decompress_types: Option<Vec<String>>,
    pub supported_delta_types: Option<Vec<String>>,     pub supported_patch_types: Option<Vec<String>>,
    pub max_convert_size: Option<u64>, pub max_archive_entries: Option<u64>, pub max_image_dimension: Option<u64>,
    pub extra: Map,
}
impl BlobAccountCapability {
    pub fn check(&self) -> Result<(), CapabilityError>;          // >=64, lowercase
    pub fn rfc9404(&self) -> Rfc9404AccountCapability;           // decision 11: the four shared fields, `adler` dropped
}
/// RFC 9404 §3.1: all four keys MUST be present (decision 11).
pub struct Rfc9404AccountCapability { pub max_size_blob_set: Option<u64>, pub max_data_sources: u64,
    pub supported_type_names: Vec<String>, pub supported_digest_algorithms: Vec<DigestAlgorithm>, pub extra: Map }

/// RFC 9530 §7.2 names: "sha-512","sha-256","md5","sha","unixsum","unixcksum","adler","crc32c".
/// Manual PartialEq/Eq/Ord/Hash on as_str(): it is a BTreeMap key, so its order must equal
/// the wire-string order (AGENTS "Wire-format maps"), and a forged Other("sha") must equal Sha.
pub enum DigestAlgorithm { Sha512, Sha256, Md5, Sha, Unixsum, Unixcksum, Adler, Crc32c, Other(String) }
impl DigestAlgorithm { pub fn property_name(&self) -> String; pub fn from_property_name(s: &str) -> Option<Self>;
                       pub fn builtin_digester(&self) -> Option<Box<dyn Digest + Send>>; } // sha, sha-256, sha-512
pub trait Digest { fn update(&mut self, bytes: &[u8]); fn finish(self: Box<Self>) -> String; } // padded RFC 4648 §4

/// An input blob reference: a literal Id or "#creationId" (RFC 8620 §5.3).
/// Needed because `Id`'s Deserialize validates and rejects '#'.
pub enum BlobRef { Id(Id), Creation(Id) }        // wire: "B1" / "#c1"

/// §3. Custom serde: "digest:*" keys go to `digests`, the rest to `extra`.
/// Absent properties are skipped on output (chunks return only dataSourceProperties).
pub struct DataSourceObject {
    pub data_as_text: Option<String>, pub data_as_base64: Option<String>,   // "data:asText", "data:asBase64"
    pub blob_id: Option<BlobRef>,                                           // "blobId"
    pub offset: Option<u64>, pub length: Option<u64>, pub size: Option<u64>, pub position: Option<u64>,
    pub digests: BTreeMap<DigestAlgorithm, String>, pub extra: Map,
}
pub struct BlobCreateObject { pub data: Vec<DataSourceObject>, #[serde(rename = "type")] pub media_type: Option<String>,
    pub size: Option<u64>, pub digests: BTreeMap<DigestAlgorithm, String>, #[serde(default)] pub no_persist: bool, pub extra: Map }

pub struct BlobObject {                                 // §4, §7, §8 (created entries)
    pub id: Id, #[serde(rename = "type")] pub media_type: Option<String>,
    pub size: Option<u64>,                              // null = deferred convert result
    pub expires: Option<Option<UTCDate>>,               // absent vs null (own `present` helper; jmap-types' is private)
    pub extra: Map,
}
pub struct BlobUpdated { pub expires: Option<Option<UTCDate>>, pub extra: Map }   // §4 partial updated object

pub struct BlobSetArgs { account_id, if_in_state: Option<State>, create: Option<BTreeMap<Id, BlobCreateObject>>,
                         update: Option<BTreeMap<BlobRef, PatchObject>>, destroy: Option<Vec<BlobRef>>, extra }
/// Mirrors jmap_types::SetResponse field shapes (BTreeMap + null_as_empty/empty_as_null, own copies),
/// except old_state/new_state: Option<State>, written as null.
pub struct BlobSetResponse { account_id, old_state, new_state, created: BTreeMap<String, BlobObject>,
    updated: BTreeMap<Id, Option<BlobUpdated>>, destroyed: Vec<Id>, not_created/not_updated/not_destroyed, extra }

/// RFC 9404 §4.1 (decision 11). UploadObject = data + type only.
pub struct BlobUploadArgs { account_id, create: BTreeMap<Id, UploadObject>, extra }
pub struct UploadObject { pub data: Vec<DataSourceObject>, #[serde(rename = "type")] pub media_type: Option<String>, pub extra: Map }
/// accountId/created/notCreated only, as gap (b) does for Blob/convert. Named apart from
/// base-client's HTTP `BlobUploadResponse`. `created` entries are `BlobObject` (size always Some).
pub struct BlobUploadCallResponse { account_id, created: BTreeMap<String, BlobObject>, not_created: BTreeMap<String, SetError>, extra }

pub struct BlobGetArgs { account_id, ids: Option<Vec<Id>>, properties: Option<Vec<BlobProperty>>,
                         offset: Option<u64>, length: Option<u64>, data_source_properties: Option<Vec<DataSourceProperty>> }
pub enum BlobProperty { Id, Data, DataAsText, DataAsBase64, Size, ImageData, Chunks, Digest(DigestAlgorithm), Other(String) }
pub enum DataSourceProperty { BlobId, Size, Offset, Length, Position, Digest(DigestAlgorithm), Other(String) }
pub struct BlobGetItem { pub id: Id, pub data_as_text: Option<Option<String>>, pub data_as_base64: Option<String>,
    pub size: Option<u64>, pub image_data: Option<Option<ImageData>>, pub chunks: Option<Vec<DataSourceObject>>,
    pub digests: BTreeMap<DigestAlgorithm, String>, pub is_encoding_problem: bool, pub is_truncated: bool, pub extra: Map }
pub struct BlobGetResponse { account_id, list: Vec<BlobGetItem>, not_found: Vec<Id>, extra }   // no "state"
pub struct ImageData { width: Option<u64>, height: Option<u64>, orientation: Option<u64>, date: Option<UTCDate>,
                       gps: Option<ImageGps>, duration: Option<f64>, comment: Option<String>, extra }
pub struct ImageGps { latitude: f64, longitude: f64, extra }

pub struct BlobLookupArgs { account_id, type_names: Vec<String>, ids: Vec<Id> }
pub struct BlobInfo { id: Id, matched_ids: BTreeMap<String, Vec<Id>>, extra }
pub struct BlobLookupResponse { account_id, list: Vec<BlobInfo>, extra }                // no notFound in -03 or RFC 9404 §4.3 text

/// §8: exactly one recipe key (custom Deserialize; the handler parses each entry from Value
/// so one bad entry is a notCreated entry, not a method error).
pub struct ConvertRequest { pub no_persist: bool, pub recipe: Recipe, pub extra: Map }
pub enum Recipe { ImageConvert(..) /*"imageConvert"*/, Archive(..), Extract(..), Compress(..), Decompress(..), Delta(..), Patch(..) }
pub struct ImageConvertRecipe { blob_id: BlobRef, media_type: String /*"type"*/, width, height: Option<u64>,
    ignore_aspect: Option<bool>, quality: Option<u64>, color_space: Option<ColorSpace>, background: Option<String>,
    strip_metadata: Option<bool>, auto_orient: Option<bool>, extra }
pub enum ColorSpace { Srgb /*"sRGB"*/, Grayscale /*"grayscale"*/, Other(String) }
pub struct ArchiveRecipe { media_type: String, entries: Vec<ArchiveEntry>, extra }
pub struct ArchiveEntry { name, blob_id: Option<BlobRef>, entry_type: Option<EntryType>, modified, created: Option<UTCDate>,
    link_target, mode: Option<String>, uid, gid: Option<u64>, owner_name, group_name: Option<String>,
    dev_major, dev_minor: Option<u64>, comment: Option<String>, compression_method: Option<CompressionMethod>, extra }
pub enum EntryType { File, Directory, Symlink, Hardlink, Fifo, BlockDevice /*"blockDevice"*/, CharDevice /*"charDevice"*/, Other(String) }
pub enum CompressionMethod { Store, Deflate, Other(String) }
pub struct ExtractRecipe { blob_id: BlobRef, media_type: Option<String>, extra }
pub struct CompressRecipe { blob_id: BlobRef, media_type: String, level: Option<u64>, checksum: Option<bool>, extra }
pub struct DecompressRecipe { blob_id: BlobRef, media_type: Option<String>, extra }
pub struct DeltaRecipe { blob_id: BlobRef, new_blob_id: BlobRef /*"newBlobId"*/, media_type: String, extra }
pub struct PatchRecipe { blob_id: BlobRef, delta_blob_id: BlobRef /*"deltaBlobId"*/, delta_type: String /*"deltaType"*/, extra }

pub struct ConvertedBlob {          // a `created` entry; flat (no nested flatten of BlobObject)
    pub id: Id, pub media_type: Option<String>, pub size: Option<u64>, pub expires: Option<Option<UTCDate>>,
    pub is_incomplete: bool /*"isIncomplete", skip if false*/, pub description: Option<String>,
    pub entries: Option<Vec<ArchiveEntry>> /* ExtractRecipe only */, pub extra: Map,
}
pub struct BlobConvertResponse { account_id, created: BTreeMap<String, ConvertedBlob>, not_created: BTreeMap<String, SetError>, extra }

pub mod errors { UNKNOWN_DATA_TYPE = "unknownDataType" /*method-level*/, UNKNOWN_FORMAT = "unknownFormat",
                 BLOB_HAS_REFERENCE = "blobHasReference", CONVERSION_FAILED = "conversionFailed" }
pub fn compress_level_range(media_type: &str) -> Option<RangeInclusive<u64>>; // §8.5.1-4 (informative): gzip 1..=9, x-bzip2 1..=9, x-xz 0..=9, zstd 1..=22
```

`BlobObject` implements `jmap_types::SetObject` (`SERVER_SET` `["id", "size"]`, `CLIENT_SETTABLE` `["type", "expires"]`, `IMMUTABLE` `["type"]`) so Blob/set can register through the conditional opt-in path (decision 1, §9).

The registry (decision 14; replaces the draft's `blob_types::data_types`) is in `jmap-types`:

```rust
// crate-jmap-types/src/data_types.rs
/// One row of the IANA "JMAP Data Types" registry (blobext-03 §11.5 fields, plus
/// object-history-03 §7.2 "Supports History"), or of a draft that registers a type.
#[non_exhaustive]
pub struct DataTypeEntry {
    pub type_name: &'static str,             // "Email"
    pub capability: &'static str,            // "urn:ietf:params:jmap:mail"
    pub can_reference_blobs: bool,
    pub can_use_for_state_change: bool,
    pub supports_history: bool,              // §7.2 default "No"
    pub reference: &'static str,             // "RFC 8621", "draft-atwood-jmap-chat-00"
}
pub const DATA_TYPES: &[DataTypeEntry];      // sorted by type_name; a test checks order and uniqueness
pub fn data_type(type_name: &str) -> Option<&'static DataTypeEntry>;
```

Rows: the IETF registrations (RFC 9404 Table 1; RFC 9425 Quota; RFC 9661 SieveScript; RFC 9670 Principal, ShareNotification; RFC 9610 AddressBook, ContactCard; draft-ietf-jmap-calendars-33; draft-ietf-jmap-filenode-20), "Supports History" from object-history-03 §7.2 Table 1, plus the atwood drafts' rows, each citing its draft. "Can Reference Blobs: Yes" for Chat, Message, Space, CustomEmoji (chat), SceneAsset, SceneObject, SceneAvatar (scene), VTCRecording (vtc), GraphNode (graph) (decision 14; overrides draft Q7). Task and TaskList (draft-ietf-jmap-tasks-06 registers no types yet) and the atwood types carry "Supports History" per decision 5 until their documents state it (open question 4).

## 5. Server design (`jmap-blob-server`)

Modules: `backend.rs`, `get.rs`, `set.rs`, `upload.rs` (decision 11), `lookup.rs`, `convert.rs`, `mode.rs` (decision 11), `order.rs` (topological sort), `range.rs` (pure range/UTF-8/digest helpers), `memory/` (feature-gated).

`register_blob_handlers(dispatcher, Arc<B>)` registers `Blob/get`, `Blob/set`, `Blob/lookup`, `Blob/convert` and `Blob/upload` (decision 11) with the sibling `reg!`/`ClosureHandler` macro (siblings use `register`). It forwards `cx.created_ids` (set, upload, convert) and `cx.using` (every handler, for the mode switch below; lookup also for type capabilities). `handle_blob_*` are public so consumers can override one method. `register_blob_handlers_with_conditional` is the opt-in path for conditional Blob/set (decision 1, §9). Capability advertisement: the consumer puts `BlobCapability` in `capabilities` and `blob_capability(...)` in `accountCapabilities` under `blob2`, and, to serve older clients, `{}` and `blob_capability(...).rfc9404()` under `blob` (decision 11).

**Mode switch (`mode.rs`, decision 11).** `BlobMode::from_using(&cx.using)`:
- `blob` and `blob2` both present → every Blob/* method answers method-level `invalidArguments` ("-03 §2.1: a client MUST NOT include both"; open question 3).
- `blob` present, `blob2` absent → `Rfc9404`.
- otherwise → `Blobext`. As in every sibling, handlers do not otherwise gate on `using`: Blob/set, Blob/convert and Blob/upload run in either mode.

Common start: mode check, `extract_account_id`, `account_exists` (`accountNotFound`), `blob_capability` (`None` → `accountNotSupportedByMethod`). Literal ids go through `parse_wire_id`; `#` refs resolve against the call's local map first, then `cx.created_ids` (`resolve_creation_ref`). Per-object backend errors map through `backend_set_error_value`; other backend `Err` → `server_fail_value_from_backend`. Digest computation: `stored_digest` first, then `digester(alg)`; an advertised algorithm with neither → method-level `serverFail` ("advertised digest algorithm has no digester"), never a wrong or empty value.

**Blob/get** (`get.rs`):
1. `ids: null` → `invalidArguments` (gap c). Apply `enforce_max_objects_in_get`.
2. Properties: null with offset and length both null → `{data,size}`; null otherwise → `invalidArguments` (gap g; in `Rfc9404` mode null is always `{data,size}`, decision 11). Unknown name, or `digest:<alg>` not advertised → `invalidArguments`. `id` is accepted and changes nothing (RFC 8620 §5.1; the conditional evaluator passes it, decision 1). `dataSourceProperties` follows the same rules (default `blobId,offset,length`), checked even without `chunks`.
3. Per id: `blob_info` → `None` adds it to `notFound`. `select_range(size, offset, length)` → `(start, end, is_truncated)`: null length truncated only when offset > size; otherwise when offset+length > size (checked add).
4. Read bytes only when a data or digest property needs them (`read_blob_range(.., start..end)`).
   - `data:asText`: invalid UTF-8 → `null` + `isEncodingProblem`.
   - `data`: valid UTF-8 → `data:asText` key; invalid → `data:asBase64` + `isEncodingProblem`.
   - `digest:<alg>` over the same octets.
5. `imageData`: `image_data`; `None` → `null`.
6. `chunks`: `blob_chunks` (whole blob, not the range), projected to `dataSourceProperties`. `position` = running sum of lengths; `size` from the hook; a chunk digest covers its `[offset, offset+length)`.
7. `Rfc9404` mode (decision 11): `imageData` and `chunks` are unknown names (`invalidArguments`); `dataSourceProperties` is an unknown argument and is ignored, as the kit's handlers ignore unknown arguments. Everything else is identical.

**Blob/set** (`set.rs`):
- Non-null `ifInState` → `invalidArguments`. `enforce_max_objects_in_set`. Response `oldState: null, newState: null`. Order: create, update, destroy.
- Create (each entry parsed from its own `Value`; the steps are `set::create_one`, shared with Blob/upload):
  1. Unknown property → `invalidProperties` [name]; top-level `digest:<alg>` not advertised → `invalidProperties` [`digest:<alg>`].
  2. `data.len() > maxDataSources` → `tooLarge`.
  3. `topo_order` over `#` refs in the create map; cycle members → `invalidProperties` `["data"]` (gap d).
  4. Resolve refs; unresolvable → `notFound` (gap e).
  5. Each source → `invalidProperties` `["data"]` unless: exactly one data key (gap f); base64 decodes (STANDARD, padded); offset/length inside the source (size from inline length or `blob_info`, whose `None` → `notFound`); `size` equals source size; `position` equals running offset; each source digest is advertised and matches the contributed octets.
  6. Total > `maxSizeBlobSet` → `tooLarge`. Whole `size` mismatch → `["size"]`.
  7. Whole digests: stream the octets through each digester; mismatch → `["digest:<alg>"]`.
  8. `create_blob(BlobCreate{ sources, media_type, size, no_persist })`; record `cid → id` in the local map even when omitted.
  9. `created[cid]` = `id`/`type`/`size`/`expires` (always present), unless `omit_from_response`.

  Handler-side checks are sound because blobs are immutable; the backend still re-verifies source access atomically.
- Update (keys may be `#refs`): the patch may set `expires` (`UTCDate|null`); any other BlobObject key is accepted only at its current `blob_info` value, else `invalidProperties`. Missing id → `notUpdated` `notFound`. `touch_blob` → `TouchOutcome::Honoured` (`updated[id]: null`) or `Applied(Option<UTCDate>)` (`{"expires": …}`).
- Destroy (ids may be `#refs`): `destroy_blob` → `notFound` / `blobHasReference` / `forbidden`.
- `ifUnchangedBy`/`atomic`: handled by the dispatcher, never in `set.rs` (decision 1; §9).

**Blob/upload** (`upload.rs`, RFC 9404 §4.1, decision 11):
- `accountId`, `create` required (else `invalidArguments`); a non-empty `update` or `destroy` → `invalidArguments` ("only create is allowed": dropping them would silently lose a write, the decision 2 reasoning). `enforce_max_objects_in_set`.
- Each `UploadObject` runs `set::create_one` with `size`/`digest:*` absent and `no_persist: false`; same SetError mapping (`invalidProperties` `["data"]`, `tooLarge`, `notFound`). `#` refs resolve against the call and `cx.created_ids`, as in RFC 9404 §4.1.2.
- `created[cid]` = `id`/`type`/`size`/`expires`, never omitted, so every upload reaches `createdIds` (§4.1 MUST). Response `BlobUploadCallResponse`: `accountId`, `created`, `notCreated`, no state (open question 3).

**Blob/lookup** (`lookup.rs`):
- `typeNames` and `ids` required. `enforce_max_objects_in_get` on `ids`.
- Each name must be in `supportedTypeNames`, `lookup_type_capability(name)` must be `Some(uri)`, and `uri` must be in `cx.using`; otherwise `JmapError::custom("unknownDataType")` with a description naming the type.
- One `blob_references(type_name, &ids)` per type. `list` in request order (a duplicate id repeats its row), every requested type a key (default `[]`). No `notFound`.
- `Rfc9404` mode: identical (RFC 9404 §4.3 has the same rules; decision 11).

**Blob/convert** (`convert.rs`):
- `create` required (else `invalidArguments`). `enforce_max_objects_in_set`. Parse each entry from `Value`; zero or several recipe keys → `invalidProperties` naming them.
- `check_recipe` against the capability (paths per gap h):
  - Null list → `[<recipeKey>]`; type not in list → `[<recipeKey>/type]` (`patch/deltaType`). Extract/decompress with null `type` skip the list check (backend auto-detects).
  - width/height > `maxImageDimension` → `tooLarge`; `quality` outside 1..=100 → `invalidProperties`; `background` not a CSS hex colour (`#` + 3, 4, 6 or 8 hex digits) → `invalidProperties`. `colorSpace` `Other` goes to the backend (spec values are open).
  - entries > `maxArchiveEntries` → `tooLarge`. Entry rules (§8.3, §8.3.1); `..` path component → `invalidProperties`; `mode` must be octal digits with value ≤ 0o7777.
- `topo_order` over every recipe ref (`blobId`, `newBlobId`, `deltaBlobId`, entry `blobId`s); cycle → every member `invalidProperties`.
- Resolve refs; missing → `notFound`. Every input's `blob_info.size` (each entry, `newBlobId`, `deltaBlobId` too) > `maxConvertSize` → `tooLarge`. Then `convert(ConvertParams{ recipe (refs resolved), no_persist })`.
- Backend returns `ConvertOutcome` or a SetError (`unknownFormat`, `conversionFailed`, `invalidProperties` for an unreadable image format, `tooLarge`).
- `created[cid]` = `ConvertedBlob`. Extract: the archive's own blob, `noPersist` ignored. The local map records `cid → id` even when omitted.

Error mapping. Method level: `accountNotFound`, `accountNotSupportedByMethod`, `invalidArguments`, `requestTooLarge`, `unknownDataType`, `serverFail`. SetError: the codes above; custom codes from `blob_types::errors`.

## 6. Backend hooks (`BlobBackend: JmapBackend`)

Async hooks are RPITIT `+ Send` and take `(caller, account_id, …)`; ranges are `Range<u64>` and anything past four arguments is a params struct (Public API rule 3). "No default" applies the hook rule: a no-op would put wrong data on the wire. RFC 9404 mode (decision 11) uses the same hooks; it adds none.

| Hook | Contract | Default |
|---|---|---|
| `blob_capability(…) -> Result<Option<BlobAccountCapability>>` | The value the Session carries (its `rfc9404()` view for `blob`); `None` = account lacks blob2 | **None**: a default would advertise limits nobody enforces |
| `blob_info(…, blob_id) -> Result<Option<BlobMeta{media_type, size: u64, expires}>>` | Caller-visible blobs only. Materializes a deferred result (§8 size MUST). Failed generation → `Ok(None)`; transient → `Err` | **None** |
| `read_blob_range(…, blob_id, range) -> Result<Option<Vec<u8>>>` | Exactly `range` (already clamped); materializes deferred blobs | **None** |
| `stored_digest(…, &DigestQuery{blob_id, alg, range}) -> Result<Option<String>>` | Precomputed base64 digest of exactly that range, or `None` | `Ok(None)`: absent, the handler computes |
| `digester(&self, alg) -> Option<Box<dyn Digest + Send>>` (sync) | A streaming digester for an advertised algorithm | `alg.builtin_digester()` (sha, sha-256, sha-512); `None` for others → `serverFail` when used, never wrong data |
| `blob_chunks(…, blob_id) -> Result<Option<Vec<ChunkRef{blob_id, offset, length, size}>>>` | The stored chunk list | One chunk `{self,0,size,size}` from `blob_info`: -03 §5 prescribes this for non-chunked servers |
| `image_data(…, blob_id) -> Result<Option<ImageData>>` | Metadata when the server can extract it | `Ok(None)` → `null` ("Otherwise null") |
| `create_blob(…, BlobCreate) -> Result<CreatedBlob, BackendSetError>` | Atomic: re-verify sources (`notFound`), quota (`overQuota`), §10.5 scan, mint id, may store a chunk list. Returns type, size, expires and `omit_from_response` (true only when no referenceable blob exists; never for `no_persist: false`, which Blob/upload always passes) | **None** |
| `touch_blob(…, blob_id, requested: Option<UTCDate>) -> Result<TouchOutcome, BackendSetError>` | `notFound` when missing | **None** |
| `destroy_blob(…, blob_id) -> Result<(), BackendSetError>` | `blobHasReference` when a *current* object references it (object-history non-current versions do not count); `notFound` when missing | **None** |
| `lookup_type_capability(&self, type_name) -> Option<&str>` (sync) | Capability URI of a blob-referencing type | `jmap_types::data_types::data_type(name)` filtered on `can_reference_blobs` (decision 14); unknown → `None` → `unknownDataType`, correct for an unknown type; private types override |
| `blob_references(…, type_name, blob_ids) -> Result<BTreeMap<Id, Vec<Id>>>` | §6: containment closure (Mailbox/Thread via Email), only ids the caller can discover by querying, current versions only, invisible blobs → no entry | **None**: an empty default would deny real references |
| `convert(…, ConvertParams) -> Result<ConvertOutcome, BackendSetError>` | Runs or defers the recipe (size `None`). A deferred id MUST work wherever blobIds are accepted, including the consumer's mail/filenode/scene hooks. Sniffs input formats (read types, auto-detect → `unknownFormat`), clamps `level`, hardened parsers, bomb limits (`tooLarge`), §10.5 scan. An omitted (`noPersist`) result stays usable as input within the same call | **None**: a default `conversionFailed` would misreport an advertised conversion. A backend with every list null is never called and writes a trivial body |

The draft's `blob_set_atomic` hook is dropped (decision 1): atomic Blob/set runs inside the dispatcher's `ConditionalBackend::begin_set` scope, which the opt-in registration requires (§9).

No permission hooks: per "Caller identity", ACLs live inside the hooks above.

## 7. Memory reference backend (`feature = "memory"`, `test-helpers`)

- `HashMap<(account, Id), StoredBlob{ repr: Bytes | Chunks(Vec<ChunkRef>) | Deferred(Recipe), media_type, expires }>`. Ids `B` + counter.
- `create_blob` stores blobId sources as a chunk list (exercises `chunks` and §9.2) and inline sources as bytes.
- Clock-injected `expires`: now + 1 h unreferenced, + 5 min `noPersist`. `touch_blob` honours up to 7 days and returns `Applied` beyond, exercising both forms.
- `noPersist` results are referenceable and reported by default; `omit_no_persist_for_test(true)` omits them, for §9.5 verbatim.
- Reference index `HashMap<(type_name, object_id), Vec<Id>>` seeded by `add_reference_for_test`; drives `destroy_blob` and `blob_references`; visibility via `hide_object_for_test`.
- `stored_digest` returns `Some` for `sha-256` of whole blobs (fast path); the rest is computed.
- Conversions (B6, decision 13): hand-rolled ustar `application/x-tar` archive and extract only, no new deps; archive results `Deferred` until read. Every other recipe is advertised `null`. Handler logic for all recipes is tested with `stub_converter_for_test(Fn(&Recipe)->Result<Vec<u8>,SetError>)` (image/compress/delta/patch, §9.5/§9.7) under `test-helpers`; `fail_deferred_for_test(id)`.
- `advertise_for_test(BlobAccountCapability)`. Default capability: `maxDataSources` 64, `supportedDigestAlgorithms` `["sha-256","sha"]`, `supportedTypeNames` `[]` (tests advertise names), archive/extract `["application/x-tar"]`, other lists null (decision 13).
- `ConditionalBackend` (B18, decision 1): the blob map sits on `jmap_server::ScopedState`, so an atomic Blob/set rolls back its created blobs, `noPersist` ones included.

## 8. Client bindings (`jmap-blob-client`)

- `JmapBlobExt::with_blob_session(session) -> SessionClient` (sealed, mirrors `jmap-mail-client`). `blob_account_id()` reads `primaryAccounts["urn:ietf:params:jmap:blob2"]`; `for_account` targets another account; `blob_capability()` parses `accountCapabilities`.
- `blob_set(BlobSetInput)` plus `blob_create`, `blob_touch(ids, expires)`, `blob_destroy(ids)`; returns `BlobSetResponse` (absent and null `oldState`/`newState`/`expires` both parse). Conditional Blob/set goes through base-client's generic `SetCall` (decision 8); `blob_set` gains no conditional parameter now.
- `blob_get(ids, BlobGetInput{properties, offset, length, data_source_properties})`.
- `blob_lookup(ids, type_names, extra_using)`: adds the registry capability (`jmap_types::data_types`, decision 14) of each name to `using`; rejects a name whose capability is not in Session `capabilities` (§2.1), and empty `ids`/`type_names`, with `InvalidArgument`.
- `blob_convert(BTreeMap<Id, ConvertRequest>)` → `BlobConvertResponse`.
- `upload_url()`: account `uploadUrl` when non-null, else `session.upload_url`, as a `JmapUrlTemplate` for `JmapClient::upload_blob`.
- `preferred_digest_algorithm(client_supported)`, `lookup_type_names()`, `verify_digest(alg, bytes, value) -> Option<bool>`.
- Args via `jmap_base_client::to_args`; `using` = core + blob2 (+ type capabilities), never `jmap:blob`. RFC 9404 client bindings (for servers that only offer `blob`) are a separate follow-up, B24 (open question 5).

## 9. Interactions and breaking changes

**conditional-02 (JMAP-y1yu, `docs/design/conditional.md`).**
- Blob/set is a Foo/set, so with `urn:ietf:params:jmap:conditional` in `using` it MUST honour `ifUnchangedBy` and `atomic`.
- Decision 1: the conditional capability is opt-in. `register_blob_handlers` keeps its signature and bound; `register_blob_handlers_with_conditional<B: BlobBackend + ConditionalBackend>` registers Blob/set with `ClosureHandler::new_set::<BlobObject>` instead, and only a deployment that uses it may advertise conditional (`Dispatcher::conditional_gaps`). The dispatcher evaluates `ifUnchangedBy` through the registered `Blob/get` (§5 accepts `id` in `properties`) and runs atomic writes in the `begin_set` scope. `set.rs` contains no precondition or rollback code, and the draft's `blob_set_atomic` hook is gone.
- Decision 2: `atomic: true` or a non-empty `ifUnchangedBy` on Blob/set without the conditional URI in `using` is rejected by the dispatcher with `invalidArguments`, on every deployment, opted in or not.
- Decision 7: `atomicFailure` / `cannotApplyAtomically` are `&str` consts in `jmap_types::conditional`, matching how blob-types defines its own codes.
- Non-null `ifInState` stays `invalidArguments` in the handler (conditional note §5.2 step 6: `Blob/get` has no `state`).
- `type` and `expires` are in `BlobObject`'s `SetObject` lists but `Blob/get` never returns them, so a pointer to them needs a ruling (open question 1). Blob/convert, Blob/lookup and Blob/upload are not Foo/set (open question 2 for convert).

**object-history-03 §3.1 (JMAP-tna1.3, `docs/design/object-history.md`).**
- "A version that is not current does not count as a reference to the blob ... such as the blobHasReference error and Blob/lookup method": stated in the `destroy_blob` and `blob_references` contracts.
- A blob referenced by a returned version "MUST be downloadable"; a server that discards it "MUST NOT return that version". That obligation sits on the object-history backend (tna1.3), not here.
- A restored Email reuses its blobId (§5.2), so it is a current reference again. Blob has no state or history and is not in `supportedTypes`.
- §7.2 adds a "Supports History" column to the same JMAP Data Types registry. The table is `jmap_types::data_types` (decision 14), owned by this note's bead B1 and a prerequisite of both epics; object-history reads `supports_history` from it.

**emailpush-05 (JMAP-r44n).** No method interaction: an EmailPush may carry an Email `blobId` that a client reads with Blob/get. Blob has no StateChange (not "Can Use for State Change").

**Existing crates.**
- `jmap-chat-client` (`methods/blob.rs`, tna1.1) has five bugs against -03:
  - `blob_lookup(type_names: None)` sends `"typeNames": null`, but `typeNames` is `String[]`; `tests/blob_smoke_tests.rs` asserts the wrong claim.
  - `using` lacks the type capability, so a conforming server answers `unknownDataType` for `"Message"`.
  - It sends the chat primary account (`session_parts` → `chat_account_id`), not the blob2 account.
  - `BlobLookupResponse.not_found` is not in -03 §6.
  - `BlobObject` lacks `expires`/`isIncomplete`/`description`/`entries`; docs cite a non-spec `unsupportedMediaType`.

  Decision 12, in three steps:
  1. Now (tna1.1, B14): `type_names: &[&str]` required (**BREAKING**), add `urn:ietf:params:jmap:chat` to `using`, use the blob2 primary account, remove `not_found` (**BREAKING**), add the fields, cite -03, **BREAKING** CHANGELOG entries.
  2. Once `jmap-blob-client` exists (B15): the chat-client blob methods and types become thin `#[deprecated]` wrappers over it, keeping their tna1.1 signatures (repo stability policy: deprecate before removing), with docs pointing at `jmap-blob-client`.
  3. Next breaking release (B16): remove them (**BREAKING**). Rule 6 then holds: one public path per item, and lookup is not chat-specific.
- `jmap-base-client`: `BlobUploadResponse.expires: Option<UTCDate>` (-03 §2.1, §7). Additive on a `#[non_exhaustive]` type.
- `jmap-cid-types`: re-cite `blobext.rs`/lib docs from RFC 9404 §4.1 to -03 §5. Docs only.
- Sibling servers (mail Email/set, Email/import, Email/parse, SieveScript; filenode; scene; chat attachments; contacts; calendars): today a blobId of `#c1` fails `parse_wire_id`, so a blob created by Blob/set, Blob/upload or Blob/convert cannot be used later in the request (RFC 8620 §5.3; -03 §8 "later method calls"; RFC 9404 §4.1 relies on it too). B20 adds resolution with `resolve_creation_ref` in mail (canonical), B21 propagates it.
- `jmap-types`: new `data_types` registry module (decision 14). Additive.
- `jmap-server`: none. `CallContext.using`/`created_ids`, `extract_created_ids_into`, `SetError::new`, `JmapError::custom` cover the needs; the conditional pieces come from JMAP-y1yu.
- Consumer hooks `read_blob`/`blob_exists`/`blob_size`/`blob_sha256` must accept Blob/set, Blob/upload and Blob/convert ids, including deferred ones (documented on `BlobBackend::convert`).
- testjig: advertise `blob2` with the real `BlobAccountCapability` in `accountCapabilities` (the loop writes `{}`, which violates §2.1 "MUST contain") and `blob` with its `rfc9404()` view (decision 11), register the handlers; its blob store is separate from the mail/filenode memory stores, so cross-type lookups are out of scope there.
- crates.io: `jmap-blob` 0.0.1 (claimed placeholder, "RFC 9404") is replaced by the umbrella on the shared version line, description updated to blobext (decision 4). JMAP-m4nc's scope is folded into JMAP-tna1: the server (B11), the umbrella (B22) and RFC 9404 client helpers (B24).

Breaking summary: chat-client only (tna1.1 now: signature, field removal; on the wire no more `typeNames: null`, chat capability in `using`, blob2 account; later B16 removes the deprecated wrappers). Everything else is new or additive.

## 10. Test plan

Fixtures are hand-written JSON copied from the spec (test-oracle rule), never generated by the code under test.
- **registry (jmap-types, decision 14)**: `DATA_TYPES` sorted and unique; every RFC 9404 Table 1 row and every object-history-03 §7.2 Table 1 value present as published (hand-copied table in the test); the nine atwood "Can Reference Blobs: Yes" rows; `data_type("Identity")` has `can_reference_blobs: false`.
- **types**:
  - Round-trip §2.1.1 and every §9 request/response (9.1-9.7). Absent and null `oldState`/`newState`/`expires` parse; a capability with a null list serializes the `null` key.
  - DSO/`BlobGetItem` custom serde: `digest:sha-256` key, unknown `digest:foo` → `Other`, vendor `extra` kept, tri-state `data:asText`, `imageData` null vs absent.
  - `BlobRef`: `"B1"` → `Id`, `"#c1"` → `Creation`, `"#"`/`"#bad id"` rejected; round-trips.
  - `DigestAlgorithm` map order: keys `sha-512`, `sha`, `sha-256` inserted out of order serialize sorted by wire string; forged `Other("sha")` equals `Sha`.
  - `updated` `{"B1": {"expires": "…"}}` parses into `BlobUpdated`.
  - Each `Other` round-trips; BTreeMap ordering test; `Recipe` exactly-one (0, 1, 2 keys); `check()` rejects `maxDataSources` 63.
  - RFC 9404 (decision 11): §3.1.1 capability example round-trips through `Rfc9404AccountCapability`; `rfc9404()` keeps the four fields and drops `adler`; §4.1.1/§4.1.2 request and response round-trip.
  - Digest vectors: RFC 9404 §4.2.1 blob "The quick brown fox jumped over the lazy dog." (45 octets) → `digest:sha` `wIVPufsDxBzOOALLDSIFKebu+U4=`; offset 4 length 9 ("quick bro") → `digest:sha` `QiRAPtfyX8K6tm1iOAtZ87Xj3Ww=`, `digest:sha-256` `gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA=`. RFC 9530 App. B.2 empty content → `47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=`; App. B.1 `{"hello": "world"}` + LF → `RK/0qy18MlBSVnWgjwz6lZEWjP/lF5HF9bvEF8FabDg=`.
- **server, Blob/get** (`tests/get.rs`): default properties; offset without properties → `invalidArguments`; `isTruncated` table (null length offset = size false, > size true; offset+length > size; `u64::MAX` offset+length); UTF-8 split → `null` + `isEncodingProblem`, `data` → `data:asBase64`; digest over a truncated range; `digest:md5` not advertised → `invalidArguments`, advertised without digester → `serverFail`; §9.2 shape with real digests; default single chunk; `dataSourceProperties` default; `imageData` null; `ids: null` → `invalidArguments`; `properties: ["id","size"]` accepted.
- **server, Blob/set** (`tests/set.rs`): §9.1; non-null `ifInState`; one test per invalid-DSO cause (two keys, none, bad base64, offset past end, length past end, wrong `size`, wrong `position`, wrong digest, unadvertised source digest) → `["data"]`; missing blobId → `notFound`; 65 sources → `tooLarge`; over `maxSizeBlobSet` → `tooLarge`; whole `size`/digest mismatch → `invalidProperties` and no blob afterwards; `#` ref within the call (reverse map order) and across calls; cycle; zero sources → size 0; chunks fed back into Blob/set; touch honoured (`null`) and capped; touch `size` change → `invalidProperties`; touch and destroy via `#ref`; touch missing → `notFound`; destroy referenced → `blobHasReference`; `noPersist` reported by default, omitted with the knob and then absent from `createdIds`.
- **server, Blob/lookup**: §9.3 verbatim; `unknownDataType` for a name not in `supportedTypeNames`, for mail not in `using`, and for `Identity` ("Can Reference Blobs: No" in the registry); hidden object id omitted; unknown blob → empty arrays, no `notFound`.
- **server, Blob/convert**: §9.4 (zip via stub), §9.5 (tar + stub gzip, `t1` omitted with the knob), §9.6 (Extract id = archive id, `entries`), §9.7 via stub; validation matrix per §8.1-8.8 error lists with pointer paths; cycle → all members; `isIncomplete` + `description`; `conversionFailed`; `unknownFormat`; `..` entry name; `maxConvertSize` (entry and `newBlobId` inputs), `maxImageDimension`, `maxArchiveEntries` → `tooLarge`; deferred blob: Blob/get `size` materializes it; failed deferred → `notFound` in get, set and convert. Recipes other than tar run through the stub converter (decision 13).
- **server, RFC 9404 mode** (`tests/rfc9404.rs`, decision 11), with `using` = core + blob (+ mail for lookup): §4.1.1 (`size` 95, `type` `image/png`); §4.1.2 (`#b4` refs → Blob/get `"How quick was that?"`, size 19); §4.2.1 digests and `notFound`; §4.2.2 G1-G5 verbatim apart from minted ids (G4: offset/length without `properties` → `data:asText` `"The q"`/`"hello"`; G5 `isTruncated` + `data:asBase64` `anVtcGVkIG92ZXIgdGhlIIGBIGRvZy4=`). The §4.2.2 response swaps the two `type` values; assert the requested types (`b2` `text/plain`), not the example's. §4.3.1 with `not-a-blob` → a row with empty arrays and no `notFound` key. Every upload id appears in `createdIds`; Blob/upload with `update` → `invalidArguments`; `chunks` in 9404 mode → `invalidArguments`; both `blob` and `blob2` in `using` → `invalidArguments` from Blob/get, Blob/set, Blob/upload, Blob/lookup and Blob/convert.
- **client** (wiremock): request bodies equal the §9 requests; `blob_lookup` adds the mail capability for `Email` and rejects a type whose capability the Session lacks; `using` never has `urn:ietf:params:jmap:blob`; responses parse §9.
- **chat-client**: tna1.1: `typeNames` always an array; `size: null` parses; `using` has chat; `accountId` is the blob2 primary account. B15: each wrapper sends the same request body as before (wiremock) and carries `#[deprecated]`.
- **umbrella** (`jmap-blob`, decision 4): builds with no features, `client`, `server`, and all features; the RFC 9404 §3.1.1 and -03 §2.1.1 capability examples parse through the re-exported types.
- **siblings (B20, B21)**: Email/import, FileNode/set etc. accept `#c1` from an earlier Blob/set.
- **conditional (B18, decision 1)**: `ifUnchangedBy {"B1": {"size": 13}}` pass and fail; `{"B1": null}`; `atomic` with the memory backend rolls back created blobs; plain `register_blob_handlers` plus conditional args without the URI → `invalidArguments` (decision 2).

## 11. Bead breakdown

Parents: types, server, memory, RFC 9404, conditional, testjig and Blob/copy beads under JMAP-tna1.2; chat-client beads under JMAP-tna1.1 (B14 is tna1.1 itself); the registry, blob-client, umbrella, base-client, cid-types and sibling beads directly under JMAP-tna1.

Shared files (workspace `Cargo.toml`, AGENTS.md, RELEASING.md, CHANGELOG) change in B0 for the three component crates and in B22 for the umbrella; other beads add their own CHANGELOG lines only. B4 fixes the blob-server public surface (trait, `register_blob_handlers`, `handle_blob_*` stubs returning `serverFail`), so B7-B10 do not touch `public-api/jmap-blob-server.txt`; B11 and B18 extend it after them.

| # | Title | Crates / files | Size | Deps |
|---|---|---|---|---|
| B0 | Scaffold blob-types/-server/-client: Cargo, lib stubs with module re-exports, README (6 sections), PLAN.md, workspace members, AGENTS crate map + dependency tree + canonical-family rows, RELEASING layers, empty snapshots | 3 new + root files | S | — |
| B1 | jmap-types: JMAP Data Types registry (decision 14), shared with object-history | jmap-types `data_types.rs` | M | — |
| B2 | blob-types: capability (+ `rfc9404()` view), `DigestAlgorithm` + digesters, `BlobRef`, DSO custom serde, Blob/set + Blob/get types, errors | blob-types `capability.rs`, `digest.rs`, `blob_ref.rs`, `source.rs`, `set.rs`, `get.rs` | M | B0 |
| B3 | blob-types: convert recipes, ArchiveEntry, enums, `compress_level_range`; lookup types | blob-types `convert.rs`, `lookup.rs` | M | B0 |
| B4 | blob-server: `BlobBackend`, `register_blob_handlers` + public handler stubs, `mode.rs`, `range.rs`, `order.rs` + unit tests | blob-server `backend.rs`, `lib.rs`, `mode.rs`, `range.rs`, `order.rs` | M | B1, B2, B3 |
| B5 | blob-server memory core: store, chunks, reference index, stored digests, touch, test-helpers | `memory/` | M | B4 |
| B6 | blob-server memory conversions (decision 13): ustar archive/extract, deferred results, stub converter, `fail_deferred_for_test`, `omit_no_persist_for_test` | `memory/convert.rs` | M | B5 |
| B7 | Blob/get handler + tests | `get.rs`, `tests/get.rs` | M | B5 |
| B8 | Blob/set handler + tests | `set.rs`, `tests/set.rs` | M | B5 |
| B9 | Blob/lookup handler + tests | `lookup.rs`, `tests/lookup.rs` | S | B5 |
| B10 | Blob/convert handler (recipe checks, refs, cycles) + tests, incl. deferred-blob get/set tests | `convert.rs`, `tests/convert.rs` | M | B6 |
| B11 | RFC 9404 `urn:ietf:params:jmap:blob` (decision 11): Blob/upload, 9404 Blob/get/lookup mode, upload types | blob-types `rfc9404.rs`; blob-server `upload.rs`, `get.rs`, `lib.rs`, `tests/rfc9404.rs` | M | B7-B10 |
| B12 | blob-client: bindings + wiremock tests | blob-client | M | B1, B2, B3 |
| B13 | base-client: `BlobUploadResponse.expires` | base-client | S | — |
| B14 (= tna1.1) | chat-client: -03 fixes (typeNames required, using, blob2 account, drop notFound, BlobObject fields, re-cite, smoke test, BREAKING CHANGELOG) | chat-client | S | — |
| B15 | chat-client: blob methods become `#[deprecated]` wrappers over blob-client (decision 12) | chat-client, RELEASING layer | S | B12, B14 |
| B16 (P3) | chat-client: remove the deprecated blob wrappers in the next breaking release (decision 12, **BREAKING**) | chat-client | S | B15 |
| B17 | cid-types: re-cite RFC 9404 → blobext-03 | cid-types (docs) | S | — |
| B18 | blob-server: conditional opt-in `register_blob_handlers_with_conditional`, `BlobObject: SetObject`, memory `ConditionalBackend` on `ScopedState` (decision 1) | blob-types `set.rs`; blob-server `lib.rs`, memory | S | B8, JMAP-y1yu |
| B19 | testjig: advertise blob2 and blob with the real account capabilities, register handlers | testjig | S | B7-B11 |
| B20 | mail-server (canonical): resolve `#creationId` in blobId-typed properties | mail-server | M | — |
| B21 | Propagate `#creationId` blobId resolution: filenode, scene, chat, contacts, calendars | sibling `*-server` crates | M | B20 |
| B22 | `jmap-blob` umbrella crate (decision 4; folds JMAP-m4nc) | new `crate-jmap-blob/` + root files | S | B11, B12 |
| B23 (P3) | RFC 8620 §6.3 `Blob/copy` in blob-server (`copy_blob` hook); core, not blobext | blob-server | M | B8 |
| B24 (P3) | blob-client: RFC 9404 bindings (Blob/upload, 9404 Blob/get/lookup), carried from JMAP-m4nc | blob-client | S | B11, B12 |

Parallel lanes: {B0, B1, B13, B14, B17, B20} first; then {B2, B3, B21}; then {B4, B12}; then B5; then {B6, B7, B8, B9}; then {B10, B15}; then B11 (right after the blob2 server, decision 11); then {B18, B19, B22, B24}. B7-B10 each own one handler module and one test file. Cross-note: object-history's beads that read "Supports History" depend on B1 (`blobext:B1`).

## 12. Open questions for Mark

Decided and removed: crate names (decision 4), chat-client blob methods (decision 12), legacy `blob` (decision 11), memory conversion scope (decision 13), data-type registry contents and home (decision 14), gap rulings (a)-(h) (decision 15). The draft's Q4 and Q5 are reshaped by decision 1 below.

1. **Conditional pointers to `type`/`expires` on Blob.** Under decision 1 the dispatcher admits any name in `BlobObject`'s `SetObject` lists, but `Blob/get` never returns `type` or `expires`, so `{"type": null}` would always hold and `{"type": "text/plain"}` always fail. *Recommend* `Blob/get` marks `type` and `expires` unreadable through `HandlerOutput::with_unreadable()` (conditional note §5.1), so such a pointer gets `forbidden` and is never compared; `size` and `digest:<alg>` work as is. Blobs are immutable, so in practice only existence (`{}`/`null`) ever fails.
2. **Conditional arguments on Blob/convert** (not a Foo/set; the conditional note leaves non-`/set` arguments untouched, and decision 2 covers only a missing URI). *Recommend* `invalidArguments` for `atomic: true` or a non-empty `ifUnchangedBy`: ignoring `atomic` would partially apply a pipeline the client asked to be all-or-nothing.
3. **RFC 9404 serving rulings (B11), for the upstream list:** (i) both `blob` and `blob2` in `using`: -03 §2.1 forbids it for clients but says nothing about servers; *recommend* method-level `invalidArguments` on every Blob/* call. (j) RFC 9404 §4.3.1's example returns `notFound` for an unknown blob, while §4.3 requires a row with empty arrays; *recommend* the normative text, no `notFound` key (a possible RFC 9404 erratum). (k) Blob/upload responses: §4.1 says "no state field" but the §4.1.2 and §4.2.2 examples carry `oldState`/`updated`/`destroyed`; *recommend* `accountId`/`created`/`notCreated` only, as gap (b). (l) Blob/upload with a non-empty `update` or `destroy`: *recommend* `invalidArguments`.
4. **"Supports History" for types without a registered value** (Task, TaskList, the atwood types). Decision 5 puts them in object-history's first round and leaves the per-type list to bead filing. *Recommend* B1 encodes decision 5's proposal (Yes, except GraphNodeOp, PresenceStatus, ReadPosition, SceneAvatar, VTCParticipant, TaskNotification) citing decision 5 and the pending draft change, and object-history's beads edit those rows if the list changes.
5. **RFC 9404 client bindings (B24).** JMAP-m4nc asked for client helpers; decision 11 covers only serving. *Recommend* keeping B24 at P3 for clients that talk to `blob`-only servers, or closing it if no consumer needs one.
