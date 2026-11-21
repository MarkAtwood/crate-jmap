//! Hand-rolled POSIX ustar (`application/x-tar`) for the memory backend
//! (decision 13: archive and extract only, no new dependencies).
//!
//! An [`ArchiveRecipe`] is checked and planned when `Blob/convert` runs
//! ([`plan_archive`]), so every per-entry error is reported then; the
//! octets are built later, when the deferred result is first read
//! ([`build_tar`]). [`parse_tar`] lists an archive for an `ExtractRecipe`.
//!
//! Format: POSIX.1-1988 ustar (IEEE Std 1003.1, `pax` "ustar Interchange
//! Format"): 512-octet header blocks, content padded to 512 octets, two
//! zero blocks at the end. Numeric fields are octal, NUL-terminated.

use jmap_blob_types::{ArchiveEntry, ArchiveRecipe, BlobRef, EntryType};
use jmap_types::{Id, SetError, SetErrorType, UTCDate};

const BLOCK: usize = 512;
const NAME_LEN: usize = 100;
const PREFIX_LEN: usize = 155;
const OWNER_LEN: usize = 32;
/// The largest value of a 7-digit octal field (mode, uid, gid, dev).
const MAX_OCTAL_7: u64 = 0o7_777_777;
/// The largest value of an 11-digit octal field (size, mtime).
const MAX_OCTAL_11: u64 = 0o77_777_777_777;

/// One archive member, checked and ready to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TarEntry {
    /// `prefix` and `name` header fields.
    pub(super) prefix: String,
    pub(super) name: String,
    pub(super) mode: u64,
    pub(super) uid: u64,
    pub(super) gid: u64,
    pub(super) mtime: u64,
    pub(super) typeflag: u8,
    pub(super) linkname: String,
    pub(super) uname: String,
    pub(super) gname: String,
    pub(super) devmajor: u64,
    pub(super) devminor: u64,
    /// The content blob of a file entry.
    pub(super) content: Option<Id>,
}

fn invalid(path: String, description: impl Into<String>) -> Box<SetError> {
    Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties([path])
            .with_description(description),
    )
}

/// Check `recipe` against the ustar format and plan its members.
///
/// `now` (seconds since the epoch) is the default `modified` (blobext-03
/// §8.3: "If null, defaults to the current server time"). `size_of`
/// returns the size of a content blob, or `None` when it does not exist
/// (`notFound`). Errors name the failing property as
/// `archive/entries/<i>/<property>`.
pub(super) fn plan_archive(
    recipe: &ArchiveRecipe,
    now: i64,
    mut size_of: impl FnMut(&Id) -> Option<u64>,
) -> Result<Vec<TarEntry>, Box<SetError>> {
    let mut out = Vec::with_capacity(recipe.entries.len());
    for (i, e) in recipe.entries.iter().enumerate() {
        out.push(plan_entry(i, e, now, &mut size_of)?);
    }
    Ok(out)
}

