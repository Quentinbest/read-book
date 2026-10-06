//! Sync and the library store (next-steps plan, Phase 13). `sync.rs` decides; this
//! module turns local writes into log records and applies other devices' records to
//! this library. Applying a remote record never writes to this device's log, so two
//! devices with the same book open don't bounce a position between them.
//!
//! Sync is on while the `sync.folder` setting names a folder. Removing a book is
//! never logged: removing it here leaves its annotations on the other devices.

use crate::store::{now_ms, AnnotationRow, Result, Store};
use crate::sync::{
    self, Absorbed, Adapter, Annotation, Body, BookKey, Change, Clock, Hlc, Line, Position,
};
use rusqlite::{params, OptionalExtension};

const FOLDER: &str = "sync.folder";
const LIBRARY_ID: &str = "sync.libraryId";
const CLOCK: &str = "sync.clock";

/// The folder sync uses, if sync is on.
pub fn folder(store: &Store) -> Result<Option<String>> {
    Ok(store.setting(FOLDER)?.filter(|f| !f.is_empty()))
}

/// Turn sync on with this folder (or off with None). Turning it on logs this
/// library's current positions and annotations, so another device can catch up.
pub fn set_folder(store: &mut Store, folder: Option<&str>) -> Result<()> {
    store.set_setting(FOLDER, folder.unwrap_or(""))?;
    if folder.is_some() {
        log_everything(store)?;
    }
    Ok(())
}

/// This device's ID: the machine's ID and this library's random ID, hashed. The
/// library ID lives in the library; the machine ID doesn't, so a library restored
/// on another Mac gets a new device ID (see `sync::device_id`).
pub fn device(store: &mut Store) -> Result<String> {
    let library = match store.setting(LIBRARY_ID)? {
        Some(id) => id,
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            store.set_setting(LIBRARY_ID, &id)?;
            id
        }
    };
    // Without a machine ID (not macOS), the library ID alone: a restored copy on
    // another machine then shares the ID, a known limit outside macOS.
    let machine = sync::machine_id().unwrap_or_default();
    Ok(sync::device_id(&machine, &library))
}

fn clock(store: &mut Store) -> Result<Clock> {
    let mut clock = Clock::new(device(store)?);
    if let Some((ms, n)) = store
        .setting(CLOCK)?
        .and_then(|s| serde_json::from_str::<(u64, u32)>(&s).ok())
    {
        clock.observe(
            &Hlc {
                ms,
                n,
                device: String::new(),
            },
            u64::MAX / 2,
        );
    }
    Ok(clock)
}

fn save_clock(store: &mut Store, clock: &mut Clock) -> Result<()> {
    // Tick once at time zero to read where the clock stands without moving wall time.
    let h = clock.tick(0);
    store.set_setting(CLOCK, &serde_json::to_string(&(h.ms, h.n)).unwrap())
}

fn book_key(store: &Store, book_id: &str) -> Result<Option<BookKey>> {
    Ok(store
        .conn()
        .query_row(
            "SELECT package_identifier, content_hash FROM books WHERE id = ?1",
            [book_id],
            |r| {
                Ok(BookKey::of(
                    r.get::<_, Option<String>>(0)?.as_deref(),
                    &r.get::<_, String>(1)?,
                ))
            },
        )
        .optional()?)
}

