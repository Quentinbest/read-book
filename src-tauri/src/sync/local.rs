//! SY7, SY8, SY12, SY13, SY19, SY29: the library's own writes, journalled as versions.
//!
//! Each Store write that changes a synced thing calls in here inside its own
//! transaction, so the change and its version are committed together. With no
//! library joined, nothing is written (SY6). Applying a remote version doesn't come
//! through here (`apply`), so it is never sent back (SY12).

use super::a9;
use super::ids::Id16;
use super::journal::{self, JournalError, Library, LocalContent};
use super::merge::Outcome;
use super::record::{Entry, Key};
use crate::store::AnnotationRow;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

type Result<T> = std::result::Result<T, JournalError>;

fn key(s: String) -> Key {
    Key::parse(&s).expect("keys built here take SY9's forms").0
}

/// SY28: a body over its type's limit stays on this device. It isn't journalled,
/// and the local save goes ahead (Stage 3's Settings › Sync names it).
fn record(
    conn: &Connection,
    lib: &Library,
    k: Key,
    content: LocalContent,
    now_ms: u64,
) -> Result<()> {
    if let LocalContent::Body(b) = &content {
        if b.len() as u64 > k.kind().body_limit() {
            // An earlier unsent value is superseded here and isn't published.
            return journal::drop_unsent(conn, lib, &k);
        }
    }
    // #109: once an edit is held for the reader's choice, later edits replace the
    // held one; none becomes publishable before the reader chooses.
    if journal::held(conn, lib, &k)? {
        return journal::hold_local(conn, lib, &k, content, now_ms).map(|_| ());
    }
    match journal::record_local(conn, lib, &k, content.clone(), now_ms) {
        // SY13: this device's own version waits in a conflict the reader hasn't
        // decided. The save goes ahead, and the edit is held as the reader's
        // candidate choice, never lost and never published without it (#109).
        Err(JournalError::Conflicted(_)) => {
            journal::hold_local(conn, lib, &k, content, now_ms).map(|_| ())
        }
        other => other.map(|_| ()),
    }
}

