//! SY2, PS1–PS2: a brokered folder and deterministic cloud storage for tests.

use std::{fmt::Debug, hash::Hash, io};

/// SY2: one entry issued by a listing. `name` is relative to `Linen/`, including
/// `dir`, so SP7 can classify it without turning its opaque token into a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed<T> {
    pub name: String,
    pub token: T,
    /// SP2, SY2: lowercase permitted characters, and no hidden name. This does
    /// not assert that the name belongs to SP8's layout or has trusted content.
    pub ours: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page<T> {
    pub entries: Vec<Listed<T>>,
    pub next: Option<String>,
}

/// SP7, SY28: a bounded read never returns a truncated file as complete bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    Bytes(Vec<u8>),
    TooLarge,
    Gone,
}

/// SY2, PS1: all paths are relative to the selected `Linen/` folder, at most
/// four segments deep, without `..`; no implementation may follow symlinks.
pub trait Folder {
    type Token: Clone + Eq + Hash + Debug;

    /// SY2: list immediate files in `dir` (empty for the root), at most 1,000
    /// per page. Pass back `next` unchanged for that listing's next page. Names
    /// made by storage are exposed only with an issued token, never as writable
    /// paths. Visibility, order and duplicates are not promises of freshness.
    fn list(&self, dir: &str, cursor: Option<&str>) -> io::Result<Page<Self::Token>>;

    /// SY2, SP7: obtain a listed token's byte size before reading; `None` means
    /// it is gone. The size is not a guarantee against a subsequent change.
    fn stat(&self, token: &Self::Token) -> io::Result<Option<u64>>;

    /// SY2, SP7, SY28: read an issued token, including a storage-made name.
    /// A real folder must use a bounded read: stop at `limit` and report
    /// `TooLarge` if more data exists, without buffering the whole file first.
    /// Enforce the bound even if the file grows after `stat`; `None` leaves the
    /// read uncapped (books use the importer's limits). Missing files return
    /// `Gone`; unavailable storage reports an error. Bytes may be half-synced
    /// and must still pass the caller's hash/signature check (SP7).
    fn read(&self, token: &Self::Token, limit: Option<u64>) -> io::Result<Read>;

    /// SY2, SP7: accept a relative path, never an opaque token. Each segment
    /// uses `[A-Za-z0-9._-]`, with no hidden names. Stage bytes beside the target
    /// as `.linen-tmp-<16 lowercase hex>` and rename onto it atomically, or return
    /// an error with the target unchanged. A failed rename may leave the temp.
    /// SP2's narrower alphabet and SP8's limits are enforced by `write_file`.
    fn write(&mut self, path: &str, bytes: &[u8]) -> io::Result<()>;

    /// SY2: remove only an issued token (including a conflict copy). An already
    /// absent file succeeds; storage failures are errors.
    fn remove(&mut self, token: &Self::Token) -> io::Result<()>;
}

pub(crate) fn ours(path: &str) -> bool {
    valid_path(path)
        && path
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-/".contains(&b))
}

fn valid_path(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    parts.len() <= 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}

#[cfg(test)]
pub(crate) use memory::Memory;

#[cfg(test)]
mod memory {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::rc::Rc;

    /// PS2: opaque outside this module; retaining the store identity prevents a
    /// token from another Memory from accidentally addressing a same-named file.
    #[derive(Clone, Debug)]
    pub(crate) struct Token {
        origin: Rc<()>,
        path: String,
    }