fn current(store: &Store, key: &str) -> Result<Option<Change>> {
    let json: Option<String> = store
        .conn()
        .query_row(
            "SELECT change FROM sync_records WHERE key = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
}

fn put(store: &Store, change: &Change, applied: bool) -> Result<()> {
    let book = match &change.body {
        Body::Position(p) => &p.book,
        Body::Annotation(a) => &a.book,
    };
    store.conn().execute(
        "INSERT INTO sync_records (key, book_key, change, applied) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET book_key = excluded.book_key,
             change = excluded.change, applied = excluded.applied",
        params![
            change.key(),
            book.0,
            serde_json::to_string(change).unwrap(),
            applied
        ],
    )?;
    Ok(())
}

fn outbox(store: &Store, change: &Change) -> Result<()> {
    store.conn().execute(
        "INSERT INTO sync_outbox (line) VALUES (?1)",
        [serde_json::to_string(change).unwrap()],
    )?;
    Ok(())
}

/// Record a local change: the next version of a record, made from the current one.
fn local(store: &mut Store, body: Body) -> Result<()> {
    let mut clock = clock(store)?;
    let mut change = Change {
        v: sync::FORMAT,
        hlc: clock.tick(now_ms() as u64),
        prev: None,
        body,
    };
    change.prev = current(store, &change.key())?.map(|c| c.hlc);
    put(store, &change, true)?;
    outbox(store, &change)?;
    save_clock(store, &mut clock)
}

/// After a local position save. Does nothing while sync is off.
pub fn position_saved(store: &mut Store, book_id: &str) -> Result<()> {
    if folder(store)?.is_none() {
        return Ok(());
    }
    let Some(book) = book_key(store, book_id)? else {
        return Ok(());
    };
    let Some((cfi, fraction)) = store.position(book_id)? else {
        return Ok(());
    };
    local(
        store,
        Body::Position(Position {
            book,
            cfi,
            fraction,
        }),
    )
}

/// After a local annotation save, delete or restore. Does nothing while sync is off.
pub fn annotation_changed(store: &mut Store, id: &str) -> Result<()> {
    if folder(store)?.is_none() {
        return Ok(());
    }
    let row = store
        .conn()
        .query_row(
            "SELECT book_id, anchored_content_hash, color, cfi_range, quote_exact, quote_prefix,
                    quote_suffix, note, created_at, deleted_at IS NOT NULL
             FROM annotations WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Annotation {
                        id: id.to_string(),
                        book: BookKey(String::new()),
                        anchored_content_hash: r.get(1)?,
                        color: r.get(2)?,
                        cfi_range: r.get(3)?,
                        quote_exact: r.get(4)?,
                        quote_prefix: r.get(5)?,
                        quote_suffix: r.get(6)?,
                        note: r.get(7)?,
                        created_at: r.get(8)?,
                        deleted: r.get(9)?,
                    },
                ))
            },
        )
        .optional()?;
    let Some((book_id, mut a)) = row else {
        return Ok(());
    };
    let Some(book) = book_key(store, &book_id)? else {
        return Ok(());
    };
    a.book = book;
    local(store, Body::Annotation(a))
}

