//! A generation's headword index (DX2, DX4, DX10): one SQLite file beside the
//! dictionary's own files, built once at import and read-only afterwards. Headwords
//! are stored as written and folded, in source order, with where each record starts
//! and ends, so a look-up reads one entry by offset and nothing loads whole.

use super::fold::fold;
use super::mdx::{with_ends, Key, Mdx, MdxError};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub const FILE: &str = "index.sqlite";

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error(transparent)]
    Mdx(#[from] MdxError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("the import was cancelled")]
    Cancelled,
}

/// One headword's record: which file (0 is the MDX, 1… the MDDs) and where.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub seq: i64,
    pub key: String,
    pub file: i64,
    pub start: u64,
    pub end: u64,
}

/// Build the index for `mdx` and its `mdds` (already copied into the generation).
/// Returns (entries, resources).
pub fn build(
    out: &Path,
    mdx: &Mdx,
    mdds: &[Mdx],
    cancel: &AtomicBool,
) -> Result<(u64, u64), IndexError> {
    let mut conn = Connection::open(out)?;
    conn.execute_batch(
        "PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;
         CREATE TABLE keys (seq INTEGER PRIMARY KEY, key TEXT NOT NULL, folded TEXT NOT NULL,
                            start INTEGER NOT NULL, end INTEGER NOT NULL);
         CREATE TABLE resources (path TEXT NOT NULL, file INTEGER NOT NULL,
                                 start INTEGER NOT NULL, end INTEGER NOT NULL);",
    )?;
    let tx = conn.transaction()?;
    let mut keys: Vec<Key> = Vec::with_capacity(mdx.entries.min(1 << 22) as usize);
    mdx.keys(|k| keys.push(k))?;
    // Source order (DX4: duplicate headwords in source order) is the key blocks' order.
    let order: std::collections::HashMap<(u64, String), usize> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| ((k.offset, k.text.clone()), i))
        .collect();
    let entries = keys.len() as u64;
    {
        let mut ins = tx.prepare(
            "INSERT INTO keys (seq, key, folded, start, end) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (n, (k, end)) in with_ends(mdx, keys).into_iter().enumerate() {
            if n % 4096 == 0 && cancel.load(Ordering::Relaxed) {
                return Err(IndexError::Cancelled);
            }
            let seq = order[&(k.offset, k.text.clone())] as i64;
            ins.execute(params![
                seq,
                k.text,
                fold(&k.text),
                k.offset as i64,
                end as i64
            ])?;
        }
    }
    let mut resources = 0u64;
    {
        let mut ins =
            tx.prepare("INSERT INTO resources (path, file, start, end) VALUES (?1, ?2, ?3, ?4)")?;
        for (i, mdd) in mdds.iter().enumerate() {
            let mut keys = vec![];
            mdd.keys(|k| keys.push(k))?;
            for (k, end) in with_ends(mdd, keys) {
                if cancel.load(Ordering::Relaxed) {
                    return Err(IndexError::Cancelled);
                }
                ins.execute(params![
                    resource_path(&k.text),
                    (i + 1) as i64,
                    k.offset as i64,
                    end as i64
                ])?;
                resources += 1;
            }
        }
    }
    tx.execute_batch(
        "CREATE INDEX keys_key ON keys(key); CREATE INDEX keys_folded ON keys(folded);
         CREATE INDEX resources_path ON resources(path);",
    )?;
    tx.commit()?;
    conn.execute_batch("VACUUM;")?;
    Ok((entries, resources))
}

/// A resource's path as entries name it: forward slashes, no leading slash, lowercase.
pub fn resource_path(key: &str) -> String {
    key.replace('\\', "/")
        .trim_start_matches('/')
        .to_lowercase()
}

/// Read-only, shared by the scheme's threads and the look-up command.
pub struct Index {
    conn: std::sync::Mutex<Connection>,
}

impl Index {
    pub fn open(path: &Path) -> Result<Index, IndexError> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Index {
            conn: std::sync::Mutex::new(conn),
        })
    }

    fn hits(&self, sql: &str, q: &str) -> Result<Vec<Hit>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(sql)?;
        let rows = stmt.query_map([q], |r| {
            Ok(Hit {
                seq: r.get(0)?,
                key: r.get(1)?,
                file: 0,
                start: r.get::<_, i64>(2)? as u64,
                end: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// DX4: exact first; then folded. Duplicate headwords come in source order.
    pub fn find(&self, q: &str) -> Result<Vec<Hit>, IndexError> {
        let exact = self.hits(
            "SELECT seq, key, start, end FROM keys WHERE key = ?1 ORDER BY seq",
            q,
        )?;
        if !exact.is_empty() {
            return Ok(exact);
        }
        self.hits(
            "SELECT seq, key, start, end FROM keys WHERE folded = ?1 ORDER BY seq",
            &fold(q),
        )
    }

    pub fn resource(&self, path: &str) -> Result<Option<Hit>, IndexError> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT file, start, end FROM resources WHERE path = ?1 LIMIT 1",
                [resource_path(path)],
                |r| {
                    Ok(Hit {
                        seq: 0,
                        key: path.to_string(),
                        file: r.get(0)?,
                        start: r.get::<_, i64>(1)? as u64,
                        end: r.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .optional()?)
    }
}