    impl PartialEq for Token {
        fn eq(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.origin, &other.origin) && self.path == other.path
        }
    }
    impl Eq for Token {}
    impl Hash for Token {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            Rc::as_ptr(&self.origin).hash(state);
            self.path.hash(state);
        }
    }

    #[derive(Default)]
    struct Store {
        files: BTreeMap<String, Vec<u8>>,
        half: HashMap<(String, String), (usize, Vec<u8>)>,
        delays: HashMap<(String, String), usize>,
        hidden: HashMap<(String, String), Hidden>,
        ordering: HashMap<String, (bool, bool)>,
        reads: HashMap<(String, String), usize>,
        copied_bytes: HashMap<(String, String), u64>,
        conflicts: bool,
        writers: HashMap<String, String>,
        failed: HashSet<String>,
        serial: u64,
    }

    struct Hidden {
        previous: Option<Vec<u8>>,
        listings: usize,
    }

    /// PS2: one shared store; views have independent reader identities. The
    /// manually advanced instant avoids depending on wall-clock scheduling.
    #[derive(Clone, Default)]
    pub(crate) struct Memory {
        store: Rc<RefCell<Store>>,
        origin: Rc<()>,
    }

    struct Listing {
        id: u64,
        dir: String,
        entries: Vec<Listed<Token>>,
    }

    pub(crate) struct View {
        memory: Memory,
        reader: String,
        listing: RefCell<Option<Listing>>,
    }

    impl Memory {
        pub(crate) fn view(&self, reader: &str) -> View {
            View {
                memory: self.clone(),
                reader: reader.to_owned(),
                listing: RefCell::new(None),
            }
        }

        /// PS2: wrong bytes for this reader's next `n` reads of an existing path.
        pub(crate) fn half_synced(&self, reader: &str, path: &str, bytes: &[u8], n: usize) {
            self.store
                .borrow_mut()
                .half
                .insert((reader.to_owned(), path.to_owned()), (n, bytes.to_vec()));
        }

        /// PS2: different views writing one name in an instant leave copies.
        pub(crate) fn conflict_copies(&self, enabled: bool) {
            let mut store = self.store.borrow_mut();
            store.conflicts = enabled;
            store.writers.clear();
        }

        pub(crate) fn advance_instant(&self) {
            self.store.borrow_mut().writers.clear();
        }

        /// PS2: the next write stays at its previous value (or absent) for this
        /// reader's next `n` fresh listings of its directory. Continuation pages
        /// do not advance visibility; listing n+1 exposes the new value.
        pub(crate) fn delay_visibility(&self, reader: &str, path: &str, n: usize) {
            self.store
                .borrow_mut()
                .delays
                .insert((reader.to_owned(), path.to_owned()), n);
        }

        /// PS2: PROVISIONAL deterministic reordering (reverse) and repetition
        /// (each entry twice), applied before splitting the listing into pages.
        pub(crate) fn listing_visibility(&self, reader: &str, reverse: bool, repeat: bool) {
            self.store
                .borrow_mut()
                .ordering
                .insert(reader.to_owned(), (reverse, repeat));
        }

        pub(crate) fn vanish(&self, path: &str) {
            let mut store = self.store.borrow_mut();
            store.files.remove(path);
            store.hidden.retain(|(_, p), _| p != path);
        }

        pub(crate) fn fail_rename(&self, path: &str) {
            self.store.borrow_mut().failed.insert(path.to_owned());
        }

        /// PS2: calls to read, including files that are gone or too large.
        pub(crate) fn read_count(&self, reader: &str, path: &str) -> usize {
            self.store
                .borrow()
                .reads
                .get(&(reader.to_owned(), path.to_owned()))
                .copied()
                .unwrap_or(0)
        }

        /// SY28, PS2: bytes copied by read, excluding fixture setup and writes.
        pub(crate) fn bytes_read(&self, reader: &str, path: &str) -> u64 {
            self.store
                .borrow()
                .copied_bytes
                .get(&(reader.to_owned(), path.to_owned()))
                .copied()
                .unwrap_or(0)
        }
    }

    fn directory(path: &str) -> &str {
        path.rsplit_once('/').map_or("", |(dir, _)| dir)
    }

    impl View {
        fn check(&self, token: &Token) -> io::Result<()> {
            if Rc::ptr_eq(&self.memory.origin, &token.origin) {
                Ok(())
            } else {
                Err(io::Error::new(io::ErrorKind::InvalidInput, "foreign token"))
            }
        }

        fn visible<'a>(&self, store: &'a Store, path: &str) -> Option<&'a Vec<u8>> {
            match store.hidden.get(&(self.reader.clone(), path.to_owned())) {
                Some(hidden) => hidden.previous.as_ref(),
                None => store.files.get(path),
            }
        }
    }

    impl Folder for View {
        type Token = Token;

        fn list(&self, dir: &str, cursor: Option<&str>) -> io::Result<Page<Token>> {
            if !dir.is_empty() && (!valid_path(dir) || dir.split('/').count() >= 4) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid directory",
                ));
            }
            let mut listing = self.listing.borrow_mut();
            let offset = if let Some(cursor) = cursor {
                let parsed = cursor.split_once(':').and_then(|(id, offset)| {
                    Some((id.parse::<u64>().ok()?, offset.parse::<usize>().ok()?))
                });
                match (parsed, listing.as_ref()) {
                    (Some((id, offset)), Some(page))
                        if page.id == id && page.dir == dir && offset < page.entries.len() =>
                    {
                        offset
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "invalid cursor",
                        ))
                    }
                }
            } else {
                let mut store = self.memory.store.borrow_mut();
                store.hidden.retain(|(reader, path), hidden| {
                    if reader != &self.reader || directory(path) != dir {
                        return true;
                    }
                    if hidden.listings == 0 {
                        return false;
                    }
                    hidden.listings -= 1;
                    true
                });
                let mut entries: Vec<_> = store
                    .files
                    .keys()
                    .filter(|path| directory(path) == dir && self.visible(&store, path).is_some())
                    .map(|path| Listed {
                        name: path.clone(),
                        token: Token {
                            origin: self.memory.origin.clone(),
                            path: path.clone(),
                        },
                        ours: ours(path),
                    })
                    .collect();
                let (reverse, repeat) = store
                    .ordering
                    .get(&self.reader)
                    .copied()
                    .unwrap_or_default();
                if reverse {
                    entries.reverse();
                }
                if repeat {
                    entries = entries
                        .into_iter()
                        .flat_map(|entry| [entry.clone(), entry])
                        .collect();
                }
                store.serial += 1;
                *listing = Some(Listing {
                    id: store.serial,
                    dir: dir.to_owned(),
                    entries,
                });
                0
            };
            let listing = listing.as_ref().expect("a listing was started or checked");
            let end = (offset + 1_000).min(listing.entries.len());
            Ok(Page {
                entries: listing.entries[offset..end].to_vec(),
                next: (end < listing.entries.len()).then(|| format!("{}:{end}", listing.id)),
            })
        }

        fn stat(&self, token: &Token) -> io::Result<Option<u64>> {
            self.check(token)?;
            Ok(self
                .visible(&self.memory.store.borrow(), &token.path)
                .map(|b| b.len() as u64))
        }

        fn read(&self, token: &Token, limit: Option<u64>) -> io::Result<Read> {
            self.check(token)?;
            let mut store = self.memory.store.borrow_mut();
            let key = (self.reader.clone(), token.path.clone());
            *store.reads.entry(key.clone()).or_default() += 1;
            if self.visible(&store, &token.path).is_none() {
                return Ok(Read::Gone);
            }
            let half_synced = store.half.get_mut(&key).is_some_and(|(remaining, _)| {
                if *remaining > 0 {
                    *remaining -= 1;
                    true
                } else {
                    false
                }
            });
            let bytes = if half_synced {
                &store.half[&key].1
            } else {
                self.visible(&store, &token.path).unwrap()
            };
            // SY28: check the bytes that will actually be read, including a
            // half-synced replacement, before copying any of them.
            if limit.is_some_and(|limit| bytes.len() as u64 > limit) {
                return Ok(Read::TooLarge);
            }
            let bytes = bytes.clone();
            *store.copied_bytes.entry(key).or_default() += bytes.len() as u64;
            Ok(Read::Bytes(bytes))
        }

        fn write(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
            if !valid_path(path) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path"));
            }
            let mut store = self.memory.store.borrow_mut();
            // SP7: deterministic unique temporary names; only the suffix source
            // is test-specific. A failed rename leaves these exact staged bytes.
            store.serial += 1;
            let prefix = path
                .rsplit_once('/')
                .map_or(String::new(), |(dir, _)| format!("{dir}/"));
            let temp = format!("{prefix}.linen-tmp-{:016x}", store.serial);
            store.files.insert(temp.clone(), bytes.to_vec());
            if store.failed.remove(path) {
                return Err(io::Error::other("injected rename failure"));
            }
            let mut target = path.to_owned();
            if store.conflicts
                && store.files.contains_key(path)
                && store
                    .writers
                    .get(path)
                    .is_some_and(|writer| writer != &self.reader)
            {
                let leaf = path.rsplit('/').next().unwrap();
                let (stem, ext) = leaf
                    .rsplit_once('.')
                    .map_or((leaf, String::new()), |(s, e)| (s, format!(".{e}")));
                // PS2: PROVISIONAL iCloud-style numbering, preserving extension.
                let mut number = 2;
                loop {
                    target = format!("{prefix}{stem} {number}{ext}");
                    if !store.files.contains_key(&target) {
                        break;
                    }
                    number += 1;
                }
            }
            let delays: Vec<_> = store
                .delays
                .keys()
                .filter(|(_, p)| p == &target)
                .cloned()
                .collect();
            for key in delays {
                let n = store.delays.remove(&key).unwrap();
                if n > 0 {
                    let previous = store
                        .hidden
                        .get(&key)
                        .map_or_else(|| store.files.get(&target).cloned(), |h| h.previous.clone());
                    store.hidden.insert(
                        key,
                        Hidden {
                            previous,
                            listings: n,
                        },
                    );
                }
            }
            let staged = store.files.remove(&temp).unwrap();
            // PS2: the writer is recorded against the path actually written: a
            // conflict copy's writer doesn't become the original's owner.
            store.writers.insert(target.clone(), self.reader.clone());
            store.files.insert(target, staged);
            Ok(())
        }

        fn remove(&mut self, token: &Token) -> io::Result<()> {
            self.check(token)?;
            self.memory.vanish(&token.path);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn memory_read_bounds_growth_after_stat_before_copying() {
        let memory = Memory::default();
        let mut writer = memory.view("writer");
        let reader = memory.view("reader");
        let path = "books/object";
        writer.write(path, b"old").unwrap();
        let token = reader.list("books", None).unwrap().entries.remove(0).token;
        let limit = reader.stat(&token).unwrap();
        assert_eq!(limit, Some(3));

        // SY28: an actual stored file grows after the reader checks its size.
        writer.write(path, b"grown").unwrap();
        assert_eq!(reader.read(&token, limit).unwrap(), Read::TooLarge);
        assert_eq!(memory.read_count("reader", path), 1);
        assert_eq!(memory.bytes_read("reader", path), 0);
        assert_eq!(
            reader.read(&token, Some(5)).unwrap(),
            Read::Bytes(b"grown".to_vec())
        );
        assert_eq!(memory.bytes_read("reader", path), 5);
        assert_eq!(
            reader.read(&token, None).unwrap(),
            Read::Bytes(b"grown".to_vec())
        );
        assert_eq!(memory.bytes_read("reader", path), 10);
        assert_eq!(reader.read(&token, Some(0)).unwrap(), Read::TooLarge);
        assert_eq!(memory.bytes_read("reader", path), 10);
        writer.write(path, b"").unwrap();
        assert_eq!(reader.read(&token, Some(0)).unwrap(), Read::Bytes(vec![]));
        memory.vanish(path);
        assert_eq!(reader.read(&token, Some(0)).unwrap(), Read::Gone);
        assert_eq!(memory.bytes_read("reader", path), 10);
    }

    #[test]
    fn memory_pages_cover_every_entry_once() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        for n in 0..2_001 {
            view.write(&format!("devices/{n}.json"), b"x").unwrap();
        }
        let mut cursor = None;
        let mut seen = HashSet::new();
        let mut sizes = Vec::new();
        loop {
            let page = view.list("devices", cursor.as_deref()).unwrap();
            sizes.push(page.entries.len());
            for entry in page.entries {
                assert!(seen.insert(entry.name));
            }
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(sizes, [1_000, 1_000, 1]);
        assert_eq!(seen.len(), 2_001);
        assert!(view.list("devices", Some("bad cursor")).is_err());
    }

    #[test]
    fn memory_half_synced_reads_are_per_reader_and_counted() {
        let memory = Memory::default();
        let mut a = memory.view("a");
        let b = memory.view("b");
        a.write("books/object", b"good").unwrap();
        let token = a.list("books", None).unwrap().entries.remove(0).token;
        memory.half_synced("a", "books/object", b"bad", 2);
        assert_eq!(a.read(&token, None).unwrap(), Read::Bytes(b"bad".to_vec()));
        assert_eq!(b.read(&token, None).unwrap(), Read::Bytes(b"good".to_vec()));
        assert_eq!(a.read(&token, None).unwrap(), Read::Bytes(b"bad".to_vec()));
        assert_eq!(a.read(&token, None).unwrap(), Read::Bytes(b"good".to_vec()));
        assert_eq!(memory.read_count("a", "books/object"), 3);
    }

    #[test]
    fn a_second_conflicting_write_never_overwrites_the_original() {
        // PS2: B's first write over A's file becomes a copy; B doesn't become
        // the original's owner, so B's second write is a copy too.
        let memory = Memory::default();
        let mut a = memory.view("a");
        let mut b = memory.view("b");
        memory.conflict_copies(true);
        a.write("books/object", b"from a").unwrap();
        b.write("books/object", b"from b, first").unwrap();
        b.write("books/object", b"from b, second").unwrap();
        let entries = a.list("books", None).unwrap().entries;
        let original = entries
            .iter()
            .find(|entry| entry.name == "books/object")
            .unwrap();
        assert_eq!(
            a.read(&original.token, None).unwrap(),
            Read::Bytes(b"from a".to_vec())
        );
    }

    #[test]
    fn memory_conflicts_require_different_views_in_one_instant() {
        for (path, copy) in [
            ("books/object", "books/object 2"),
            ("devices/state.json", "devices/state 2.json"),
        ] {
            let memory = Memory::default();
            let mut a = memory.view("a");
            let mut b = memory.view("b");
            memory.conflict_copies(true);
            a.write(path, b"first").unwrap();
            a.write(path, b"same writer").unwrap();
            b.write(path, b"second").unwrap();
            let dir = path.split('/').next().unwrap();
            let entries = a.list(dir, None).unwrap().entries;
            assert_eq!(entries.len(), 2);
            let original = entries.iter().find(|entry| entry.name == path).unwrap();
            let conflict = entries.iter().find(|entry| entry.name == copy).unwrap();
            assert!(original.ours);
            assert!(!conflict.ours);
            assert_eq!(
                a.read(&original.token, None).unwrap(),
                Read::Bytes(b"same writer".to_vec())
            );
            assert_eq!(
                b.read(&conflict.token, None).unwrap(),
                Read::Bytes(b"second".to_vec())
            );
            assert!(b.write(copy, b"not writable").is_err());
            memory.advance_instant();
            a.write(path, b"later").unwrap();
            assert_eq!(a.list(dir, None).unwrap().entries.len(), 2);
            assert_eq!(
                b.read(&original.token, None).unwrap(),
                Read::Bytes(b"later".to_vec())
            );
            b.remove(&conflict.token).unwrap();
            assert_eq!(a.list(dir, None).unwrap().entries.len(), 1);
        }
    }

    #[test]
    fn memory_delays_new_and_replaced_files_for_one_reader() {
        for existing in [false, true] {
            let memory = Memory::default();
            let mut a = memory.view("a");
            let b = memory.view("b");
            if existing {
                a.write("books/object", b"old").unwrap();
            }
            memory.delay_visibility("b", "books/object", 2);
            a.write("books/object", b"new").unwrap();
            let token = a.list("books", None).unwrap().entries.remove(0).token;
            for _ in 0..2 {
                assert_eq!(a.read(&token, None).unwrap(), Read::Bytes(b"new".to_vec()));
                let page = b.list("books", None).unwrap();
                assert_eq!(page.entries.len(), usize::from(existing));
                assert_eq!(
                    b.read(&token, None).unwrap(),
                    if existing {
                        Read::Bytes(b"old".to_vec())
                    } else {
                        Read::Gone
                    }
                );
                assert_eq!(b.stat(&token).unwrap(), existing.then_some(3));
            }
            assert_eq!(b.list("books", None).unwrap().entries.len(), 1);
            assert_eq!(b.read(&token, None).unwrap(), Read::Bytes(b"new".to_vec()));
        }
    }

    #[test]
    fn memory_reorders_and_repeats_only_the_selected_reader() {
        let memory = Memory::default();
        let mut a = memory.view("a");
        let b = memory.view("b");
        for path in ["books/a", "books/b", "books/c"] {
            a.write(path, b"x").unwrap();
        }
        memory.listing_visibility("b", true, true);
        let names = |entries: Vec<Listed<_>>| {
            entries
                .into_iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(a.list("books", None).unwrap().entries),
            ["books/a", "books/b", "books/c"]
        );
        assert_eq!(
            names(b.list("books", None).unwrap().entries),
            ["books/c", "books/c", "books/b", "books/b", "books/a", "books/a"]
        );
    }

    #[test]
    fn memory_vanish_invalidates_reads_and_stat_across_views() {
        let memory = Memory::default();
        let mut a = memory.view("a");
        let b = memory.view("b");
        a.write("books/object", b"x").unwrap();
        let token = b.list("books", None).unwrap().entries.remove(0).token;
        memory.vanish("books/object");
        assert_eq!(a.stat(&token).unwrap(), None);
        assert_eq!(b.read(&token, None).unwrap(), Read::Gone);
        assert!(a.list("books", None).unwrap().entries.is_empty());
        a.remove(&token).unwrap();
    }

    #[test]
    fn memory_rejects_escaping_paths_and_foreign_tokens() {
        let memory = Memory::default();
        let mut view = memory.view("a");
        for path in [
            "",
            "/books/a",
            "books/../a",
            "a/b/c/d/e",
            "books/.hidden",
            "books/a b",
            "books//a",
            "books/a\\b",
        ] {
            assert!(view.write(path, b"x").is_err(), "{path}");
        }
        view.write("books/object", b"x").unwrap();
        let token = view.list("books", None).unwrap().entries.remove(0).token;
        let mut other = Memory::default().view("a");
        assert!(other.read(&token, None).is_err());
        assert!(other.stat(&token).is_err());
        assert!(other.remove(&token).is_err());
    }
}