/// Log this library's current state (sync just turned on).
fn log_everything(store: &mut Store) -> Result<()> {
    let books: Vec<String> = {
        let mut stmt = store.conn().prepare("SELECT book_id FROM positions")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for b in books {
        position_saved(store, &b)?;
    }
    let ids: Vec<String> = {
        let mut stmt = store.conn().prepare("SELECT id FROM annotations")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for id in ids {
        annotation_changed(store, &id)?;
    }
    Ok(())
}

/// The local book with this identity, if this library has it.
fn local_book(store: &Store, key: &BookKey) -> Result<Option<String>> {
    let (by, value) = key.0.split_once(':').unwrap_or(("", ""));
    let sql = match by {
        "id" => "SELECT id FROM books WHERE trim(package_identifier) = ?1 AND removed_at IS NULL",
        "hash" => "SELECT id FROM books WHERE content_hash = ?1 AND removed_at IS NULL",
        _ => return Ok(None),
    };
    Ok(store
        .conn()
        .query_row(&format!("{sql} ORDER BY added_at LIMIT 1"), [value], |r| {
            r.get(0)
        })
        .optional()?)
}

/// Write a remote record into this library's tables. Returns false when the book
/// isn't here; the record then waits in `sync_records` until it is imported.
fn apply(store: &mut Store, change: &Change) -> Result<bool> {
    match &change.body {
        Body::Position(p) => {
            let Some(book_id) = local_book(store, &p.book)? else {
                return Ok(false);
            };
            store.conn().execute(
                "INSERT INTO positions (book_id, cfi, fraction, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(book_id) DO UPDATE SET cfi = excluded.cfi,
                     fraction = excluded.fraction, updated_at = excluded.updated_at",
                params![book_id, p.cfi, p.fraction, change.hlc.ms as i64],
            )?;
        }
        Body::Annotation(a) => {
            let Some(book_id) = local_book(store, &a.book)? else {
                return Ok(false);
            };
            store.save_annotation(&AnnotationRow {
                id: a.id.clone(),
                book_id,
                anchored_content_hash: a.anchored_content_hash.clone(),
                color: a.color.clone(),
                cfi_range: a.cfi_range.clone(),
                quote_exact: a.quote_exact.clone(),
                quote_prefix: a.quote_prefix.clone(),
                quote_suffix: a.quote_suffix.clone(),
                note: a.note.clone(),
                created_at: a.created_at,
                updated_at: change.hlc.ms as i64,
                // Anchored again on this device's copy of the book when it opens (A9).
                anchor_status: "reanchor".into(),
            })?;
            if a.deleted {
                store.conn().execute(
                    "UPDATE annotations SET deleted_at = ?2 WHERE id = ?1",
                    params![a.id, change.hlc.ms as i64],
                )?;
            }
        }
    }
    Ok(true)
}

/// What one sync did, for Settings and the tests.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub written: usize,
    pub applied: usize,
    pub waiting: usize,
    pub set_aside: usize,
}

/// Write this device's waiting changes, then read and apply every device's logs.
pub fn run(store: &mut Store, adapter: &dyn Adapter) -> std::io::Result<Report> {
    let io = |e: crate::store::StoreError| std::io::Error::other(e.to_string());
    let device = device(store).map_err(io)?;
    let mut report = Report::default();

    // 1. Write: the outbox goes to this device's log, then is cleared.
    let waiting: Vec<(i64, String)> = {
        let conn = store.conn();
        let mut stmt = conn
            .prepare("SELECT seq, line FROM sync_outbox ORDER BY seq")
            .map_err(|e| io(e.into()))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| io(e.into()))?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(|e| io(e.into()))?
    };
    let lines: Vec<String> = waiting.iter().map(|(_, l)| l.clone()).collect();
    adapter.append(&device, &lines)?;
    if let Some((last, _)) = waiting.last() {
        store
            .conn()
            .execute("DELETE FROM sync_outbox WHERE seq <= ?1", [last])
            .map_err(|e| io(e.into()))?;
    }
    report.written = lines.len();

    // 2. Read: merge every log line into the current records.
    let lines: Vec<Line> = adapter
        .read_all()?
        .into_iter()
        .filter_map(|(_, l)| sync::parse_line(&l))
        .collect();
    let mut clock = clock(store).map_err(io)?;
    let mut replica = sync::Replica::default();
    {
        let conn = store.conn();
        let mut stmt = conn
            .prepare("SELECT change FROM sync_records")
            .map_err(|e| io(e.into()))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| io(e.into()))?;
        for json in rows {
            let json = json.map_err(|e| io(e.into()))?;
            if let Ok(c) = serde_json::from_str::<Change>(&json) {
                replica.records.insert(c.key(), c);
            }
        }
    }
    let before = replica.records.clone();
    let Absorbed {
        from_the_future,
        copies,
    } = replica.absorb(&mut clock, now_ms() as u64, &lines);
    for raw in &replica.unknown {
        store
            .conn()
            .execute(
                "INSERT OR IGNORE INTO sync_aside (line, reason) VALUES (?1, 'format')",
                [raw],
            )
            .map_err(|e| io(e.into()))?;
    }
    report.set_aside = replica.unknown.len() + from_the_future;

    // 3. Apply what changed, without logging it, except conflict copies, which
    //    this device logs (it made them).
    let tx_store = store;
    for (key, change) in &replica.records {
        if before.get(key) == Some(change) {
            continue;
        }
        let applied = apply(tx_store, change).map_err(io)?;
        put(tx_store, change, applied).map_err(io)?;
        if applied {
            report.applied += 1;
        } else {
            report.waiting += 1;
        }
    }
    for copy in &copies {
        outbox(tx_store, copy).map_err(io)?;
    }
    save_clock(tx_store, &mut clock).map_err(io)?;
    Ok(report)
}

