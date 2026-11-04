//! SP7–SP8, PS3: one boundary for layout, size checks and retrying cloud files.

use super::folder::{ours, Folder, Listed};
use super::ids::{identity_hash, Hash32, Id16};
use super::shared::{self, Area, SharedFolder};
use std::{collections::BTreeSet, io};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Claim,
    Trust,
    Enrollment,
    Device,
    DeviceOverflow,
    Change,
    Index,
    Part,
    Blob,
    Book,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub kind: Kind,
    pub limit: Option<u64>,
}

/// SP1, SP8–SP9: exact layout under `Linen/`, including ID and hash spellings.
pub fn layout(path: &str) -> Option<Slot> {
    if !ours(path) {
        return None;
    }
    let id = |s: &str| Id16::parse(s).is_some();
    let hash = |s: &str| Hash32::parse(s).is_some();
    let json_id = |s: &str| s.strip_suffix(".json").is_some_and(id);
    let json_hash = |s: &str| s.strip_suffix(".json").is_some_and(hash);
    let parts: Vec<_> = path.split('/').collect();
    let (kind, limit) = match parts.as_slice() {
        ["libraries", name] if json_id(name) => (Kind::Claim, Some(4 << 10)),
        // SP8: PROVISIONAL trust-record limit, as in the protocol table.
        ["trust", name] if json_hash(name) => (Kind::Trust, Some(64 << 10)),
        ["enroll", name] if enrollment(name) => (Kind::Enrollment, Some(16 << 10)),
        ["devices", name] if json_id(name) => (
            Kind::Device,
            Some(super::device_file::MAX_DEVICE_FILE as u64),
        ),
        ["devices", device, name] if id(device) && json_hash(name) => (
            Kind::DeviceOverflow,
            Some(super::device_file::MAX_OVERFLOW_FILE as u64),
        ),
        ["changes", device, name]
            if id(device)
                && name.strip_suffix(".json").is_some_and(|s| {
                    s.len() == 12 && s.bytes().all(|b| b.is_ascii_digit()) && s != "000000000000"
                }) =>
        {
            (Kind::Change, Some(super::chain::MAX_CHANGE_BYTES as u64))
        }
        ["state", device, generation, "index.json"] if id(device) && hash(generation) => {
            (Kind::Index, Some(super::view::MAX_INDEX as u64))
        }
        ["state", device, generation, name]
            if id(device) && hash(generation) && json_hash(name) =>
        {
            (Kind::Part, Some(super::view::MAX_PART as u64))
        }
        // SP8, SY28: the path alone has no body type. Use record.rs's largest
        // per-type limit (Annotation/Note, PROVISIONAL 4 MB); the caller still
        // checks the actual record kind's body_limit before accepting a body.
        ["blobs", name] if hash(name) => (
            Kind::Blob,
            Some(super::record::Kind::Annotation.body_limit()),
        ),
        // SP8, SY2: books have no cap here; Stage 4 applies importer limits.
        ["books", name] if hash(name) => (Kind::Book, None),
        _ => return None,
    };
    Some(Slot { kind, limit })
}

fn enrollment(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".json") else {
        return false;
    };
    let parts: Vec<_> = stem.split('.').collect();
    let id = |s: &str| Id16::parse(s).is_some();
    match parts.as_slice() {
        [applicant] | [applicant, "reveal"] => id(applicant),
        [applicant, device] | [applicant, device, "ok"] => id(applicant) && id(device),
        _ => false,
    }
}

/// SY2, SP2, SP7–SP8: validate before staging any bytes. Folder owns the atomic
/// temporary write and rename; no caller can write through a storage-made token.
pub fn write_file<F: Folder>(folder: &mut F, path: &str, bytes: &[u8]) -> io::Result<()> {
    let slot = layout(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "outside sync layout"))?;
    if slot.limit.is_some_and(|limit| bytes.len() as u64 > limit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file exceeds sync limit",
        ));
    }
    folder.write(path, bytes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    Outside,
    Candidate { of: String },
    TooLarge,
    Gone,
    Bytes(Vec<u8>),
}

