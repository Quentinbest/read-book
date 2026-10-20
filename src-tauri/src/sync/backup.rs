//! SY27 (item 106): a consistent copy of the library before every risky moment.
//!
//! Before the first sync and before a pass that applies a remote deletion, the
//! library folder gets `Backups/‹date-time›/`: the database, copied with SQLite's
//! online backup API (so rows still in the WAL are in it), and clones of `Books/`
//! (APFS clones, which take no space until files change). The copy is marked as a
//! backup: a library restored from it starts with sync paused and applies nothing
//! from storage until the reader resumes (Resume's two choices come with Restore).
//! A pass within the hour after a backup relies on it only if nothing the pass
//! could remove or replace changed since it was taken. Book files (Stage 4) aren't
//! replaced or removed by sync yet, so `Books/` is cloned whole.

use super::journal::{JournalError, Library};
use super::record::Key;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, BackupError>;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("backup manifest: {0}")]
    Manifest(#[from] serde_json::Error),
}

/// SY27 (prov., item 106): how many backups are kept, and for how long, besides the
/// newest (whatever its age) and the one SY26 keeps while the reader is asked.
/// PROVISIONAL.
pub const KEEP: usize = 10;
pub const KEEP_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// At most one backup an hour, unless the newest doesn't cover a pass.
pub const HOUR_MS: i64 = 60 * 60 * 1000;

const MANIFEST: &str = "backup.json";

/// What a backup holds, written beside its database.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// When it was taken (this device's clock).
    pub taken_at: i64,
    /// Why: `first sync` or `remote deletion`.
    pub reason: String,
    /// The journal's last row at the time: a record changed since has a later one.
    pub journal_row: i64,
}

/// One backup in the library folder.
#[derive(Debug, Clone)]
pub struct Backup {
    pub path: PathBuf,
    pub manifest: Manifest,
}

/// The library folder's backups, oldest first.
pub fn list(root: &Path) -> Result<Vec<Backup>> {
    let dir = root.join("Backups");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries {
        let path = entry?.path();
        let Ok(text) = std::fs::read_to_string(path.join(MANIFEST)) else {
            continue; // not a finished backup
        };
        let manifest: Manifest = serde_json::from_str(&text)?;
        out.push(Backup { path, manifest });
    }
    out.sort_by_key(|b| b.manifest.taken_at);
    Ok(out)
}

fn journal_row(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(rowid), 0) FROM sync_versions",
        [],
        |r| r.get(0),
    )?)
}

/// SY27: takes a backup of the library at `root` (its database `conn` and `Books/`).
/// The caller holds the store, so nothing writes meanwhile.
pub fn take(conn: &Connection, root: &Path, reason: &str, now_ms: i64) -> Result<Backup> {
    let name = super::a9::iso_time(now_ms).replace(':', "-");
    let path = root.join("Backups").join(&name);
    std::fs::create_dir_all(&path)?;
    // The online backup API copies the database as one consistent snapshot, rows
    // still in the WAL included.
    conn.backup(rusqlite::MAIN_DB, path.join("linen.db"), None)?;
    // Marked as a backup: restored, the library starts paused and applies nothing.
    {
        let copy = Connection::open(path.join("linen.db"))?;
        copy.execute(
            "UPDATE sync_libraries SET state = 'paused', restored_from = ?1",
            [now_ms],
        )?;
    }
    let books = root.join("Books");
    if books.is_dir() {
        let into = path.join("Books");
        std::fs::create_dir_all(&into)?;
        for entry in std::fs::read_dir(&books)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                // APFS: `fs::copy` clones (fclonefileat), so this takes no space
                // until a file changes.
                std::fs::copy(entry.path(), into.join(entry.file_name()))?;
            }
        }
    }
    let manifest = Manifest {
        taken_at: now_ms,
        reason: reason.to_owned(),
        journal_row: journal_row(conn)?,
    };
    // Written last: a backup without it was interrupted and isn't one.
    std::fs::write(path.join(MANIFEST), serde_json::to_vec_pretty(&manifest)?)?;
    Ok(Backup { path, manifest })
}