/// After an import: apply records that were waiting for this book.
pub fn book_imported(store: &mut Store, book_id: &str) -> Result<usize> {
    let Some(key) = book_key(store, book_id)? else {
        return Ok(0);
    };
    let waiting: Vec<String> = {
        let mut stmt = store
            .conn()
            .prepare("SELECT change FROM sync_records WHERE book_key = ?1 AND applied = 0")?;
        let rows = stmt.query_map([&key.0], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut n = 0;
    for json in waiting {
        if let Ok(change) = serde_json::from_str::<Change>(&json) {
            if apply(store, &change)? {
                put(store, &change, true)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::FolderAdapter;

    fn library(pkg: &str) -> Store {
        let mut s = Store::open_in_memory().unwrap();
        add_book(&mut s, "local-book", pkg);
        s
    }

    fn add_book(s: &mut Store, id: &str, pkg: &str) {
        s.conn()
            .execute(
                "INSERT INTO books (id, content_hash, package_identifier, file_path, title,
                     title_source, added_at)
                 VALUES (?1, 'hash', ?2, 'f.epub', 'Moby-Dick', 'package', 1)",
                params![id, pkg],
            )
            .unwrap();
    }

    fn highlight(id: &str, book_id: &str, note: Option<&str>) -> AnnotationRow {
        AnnotationRow {
            id: id.into(),
            book_id: book_id.into(),
            anchored_content_hash: "hash".into(),
            color: "yellow".into(),
            cfi_range: "epubcfi(/6/4!/4/2,/1:0,/1:5)".into(),
            quote_exact: "Call me".into(),
            quote_prefix: String::new(),
            quote_suffix: String::new(),
            note: note.map(str::to_string),
            created_at: 1,
            updated_at: 1,
            anchor_status: "anchored".into(),
        }
    }

    /// Two libraries (two Macs) share one folder; each has its own device ID,
    /// because each library has its own library ID.
    fn pair() -> (tempfile::TempDir, FolderAdapter, Store, Store) {
        let dir = tempfile::tempdir().unwrap();
        let folder = FolderAdapter::new(dir.path());
        let mut a = library("urn:moby");
        // The same book, imported separately: a different local ID.
        let mut b = Store::open_in_memory().unwrap();
        add_book(&mut b, "other-local-id", "urn:moby");
        let path = dir.path().to_string_lossy().into_owned();
        set_folder(&mut a, Some(&path)).unwrap();
        set_folder(&mut b, Some(&path)).unwrap();
        (dir, folder, a, b)
    }

    #[test]
    fn nothing_is_logged_while_sync_is_off() {
        let mut s = library("urn:moby");
        s.save_position("local-book", "epubcfi(/6/8)", 0.3, None)
            .unwrap();
        position_saved(&mut s, "local-book").unwrap();
        let n: i64 = s
            .conn()
            .query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_position_and_a_highlight_reach_the_other_mac() {
        let (_dir, folder, mut a, mut b) = pair();
        a.save_position("local-book", "epubcfi(/6/8)", 0.3, None)
            .unwrap();
        position_saved(&mut a, "local-book").unwrap();
        a.save_annotation(&highlight("h1", "local-book", Some("whales")))
            .unwrap();
        annotation_changed(&mut a, "h1").unwrap();
        assert_eq!(run(&mut a, &folder).unwrap().written, 2);
        let report = run(&mut b, &folder).unwrap();
        assert_eq!(report.applied, 2);
        assert_eq!(
            b.position("other-local-id").unwrap(),
            Some(("epubcfi(/6/8)".into(), 0.3))
        );
        let notes = b.annotations("other-local-id").unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].note.as_deref(), Some("whales"));
        // Applying it wrote nothing to b's own log: no ping-pong.
        assert_eq!(run(&mut b, &folder).unwrap().written, 0);
        assert_eq!(run(&mut a, &folder).unwrap().applied, 0);
    }

    #[test]
    fn a_deletion_travels_and_an_undo_brings_it_back() {
        let (_dir, folder, mut a, mut b) = pair();
        a.save_annotation(&highlight("h1", "local-book", None))
            .unwrap();
        annotation_changed(&mut a, "h1").unwrap();
        run(&mut a, &folder).unwrap();
        run(&mut b, &folder).unwrap();
        b.delete_annotation("h1").unwrap();
        annotation_changed(&mut b, "h1").unwrap();
        run(&mut b, &folder).unwrap();
        run(&mut a, &folder).unwrap();
        assert!(a.annotations("local-book").unwrap().is_empty());
        a.restore_annotation("h1").unwrap();
        annotation_changed(&mut a, "h1").unwrap();
        run(&mut a, &folder).unwrap();
        run(&mut b, &folder).unwrap();
        assert_eq!(b.annotations("other-local-id").unwrap().len(), 1);
    }

    #[test]
    fn a_record_for_a_missing_book_waits_for_its_import() {
        let dir = tempfile::tempdir().unwrap();
        let folder = FolderAdapter::new(dir.path());
        let path = dir.path().to_string_lossy().into_owned();
        let mut a = library("urn:moby");
        set_folder(&mut a, Some(&path)).unwrap();
        a.save_annotation(&highlight("h1", "local-book", None))
            .unwrap();
        annotation_changed(&mut a, "h1").unwrap();
        run(&mut a, &folder).unwrap();

        let mut b = Store::open_in_memory().unwrap();
        set_folder(&mut b, Some(&path)).unwrap();
        assert_eq!(run(&mut b, &folder).unwrap().waiting, 1);
        add_book(&mut b, "imported-later", "urn:moby");
        assert_eq!(book_imported(&mut b, "imported-later").unwrap(), 1);
        assert_eq!(b.annotations("imported-later").unwrap().len(), 1);
    }

    #[test]
    fn concurrent_note_edits_keep_both_texts_on_both_macs() {
        let (_dir, folder, mut a, mut b) = pair();
        a.save_annotation(&highlight("h1", "local-book", Some("first")))
            .unwrap();
        annotation_changed(&mut a, "h1").unwrap();
        run(&mut a, &folder).unwrap();
        run(&mut b, &folder).unwrap();
        // Both edit the note before syncing again.
        a.save_annotation(&highlight("h1", "local-book", Some("from a")))
            .unwrap();
        annotation_changed(&mut a, "h1").unwrap();
        b.save_annotation(&highlight("h1", "other-local-id", Some("from b")))
            .unwrap();
        annotation_changed(&mut b, "h1").unwrap();
        for _ in 0..3 {
            run(&mut a, &folder).unwrap();
            run(&mut b, &folder).unwrap();
        }
        let texts = |s: &Store, book: &str| {
            let mut t: Vec<String> = s
                .annotations(book)
                .unwrap()
                .into_iter()
                .filter_map(|n| n.note)
                .collect();
            t.sort();
            t
        };
        assert_eq!(texts(&a, "local-book"), ["from a", "from b"]);
        assert_eq!(texts(&b, "other-local-id"), ["from a", "from b"]);
    }

    #[test]
    fn records_from_a_newer_format_are_set_aside_not_dropped() {
        let (dir, folder, mut a, _b) = pair();
        let raw = r#"{"v":9,"hlc":{"ms":1,"n":0,"device":"z"},"body":{"kind":"bookmark"}}"#;
        std::fs::create_dir_all(dir.path().join("Linen Sync/v1")).unwrap();
        std::fs::write(dir.path().join("Linen Sync/v1/z.jsonl"), format!("{raw}\n")).unwrap();
        assert_eq!(run(&mut a, &folder).unwrap().set_aside, 1);
        let kept: String = a
            .conn()
            .query_row("SELECT line FROM sync_aside", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, raw);
    }
}
