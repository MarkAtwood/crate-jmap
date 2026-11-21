//! `Blob/convert` (blobext-03 §8).
//!
//! Each entry is checked in three passes before the backend runs it:
//! 1. shape ([`parse_entry`]): exactly one recipe key, every property known
//!    and of its wire type, `invalidProperties` naming a JSON-pointer path
//!    into the request otherwise (gap h, docs/design/blobext.md §2);
//! 2. capability ([`check_recipe`]): the recipe's type in its advertised
//!    list, image and archive limits, archive entry rules;
//! 3. inputs ([`check_inputs`]), in `#creationId` dependency order: every
//!    blobId resolves to a blob the caller can see, within
//!    `maxConvertSize`.

use std::collections::BTreeMap;

use jmap_blob_types::{
    ArchiveEntry, ArchiveRecipe, BlobAccountCapability, BlobConvertResponse, BlobRef,
    ConvertedBlob, EntryType, ImageConvertRecipe, Recipe,
};
use jmap_server::{HandlerOutput, enforce_max_objects_in_set, server_fail_from_backend};
use jmap_types::{Id, JmapError, SetError, UTCDate};
use serde_json::{Map, Value};

use crate::backend::{BlobBackend, ConvertParams};
use crate::order::topo_order;
use crate::set::{Ctx, backend_set_error, invalid, not_found, too_large};
use crate::start::{Start, start};

/// Handle `Blob/convert` (blobext-03 §8).
///
/// `created_ids` are the request's creation ids so far, for
/// `#creationId` references to blobs created by earlier calls; `using` is
/// the request's capability list, which selects blob2 or RFC 9404
/// behaviour ([`BlobMode`](crate::BlobMode)). Blob/convert runs in either
/// mode.
///
/// Entries run in dependency order, so a recipe may read the result of
/// another entry of the same call; a reference to a failed entry is
/// `notFound`. Every created `BlobObject` carries `expires`. An
/// `ExtractRecipe` creates no blob: its result has the archive's id, type
/// and size plus `entries`, `noPersist` has no effect on it, and a
/// reference to its creation id names the archive. A `noPersist` result
/// the backend leaves out of the response stays usable within the call.
/// The response holds `accountId`, `created` and `notCreated` only
/// (gap b).
///
/// Per-entry failures go to `notCreated`:
///
/// - `invalidProperties` naming the failing property as a path into the
///   request (`archive/entries/3/name`, `patch/deltaType`): an unknown or
///   ill-typed property, zero or several recipe keys (naming every recipe
///   key present), a recipe whose capability list is `null` (naming the
///   recipe key), a type not in that list, an image `quality` outside 1 to
///   100 or a `background` that is not a CSS hex colour, an archive entry
///   breaking the §8.3 rules (blobId only and always on files, directory
///   names ending in `/`, linkTarget only and always on links, an
///   entryType the format lacks, a `..` name component, a `mode` that is
///   not octal 0 to 7777), or a creation cycle (naming the recipe key);
/// - `notFound` for an input blob the caller cannot see, or a
///   `#creationId` with no creation;
/// - `tooLarge` above `maxImageDimension`, `maxArchiveEntries` or (for any
///   input blob) `maxConvertSize`;
/// - whatever [`BlobBackend::convert`] returns (`unknownFormat`,
///   `conversionFailed`, ...).
///
/// # Errors
///
/// A method-level [`JmapError`]: `invalidArguments` for both blob
/// capabilities in `using`, a missing or malformed `create`, `atomic:
/// true`, or a non-empty `ifUnchangedBy` (Blob/convert is not a Foo/set:
/// decision 19); `accountNotFound`; `accountNotSupportedByMethod`;
/// `requestTooLarge` above `maxObjectsInSet`; `serverFail` for a backend
/// error outside a single entry.
pub async fn handle_blob_convert<B: BlobBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    created_ids: &BTreeMap<Id, Id>,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let Start {
        account_id,
        mut args,
        capability,
        ..
    } = start(backend, caller, args, using).await?;
    reject_conditional(&args)?;
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;
    let create = match args.remove("create") {
        Some(Value::Object(m)) => m,
        _ => {
            return Err(JmapError::invalid_arguments(
                "create is required and must be an object (blobext-03 §8)",
            ));
        }
    };

    let mut ctx = Ctx {
        backend,
        caller,
        account_id: &account_id,
        capability: &capability,
        created_ids,
        in_call: create.keys().map(|k| Id::from(k.as_str())).collect(),
        local: BTreeMap::new(),
    };
    let mut response = BlobConvertResponse::new(account_id.clone());

    let mut parsed: BTreeMap<Id, Entry> = BTreeMap::new();
    for (cid, value) in create {
        if let Err(e) = Id::new_validated(cid.as_str()) {
            response.not_created.insert(
                cid,
                invalid(["id"]).with_description(format!("creation id is not an Id: {e}")),
            );
            continue;
        }
        match parse_entry(value).and_then(|mut entry| {
            check_recipe(&entry.recipe, &capability)?;
            entry.deps = creation_refs(&mut entry.recipe);
            Ok(entry)
        }) {
            Ok(entry) => {
                parsed.insert(Id::from(cid.as_str()), entry);
            }
            Err(e) => {
                response.not_created.insert(cid, *e);
            }
        }
    }

    let nodes: Vec<Id> = parsed.keys().cloned().collect();
    let order = topo_order(&nodes, |cid| {
        parsed.get(cid).map(|e| e.deps.clone()).unwrap_or_default()
    });
    for cid in &order.cyclic {
        if let Some(entry) = parsed.get(cid) {
            response.not_created.insert(
                cid.to_string(),
                invalid([entry.recipe.key()])
                    .with_description("creation references form a cycle (blobext-03 §8)"),
            );
        }
    }
    for cid in order.order {
        let Some(entry) = parsed.remove(&cid) else {
            continue;
        };
        match convert_one(&mut ctx, entry).await? {
            Ok(blob) => {
                ctx.local.insert(cid.clone(), blob.id.clone());
                if let Some(blob) = blob.shown {
                    response.created.insert(cid.to_string(), blob);
                }
            }
            Err(e) => {
                response.not_created.insert(cid.to_string(), *e);
            }
        }
    }

    let primary = serde_json::to_value(&response)
        .map_err(|e| JmapError::server_fail(format!("serializing Blob/convert response: {e}")))?;
    Ok(HandlerOutput::new(primary))
}