fn plan_entry(
    i: usize,
    e: &ArchiveEntry,
    now: i64,
    size_of: &mut impl FnMut(&Id) -> Option<u64>,
) -> Result<TarEntry, Box<SetError>> {
    let path = |p: &str| format!("archive/entries/{i}/{p}");
    let entry_type = e.entry_type.clone().unwrap_or(EntryType::File);
    let typeflag = match entry_type {
        EntryType::File => b'0',
        EntryType::Hardlink => b'1',
        EntryType::Symlink => b'2',
        EntryType::CharDevice => b'3',
        EntryType::BlockDevice => b'4',
        EntryType::Directory => b'5',
        EntryType::Fifo => b'6',
        _ => {
            return Err(invalid(
                path("entryType"),
                "entry type not supported by application/x-tar",
            ));
        }
    };

    // §8.3: blobId "MUST be non-null for file entries. MUST be null or
    // absent for" every other type.
    let content = match (&entry_type, &e.blob_id) {
        (EntryType::File, Some(BlobRef::Id(id))) => {
            let size = size_of(id).ok_or_else(|| {
                Box::new(
                    SetError::new(SetErrorType::NotFound)
                        .with_not_found(vec![id.clone()])
                        .with_description(format!("entry blob {} not found", id.as_ref())),
                )
            })?;
            if size > MAX_OCTAL_11 {
                return Err(invalid(path("blobId"), "entry too large for ustar"));
            }
            Some(id.clone())
        }
        (EntryType::File, Some(BlobRef::Creation(_))) => {
            return Err(invalid(path("blobId"), "unresolved creation reference"));
        }
        (EntryType::File, None) => {
            return Err(invalid(path("blobId"), "a file entry needs a blobId"));
        }
        (_, Some(_)) => {
            return Err(invalid(path("blobId"), "only file entries have a blobId"));
        }
        (_, None) => None,
    };

    let (prefix, name) = split_name(&e.name).ok_or_else(|| {
        invalid(
            path("name"),
            "name does not fit the ustar name and prefix fields",
        )
    })?;
    if matches!(entry_type, EntryType::Directory) != e.name.ends_with('/') {
        return Err(invalid(
            path("name"),
            "directory entries, and only they, have a name ending with \"/\"",
        ));
    }

    // §8.3: linkTarget "MUST be non-null when entryType is "symlink" or
    // "hardlink". MUST be null for all other entry types."
    let is_link = matches!(entry_type, EntryType::Symlink | EntryType::Hardlink);
    let linkname = match (&e.link_target, is_link) {
        (Some(t), true) if !t.is_empty() && t.len() <= NAME_LEN && !t.contains('\0') => t.clone(),
        (Some(_), true) => {
            return Err(invalid(
                path("linkTarget"),
                "link target does not fit ustar",
            ));
        }
        (None, true) => return Err(invalid(path("linkTarget"), "links need a linkTarget")),
        (Some(_), false) => {
            return Err(invalid(path("linkTarget"), "only links have a linkTarget"));
        }
        (None, false) => String::new(),
    };

    let mode = match &e.mode {
        Some(m) => {
            parse_mode(m).ok_or_else(|| invalid(path("mode"), "mode is not octal 0 to 7777"))?
        }
        None => match entry_type {
            EntryType::Directory => 0o755,
            EntryType::Symlink => 0o777,
            _ => 0o644,
        },
    };

    let mtime = match &e.modified {
        Some(d) => epoch(d).ok_or_else(|| invalid(path("modified"), "modified is out of range"))?,
        None => u64::try_from(now).unwrap_or(0),
    };

    let numeric = |v: Option<u64>, p: &str| -> Result<u64, Box<SetError>> {
        let v = v.unwrap_or(0);
        if v > MAX_OCTAL_7 {
            Err(invalid(path(p), "value too large for ustar"))
        } else {
            Ok(v)
        }
    };
    let owner = |v: &Option<String>, p: &str| -> Result<String, Box<SetError>> {
        let v = v.clone().unwrap_or_default();
        if v.len() > OWNER_LEN || v.contains('\0') {
            Err(invalid(path(p), "name too long for ustar"))
        } else {
            Ok(v)
        }
    };

    Ok(TarEntry {
        prefix,
        name,
        mode,
        uid: numeric(e.uid, "uid")?,
        gid: numeric(e.gid, "gid")?,
        mtime,
        typeflag,
        linkname,
        uname: owner(&e.owner_name, "ownerName")?,
        gname: owner(&e.group_name, "groupName")?,
        devmajor: numeric(e.dev_major, "devMajor")?,
        devminor: numeric(e.dev_minor, "devMinor")?,
        content,
    })
}

/// Parse an octal permission string such as `"0644"` (at most 0o7777).
fn parse_mode(m: &str) -> Option<u64> {
    if m.is_empty() || !m.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return None;
    }
    u64::from_str_radix(m, 8).ok().filter(|v| *v <= 0o7777)
}

fn epoch(d: &UTCDate) -> Option<u64> {
    let s = d.to_epoch_seconds().ok()?;
    u64::try_from(s).ok().filter(|v| *v <= MAX_OCTAL_11)
}

/// Split a path into the ustar `prefix` and `name` fields: the whole path
/// in `name` when it fits, else at a `/` with the prefix up to 155 octets
/// and a non-empty name up to 100.
fn split_name(path: &str) -> Option<(String, String)> {
    if path.is_empty() || path.contains('\0') {
        return None;
    }
    if path.len() <= NAME_LEN {
        return Some((String::new(), path.to_owned()));
    }
    path.char_indices()
        .filter(|&(_, c)| c == '/')
        .map(|(i, _)| (&path[..i], &path[i + 1..]))
        .find(|(p, n)| p.len() <= PREFIX_LEN && !n.is_empty() && n.len() <= NAME_LEN)
        .map(|(p, n)| (p.to_owned(), n.to_owned()))
}

