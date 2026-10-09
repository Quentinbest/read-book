//! SQLite store (plan §3 Persistence, §4 data model, E5).
//!
//! WAL journal, one transaction per write, versioned migrations tracked in
//! `PRAGMA user_version`. Spike F measured progress writes at p95 < 0.1 ms with
//! `synchronous=NORMAL`, and no committed write lost across 200 SIGKILLs.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// Each entry upgrades the schema from version `index` to `index + 1`.
/// Never edit a shipped migration; add a new one (plan §6.3, §6.6).
pub const MIGRATIONS: &[&str] = &[
    // 1: initial schema (plan §4, first cut)
    r#"
    CREATE TABLE books (
        id TEXT PRIMARY KEY,
        content_hash TEXT NOT NULL,
        package_identifier TEXT,
        file_path TEXT NOT NULL,
        title TEXT NOT NULL,
        title_source TEXT NOT NULL,            -- 'package' | 'heading' | 'filename' (E4)
        authors TEXT NOT NULL DEFAULT '[]',    -- JSON array
        language TEXT,
        page_direction TEXT NOT NULL DEFAULT 'default',
        writing_mode TEXT,
        layout TEXT NOT NULL DEFAULT 'reflowable',
        has_page_list INTEGER NOT NULL DEFAULT 0,
        a11y_metadata TEXT NOT NULL DEFAULT '[]',
        cover_path TEXT,
        generated_cover_tint TEXT,
        added_at INTEGER NOT NULL,
        opened_at INTEGER,
        finished_at INTEGER,
        replaced_at INTEGER
    );
    CREATE UNIQUE INDEX books_content_hash ON books(content_hash);
    CREATE INDEX books_package_identifier ON books(package_identifier);

    CREATE TABLE book_damage (
        book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
        item_href TEXT NOT NULL,
        error_kind TEXT NOT NULL,
        PRIMARY KEY (book_id, item_href)
    );

    CREATE TABLE positions (
        book_id TEXT PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
        cfi TEXT NOT NULL,
        fraction REAL NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE book_settings (
        book_id TEXT PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
        layout_mode TEXT NOT NULL DEFAULT 'pages',
        navigator_docked TEXT                  -- tab name, or NULL (S13)
    );

    CREATE TABLE annotations (
        id TEXT PRIMARY KEY,
        book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
        anchored_content_hash TEXT NOT NULL,
        color TEXT NOT NULL,
        cfi_range TEXT NOT NULL,
        quote_exact TEXT NOT NULL,
        quote_prefix TEXT NOT NULL DEFAULT '',
        quote_suffix TEXT NOT NULL DEFAULT '',
        note TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        deleted_at INTEGER,
        anchor_status TEXT NOT NULL DEFAULT 'anchored'
    );
    CREATE INDEX annotations_book ON annotations(book_id, deleted_at);

    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE extensions (
        id TEXT PRIMARY KEY,
        version TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        granted_permissions TEXT NOT NULL DEFAULT '[]',
        installed_at INTEGER NOT NULL,
        crash_log TEXT NOT NULL DEFAULT '[]'
    );

    CREATE TABLE extension_storage (
        ext_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
        key TEXT NOT NULL,
        value BLOB NOT NULL,
        PRIMARY KEY (ext_id, key)
    );
    "#,
    // 2: the search index's extracted chapter text (F8), valid for one version of the file
    r#"
    CREATE TABLE search_text (
        book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
        content_hash TEXT NOT NULL,
        section INTEGER NOT NULL,
        text TEXT NOT NULL,
        PRIMARY KEY (book_id, section)
    );
    "#,
    // 3: Remove with Undo (G4, provisional): a removed book stays until the next launch;
    // Continue reading names the chapter (E6).
    r#"
    ALTER TABLE books ADD COLUMN removed_at INTEGER;
    ALTER TABLE positions ADD COLUMN chapter_label TEXT;
    "#,
    // 4: Reading Lens Stage 2b (DX1, DX2, DX11): the reader's own MDX dictionaries, in order.
    // Each row names its active generation, a folder in the library's Dictionaries folder.
    r#"
    CREATE TABLE dictionaries (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        title TEXT NOT NULL,
        source_hash TEXT NOT NULL,
        generation TEXT NOT NULL,
        position INTEGER NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        entries INTEGER NOT NULL,
        resources INTEGER NOT NULL DEFAULT 0,
        added_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    "#,
];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {0} is newer than this app ({1})")]
    TooNew(i64, i64),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// An annotation as stored (A9): a CFI and a text quote, and the file version it was made on.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnnotationRow {
    pub id: String,
    pub book_id: String,
    pub anchored_content_hash: String,
    pub color: String,
    pub cfi_range: String,
    pub quote_exact: String,
    pub quote_prefix: String,
    pub quote_suffix: String,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// 'anchored' | 'reanchor' (the file was replaced) | 'unplaced' (A8: Couldn't place)
    pub anchor_status: String,
}

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BookRow {
    pub id: String,
    pub content_hash: String,
    pub package_identifier: Option<String>,
    pub file_path: String,
    pub title: String,
    pub title_source: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub page_direction: String,
    pub layout: String,
    pub has_page_list: bool,
    pub cover_path: Option<String>,
    pub generated_cover_tint: Option<String>,
    pub added_at: i64,
    pub opened_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub replaced_at: Option<i64>,
    pub damaged_items: i64,
    pub fraction: Option<f64>,
    /// E6: the chapter at the saved position, for Continue reading.
    pub chapter_label: Option<String>,
}

/// A dictionary the reader added (Reading Lens DX1, DX11).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DictionaryRow {
    pub id: String,
    pub name: String,
    pub title: String,
    pub source_hash: String,
    pub generation: String,
    pub position: i64,
    pub enabled: bool,
    pub entries: i64,
    pub resources: i64,
    pub added_at: i64,
    pub updated_at: i64,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()?;
        let latest = MIGRATIONS.len() as i64;
        if current > latest {
            return Err(StoreError::TooNew(current, latest));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn book_by_hash(&self, hash: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM books WHERE content_hash = ?1",
                [hash],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Books with this OPF identifier: (id, file_path, title), newest first.
    pub fn books_by_package_identifier(
        &self,
        ident: &str,
    ) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_path, title FROM books WHERE package_identifier = ?1 ORDER BY added_at DESC",
        )?;
        let rows = stmt.query_map([ident], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn books(&self) -> Result<Vec<BookRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.content_hash, b.package_identifier, b.file_path, b.title, b.title_source,
                    b.authors, b.language, b.page_direction, b.layout, b.has_page_list, b.cover_path,
                    b.generated_cover_tint, b.added_at, b.opened_at, b.finished_at, b.replaced_at,
                    (SELECT COUNT(*) FROM book_damage d WHERE d.book_id = b.id), p.fraction,
                    p.chapter_label
             FROM books b LEFT JOIN positions p ON p.book_id = b.id
             WHERE b.removed_at IS NULL
             ORDER BY COALESCE(b.opened_at, b.added_at) DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let authors: String = r.get(6)?;
            Ok(BookRow {
                id: r.get(0)?,
                content_hash: r.get(1)?,
                package_identifier: r.get(2)?,
                file_path: r.get(3)?,
                title: r.get(4)?,
                title_source: r.get(5)?,
                authors: serde_json::from_str(&authors).unwrap_or_default(),
                language: r.get(7)?,
                page_direction: r.get(8)?,
                layout: r.get(9)?,
                has_page_list: r.get::<_, i64>(10)? != 0,
                cover_path: r.get(11)?,
                generated_cover_tint: r.get(12)?,
                added_at: r.get(13)?,
                opened_at: r.get(14)?,
                finished_at: r.get(15)?,
                replaced_at: r.get(16)?,
                damaged_items: r.get(17)?,
                fraction: r.get(18)?,
                chapter_label: r.get(19)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn save_position(
        &mut self,
        book_id: &str,
        cfi: &str,
        fraction: f64,
        chapter_label: Option<&str>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO positions (book_id, cfi, fraction, updated_at, chapter_label)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(book_id) DO UPDATE SET cfi = excluded.cfi, fraction = excluded.fraction,
             updated_at = excluded.updated_at, chapter_label = excluded.chapter_label",
            params![book_id, cfi, fraction, now_ms(), chapter_label],
        )?;
        tx.execute(
            "UPDATE books SET opened_at = ?2 WHERE id = ?1",
            params![book_id, now_ms()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// G4 (provisional): Remove takes the book out of the library at once; Undo puts it
    /// back. Its file, cover and annotations go at the next launch (`purge_removed`).
    pub fn remove_book(&mut self, book_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE books SET removed_at = ?2 WHERE id = ?1",
            params![book_id, now_ms()],
        )?;
        Ok(())
    }

    pub fn restore_book(&mut self, book_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE books SET removed_at = NULL WHERE id = ?1",
            [book_id],
        )?;
        Ok(())
    }

    /// Delete books removed in an earlier session, with everything that belongs to
    /// them. Returns their files and covers, for the caller to delete from disk.
    pub fn purge_removed(&mut self) -> Result<Vec<(String, Option<String>)>> {
        let tx = self.conn.transaction()?;
        let gone: Vec<(String, String, Option<String>)> = {
            let mut stmt = tx.prepare(
                "SELECT id, file_path, cover_path FROM books WHERE removed_at IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for (id, _, _) in &gone {
            // Annotations are soft-deleted rows too; all of the book's rows go (ON DELETE CASCADE).
            tx.execute("DELETE FROM books WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(gone.into_iter().map(|(_, f, c)| (f, c)).collect())
    }

    /// A9: the book's live annotations (deleted ones are kept for Undo, not listed).
    pub fn annotations(&self, book_id: &str) -> Result<Vec<AnnotationRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, book_id, anchored_content_hash, color, cfi_range, quote_exact, quote_prefix,
                    quote_suffix, note, created_at, updated_at, anchor_status
             FROM annotations WHERE book_id = ?1 AND deleted_at IS NULL ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([book_id], |r| {
                Ok(AnnotationRow {
                    id: r.get(0)?,
                    book_id: r.get(1)?,
                    anchored_content_hash: r.get(2)?,
                    color: r.get(3)?,
                    cfi_range: r.get(4)?,
                    quote_exact: r.get(5)?,
                    quote_prefix: r.get(6)?,
                    quote_suffix: r.get(7)?,
                    note: r.get(8)?,
                    created_at: r.get(9)?,
                    updated_at: r.get(10)?,
                    anchor_status: r.get(11)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Insert or update an annotation; saving a deleted one restores it (Undo, A7).
    pub fn save_annotation(&mut self, a: &AnnotationRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO annotations (id, book_id, anchored_content_hash, color, cfi_range, quote_exact,
                 quote_prefix, quote_suffix, note, created_at, updated_at, deleted_at, anchor_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12)
             ON CONFLICT(id) DO UPDATE SET anchored_content_hash = excluded.anchored_content_hash,
                 color = excluded.color, cfi_range = excluded.cfi_range,
                 quote_exact = excluded.quote_exact, quote_prefix = excluded.quote_prefix,
                 quote_suffix = excluded.quote_suffix, note = excluded.note,
                 updated_at = excluded.updated_at, deleted_at = NULL,
                 anchor_status = excluded.anchor_status",
            params![
                a.id,
                a.book_id,
                a.anchored_content_hash,
                a.color,
                a.cfi_range,
                a.quote_exact,
                a.quote_prefix,
                a.quote_suffix,
                a.note,
                a.created_at,
                a.updated_at,
                a.anchor_status
            ],
        )?;
        Ok(())
    }

    /// A7: delete at once; the row stays (marked deleted) so Undo can restore it exactly.
    pub fn delete_annotation(&mut self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE annotations SET deleted_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    pub fn restore_annotation(&mut self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE annotations SET deleted_at = NULL WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    pub fn position(&self, book_id: &str) -> Result<Option<(String, f64)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT cfi, fraction FROM positions WHERE book_id = ?1",
                [book_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

// ---- Reading Lens dictionaries (DX1, DX2, DX11)
impl Store {
    pub fn dictionaries(&self) -> Result<Vec<DictionaryRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, source_hash, generation, position, enabled, entries, resources,
                    added_at, updated_at FROM dictionaries ORDER BY position, added_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DictionaryRow {
                id: r.get(0)?,
                name: r.get(1)?,
                title: r.get(2)?,
                source_hash: r.get(3)?,
                generation: r.get(4)?,
                position: r.get(5)?,
                enabled: r.get::<_, i64>(6)? != 0,
                entries: r.get(7)?,
                resources: r.get(8)?,
                added_at: r.get(9)?,
                updated_at: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// DX1, DX2: a new dictionary, or a new generation of one with the same name.
    /// Returns the row and the generation it replaced, if any.
    pub fn dictionary_activate(
        &mut self,
        name: &str,
        title: &str,
        source_hash: &str,
        generation: &str,
        entries: i64,
        resources: i64,
    ) -> Result<(DictionaryRow, Option<String>)> {
        let tx = self.conn.transaction()?;
        let now = now_ms();
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT id, generation FROM dictionaries WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (id, replaced) = match existing {
            Some((id, old)) => {
                tx.execute(
                    "UPDATE dictionaries SET title = ?2, source_hash = ?3, generation = ?4, entries = ?5,
                     resources = ?6, updated_at = ?7 WHERE id = ?1",
                    params![id, title, source_hash, generation, entries, resources, now],
                )?;
                (id, Some(old))
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                let position: i64 = tx.query_row(
                    "SELECT COALESCE(MAX(position) + 1, 0) FROM dictionaries",
                    [],
                    |r| r.get(0),
                )?;
                tx.execute(
                    "INSERT INTO dictionaries (id, name, title, source_hash, generation, position, enabled,
                     entries, resources, added_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?9)",
                    params![id, name, title, source_hash, generation, position, entries, resources, now],
                )?;
                (id, None)
            }
        };
        tx.commit()?;
        let row = self
            .dictionaries()?
            .into_iter()
            .find(|d| d.id == id)
            .expect("the row just written");
        Ok((row, replaced))
    }

    pub fn dictionary_remove(&mut self, id: &str) -> Result<Option<String>> {
        let tx = self.conn.transaction()?;
        let generation: Option<String> = tx
            .query_row(
                "SELECT generation FROM dictionaries WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute("DELETE FROM dictionaries WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(generation)
    }

    pub fn dictionary_set_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE dictionaries SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )?;
        Ok(())
    }

    /// DX11: the order the peek tries them in.
    pub fn dictionary_reorder(&mut self, ids: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE dictionaries SET position = ?2 WHERE id = ?1",
                params![id, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_a_new_database_to_the_latest_version() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len() as i64);
        let tables: Vec<String> = s
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        for t in [
            "annotations",
            "book_damage",
            "book_settings",
            "books",
            "extension_storage",
            "extensions",
            "positions",
            "settings",
        ] {
            assert!(tables.contains(&t.to_string()), "{t}");
        }
    }

    #[test]
    fn reopening_does_not_rerun_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linen.db");
        {
            let mut s = Store::open(&path).unwrap();
            s.set_setting("theme", "sepia").unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.setting("theme").unwrap().as_deref(), Some("sepia"));
        let mode: String = s
            .conn()
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn refuses_a_database_from_a_newer_app() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linen.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", 999).unwrap();
        }
        assert!(matches!(
            Store::open(&path),
            Err(StoreError::TooNew(999, _))
        ));
    }
}

#[cfg(test)]
mod removal_tests {
    use super::*;

    fn book(store: &Store, id: &str) {
        store
            .conn()
            .execute(
                "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                 VALUES (?1, ?1, ?1, ?1, 'package', 0)",
                [id],
            )
            .unwrap();
    }

    #[test]
    fn remove_hides_the_book_restore_brings_it_back_and_purge_deletes_everything() {
        let mut store = Store::open_in_memory().unwrap();
        book(&store, "b1");
        book(&store, "b2");
        store
            .save_position("b1", "epubcfi(/6/2!/4)", 0.5, Some("One"))
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO annotations (id, book_id, anchored_content_hash, color, cfi_range, quote_exact, created_at, updated_at)
                 VALUES ('a1', 'b1', 'b1', 'yellow', 'x', 'q', 0, 0)",
                [],
            )
            .unwrap();
        store.remove_book("b1").unwrap();
        assert_eq!(
            store
                .books()
                .unwrap()
                .iter()
                .map(|b| b.id.as_str())
                .collect::<Vec<_>>(),
            ["b2"]
        );
        store.restore_book("b1").unwrap();
        let listed = store.books().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed
                .iter()
                .find(|b| b.id == "b1")
                .unwrap()
                .chapter_label
                .as_deref(),
            Some("One")
        );
        store.remove_book("b1").unwrap();
        let gone = store.purge_removed().unwrap();
        assert_eq!(gone, vec![("b1".to_string(), None)]);
        let count = |sql: &str| {
            store
                .conn()
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        assert_eq!(count("SELECT COUNT(*) FROM books"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM annotations"), 0);
        assert_eq!(count("SELECT COUNT(*) FROM positions"), 0);
    }
}

#[cfg(test)]
mod annotation_tests {
    use super::*;

    fn row(id: &str) -> AnnotationRow {
        AnnotationRow {
            id: id.into(),
            book_id: "b1".into(),
            anchored_content_hash: "h1".into(),
            color: "yellow".into(),
            cfi_range: "epubcfi(/6/4!/4/2,/1:0,/1:5)".into(),
            quote_exact: "Call me".into(),
            quote_prefix: String::new(),
            quote_suffix: " Ishmael".into(),
            note: None,
            created_at: 1,
            updated_at: 1,
            anchor_status: "anchored".into(),
        }
    }

    #[test]
    fn save_delete_and_undo_restore_the_exact_annotation() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("linen.db")).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                 VALUES ('b1', 'h1', '/b1.epub', 'B', 'package', 0)",
                [],
            )
            .unwrap();
        let mut a = row("a1");
        store.save_annotation(&a).unwrap();
        a.note = Some("a note".into());
        a.color = "green".into();
        a.updated_at = 2;
        store.save_annotation(&a).unwrap();
        assert_eq!(store.annotations("b1").unwrap(), vec![a.clone()]);
        store.delete_annotation("a1").unwrap();
        assert!(store.annotations("b1").unwrap().is_empty());
        store.restore_annotation("a1").unwrap();
        assert_eq!(store.annotations("b1").unwrap(), vec![a]);
    }
}