/// Decision 19: `atomic: true` or a non-empty `ifUnchangedBy` asks for
/// Foo/set semantics Blob/convert does not have; `atomic: false`, `{}` and
/// `null` are harmless.
fn reject_conditional(args: &Map<String, Value>) -> Result<(), JmapError> {
    match args.get("atomic") {
        None | Some(Value::Null | Value::Bool(false)) => {}
        Some(Value::Bool(true)) => {
            return Err(JmapError::invalid_arguments(
                "Blob/convert is not atomic: entries run independently (decision 19)",
            ));
        }
        Some(_) => return Err(JmapError::invalid_arguments("atomic must be a boolean")),
    }
    match args.get("ifUnchangedBy") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(m)) if m.is_empty() => Ok(()),
        Some(_) => Err(JmapError::invalid_arguments(
            "Blob/convert takes no ifUnchangedBy conditions (decision 19)",
        )),
    }
}

/// One parsed conversion request.
struct Entry {
    no_persist: bool,
    recipe: Recipe,
    /// Creation ids the recipe refers to.
    deps: Vec<Id>,
}

/// A conversion that succeeded.
struct Converted {
    /// The id a reference to the entry's creation id names.
    id: Id,
    /// The `created` entry, or `None` when the backend omitted it.
    shown: Option<ConvertedBlob>,
}

// ---- pass 1: shape -------------------------------------------------------

/// The wire type of one recipe or entry property.
#[derive(Clone, Copy)]
enum Kind {
    /// A blobId or `#creationId`, required.
    Ref,
    /// A blobId or `#creationId`, or null.
    OptRef,
    /// A string, required.
    Str,
    OptStr,
    OptUInt,
    OptBool,
    OptDate,
    /// A list of [`ArchiveEntry`] objects, required.
    Entries,
}