/// SY8: a book's sync ID in this library, made on first use. Local book IDs never
/// leave the device; records name sync IDs.
pub fn sync_id(conn: &Connection, lib: &Library, book_id: &str) -> Result<String> {
    let found: Option<String> = conn
        .query_row(
            "SELECT sync_id FROM sync_books WHERE library = ?1 AND book_id = ?2",
            params![lib.id.to_string(), book_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO sync_books (library, sync_id, book_id) VALUES (?1, ?2, ?3)",
        params![lib.id.to_string(), id, book_id],
    )?;
    Ok(id)
}

/// SP13: a tombstone's causal frontier: for each device, the latest change of it
/// whose every version this device has applied (or settled past). A change received
/// but still waiting (for its body, book, ancestry or clock) wasn't seen by the
/// reader, so the frontier stops before it and a deletion stays in conflict with it
/// (SY13).
pub fn covers(conn: &Connection, lib: &Library) -> Result<Vec<Entry>> {
    let rows: Vec<(String, i64)> = conn
        .prepare("SELECT device, seq FROM sync_seen WHERE library = ?1")?
        .query_map([lib.id.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for (d, seen) in rows {
        let pending: Option<i64> = conn.query_row(
            "SELECT MIN(seq) FROM sync_versions
             WHERE library = ?1 AND device = ?2 AND applied IN (0, -1, -2)",
            params![lib.id.to_string(), d],
            |r| r.get(0),
        )?;
        let upto = pending.map_or(seen, |p| (p - 1).min(seen));
        if upto < 1 {
            continue;
        }
        let hash: Option<String> = conn
            .query_row(
                "SELECT hash FROM sync_changes WHERE library = ?1 AND device = ?2 AND seq = ?3",
                params![lib.id.to_string(), d, upto],
                |r| r.get(0),
            )
            .optional()?;
        if let (Some(device), Some(hash)) = (
            Id16::parse(&d),
            hash.as_deref().and_then(super::ids::Hash32::parse),
        ) {
            out.push(Entry {
                device,
                seq: upto as u64,
                hash,
            });
        }
    }
    Ok(out)
}

pub(crate) fn place_body(cfi: &str, fraction: f64, chapter: Option<&str>) -> Vec<u8> {
    let mut b = json!({ "cfi": cfi, "fraction": fraction });
    if let Some(c) = chapter {
        b["chapter"] = json!(c);
    }
    b.to_string().into_bytes()
}

fn note_body(text: &str) -> Vec<u8> {
    json!({ "text": text }).to_string().into_bytes()
}

fn live_note(note: &Option<String>) -> Option<&str> {
    note.as_deref().filter(|t| !t.trim().is_empty())
}

/// SY12: a place saved, and the book marked opened (`save_position` does both).
pub fn on_position(
    conn: &Connection,
    book_id: &str,
    cfi: &str,
    fraction: f64,
    chapter: Option<&str>,
    now_ms: u64,
) -> Result<()> {
    let Some(lib) = journal::active(conn)? else {
        return Ok(());
    };
    let id = sync_id(conn, &lib, book_id)?;
    record(
        conn,
        &lib,
        key(format!("place:{id}")),
        LocalContent::Body(place_body(cfi, fraction, chapter)),
        now_ms,
    )?;
    record(
        conn,
        &lib,
        key(format!("bookRead:{id}:opened")),
        LocalContent::Body(b"{}".to_vec()),
        now_ms,
    )?;
    Ok(())
}

/// SY13: a highlight saved (made, recoloured, re-anchored, or its note edited).
/// `before` is its row before the write, if it had one.
pub fn on_annotation(
    conn: &Connection,
    before: Option<&AnnotationRow>,
    after: &AnnotationRow,
    now_ms: u64,
) -> Result<()> {
    let Some(lib) = journal::active(conn)? else {
        return Ok(());
    };
    let book = sync_id(conn, &lib, &after.book_id)?;
    // SY9: the highlight's own fields, not its note nor the time a note edit stamps.
    let own = |a: &AnnotationRow| AnnotationRow {
        note: None,
        updated_at: 0,
        ..a.clone()
    };
    let changed = before.map_or(true, |b| own(b) != own(after));
    if changed {
        let body = a9::body_of(after, &book).to_string().into_bytes();
        record(
            conn,
            &lib,
            key(format!("annotation:{}", after.id)),
            LocalContent::Body(body),
            now_ms,
        )?;
    }
    let was = before.and_then(|b| live_note(&b.note));
    match (was, live_note(&after.note)) {
        (old, Some(new)) if old != Some(new) => {
            record(
                conn,
                &lib,
                key(format!("note:{}", after.id)),
                LocalContent::Body(note_body(new)),
                now_ms,
            )?;
        }
        // SY9: emptying a note is a deletion, kept for the week and restorable.
        (Some(_), None) => {
            let covers = covers(conn, &lib)?;
            record(
                conn,
                &lib,
                key(format!("note:{}", after.id)),
                LocalContent::Deleted {
                    covers,
                    identity: None,
                },
                now_ms,
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// SY13: a highlight deleted. Its note, if it had a live one, is deleted with it in
/// the same change, under the same frontier (SP13).
pub fn on_annotation_deleted(conn: &Connection, row: &AnnotationRow, now_ms: u64) -> Result<()> {
    let Some(lib) = journal::active(conn)? else {
        return Ok(());
    };
    let covers = covers(conn, &lib)?;
    record(
        conn,
        &lib,
        key(format!("annotation:{}", row.id)),
        LocalContent::Deleted {
            covers: covers.clone(),
            identity: None,
        },
        now_ms,
    )?;
    if live_note(&row.note).is_some() {
        record(
            conn,
            &lib,
            key(format!("note:{}", row.id)),
            LocalContent::Deleted {
                covers,
                identity: None,
            },
            now_ms,
        )?;
    }
    Ok(())
}

/// SY19: a deleted highlight brought back (Undo, or Restore). The new version
/// descends from the tombstone, so it comes back on every device, with the note
/// deleted with it.
pub fn on_annotation_restored(conn: &Connection, row: &AnnotationRow, now_ms: u64) -> Result<()> {
    on_annotation(conn, None, row, now_ms)
}

/// SY29: on joining a library, this device publishes its whole current library as
/// its own new changes: independent additions, never older than that library's
/// tombstones, so nothing there is replaced wholesale. On a return, what it deleted
/// meanwhile is published too.
pub fn publish_library(conn: &Connection, now_ms: u64) -> Result<usize> {
    if journal::active(conn)?.is_none() {
        return Ok(0);
    }
    let mut n = 0;
    let places: Vec<(String, String, f64, Option<String>)> = conn
        .prepare("SELECT book_id, cfi, fraction, chapter_label FROM positions")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (book, cfi, fraction, chapter) in places {
        on_position(conn, &book, &cfi, fraction, chapter.as_deref(), now_ms)?;
        n += 1;
    }
    let rows: Vec<(AnnotationRow, bool)> = conn
        .prepare(
            "SELECT id, book_id, anchored_content_hash, color, cfi_range, quote_exact, quote_prefix,
                    quote_suffix, note, created_at, updated_at, anchor_status, deleted_at IS NOT NULL
             FROM annotations",
        )?
        .query_map([], |r| {
            Ok((AnnotationRow {
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
            }, r.get(12)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let lib = journal::active(conn)?.expect("checked above");
    for (a, deleted) in &rows {
        if !deleted {
            on_annotation(conn, None, a, now_ms)?;
            n += 1;
            continue;
        }
        // A highlight deleted here that this library still shows (a return to a
        // library after deleting it elsewhere) is deleted there too; one it never
        // had isn't published.
        let k = key(format!("annotation:{}", a.id));
        let shows = match journal::record_of(conn, &lib, &k, journal::Which::Applied)?
            .outcome(&std::collections::HashSet::new())
        {
            Some(Outcome::Settled(p)) => !p.is_deleted(),
            Some(Outcome::Conflict { shown, .. }) => !shown.is_deleted(),
            None => false,
        };
        if shows {
            on_annotation_deleted(conn, a, now_ms)?;
            n += 1;
        }
    }
    Ok(n)
}