fn put(field: &mut [u8], bytes: &[u8]) {
    let n = bytes.len().min(field.len());
    field[..n].copy_from_slice(&bytes[..n]);
}

/// Write `value` as zero-padded octal filling `field` but its last octet,
/// which stays NUL.
fn put_octal(field: &mut [u8], value: u64) {
    let digits = field.len() - 1;
    put(field, format!("{value:0digits$o}").as_bytes());
}

/// The 512-octet header of `e`, whose content is `size` octets.
pub(super) fn header(e: &TarEntry, size: u64) -> [u8; BLOCK] {
    let mut h = [0u8; BLOCK];
    put(&mut h[0..100], e.name.as_bytes());
    put_octal(&mut h[100..108], e.mode);
    put_octal(&mut h[108..116], e.uid);
    put_octal(&mut h[116..124], e.gid);
    put_octal(&mut h[124..136], size);
    put_octal(&mut h[136..148], e.mtime);
    h[156] = e.typeflag;
    put(&mut h[157..257], e.linkname.as_bytes());
    put(&mut h[257..263], b"ustar\0");
    put(&mut h[263..265], b"00");
    put(&mut h[265..297], e.uname.as_bytes());
    put(&mut h[297..329], e.gname.as_bytes());
    put_octal(&mut h[329..337], e.devmajor);
    put_octal(&mut h[337..345], e.devminor);
    put(&mut h[345..500], e.prefix.as_bytes());
    // The checksum is the sum of the header octets with the checksum
    // field read as eight spaces, written as six octal digits, NUL, space.
    h[148..156].copy_from_slice(b"        ");
    let sum: u64 = h.iter().map(|&b| u64::from(b)).sum();
    put(&mut h[148..156], format!("{sum:06o}\0 ").as_bytes());
    h
}

/// Build the archive: each member's header and content, padded, then two
/// zero blocks. `content` returns a file entry's octets, or `None` when the
/// blob is gone (the deferred generation then fails).
pub(super) fn build_tar(
    entries: &[TarEntry],
    mut content: impl FnMut(&Id) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for e in entries {
        let data = match &e.content {
            Some(id) => content(id)?,
            None => Vec::new(),
        };
        out.extend_from_slice(&header(e, data.len() as u64));
        out.extend_from_slice(&data);
        out.resize(out.len().next_multiple_of(BLOCK), 0);
    }
    out.resize(out.len() + 2 * BLOCK, 0);
    Some(out)
}

/// `true` when `bytes` starts with a ustar header (magic `ustar` at
/// offset 257): the auto-detection of an `ExtractRecipe` with a null `type`.
pub(super) fn looks_like_tar(bytes: &[u8]) -> bool {
    bytes.get(257..262) == Some(b"ustar")
}

/// One member found by [`parse_tar`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedEntry {
    pub(super) entry: ArchiveEntry,
    /// The content of a file entry.
    pub(super) content: Option<Vec<u8>>,
}

/// The listing of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedTar {
    pub(super) entries: Vec<ParsedEntry>,
    /// Why listing stopped early, if it did (`isIncomplete`).
    pub(super) problem: Option<String>,
}