const IMAGE_CONVERT: &[(&str, Kind)] = &[
    ("blobId", Kind::Ref),
    ("type", Kind::Str),
    ("width", Kind::OptUInt),
    ("height", Kind::OptUInt),
    ("ignoreAspect", Kind::OptBool),
    ("quality", Kind::OptUInt),
    ("colorSpace", Kind::OptStr),
    ("background", Kind::OptStr),
    ("stripMetadata", Kind::OptBool),
    ("autoOrient", Kind::OptBool),
];
const ARCHIVE: &[(&str, Kind)] = &[("type", Kind::Str), ("entries", Kind::Entries)];
const EXTRACT: &[(&str, Kind)] = &[("blobId", Kind::Ref), ("type", Kind::OptStr)];
const COMPRESS: &[(&str, Kind)] = &[
    ("blobId", Kind::Ref),
    ("type", Kind::Str),
    ("level", Kind::OptUInt),
    ("checksum", Kind::OptBool),
];
const DECOMPRESS: &[(&str, Kind)] = &[("blobId", Kind::Ref), ("type", Kind::OptStr)];
const DELTA: &[(&str, Kind)] = &[
    ("blobId", Kind::Ref),
    ("newBlobId", Kind::Ref),
    ("type", Kind::Str),
];
const PATCH: &[(&str, Kind)] = &[
    ("blobId", Kind::Ref),
    ("deltaBlobId", Kind::Ref),
    ("deltaType", Kind::Str),
];
const ENTRY: &[(&str, Kind)] = &[
    ("name", Kind::Str),
    ("blobId", Kind::OptRef),
    ("entryType", Kind::OptStr),
    ("modified", Kind::OptDate),
    ("created", Kind::OptDate),
    ("linkTarget", Kind::OptStr),
    ("mode", Kind::OptStr),
    ("uid", Kind::OptUInt),
    ("gid", Kind::OptUInt),
    ("ownerName", Kind::OptStr),
    ("groupName", Kind::OptStr),
    ("devMajor", Kind::OptUInt),
    ("devMinor", Kind::OptUInt),
    ("comment", Kind::OptStr),
    ("compressionMethod", Kind::OptStr),
];

fn schema(recipe_key: &str) -> &'static [(&'static str, Kind)] {
    match recipe_key {
        "imageConvert" => IMAGE_CONVERT,
        "archive" => ARCHIVE,
        "extract" => EXTRACT,
        "compress" => COMPRESS,
        "decompress" => DECOMPRESS,
        "delta" => DELTA,
        _ => PATCH,
    }
}

/// Parse one `create` value (blobext-03 §8).
///
/// # Errors
///
/// `invalidProperties` naming the path of an unknown or ill-typed
/// property, or every recipe key when there is not exactly one.
fn parse_entry(value: Value) -> Result<Entry, Box<SetError>> {
    let Value::Object(mut map) = value else {
        return Err(Box::new(
            invalid(Vec::<String>::new()).with_description("a conversion request is an object"),
        ));
    };
    let no_persist = match map.remove("noPersist") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => b,
        Some(_) => return Err(Box::new(invalid(["noPersist"]))),
    };
    let present: Vec<&str> = Recipe::KEYS
        .iter()
        .copied()
        .filter(|k| map.contains_key(*k))
        .collect();
    let key = match present.as_slice() {
        [key] => *key,
        keys => {
            return Err(Box::new(invalid(keys.iter().copied()).with_description(
                format!(
                    "a conversion request needs exactly one of {}",
                    Recipe::KEYS.join(", ")
                ),
            )));
        }
    };
    let unknown: Vec<String> = map.keys().filter(|k| k.as_str() != key).cloned().collect();
    if !unknown.is_empty() {
        return Err(Box::new(
            invalid(unknown).with_description("unknown conversion request property"),
        ));
    }
    let value = map.remove(key).unwrap_or(Value::Null);
    check_shape(&value, key, schema(key))?;
    let recipe = Recipe::from_key_value(key, value)
        .map_err(|e| Box::new(invalid([key]).with_description(e.to_string())))?;
    Ok(Entry {
        no_persist,
        recipe,
        deps: Vec::new(),
    })
}

