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
                    (SELECT COUNT(*) FROM book_damage d WHERE d.book_id = b.id), p.fraction
             FROM books b LEFT JOIN positions p ON p.book_id = b.id
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
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn save_position(&mut self, book_id: &str, cfi: &str, fraction: f64) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO positions (book_id, cfi, fraction, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(book_id) DO UPDATE SET cfi = excluded.cfi, fraction = excluded.fraction,
             updated_at = excluded.updated_at",
            params![book_id, cfi, fraction, now_ms()],
        )?;
        tx.execute(
            "UPDATE books SET opened_at = ?2 WHERE id = ?1",
            params![book_id, now_ms()],
        )?;
        tx.commit()?;
        Ok(())
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