/// SP7: PROVISIONAL copy-name recognition until Stage 6 measures real names.
/// Both JSON files and shared objects must map back into the exact SP8 layout;
/// the eventual signature/hash check, not this guess, establishes identity.
pub fn candidate_of(path: &str) -> Option<String> {
    let (dir, name) = path.rsplit_once('/')?;
    let base = if let Some(stem) = name.strip_suffix(".json") {
        let end = stem.find(|c: char| {
            !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '.' && c != '-'
        })?;
        format!("{}.json", &stem[..end])
    } else {
        shared::copy_of(name)?.to_owned()
    };
    let base = format!("{dir}/{base}");
    layout(&base)?;
    Some(base)
}

/// SP7: classify a listed name before any read. A candidate is metadata only;
/// `read_candidate` is a separate, explicit decision by its caller.
pub fn read_file<F: Folder>(folder: &F, listed: &Listed<F::Token>) -> io::Result<Read> {
    if !listed.ours {
        return Ok(candidate_of(&listed.name).map_or(Read::Outside, |of| Read::Candidate { of }));
    }
    match layout(&listed.name) {
        Some(slot) => read_slot(folder, &listed.token, slot),
        None => Ok(Read::Outside),
    }
}

/// SP7: read only a recognised copy, with the copied file's size limit. The
/// caller must check these bytes as that file's, never as a new independent file.
pub fn read_candidate<F: Folder>(folder: &F, listed: &Listed<F::Token>) -> io::Result<Read> {
    let slot = (!listed.ours)
        .then(|| candidate_of(&listed.name))
        .flatten()
        .and_then(|path| layout(&path));
    match slot {
        Some(slot) => read_slot(folder, &listed.token, slot),
        None => Ok(Read::Outside),
    }
}

fn read_slot<F: Folder>(folder: &F, token: &F::Token, slot: Slot) -> io::Result<Read> {
    let Some(size) = folder.stat(token)? else {
        return Ok(Read::Gone);
    };
    if slot.limit.is_some_and(|limit| size > limit) {
        return Ok(Read::TooLarge);
    }
    let Some(bytes) = folder.read(token)? else {
        return Ok(Read::Gone);
    };
    // SP3, SY28: storage may change after stat; recheck before returning bytes.
    if slot.limit.is_some_and(|limit| bytes.len() as u64 > limit) {
        return Ok(Read::TooLarge);
    }
    Ok(Read::Bytes(bytes))
}

/// SP7, PS3: a failed hash/signature is not permanent rejection. The caller
/// records its path and takes the set when starting the next pass.
#[derive(Default, Debug)]
pub struct Retry(BTreeSet<String>);

impl Retry {
    pub fn insert(&mut self, path: impl Into<String>) {
        self.0.insert(path.into());
    }

    pub fn take(&mut self) -> BTreeSet<String> {
        std::mem::take(&mut self.0)
    }
}

/// PS1, SP7: the existing shared-object rules over the general folder. Tokens
/// retain their listed paths so every read can enforce SP8, including copies.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "PS8: used by tests until the pass is built")
)]
pub(crate) struct Shared<'a, F: Folder> {
    pub(crate) folder: &'a mut F,
}