/// Check that `value` is an object whose properties follow `schema`.
fn check_shape(value: &Value, path: &str, schema: &[(&str, Kind)]) -> Result<(), Box<SetError>> {
    let bad = |p: String, why: &str| Box::new(invalid([p]).with_description(why.to_owned()));
    let Value::Object(map) = value else {
        return Err(bad(path.to_owned(), "expected an object"));
    };
    if let Some(k) = map.keys().find(|k| !schema.iter().any(|(n, _)| n == k)) {
        return Err(bad(format!("{path}/{k}"), "unknown property"));
    }
    for &(name, kind) in schema {
        let p = format!("{path}/{name}");
        let v = map.get(name).unwrap_or(&Value::Null);
        let ok = match (kind, v) {
            (
                Kind::OptRef | Kind::OptStr | Kind::OptUInt | Kind::OptBool | Kind::OptDate,
                Value::Null,
            ) => true,
            (Kind::Ref | Kind::OptRef, Value::String(s)) => BlobRef::parse(s).is_ok(),
            (Kind::Str | Kind::OptStr, Value::String(_)) => true,
            (Kind::OptUInt, Value::Number(n)) => n.as_u64().is_some(),
            (Kind::OptBool, Value::Bool(_)) => true,
            (Kind::OptDate, Value::String(s)) => UTCDate::new_validated(s.clone()).is_ok(),
            (Kind::Entries, Value::Array(entries)) => {
                for (i, e) in entries.iter().enumerate() {
                    check_shape(e, &format!("{p}/{i}"), ENTRY)?;
                }
                true
            }
            _ => false,
        };
        if !ok {
            return Err(bad(p, "missing, or not of the expected type"));
        }
    }
    Ok(())
}

// ---- pass 2: capability --------------------------------------------------

/// Check `recipe` against the account's capability (blobext-03 §8.1-§8.8).
///
/// Read types (§8.1 `supportedImageReadTypes`) are the backend's to check:
/// it knows the source format only from the octets.
///
/// # Errors
///
/// `invalidProperties` or `tooLarge`, naming the failing path.
fn check_recipe(recipe: &Recipe, cap: &BlobAccountCapability) -> Result<(), Box<SetError>> {
    let key = recipe.key();
    let (list, type_field, media_type) = match recipe {
        Recipe::ImageConvert(r) => (
            &cap.supported_image_write_types,
            "type",
            Some(&r.media_type),
        ),
        Recipe::Archive(r) => (&cap.supported_archive_types, "type", Some(&r.media_type)),
        Recipe::Extract(r) => (&cap.supported_extract_types, "type", r.media_type.as_ref()),
        Recipe::Compress(r) => (&cap.supported_compress_types, "type", Some(&r.media_type)),
        Recipe::Decompress(r) => (
            &cap.supported_decompress_types,
            "type",
            r.media_type.as_ref(),
        ),
        Recipe::Delta(r) => (&cap.supported_delta_types, "type", Some(&r.media_type)),
        Recipe::Patch(r) => (&cap.supported_patch_types, "deltaType", Some(&r.delta_type)),
        // A recipe this crate does not know yet: the backend decides.
        _ => return Ok(()),
    };
    let Some(list) = list else {
        return Err(Box::new(
            invalid([key]).with_description("this server does not support this conversion"),
        ));
    };
    // A null extract or decompress type asks the backend to auto-detect.
    if let Some(t) = media_type
        && !list.iter().any(|s| s.eq_ignore_ascii_case(t))
    {
        return Err(Box::new(
            invalid([format!("{key}/{type_field}")])
                .with_description(format!("{t} is not an advertised type for {key}")),
        ));
    }
    match recipe {
        Recipe::ImageConvert(r) => check_image(r, cap),
        Recipe::Archive(r) => check_archive(r, cap),
        _ => Ok(()),
    }
}

fn check_image(r: &ImageConvertRecipe, cap: &BlobAccountCapability) -> Result<(), Box<SetError>> {
    if let Some(max) = cap.max_image_dimension {
        for (name, value) in [("width", r.width), ("height", r.height)] {
            if value.is_some_and(|v| v > max) {
                return Err(Box::new(
                    too_large(&format!(
                        "imageConvert/{name} exceeds maxImageDimension {max}"
                    ))
                    .with_properties([format!("imageConvert/{name}")]),
                ));
            }
        }
    }
    if r.quality.is_some_and(|q| !(1..=100).contains(&q)) {
        return Err(Box::new(
            invalid(["imageConvert/quality"]).with_description("quality is 1 to 100"),
        ));
    }
    if r.background.as_deref().is_some_and(|b| !is_hex_colour(b)) {
        return Err(Box::new(
            invalid(["imageConvert/background"])
                .with_description("background is a CSS hex colour such as #ffffff"),
        ));
    }
    Ok(())
}