fn field_str(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

fn field_octal(field: &[u8]) -> Option<u64> {
    let s = field_str(field);
    let s = s.trim_matches(|c: char| c == ' ' || c == '\0');
    if s.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(s, 8).ok()
}

/// List the members of a ustar archive.
///
/// Returns `None` when the first header is not a valid tar header (not an
/// archive at all). A later bad header or truncated content stops the
/// listing with [`ParsedTar::problem`] set and the members found so far.
pub(super) fn parse_tar(bytes: &[u8]) -> Option<ParsedTar> {
    let mut entries = Vec::new();
    let mut pos = 0usize;
    let problem = loop {
        let Some(h) = bytes.get(pos..pos + BLOCK) else {
            if entries.is_empty() {
                return None;
            }
            break Some("archive ends without its end-of-archive blocks".to_owned());
        };
        if h.iter().all(|&b| b == 0) {
            break None;
        }
        let stored = field_octal(&h[148..156]);
        let mut copy = [0u8; BLOCK];
        copy.copy_from_slice(h);
        copy[148..156].copy_from_slice(b"        ");
        let sum: u64 = copy.iter().map(|&b| u64::from(b)).sum();
        if stored != Some(sum) {
            if entries.is_empty() {
                return None;
            }
            break Some(format!("bad header checksum at offset {pos}"));
        }
        let Some(size) = field_octal(&h[124..136]).and_then(|s| usize::try_from(s).ok()) else {
            break Some(format!("bad size field at offset {pos}"));
        };
        let name = {
            let n = field_str(&h[0..100]);
            let p = if &h[257..262] == b"ustar" {
                field_str(&h[345..500])
            } else {
                String::new()
            };
            if p.is_empty() { n } else { format!("{p}/{n}") }
        };
        let entry_type = match h[156] {
            b'0' | 0 => EntryType::File,
            b'1' => EntryType::Hardlink,
            b'2' => EntryType::Symlink,
            b'3' => EntryType::CharDevice,
            b'4' => EntryType::BlockDevice,
            b'5' => EntryType::Directory,
            b'6' => EntryType::Fifo,
            other => EntryType::Other(char::from(other).to_string()),
        };
        let start = pos + BLOCK;
        let Some(data) = bytes.get(start..start + size) else {
            break Some(format!("content of {name:?} is truncated"));
        };

        let mut entry = ArchiveEntry::new(name);
        entry.modified = field_octal(&h[136..148])
            .and_then(|s| i64::try_from(s).ok())
            .and_then(jmap_server::utc_date_from_epoch_seconds);
        entry.mode = field_octal(&h[100..108]).map(|m| format!("{m:04o}"));
        entry.uid = field_octal(&h[108..116]);
        entry.gid = field_octal(&h[116..124]);
        let nonempty = |s: String| (!s.is_empty()).then_some(s);
        entry.owner_name = nonempty(field_str(&h[265..297]));
        entry.group_name = nonempty(field_str(&h[297..329]));
        if matches!(entry_type, EntryType::Symlink | EntryType::Hardlink) {
            entry.link_target = nonempty(field_str(&h[157..257]));
        }
        if matches!(entry_type, EntryType::CharDevice | EntryType::BlockDevice) {
            entry.dev_major = field_octal(&h[329..337]);
            entry.dev_minor = field_octal(&h[337..345]);
        }
        let content = matches!(entry_type, EntryType::File).then(|| data.to_vec());
        entry.entry_type = Some(entry_type);
        entries.push(ParsedEntry { entry, content });
        pos = start + size.next_multiple_of(BLOCK);
    };
    Some(ParsedTar { entries, problem })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, blob: &str) -> ArchiveEntry {
        ArchiveEntry::file(name, BlobRef::Id(Id::from(blob)))
    }

    fn with(mut e: ArchiveEntry, f: impl FnOnce(&mut ArchiveEntry)) -> ArchiveEntry {
        f(&mut e);
        e
    }

    fn plan(entries: Vec<ArchiveEntry>) -> Result<Vec<TarEntry>, Box<SetError>> {
        plan_archive(&ArchiveRecipe::new("application/x-tar", entries), 0, |id| {
            (id.as_ref() != "Bmissing").then_some(5)
        })
    }

    /// The header of `hello.txt` (mode 0644, uid/gid 0, size 5, mtime
    /// 1700000000), checked field by field against hand-written values.
    ///
    /// Oracle: octal 5 is "00000000005" and 1700000000 is 0o14524770400.
    /// The checksum oracle sums the literal field contents listed below,
    /// independently of `header`, with the checksum field as eight spaces.
    #[test]
    fn header_matches_ustar_oracle() {
        let mut e = plan(vec![file("hello.txt", "B1")]).unwrap().remove(0);
        e.mtime = 1_700_000_000;
        let h = header(&e, 5);

        assert_eq!(&h[0..10], b"hello.txt\0");
        assert_eq!(&h[100..108], b"0000644\0");
        assert_eq!(&h[108..116], b"0000000\0");
        assert_eq!(&h[116..124], b"0000000\0");
        assert_eq!(&h[124..136], b"00000000005\0");
        assert_eq!(&h[136..148], b"14524770400\0");
        assert_eq!(h[156], b'0');
        assert_eq!(&h[257..265], b"ustar\x0000");
        assert_eq!(&h[329..337], b"0000000\0");

        // Hand oracle for the checksum: the literal non-zero fields.
        let literal: [&[u8]; 10] = [
            b"hello.txt",
            b"0000644",
            b"0000000",
            b"0000000",
            b"00000000005",
            b"14524770400",
            b"        ", // checksum field as spaces
            b"0",
            b"ustar00",
            b"00000000000000", // devmajor + devminor digits
        ];
        let expected: u64 = literal
            .iter()
            .flat_map(|f| f.iter())
            .map(|&b| u64::from(b))
            .sum();
        assert_eq!(&h[148..156], format!("{expected:06o}\0 ").as_bytes());
    }

    #[test]
    fn build_then_parse_round_trips() {
        let entries = plan(vec![
            with(ArchiveEntry::new("site/"), |e| {
                e.entry_type = Some(EntryType::Directory);
            }),
            file("site/index.html", "B1"),
            with(ArchiveEntry::new("site/home.html"), |e| {
                e.entry_type = Some(EntryType::Symlink);
                e.link_target = Some("index.html".into());
            }),
        ])
        .unwrap();
        let tar = build_tar(&entries, |_| Some(b"<p/>\n".to_vec())).unwrap();
        assert_eq!(tar.len(), 4 * BLOCK + 2 * BLOCK);
        assert!(looks_like_tar(&tar));

        let parsed = parse_tar(&tar).unwrap();
        assert_eq!(parsed.problem, None);
        let names: Vec<_> = parsed
            .entries
            .iter()
            .map(|p| p.entry.name.as_str())
            .collect();
        assert_eq!(names, ["site/", "site/index.html", "site/home.html"]);
        assert_eq!(parsed.entries[0].entry.mode.as_deref(), Some("0755"));
        assert_eq!(parsed.entries[1].content.as_deref(), Some(&b"<p/>\n"[..]));
        assert_eq!(parsed.entries[1].entry.mode.as_deref(), Some("0644"));
        assert_eq!(
            parsed.entries[2].entry.link_target.as_deref(),
            Some("index.html")
        );
        assert_eq!(parsed.entries[2].entry.entry_type, Some(EntryType::Symlink));
    }

    #[test]
    fn long_names_use_the_prefix() {
        let dir = "d".repeat(120);
        let name = format!("{dir}/file.txt");
        let e = plan(vec![file(&name, "B1")]).unwrap().remove(0);
        assert_eq!(
            (e.prefix.as_str(), e.name.as_str()),
            (dir.as_str(), "file.txt")
        );
        let tar = build_tar(&[e], |_| Some(Vec::new())).unwrap();
        assert_eq!(parse_tar(&tar).unwrap().entries[0].entry.name, name);
        // No `/` to split at.
        assert!(plan(vec![file(&"x".repeat(101), "B1")]).is_err());
    }

    #[test]
    fn planning_errors_name_the_property() {
        let props = |r: Result<Vec<TarEntry>, Box<SetError>>| r.unwrap_err().properties.unwrap();
        assert_eq!(
            props(plan(vec![file("a", "B1"), ArchiveEntry::new("b")])),
            ["archive/entries/1/blobId"]
        );
        assert_eq!(
            props(plan(vec![with(ArchiveEntry::new("l"), |e| {
                e.entry_type = Some(EntryType::Symlink);
            })])),
            ["archive/entries/0/linkTarget"]
        );
        assert_eq!(
            props(plan(vec![with(file("a", "B1"), |e| {
                e.mode = Some("0999".into());
            })])),
            ["archive/entries/0/mode"]
        );
        assert_eq!(
            props(plan(vec![with(ArchiveEntry::new("nodir"), |e| {
                e.entry_type = Some(EntryType::Directory);
            })])),
            ["archive/entries/0/name"]
        );
        assert_eq!(
            props(plan(vec![with(ArchiveEntry::new("s"), |e| {
                e.entry_type = Some(EntryType::Other("socket".into()));
            })])),
            ["archive/entries/0/entryType"]
        );
        let missing = plan(vec![file("a", "Bmissing")]).unwrap_err();
        assert_eq!(missing.error_type, SetErrorType::NotFound);
    }

    #[test]
    fn parse_rejects_non_tar_and_reports_truncation() {
        assert!(parse_tar(b"not a tar at all").is_none());
        assert!(parse_tar(&[1u8; 1024]).is_none());
        assert!(!looks_like_tar(b"PK\x03\x04"));

        let entries = plan(vec![file("a", "B1"), file("b", "B1")]).unwrap();
        let tar = build_tar(&entries, |_| Some(vec![b'x'; 600])).unwrap();
        // Cut inside the second member's content.
        let cut = &tar[..BLOCK * 3 + BLOCK + 100];
        let parsed = parse_tar(cut).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert!(parsed.problem.unwrap().contains("truncated"));
    }
}