impl<F: Folder> SharedFolder for Shared<'_, F> {
    type Token = Listed<F::Token>;

    fn list(&self, area: Area) -> Vec<(String, Self::Token)> {
        let dir = match area {
            Area::Books => "books",
            Area::Blobs => "blobs",
        };
        let mut result = Vec::new();
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        loop {
            // SharedFolder predates fallible listings. A failed/incomplete
            // snapshot must take no tidy actions; retry on the next pass.
            let Ok(page) = self.folder.list(dir, cursor.as_deref()) else {
                return Vec::new();
            };
            for entry in page.entries {
                let Some((parent, name)) = entry.name.rsplit_once('/') else {
                    continue;
                };
                if parent == dir
                    && (layout(&entry.name).is_some()
                        || (!entry.ours && candidate_of(&entry.name).is_some()))
                {
                    result.push((name.to_owned(), entry));
                }
            }
            cursor = page.next;
            let Some(next) = &cursor else {
                return result;
            };
            if !seen.insert(next.clone()) {
                return Vec::new();
            }
        }
    }

    fn hash(&self, token: &Self::Token) -> Option<Hash32> {
        let read = if token.ours {
            read_file(self.folder, token)
        } else {
            read_candidate(self.folder, token)
        };
        match read.ok()? {
            Read::Bytes(bytes) => Some(identity_hash(&bytes)),
            _ => None,
        }
    }

    fn replace(&mut self, name: &str, from: &Self::Token, expected: &Hash32) -> io::Result<()> {
        let (dir, _) = from
            .name
            .rsplit_once('/')
            .ok_or_else(|| io::Error::other("missing object area"))?;
        if !matches!(dir, "books" | "blobs") || Hash32::parse(name).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a shared object",
            ));
        }
        let read = if from.ours {
            read_file(self.folder, from)?
        } else {
            read_candidate(self.folder, from)?
        };
        let Read::Bytes(bytes) = read else {
            return Err(io::Error::other("copy unavailable"));
        };
        if identity_hash(&bytes) != *expected {
            return Err(io::Error::other("the copy changed since it was checked"));
        }
        write_file(self.folder, &format!("{dir}/{name}"), &bytes)
    }

    fn remove(&mut self, token: &Self::Token) {
        // SY11: SharedFolder cannot report a removal failure. A remaining copy
        // is safe and appears again on the next pass.
        let _ = self.folder.remove(&token.token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{crypto::blob_name, folder::Memory};

    fn entry<F: Folder>(folder: &F, path: &str) -> Listed<F::Token> {
        folder
            .list(path.rsplit_once('/').unwrap().0, None)
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.name == path)
            .unwrap()
    }

    #[test]
    fn layout_matches_every_sp8_row_and_limit() {
        let d = "ab".repeat(16);
        let h = "cd".repeat(32);
        for (path, kind, limit) in [
            (format!("libraries/{d}.json"), Kind::Claim, Some(4 << 10)),
            (format!("trust/{h}.json"), Kind::Trust, Some(64 << 10)),
            (format!("enroll/{d}.json"), Kind::Enrollment, Some(16 << 10)),
            (
                format!("enroll/{d}.reveal.json"),
                Kind::Enrollment,
                Some(16 << 10),
            ),
            (
                format!("enroll/{d}.{d}.json"),
                Kind::Enrollment,
                Some(16 << 10),
            ),
            (
                format!("enroll/{d}.{d}.ok.json"),
                Kind::Enrollment,
                Some(16 << 10),
            ),
            (format!("devices/{d}.json"), Kind::Device, Some(256 << 10)),
            (
                format!("devices/{d}/{h}.json"),
                Kind::DeviceOverflow,
                Some(8 << 20),
            ),
            (
                format!("changes/{d}/000000000001.json"),
                Kind::Change,
                Some(8 << 20),
            ),
            (
                format!("state/{d}/{h}/index.json"),
                Kind::Index,
                Some(1 << 20),
            ),
            (format!("state/{d}/{h}/{h}.json"), Kind::Part, Some(8 << 20)),
            (format!("blobs/{h}"), Kind::Blob, Some(4 << 20)),
            (format!("books/{h}"), Kind::Book, None),
        ] {
            assert_eq!(layout(&path), Some(Slot { kind, limit }), "{path}");
        }
        for path in [
            format!("libraries/{h}.json"),
            format!("trust/{d}.json"),
            format!("devices/{}/{}.json", d.to_uppercase(), h),
            format!("changes/{d}/1.json"),
            format!("changes/{d}/000000000000.json"),
            format!("changes/{d}/1000000000000.json"),
            format!("state/{d}/index.json"),
            format!("enroll/{d}.ok.json"),
            format!("enroll/{d}.{d}.reveal.json"),
            format!("books/{h}.json"),
            format!("blobs/{}", "g".repeat(64)),
        ] {
            assert_eq!(layout(&path), None, "{path}");
        }
    }

    #[test]
    fn write_file_refuses_bad_paths_and_oversize_before_writing() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        let d = "ab".repeat(16);
        for path in [
            "strangers/file.json".to_owned(),
            format!("/libraries/{d}.json"),
            format!("libraries/../libraries/{d}.json"),
            format!("a/b/c/d/{d}.json"),
            format!("libraries/{}.json", d.to_uppercase()),
            format!("libraries/{d} 2.json"),
            format!("libraries/{d}_.json"),
            format!("libraries//{d}.json"),
            format!("libraries/.{d}.json"),
            format!("libraries/{d}.json/"),
        ] {
            assert!(write_file(&mut view, &path, b"x").is_err(), "{path}");
        }
        let path = format!("libraries/{d}.json");
        assert!(write_file(&mut view, &path, &vec![0; (4 << 10) + 1]).is_err());
        assert!(view.list("libraries", None).unwrap().entries.is_empty());
        let exact = vec![0; 4 << 10];
        write_file(&mut view, &path, &exact).unwrap();
        assert_eq!(
            read_file(&view, &entry(&view, &path)).unwrap(),
            Read::Bytes(exact)
        );
    }

    #[test]
    fn read_file_outside_and_oversize_never_read_bytes() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        let huge = format!("libraries/{}.json", "ab".repeat(16));
        for path in ["strangers/file.json", "libraries/not-an-id.json", &huge] {
            // Bypass write_file to model untrusted storage contents.
            view.write(path, &vec![0; (4 << 10) + 1]).unwrap();
            assert_eq!(
                read_file(&view, &entry(&view, path)).unwrap(),
                if path == huge {
                    Read::TooLarge
                } else {
                    Read::Outside
                }
            );
            assert_eq!(memory.read_count("a", path), 0);
        }
    }

    #[test]
    fn read_file_candidates_use_both_name_shapes_without_reading() {
        for path in [
            format!("changes/{}/000000000001.json", "ab".repeat(16)),
            format!("blobs/{}", "cd".repeat(32)),
        ] {
            let memory = Memory::default();
            let mut a = memory.view("a");
            let mut b = memory.view("b");
            memory.conflict_copies(true);
            write_file(&mut a, &path, b"original").unwrap();
            write_file(&mut b, &path, b"copy").unwrap();
            let copy = a
                .list(path.rsplit_once('/').unwrap().0, None)
                .unwrap()
                .entries
                .into_iter()
                .find(|entry| !entry.ours)
                .unwrap();
            assert_eq!(read_file(&a, &copy).unwrap(), Read::Candidate { of: path });
            assert_eq!(memory.read_count("a", &copy.name), 0);
            assert_eq!(
                read_candidate(&a, &copy).unwrap(),
                Read::Bytes(b"copy".to_vec())
            );
        }
    }

    #[test]
    fn read_candidate_checks_the_base_limit_before_reading() {
        let memory = Memory::default();
        let mut a = memory.view("a");
        let mut b = memory.view("b");
        let path = format!("libraries/{}.json", "ab".repeat(16));
        memory.conflict_copies(true);
        a.write(&path, b"good").unwrap();
        b.write(&path, &vec![0; (4 << 10) + 1]).unwrap();
        let copy = a
            .list("libraries", None)
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| !entry.ours)
            .unwrap();
        assert_eq!(read_candidate(&a, &copy).unwrap(), Read::TooLarge);
        assert_eq!(memory.read_count("a", &copy.name), 0);
    }

    #[test]
    fn read_file_vanished_is_gone_without_reading() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        let path = format!("devices/{}.json", "ab".repeat(16));
        write_file(&mut view, &path, b"good").unwrap();
        let listed = entry(&view, &path);
        memory.vanish(&path);
        assert_eq!(read_file(&view, &listed).unwrap(), Read::Gone);
        assert_eq!(memory.read_count("a", &path), 0);
    }

    #[test]
    fn read_file_half_synced_is_retried_then_good() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        let path = format!("blobs/{}", "cd".repeat(32));
        write_file(&mut view, &path, b"good").unwrap();
        memory.half_synced("a", &path, b"half", 1);
        let mut retry = Retry::default();
        let Read::Bytes(bytes) = read_file(&view, &entry(&view, &path)).unwrap() else {
            panic!("read bytes");
        };
        if identity_hash(&bytes) != identity_hash(b"good") {
            retry.insert(&path);
            retry.insert(&path);
        }
        let next = retry.take();
        assert_eq!(next.len(), 1);
        assert!(next.contains(&path));
        for path in next {
            assert_eq!(
                read_file(&view, &entry(&view, &path)).unwrap(),
                Read::Bytes(b"good".to_vec())
            );
        }
        assert!(retry.take().is_empty());
    }

    #[test]
    fn read_file_rechecks_size_when_bytes_change_after_stat() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        let path = format!("libraries/{}.json", "ab".repeat(16));
        write_file(&mut view, &path, b"small").unwrap();
        memory.half_synced("a", &path, &vec![0; (4 << 10) + 1], 1);
        assert_eq!(
            read_file(&view, &entry(&view, &path)).unwrap(),
            Read::TooLarge
        );
    }

    #[test]
    fn failed_rename_leaves_old_target_and_an_outside_temporary_file() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        let path = format!("devices/{}.json", "ab".repeat(16));
        write_file(&mut view, &path, b"old").unwrap();
        memory.fail_rename(&path);
        assert!(write_file(&mut view, &path, b"new").is_err());
        let entries = view.list("devices", None).unwrap().entries;
        assert_eq!(entries.len(), 2);
        let temp = entries.iter().find(|entry| !entry.ours).unwrap();
        let suffix = temp.name.strip_prefix("devices/.linen-tmp-").unwrap();
        assert_eq!(suffix.len(), 16);
        assert!(suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(read_file(&view, temp).unwrap(), Read::Outside);
        assert_eq!(memory.read_count("a", &temp.name), 0);
        assert_eq!(view.read(&temp.token).unwrap(), Some(b"new".to_vec()));
        assert_eq!(
            read_file(&view, &entry(&view, &path)).unwrap(),
            Read::Bytes(b"old".to_vec())
        );
        write_file(&mut view, &path, b"later").unwrap();
        assert_eq!(view.list("devices", None).unwrap().entries.len(), 2);
        assert_eq!(
            read_file(&view, &entry(&view, &path)).unwrap(),
            Read::Bytes(b"later".to_vec())
        );
    }

    #[test]
    fn shared_tidy_over_memory_keeps_one_concurrently_written_blob() {
        let memory = Memory::default();
        let mut a = memory.view("a");
        let mut b = memory.view("b");
        let naming = [17; 32];
        let name = blob_name(&naming, &identity_hash(b"good"));
        let path = format!("blobs/{name}");
        memory.conflict_copies(true);
        write_file(&mut a, &path, b"good").unwrap();
        write_file(&mut b, &path, b"good").unwrap();
        assert_eq!(a.list("blobs", None).unwrap().entries.len(), 2);
        let taken = shared::tidy(&mut Shared { folder: &mut a }, Area::Blobs, &naming);
        assert!(matches!(
            taken[0].1.as_slice(),
            [shared::Step::RemoveCopy(_)]
        ));
        let remaining = b.list("blobs", None).unwrap().entries;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, path);
        assert_eq!(
            read_file(&b, &remaining[0]).unwrap(),
            Read::Bytes(b"good".to_vec())
        );
    }

    #[test]
    fn shared_replace_rechecks_hash_and_preserves_on_failed_rename() {
        for area in [Area::Books, Area::Blobs] {
            let memory = Memory::default();
            let mut a = memory.view("a");
            let mut b = memory.view("b");
            let naming = [17; 32];
            let name = match area {
                Area::Books => crate::sync::crypto::book_name(&naming, &identity_hash(b"good")),
                Area::Blobs => blob_name(&naming, &identity_hash(b"good")),
            };
            let dir = if area == Area::Books {
                "books"
            } else {
                "blobs"
            };
            let path = format!("{dir}/{name}");
            memory.conflict_copies(true);
            a.write(&path, b"bad").unwrap();
            b.write(&path, b"good").unwrap();
            memory.advance_instant();
            let copy = a
                .list(dir, None)
                .unwrap()
                .entries
                .into_iter()
                .find(|entry| !entry.ours)
                .unwrap();
            memory.half_synced("a", &copy.name, b"changed", 1);
            assert!(Shared { folder: &mut a }
                .replace(&name, &copy, &identity_hash(b"good"))
                .is_err());
            assert_eq!(
                read_file(&a, &entry(&a, &path)).unwrap(),
                Read::Bytes(b"bad".to_vec())
            );
            memory.fail_rename(&path);
            let taken = shared::tidy(&mut Shared { folder: &mut a }, area, &naming);
            assert!(taken[0].1.is_empty());
            assert_eq!(
                read_candidate(&a, &copy).unwrap(),
                Read::Bytes(b"good".to_vec())
            );
            shared::tidy(&mut Shared { folder: &mut a }, area, &naming);
            assert_eq!(
                read_file(&a, &entry(&a, &path)).unwrap(),
                Read::Bytes(b"good".to_vec())
            );
            assert_eq!(a.read(&copy.token).unwrap(), None);
        }
    }
}