/// `#` and 3, 4, 6 or 8 hex digits (CSS Color 4 hex notation).
fn is_hex_colour(s: &str) -> bool {
    s.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
    })
}

/// The entry types a known archive format holds (blobext-03 §8.3.1-§8.3.3),
/// or `None` for a format the backend checks itself.
fn format_entry_types(media_type: &str) -> Option<&'static [EntryType]> {
    const ALL: &[EntryType] = &[
        EntryType::File,
        EntryType::Directory,
        EntryType::Symlink,
        EntryType::Hardlink,
        EntryType::Fifo,
        EntryType::BlockDevice,
        EntryType::CharDevice,
    ];
    if media_type.eq_ignore_ascii_case("application/zip") {
        Some(&[EntryType::File, EntryType::Directory])
    } else if media_type.eq_ignore_ascii_case("application/x-tar")
        || media_type.eq_ignore_ascii_case("application/x-cpio")
    {
        Some(ALL)
    } else {
        None
    }
}

fn check_archive(r: &ArchiveRecipe, cap: &BlobAccountCapability) -> Result<(), Box<SetError>> {
    if let Some(max) = cap.max_archive_entries
        && u64::try_from(r.entries.len()).unwrap_or(u64::MAX) > max
    {
        return Err(Box::new(
            too_large(&format!("more than maxArchiveEntries ({max}) entries"))
                .with_properties(["archive/entries"]),
        ));
    }
    let allowed = format_entry_types(&r.media_type);
    for (i, e) in r.entries.iter().enumerate() {
        check_entry(e, allowed).map_err(|(property, why)| {
            Box::new(invalid([format!("archive/entries/{i}/{property}")]).with_description(why))
        })?;
    }
    Ok(())
}

/// The §8.3 rules for one entry, as `(property, reason)` on failure.
fn check_entry(
    e: &ArchiveEntry,
    allowed: Option<&[EntryType]>,
) -> Result<(), (&'static str, &'static str)> {
    let entry_type = e.entry_type.clone().unwrap_or(EntryType::File);
    if let EntryType::Other(_) = entry_type {
        // Not a §8.3 value: only the backend knows whether its format has it.
    } else if allowed.is_some_and(|a| !a.contains(&entry_type)) {
        return Err((
            "entryType",
            "the archive format has no entries of this type",
        ));
    }
    let is_file = entry_type == EntryType::File;
    match (is_file, &e.blob_id) {
        (true, None) => return Err(("blobId", "a file entry needs a blobId")),
        (false, Some(_)) if !matches!(entry_type, EntryType::Other(_)) => {
            return Err(("blobId", "only file entries have a blobId"));
        }
        _ => {}
    }
    if entry_type == EntryType::Directory && !e.name.ends_with('/') {
        return Err(("name", "a directory entry's name ends with \"/\""));
    }
    if e.name.split('/').any(|c| c == "..") {
        return Err(("name", "a \"..\" name component would escape the archive"));
    }
    let is_link = matches!(entry_type, EntryType::Symlink | EntryType::Hardlink);
    match (is_link, &e.link_target) {
        (true, None) => return Err(("linkTarget", "a link entry needs a linkTarget")),
        (false, Some(_)) if !matches!(entry_type, EntryType::Other(_)) => {
            return Err(("linkTarget", "only link entries have a linkTarget"));
        }
        _ => {}
    }
    if let Some(mode) = &e.mode
        && !(!mode.is_empty()
            && mode.bytes().all(|b| (b'0'..=b'7').contains(&b))
            && u64::from_str_radix(mode, 8).is_ok_and(|m| m <= 0o7777))
    {
        return Err(("mode", "mode is an octal string from 0 to 7777"));
    }
    Ok(())
}

// ---- pass 3: inputs ------------------------------------------------------