/// Whether the newest backup covers a pass that could remove or replace `keys`:
/// taken within the hour, with none of them changed since.
pub fn covers(
    conn: &Connection,
    root: &Path,
    lib: &Library,
    keys: &[Key],
    now_ms: i64,
) -> Result<bool> {
    let Some(newest) = list(root)?.pop() else {
        return Ok(false);
    };
    if now_ms - newest.manifest.taken_at >= HOUR_MS {
        return Ok(false);
    }
    for key in keys {
        let changed: bool = conn
            .query_row(
                "SELECT 1 FROM sync_versions WHERE library = ?1 AND key = ?2 AND rowid > ?3",
                params![
                    lib.id.to_string(),
                    key.as_str(),
                    newest.manifest.journal_row
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if changed {
            return Ok(false);
        }
    }
    Ok(true)
}

/// SY27: before a sync pass, the backup it needs, if any. A first sync (nothing
/// received yet from storage) always gets one; a pass that would apply a remote
/// deletion gets one unless the newest covers it. `deletes` are the keys the pass's
/// changes delete. Old backups are pruned after a new one is taken.
pub fn before_pass(
    conn: &Connection,
    root: &Path,
    lib: &Library,
    deletes: &[Key],
    now_ms: i64,
) -> Result<Option<Backup>> {
    let first: bool = conn
        .query_row(
            "SELECT 1 FROM sync_seen WHERE library = ?1",
            [lib.id.to_string()],
            |_| Ok(()),
        )
        .optional()?
        .is_none();
    let reason = if first {
        "first sync"
    } else if !deletes.is_empty() && !covers(conn, root, lib, deletes, now_ms)? {
        "remote deletion"
    } else {
        return Ok(None);
    };
    let backup = take(conn, root, reason, now_ms)?;
    prune(conn, root, lib, now_ms)?;
    Ok(Some(backup))
}

/// The keys a change's versions delete (tombstones): what `before_pass` asks about.
pub fn deletions(batch: &super::apply::Batch) -> Vec<Key> {
    batch
        .versions
        .iter()
        .filter(|v| v.get("deleted").is_some())
        .filter_map(|v| v["key"].as_str().and_then(Key::parse).map(|(k, _)| k))
        .collect()
}

/// SY27: keeps up to `KEEP` backups from the last `KEEP_MS`, the newest whatever
/// its age, and the one SY26 keeps while the reader is asked (the newest from
/// before the first change it counted); removes the rest.
pub fn prune(conn: &Connection, root: &Path, lib: &Library, now_ms: i64) -> Result<()> {
    let backups = list(root)?;
    let Some(newest) = backups.last().map(|b| b.path.clone()) else {
        return Ok(());
    };
    let first_counted: Option<i64> = conn
        .query_row(
            "SELECT MIN(c.at) FROM sync_counted c JOIN sync_question q ON q.library = c.library
             WHERE c.library = ?1 AND q.asking = 1 AND c.at > COALESCE(q.answered_at, 0)
               AND c.approved = 0",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let kept_for_question = first_counted.and_then(|first| {
        backups
            .iter()
            .rev()
            .find(|b| b.manifest.taken_at <= first)
            .map(|b| b.path.clone())
    });
    // Up to `KEEP` from the last week, newest first (the newest among them).
    let mut recent = 0;
    for b in backups.iter().rev() {
        let within = now_ms - b.manifest.taken_at < KEEP_MS && {
            recent += 1;
            recent <= KEEP
        };
        let keep = within || b.path == newest || Some(&b.path) == kept_for_question.as_ref();
        if !keep {
            std::fs::remove_dir_all(&b.path)?;
        }
    }
    Ok(())
}

/// SY27: the backup this library was restored from, if it was (its time). Such a
/// library starts with sync paused (`guard::paused`) and applies nothing.
pub fn restored_from(conn: &Connection, lib: &Library) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT restored_from FROM sync_libraries WHERE library = ?1",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::apply::tests::{device, highlight, publish_next, ANNO, LIB};
    use crate::sync::apply::{receive, Received};
    use crate::sync::ids::Id16;

    const HOUR: i64 = HOUR_MS;

    /// A library folder: its database on disk, and a book file.
    fn folder() -> (tempfile::TempDir, Store, Library) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Books")).unwrap();
        std::fs::write(dir.path().join("Books/moby.epub"), b"PK book").unwrap();
        let store = Store::open(&dir.path().join("linen.db")).unwrap();
        let lib = Library {
            id: LIB,
            device: Id16([2; 16]),
        };
        crate::sync::journal::join(store.conn(), &lib, "icloud-drive", 0).unwrap();
        (dir, store, lib)
    }

    #[test]
    fn a_backup_holds_the_database_with_its_wal_and_the_books() {
        let (dir, mut store, _) = folder();
        store
            .conn()
            .execute(
                "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                 VALUES ('b', 'h', 'moby.epub', 'Moby-Dick', 'package', 0)",
                [],
            )
            .unwrap();
        // Still in the WAL: not checkpointed.
        store
            .save_position("b", "epubcfi(/6/2)", 0.4, None)
            .unwrap();
        let backup = take(store.conn(), dir.path(), "first sync", 1_000).unwrap();
        let copy = Store::open(&backup.path.join("linen.db")).unwrap();
        assert_eq!(copy.position("b").unwrap().unwrap().0, "epubcfi(/6/2)");
        assert_eq!(
            std::fs::read(backup.path.join("Books/moby.epub")).unwrap(),
            b"PK book"
        );
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn a_library_restored_from_a_backup_starts_paused() {
        let (dir, store, lib) = folder();
        let backup = take(store.conn(), dir.path(), "first sync", 1_000).unwrap();
        let copy = Store::open(&backup.path.join("linen.db")).unwrap();
        assert_eq!(restored_from(copy.conn(), &lib).unwrap(), Some(1_000));
        assert!(crate::sync::guard::paused(copy.conn(), &lib).unwrap());
        // The live library isn't.
        assert_eq!(restored_from(store.conn(), &lib).unwrap(), None);
        // A restored library receives nothing.
        let (mut a, la) = device(1, "book-a");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(copy.conn(), &lib, &batch, 2_000).unwrap(),
            Received::Paused
        );
    }

    #[test]
    fn a_first_sync_and_a_remote_deletion_get_a_backup() {
        let (dir, store, lib) = folder();
        let key = Key::parse(&format!("annotation:{ANNO}")).unwrap().0;
        // First sync: nothing received yet.
        let first = before_pass(store.conn(), dir.path(), &lib, &[], 10).unwrap();
        assert_eq!(first.map(|b| b.manifest.reason), Some("first sync".into()));
        store
            .conn()
            .execute(
                "INSERT INTO sync_seen (library, device, seq, hash) VALUES (?1, 'x', 1, 'h')",
                [lib.id.to_string()],
            )
            .unwrap();
        // Nothing deleted: none.
        assert!(before_pass(store.conn(), dir.path(), &lib, &[], 20)
            .unwrap()
            .is_none());
        // A deletion within the hour of a backup that holds it: none.
        assert!(before_pass(
            store.conn(),
            dir.path(),
            &lib,
            std::slice::from_ref(&key),
            30
        )
        .unwrap()
        .is_none());
        // After an hour: a new one.
        let later = before_pass(store.conn(), dir.path(), &lib, &[key], 10 + HOUR).unwrap();
        assert_eq!(
            later.map(|b| b.manifest.reason),
            Some("remote deletion".into())
        );
    }

    #[test]
    fn a_record_changed_since_the_backup_needs_a_new_one() {
        let (dir, mut store, lib) = folder();
        store
            .conn()
            .execute(
                "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                 VALUES ('book-b', 'h', 'moby.epub', 'Moby-Dick', 'package', 0)",
                [],
            )
            .unwrap();
        take(store.conn(), dir.path(), "first sync", 10).unwrap();
        store
            .save_annotation(&highlight(
                "book-b",
                Some("written after"),
                1_760_000_001_000,
            ))
            .unwrap();
        let key = Key::parse(&format!("note:{ANNO}")).unwrap().0;
        assert!(!covers(store.conn(), dir.path(), &lib, &[key], 20).unwrap());
    }

    #[test]
    fn backups_are_kept_a_week_ten_at_most_and_the_newest_always() {
        let (dir, store, lib) = folder();
        let day = 24 * HOUR;
        // 14 backups, one a day for two weeks.
        for i in 0..14 {
            take(store.conn(), dir.path(), "remote deletion", i * day).unwrap();
        }
        // On day 30, only the newest remains: the rest are over a week old.
        prune(store.conn(), dir.path(), &lib, 30 * day).unwrap();
        let left = list(dir.path()).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].manifest.taken_at, 13 * day);
        // Twelve within the week: ten kept besides the newest.
        for i in 0..12 {
            take(
                store.conn(),
                dir.path(),
                "remote deletion",
                40 * day + i * HOUR,
            )
            .unwrap();
        }
        prune(store.conn(), dir.path(), &lib, 41 * day).unwrap();
        assert_eq!(list(dir.path()).unwrap().len(), 10);
    }

    #[test]
    fn the_backup_from_before_an_open_question_is_kept() {
        let (dir, store, lib) = folder();
        let day = 24 * HOUR;
        take(store.conn(), dir.path(), "first sync", 0).unwrap();
        // A question open since day 1 (its first counted change).
        store
            .conn()
            .execute(
                "INSERT INTO sync_question (library, asking, approved) VALUES (?1, 1, 0)",
                [lib.id.to_string()],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO sync_counted (library, at, what, kind, book, bytes, approved)
                 VALUES (?1, ?2, 'deleted', 'notes', NULL, 0, 0)",
                params![lib.id.to_string(), day],
            )
            .unwrap();
        for i in 2..14 {
            take(store.conn(), dir.path(), "remote deletion", i * day).unwrap();
        }
        prune(store.conn(), dir.path(), &lib, 30 * day).unwrap();
        let left: Vec<i64> = list(dir.path())
            .unwrap()
            .iter()
            .map(|b| b.manifest.taken_at)
            .collect();
        assert_eq!(
            left,
            vec![0, 13 * day],
            "the one from before the question, and the newest"
        );
    }
}