/// Every blob reference in `recipe` with its path in the request.
fn refs_mut(recipe: &mut Recipe) -> Vec<(String, &mut BlobRef)> {
    match recipe {
        Recipe::ImageConvert(r) => vec![("imageConvert/blobId".to_owned(), &mut r.blob_id)],
        Recipe::Archive(r) => r
            .entries
            .iter_mut()
            .enumerate()
            .filter_map(|(i, e)| {
                e.blob_id
                    .as_mut()
                    .map(|b| (format!("archive/entries/{i}/blobId"), b))
            })
            .collect(),
        Recipe::Extract(r) => vec![("extract/blobId".to_owned(), &mut r.blob_id)],
        Recipe::Compress(r) => vec![("compress/blobId".to_owned(), &mut r.blob_id)],
        Recipe::Decompress(r) => vec![("decompress/blobId".to_owned(), &mut r.blob_id)],
        Recipe::Delta(r) => vec![
            ("delta/blobId".to_owned(), &mut r.blob_id),
            ("delta/newBlobId".to_owned(), &mut r.new_blob_id),
        ],
        Recipe::Patch(r) => vec![
            ("patch/blobId".to_owned(), &mut r.blob_id),
            ("patch/deltaBlobId".to_owned(), &mut r.delta_blob_id),
        ],
        _ => Vec::new(),
    }
}

/// The creation ids `recipe` refers to.
fn creation_refs(recipe: &mut Recipe) -> Vec<Id> {
    refs_mut(recipe)
        .into_iter()
        .filter_map(|(_, r)| match r {
            BlobRef::Creation(cid) => Some(cid.clone()),
            _ => None,
        })
        .collect()
}

/// Resolve every reference of `recipe` in place and check each input.
///
/// # Errors
///
/// Per entry: `notFound` for a `#creationId` with no creation or a blob
/// the caller cannot see; `tooLarge` above `maxConvertSize`. Method level:
/// `serverFail` for a backend error.
async fn check_inputs<B: BlobBackend>(
    ctx: &Ctx<'_, B>,
    recipe: &mut Recipe,
) -> Result<Result<(), Box<SetError>>, JmapError> {
    for (path, r) in refs_mut(recipe) {
        let Some(id) = ctx.resolve(r) else {
            return Ok(Err(Box::new(not_found(&format!(
                "{path}: no blob was created under {r}"
            )))));
        };
        let meta = ctx
            .backend
            .blob_info(ctx.caller, ctx.account_id, &id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        let Some(meta) = meta else {
            return Ok(Err(Box::new(
                not_found(&format!("{path}: no such blob")).with_not_found(vec![id]),
            )));
        };
        if let Some(max) = ctx.capability.max_convert_size
            && meta.size > max
        {
            return Ok(Err(Box::new(
                too_large(&format!("{path}: the blob exceeds maxConvertSize"))
                    .with_properties([path])
                    .with_max_size(max),
            )));
        }
        *r = BlobRef::Id(id);
    }
    Ok(Ok(()))
}

/// Run one checked entry.
async fn convert_one<B: BlobBackend>(
    ctx: &mut Ctx<'_, B>,
    mut entry: Entry,
) -> Result<Result<Converted, Box<SetError>>, JmapError> {
    if let Err(e) = check_inputs(ctx, &mut entry.recipe).await? {
        return Ok(Err(e));
    }
    let is_extract = matches!(entry.recipe, Recipe::Extract(_));
    let mut params = ConvertParams::new(entry.recipe);
    params.no_persist = entry.no_persist && !is_extract;
    let outcome = match ctx
        .backend
        .convert(ctx.caller, ctx.account_id, params)
        .await
    {
        Ok(o) => o,
        Err(e) => return Ok(Err(Box::new(backend_set_error(e)))),
    };
    let id = outcome.id.clone();
    // An extract creates nothing, so there is nothing to leave out.
    let shown = (is_extract || !outcome.omit_from_response).then(|| {
        let mut blob = ConvertedBlob::new(outcome.id, outcome.media_type, outcome.size);
        blob.expires = Some(outcome.expires);
        blob.is_incomplete = outcome.is_incomplete;
        blob.description = outcome.description;
        blob.entries = outcome.entries;
        blob
    });
    Ok(Ok(Converted { id, shown }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours() {
        for ok in ["#fff", "#ffff", "#ffffff", "#FFFFFF00", "#0a0B0c"] {
            assert!(is_hex_colour(ok), "{ok}");
        }
        for bad in [
            "fff",
            "#ff",
            "#fffff",
            "#fffffffff",
            "#ggg",
            "#",
            "",
            "white",
        ] {
            assert!(!is_hex_colour(bad), "{bad}");
        }
    }
}
