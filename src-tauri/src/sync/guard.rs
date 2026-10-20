//! SY26 (item 106): large changes from other devices wait for the reader.
//!
//! Every change applied from another device keeps, in an undo journal, what it
//! replaced (the body itself, or that nothing was there), and is counted across
//! passes and kinds, over a rolling 24 hours and in total since the reader last
//! answered, so changes paced to stay under a day's limit are still caught. Past a
//! threshold, the change that would cross it isn't applied, and nothing more is,
//! until the reader answers: Apply lets it through; Stop syncing pauses sync and
//! puts back, from the undo journal, every change counted since the last answer.
//!
//! The question's wording, and the backup SY27 keeps until the answer, come with
//! Stage 3 and Stage 1b.3's backups.

use super::apply::{self, Tally};
use super::ids::{DeviceId, Hash32};
use super::journal::{self, JournalError, Library, LocalContent, Which};
use super::merge::{Outcome, Placed};
use super::record::{Content, Key, Kind};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, HashSet};

type Result<T> = std::result::Result<T, JournalError>;

/// SY26 (prov., item 106): past any of these, in 24 hours or since the reader last
/// answered, Linen asks before applying more. PROVISIONAL. Books and their files
/// (20 books or 5 GB added or replaced, 5 files replaced, 10 books or a quarter of
/// the library removed) are counted from Stage 4, when book records are applied.
pub const DELETED_NOTES: i64 = 50; // highlights and notes deleted
pub const ADDED_NOTES: i64 = 200; // highlights and notes added
pub const CHANGED_NOTES: i64 = 50; // highlights, notes and settings rewritten
pub const PLACE_BOOKS: i64 = 20; // books whose place or reading state changed
pub const BOOK_NOTE_BYTES: i64 = 50 * 1024 * 1024; // highlights and notes added to one book

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// SY26 (prov.): the totals since the last answer are asked about at most weekly,
/// and the undo journal keeps a week at least.
const WEEK_MS: i64 = 7 * DAY_MS;

/// What other devices changed in a window, as SY26 counts it.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Counts {
    pub deleted: i64,
    pub added: i64,
    pub changed: i64,
    pub place_books: i64,
    /// The most bytes of highlights and notes added to one book (rewrites count
    /// what they grow by).
    pub book_bytes: i64,
}

impl Counts {
    fn over(&self) -> bool {
        self.deleted > DELETED_NOTES
            || self.added > ADDED_NOTES
            || self.changed > CHANGED_NOTES
            || self.place_books > PLACE_BOOKS
            || self.book_bytes > BOOK_NOTE_BYTES
    }
}

/// The reader's answer to the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Apply (or, for removals, Remove): the waiting change goes through.
    Apply,
    /// Sync pauses, and every change counted since the last answer is put back.
    StopSyncing,
}

struct Question {
    asking: bool,
    approved: bool,
    device: Option<String>,
    seq: Option<i64>,
    hash: Option<String>,
    answered_at: Option<i64>,
    totals_asked_at: Option<i64>,
}

fn question(conn: &Connection, lib: &Library) -> Result<Option<Question>> {
    Ok(conn
        .query_row(
            "SELECT asking, approved, device, seq, answered_at, totals_asked_at, hash
             FROM sync_question WHERE library = ?1",
            [lib.id.to_string()],
            |r| {
                Ok(Question {
                    asking: r.get(0)?,
                    approved: r.get(1)?,
                    device: r.get(2)?,
                    seq: r.get(3)?,
                    answered_at: r.get(4)?,
                    totals_asked_at: r.get(5)?,
                    hash: r.get(6)?,
                })
            },
        )
        .optional()?)
}

/// What the open question counted, if one is open.
pub fn asking(conn: &Connection, lib: &Library) -> Result<Option<Counts>> {
    let counts: Option<Option<String>> = conn
        .query_row(
            "SELECT counts FROM sync_question WHERE library = ?1 AND asking = 1",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(counts.map(|c| {
        c.and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }))
}

/// SY26: whether sync is paused (Stop syncing, or SY27's restored backup).
pub fn paused(conn: &Connection, lib: &Library) -> Result<bool> {
    let state: Option<String> = conn
        .query_row(
            "SELECT state FROM sync_libraries WHERE library = ?1",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(state.as_deref() == Some("paused"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// No question open: apply, counting.
    Open,
    /// A question is open: nothing more is applied.
    Asking,
    /// The change the reader let through: apply it, not counted again.
    Approved,
}

/// A version, as an approval names it: its writer, sequence and key.
pub(crate) type VersionId = (String, u64, String);

pub(crate) fn version_id(p: &Placed) -> VersionId {
    (
        p.version.device.to_string(),
        p.seq,
        p.version.key.as_str().to_owned(),
    )
}

/// Whether a change from `from` (a device's change, by device, sequence and hash,
/// or `None` for a pass over pending versions) may be applied now. An approval is
/// for the change the reader was shown: another change at the same sequence (a
/// different hash) isn't covered.
pub(crate) fn gate(
    conn: &Connection,
    lib: &Library,
    from: Option<(DeviceId, u64, Hash32)>,
) -> Result<Gate> {
    let Some(q) = question(conn, lib)? else {
        return Ok(Gate::Open);
    };
    if !q.asking {
        return Ok(Gate::Open);
    }
    let same = match from {
        Some((d, s, h)) => {
            q.device.as_deref() == Some(d.to_string().as_str())
                && q.seq == Some(s as i64)
                && q.hash.as_deref() == Some(h.to_string().as_str())
        }
        None => q.device.is_none(),
    };
    Ok(if q.approved && same {
        Gate::Approved
    } else {
        Gate::Asking
    })
}

fn counts(conn: &Connection, lib: &Library, since: i64) -> Result<Counts> {
    let id = lib.id.to_string();
    let (deleted, added, changed): (Option<i64>, Option<i64>, Option<i64>) = conn.query_row(
        "SELECT SUM(kind = 'notes' AND what = 'deleted'), SUM(kind = 'notes' AND what = 'added'),
                SUM(kind IN ('notes', 'settings') AND what = 'changed')
         FROM sync_counted WHERE library = ?1 AND at > ?2 AND approved = 0",
        params![id, since],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let place_books: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT book) FROM sync_counted
         WHERE library = ?1 AND at > ?2 AND approved = 0 AND kind = 'places'",
        params![id, since],
        |r| r.get(0),
    )?;
    let book_bytes: Option<i64> = conn.query_row(
        "SELECT MAX(n) FROM (SELECT SUM(bytes) AS n FROM sync_counted
         WHERE library = ?1 AND at > ?2 AND approved = 0 AND kind = 'notes'
           AND what IN ('added', 'changed')
         GROUP BY book)",
        params![id, since],
        |r| r.get(0),
    )?;
    Ok(Counts {
        deleted: deleted.unwrap_or(0),
        added: added.unwrap_or(0),
        changed: changed.unwrap_or(0),
        place_books,
        book_bytes: book_bytes.unwrap_or(0),
    })
}

/// SY26: what was counted, if it calls for the question: over 24 hours, or in total
/// since the reader last answered (asked at most once a week). The `bool` says the
/// totals called for it.
pub(crate) fn over(
    conn: &Connection,
    lib: &Library,
    now_ms: i64,
) -> Result<Option<(Counts, bool)>> {
    let q = question(conn, lib)?;
    let since = q.as_ref().and_then(|q| q.answered_at).unwrap_or(0);
    let day = counts(conn, lib, since.max(now_ms - DAY_MS))?;
    if day.over() {
        return Ok(Some((day, false)));
    }
    let weekly = q
        .and_then(|q| q.totals_asked_at)
        .map_or(true, |t| now_ms - t >= WEEK_MS);
    let total = counts(conn, lib, since)?;
    Ok((weekly && total.over()).then_some((total, true)))
}

/// SY26: the question is open. `from` is the change that waits for it (`None` for
/// a pass over pending versions), and `versions` are the versions that would have
/// been applied and were counted: an approval lets through those and no others.
pub(crate) fn ask(
    conn: &Connection,
    lib: &Library,
    from: Option<(DeviceId, u64, Hash32)>,
    versions: &[VersionId],
    counts: &Counts,
    totals: bool,
    now_ms: i64,
) -> Result<()> {
    let counts = serde_json::to_string(counts).expect("counts serialize");
    let versions = serde_json::to_string(versions).expect("versions serialize");
    conn.execute(
        "INSERT INTO sync_question
             (library, asking, approved, asked_at, device, seq, hash, counts, totals_asked_at, versions)
         VALUES (?1, 1, 0, ?2, ?3, ?4, ?5, ?6, CASE WHEN ?7 THEN ?2 END, ?8)
         ON CONFLICT(library) DO UPDATE SET asking = 1, approved = 0, asked_at = ?2, device = ?3,
             seq = ?4, hash = ?5, counts = ?6,
             totals_asked_at = CASE WHEN ?7 THEN ?2 ELSE totals_asked_at END, versions = ?8",
        params![
            lib.id.to_string(),
            now_ms,
            from.map(|(d, _, _)| d.to_string()),
            from.map(|(_, s, _)| s as i64),
            from.map(|(_, _, h)| h.to_string()),
            counts,
            totals,
            versions
        ],
    )?;
    Ok(())
}

/// The versions the open question counted, which an approval lets through.
pub(crate) fn approved_versions(conn: &Connection, lib: &Library) -> Result<HashSet<VersionId>> {
    let versions: Option<Option<String>> = conn
        .query_row(
            "SELECT versions FROM sync_question WHERE library = ?1",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(versions
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default())
}

/// The change the reader let through has been applied: the question is closed.
pub(crate) fn settle(conn: &Connection, lib: &Library) -> Result<()> {
    conn.execute(
        "UPDATE sync_question SET asking = 0, approved = 0 WHERE library = ?1",
        [lib.id.to_string()],
    )?;
    Ok(())
}

/// The reader answers the open question. Returns how many records Stop syncing put
/// back.
pub fn answer(conn: &Connection, lib: &Library, answer: Answer, now_ms: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let since = question(&tx, lib)?.and_then(|q| q.answered_at).unwrap_or(0);
    let mut put_back = 0;
    match answer {
        Answer::Apply => {
            tx.execute(
                "UPDATE sync_question SET approved = 1, answered_at = ?2 WHERE library = ?1 AND asking = 1",
                params![lib.id.to_string(), now_ms],
            )?;
        }
        Answer::StopSyncing => {
            put_back = revert(&tx, lib, since, now_ms)?;
            tx.execute(
                "UPDATE sync_libraries SET state = 'paused' WHERE library = ?1",
                [lib.id.to_string()],
            )?;
            tx.execute(
                "UPDATE sync_question SET asking = 0, approved = 0, answered_at = ?2 WHERE library = ?1",
                params![lib.id.to_string(), now_ms],
            )?;
        }
    }
    tx.commit()?;
    Ok(put_back)
}

/// A version shown: its writer and hash.
pub(crate) type Shown = (DeviceId, Hash32);

/// What the library shows of `key` before a remote version replaces it: the
/// version shown (its writer and hash), and its body, or `None` when it shows
/// nothing (never there, or deleted). This device's unsent edit counts, at
/// sequence `next`. A change is a different version becoming the one shown,
/// whether new or one that lost before and wins now.
pub(crate) fn before(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    next: u64,
) -> Result<(Option<Shown>, Option<Vec<u8>>)> {
    let mut record = journal::record_of(conn, lib, key, Which::Applied)?;
    // This device's own edit not yet sent, held for a choice or not: what the
    // library shows of it here.
    let local: Option<String> = conn
        .query_row(
            "SELECT raw FROM sync_versions WHERE library = ?1 AND key = ?2 AND seq IS NULL",
            params![lib.id.to_string(), key.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(Ok(super::record::Checked::Version(v))) = local
        .as_deref()
        .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
        .map(|v| super::record::check_version(v, lib.device))
    {
        record.add(Placed {
            version: v,
            seq: next,
        });
    }
    let shown = match record.outcome(&HashSet::new()) {
        Some(Outcome::Settled(p)) => Some(p),
        Some(Outcome::Conflict { shown, .. }) => Some(shown),
        None => None,
    };
    let id = shown.map(|p| (p.version.device, p.version.hash));
    // Last opened and finished: what a version sets is a time (its clock), which
    // its body doesn't hold; they are compared by the library's time.
    if let Some(column) = stamp_column(key.kind()) {
        let at: Option<i64> = conn
            .query_row(
                &format!(
                    "SELECT b.{column} FROM sync_books s JOIN books b ON b.id = s.book_id
                     WHERE s.library = ?1 AND s.sync_id = ?2"
                ),
                params![lib.id.to_string(), subject_of(key)],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        return Ok((id, at.map(stamp)));
    }
    let body = match shown.map(|p| &p.version.content) {
        Some(Content::Body { hash, .. }) => journal::body(conn, hash)?,
        _ => None,
    };
    Ok((id, body))
}

/// The library's column for a reading state, which a version sets to its time.
fn stamp_column(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::BookOpened => Some("opened_at"),
        Kind::BookFinished => Some("finished_at"),
        _ => None,
    }
}

fn stamp(at: i64) -> Vec<u8> {
    serde_json::json!({ "at": at }).to_string().into_bytes()
}

fn subject_of(key: &Key) -> &str {
    key.as_str().split(':').nth(1).unwrap_or_default()
}

/// A highlight's row as SY26 counts it: its book, whether it shows, its bytes
/// (CFI, quotes and note, as SY28's per-book total) and whether it has a note.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    book: String,
    live: bool,
    bytes: i64,
    note: bool,
}

/// The row of highlight `key` (an `annotation:` key), if there is one.
pub(crate) fn row(conn: &Connection, key: &Key) -> Result<Option<Row>> {
    if key.kind() != Kind::Annotation {
        return Ok(None);
    }
    Ok(conn
        .query_row(
            "SELECT book_id, deleted_at IS NULL,
                    length(CAST(cfi_range AS BLOB)) + length(CAST(quote_exact AS BLOB))
                    + length(CAST(quote_prefix AS BLOB)) + length(CAST(quote_suffix AS BLOB))
                    + COALESCE(length(CAST(note AS BLOB)), 0),
                    COALESCE(trim(note), '') != ''
             FROM annotations WHERE id = ?1",
            [subject_of(key)],
            |r| {
                Ok(Row {
                    book: r.get(0)?,
                    live: r.get(1)?,
                    bytes: r.get(2)?,
                    note: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// A remote version has just been applied over `before`: the undo journal keeps
/// what it replaced, and the change is counted (unless the reader let it through).
#[allow(clippy::too_many_arguments)]
pub(crate) fn applied(
    conn: &Connection,
    lib: &Library,
    p: &Placed,
    before: Option<Vec<u8>>,
    row_before: Option<Row>,
    approved: bool,
    now_ms: i64,
) -> Result<()> {
    let after = match (&p.version.content, p.version.kind) {
        (Content::Body { .. }, Kind::BookOpened) => Some(stamp(p.version.clock.ms() as i64)),
        (Content::Body { hash, .. }, Kind::BookFinished) => {
            let finished = journal::body(conn, hash)?
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .is_some_and(|b| b["finished"] == true);
            finished.then(|| stamp(p.version.clock.ms() as i64))
        }
        (Content::Body { hash, .. }, _) => journal::body(conn, hash)?,
        _ => None,
    };
    let since = question(conn, lib)?
        .and_then(|q| q.answered_at)
        .unwrap_or(0);
    let what = match (&before, &after) {
        (None, Some(_)) => "added",
        (Some(_), None) => "deleted",
        (Some(b), Some(a)) if b != a => "changed",
        _ => {
            // The same content from another version: nothing to count, but the
            // record's entry moves on to it, so Stop syncing still knows it.
            if !approved {
                conn.execute(
                    "UPDATE sync_undo SET device = ?3, seq = ?4, hash = ?5
                     WHERE library = ?1 AND key = ?2 AND at > ?6",
                    params![
                        lib.id.to_string(),
                        p.version.key.as_str(),
                        p.version.device.to_string(),
                        p.seq as i64,
                        p.version.hash.to_string(),
                        since
                    ],
                )?;
            }
            return Ok(());
        }
    };
    let subject = p.version.key.as_str().split(':').nth(1).unwrap_or_default();
    let (kind, book): (&str, Option<String>) = match p.version.kind {
        Kind::Annotation | Kind::Note => (
            "notes",
            conn.query_row(
                "SELECT book_id FROM annotations WHERE id = ?1",
                [subject],
                |r| r.get(0),
            )
            .optional()?,
        ),
        Kind::Place | Kind::BookOpened | Kind::BookFinished => ("places", Some(subject.to_owned())),
        Kind::Setting | Kind::BookSetting => ("settings", None),
        Kind::Book | Kind::BookFile => ("books", Some(subject.to_owned())),
        Kind::Choice => return Ok(()),
    };
    // A note tombstone is counted with its highlight's deletion when that deletion
    // was counted in this window and the note hasn't changed since (an earlier
    // look at the entries, before this change moves them on).
    let note_counted_with_highlight = p.version.kind == Kind::Note && {
        let last = |key: String| -> Result<Option<i64>> {
            Ok(conn.query_row(
                "SELECT MAX(turn) FROM sync_undo WHERE library = ?1 AND key = ?2 AND at > ?3",
                params![lib.id.to_string(), key, since],
                |r| r.get(0),
            )?)
        };
        let highlight = last(format!("annotation:{subject}"))?;
        let note = last(p.version.key.as_str().to_owned())?;
        highlight.is_some_and(|h| note.map_or(true, |n| n < h))
    };
    // Since the last answer, one entry a record: what it was before the first
    // change (kept), and the last change (moved on). So the journal holds what Stop
    // syncing needs however long the counting runs, bounded by the records changed.
    // What the reader let through is kept as of the answer, outside the window Stop
    // syncing puts back; anything else strictly after it, even in the same
    // millisecond.
    let at = if approved {
        since
    } else {
        now_ms.max(since + 1)
    };
    // Each insert or move also takes the next turn, so two changes in one
    // millisecond still tell which came first.
    let turn: i64 = conn.query_row(
        "SELECT COALESCE(MAX(turn), 0) + 1 FROM sync_undo WHERE library = ?1",
        [lib.id.to_string()],
        |r| r.get(0),
    )?;
    let moved = if approved {
        0
    } else {
        conn.execute(
            "UPDATE sync_undo SET device = ?3, seq = ?4, hash = ?5, at = ?6, turn = ?8
         WHERE library = ?1 AND key = ?2 AND at > ?7",
            params![
                lib.id.to_string(),
                p.version.key.as_str(),
                p.version.device.to_string(),
                p.seq as i64,
                p.version.hash.to_string(),
                at,
                since,
                turn
            ],
        )?
    };
    if moved == 0 {
        conn.execute(
            "INSERT INTO sync_undo (library, key, device, seq, hash, before, at, turn)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                lib.id.to_string(),
                p.version.key.as_str(),
                p.version.device.to_string(),
                p.seq as i64,
                p.version.hash.to_string(),
                before,
                at,
                turn
            ],
        )?;
    }
    // Places count by book: one row a book in the day is enough, moved on to the
    // latest change so the book stays in the rolling 24 hours as long as it changes.
    if kind == "places" {
        let moved = conn.execute(
            "UPDATE sync_counted SET at = ?5
             WHERE library = ?1 AND kind = 'places' AND book = ?2 AND at > ?3 AND approved = ?4",
            params![
                lib.id.to_string(),
                book,
                since.max(now_ms - DAY_MS),
                approved,
                at
            ],
        )?;
        if moved > 0 {
            return Ok(());
        }
    }
    // Bytes a record brings: all of an added one, what a rewrite grows by.
    let mut bytes = match (what, &before, &after) {
        ("added", _, Some(a)) => a.len() as i64,
        ("changed", Some(b), Some(a)) => (a.len() as i64 - b.len() as i64).max(0),
        _ => 0,
    };
    let (mut what, mut book, mut items) = (what, book, 1);
    // A highlight counts by what its row does in the library: one that appears,
    // disappears or moves to another book takes its note with it (counted too),
    // and brings its bytes, note included, to the book it shows in.
    if p.version.kind == Kind::Annotation {
        let row_after = row(conn, &p.version.key)?;
        let note = |r: &Row| if r.note { 2 } else { 1 };
        match (row_before.filter(|r| r.live), row_after.filter(|r| r.live)) {
            (None, Some(a)) => {
                (what, items, bytes, book) = ("added", note(&a), a.bytes, Some(a.book))
            }
            (Some(b), None) => (what, items, bytes, book) = ("deleted", note(&b), 0, Some(b.book)),
            (Some(b), Some(a)) if b.book != a.book => {
                (what, items, bytes, book) = ("changed", note(&a), a.bytes, Some(a.book))
            }
            (Some(b), Some(a)) => {
                (what, items, bytes, book) =
                    ("changed", 1, (a.bytes - b.bytes).max(0), Some(a.book))
            }
            (None, None) => return Ok(()),
        }
    }
    // A note tombstone for a highlight whose deletion was counted in this window
    // was counted with it; one hidden by a deletion before the last answer counts.
    if p.version.kind == Kind::Note && what == "deleted" {
        let hidden: Option<bool> = conn
            .query_row(
                "SELECT deleted_at IS NOT NULL FROM annotations WHERE id = ?1",
                [subject],
                |r| r.get(0),
            )
            .optional()?;
        if hidden == Some(true) && note_counted_with_highlight {
            return Ok(());
        }
    }
    // One row an item; the record's bytes go on the first only.
    for i in 0..items {
        let bytes = if i == 0 { bytes } else { 0 };
        conn.execute(
            "INSERT INTO sync_counted (library, at, what, kind, book, bytes, approved)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![lib.id.to_string(), at, what, kind, book, bytes, approved],
        )?;
    }
    Ok(())
}

/// SY26: the reader changed `key` on this device. What other devices changed of it
/// before isn't put back by Stop syncing (the reader's edit stays), so its undo
/// entry ends; a later remote change starts a new one, from the reader's edit.
pub(crate) fn changed_here(conn: &Connection, lib: &Library, key: &Key) -> Result<()> {
    conn.execute(
        "DELETE FROM sync_undo WHERE library = ?1 AND key = ?2",
        params![lib.id.to_string(), key.as_str()],
    )?;
    Ok(())
}

/// SY26: the undo journal keeps a week, and everything since the last answer (the
/// counting that hasn't been answered yet); what was counted is needed only since
/// the last answer.
pub(crate) fn prune(conn: &Connection, lib: &Library, now_ms: i64) -> Result<()> {
    let since = question(conn, lib)?
        .and_then(|q| q.answered_at)
        .unwrap_or(0);
    conn.execute(
        "DELETE FROM sync_undo WHERE library = ?1 AND at < ?2 AND at <= ?3",
        params![lib.id.to_string(), now_ms - WEEK_MS, since],
    )?;
    conn.execute(
        "DELETE FROM sync_counted WHERE library = ?1 AND at <= ?2",
        params![lib.id.to_string(), since],
    )?;
    Ok(())
}

/// SY26's Stop syncing: every record changed by another device since the last
/// answer goes back to what it was before the first of those changes, as a new
/// version descending from the one undone, unless the reader changed it here since.
fn revert(conn: &Connection, lib: &Library, since: i64, now_ms: i64) -> Result<usize> {
    struct Entry {
        before: Option<Vec<u8>>,
        device: String,
        hash: String,
    }
    let rows: Vec<(String, String, String, Option<Vec<u8>>)> = conn
        .prepare(
            "SELECT key, device, hash, before FROM sync_undo
             WHERE library = ?1 AND at > ?2 ORDER BY rowid",
        )?
        .query_map(params![lib.id.to_string(), since], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    // For each key: what was there before the first change, and the last change.
    let mut keys: BTreeMap<String, Entry> = BTreeMap::new();
    for (key, device, hash, before) in rows {
        keys.entry(key)
            .and_modify(|e| {
                e.device = device.clone();
                e.hash = hash.clone();
            })
            .or_insert(Entry {
                before,
                device,
                hash,
            });
    }
    let mut n = 0;
    for (key, e) in keys {
        let Some((key, _)) = Key::parse(&key) else {
            continue;
        };
        // Only while the library still shows that change, and the reader hasn't
        // changed the record here since (an unsent or held edit, or a later one).
        let record = journal::record_of(conn, lib, &key, Which::Applied)?;
        let shown = match record.outcome(&HashSet::new()) {
            Some(Outcome::Settled(p)) => p,
            Some(Outcome::Conflict { shown, .. }) => shown,
            None => continue,
        };
        // An edit the reader made after the change ended its entry (`changed_here`),
        // so an unsent or held edit here came before it, and is what goes back.
        if shown.version.device.to_string() != e.device || shown.version.hash.to_string() != e.hash
        {
            continue;
        }
        // A reading state is put back as a new version of its state (opened;
        // finished or not), descending from the one undone. Its time is the
        // version's own, so the earlier time is put back in this library after it;
        // other devices get the state with the restoration's time.
        let stamp_at = stamp_column(key.kind()).map(|_| {
            e.before
                .as_deref()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
                .and_then(|v| v["at"].as_i64())
        });
        let content = match (key.kind(), &e.before) {
            (Kind::BookOpened, Some(_)) => LocalContent::Body(b"{}".to_vec()),
            (Kind::BookFinished, before) => LocalContent::Body(
                serde_json::json!({ "finished": before.is_some() })
                    .to_string()
                    .into_bytes(),
            ),
            (_, Some(body)) => LocalContent::Body(body.clone()),
            (_, None) => LocalContent::Deleted {
                covers: super::local::covers(conn, lib)?,
                identity: None,
            },
        };
        // Where this device's own version waits hidden in a conflict, Stop syncing
        // is the reader's decision that settles it: the version put back descends
        // from every head, as a choice does, and is sent like any other.
        let wall = now_ms.max(0) as u64;
        let put = match journal::record_local(conn, lib, &key, content.clone(), wall) {
            Err(JournalError::Conflicted(_)) => journal::hold_local(conn, lib, &key, content, wall)
                .and_then(|v| journal::release_held(conn, lib, &key).map(|_| v)),
            other => other,
        };
        let v = put?;
        let mut restoring = Tally::default();
        restoring.restoring = true;
        if apply::write(conn, lib, &v, &mut restoring)? {
            n += 1;
        }
        if let (Some(column), Some(at)) = (stamp_column(key.kind()), stamp_at) {
            conn.execute(
                &format!(
                    "UPDATE books SET {column} = ?2 WHERE id IN
                     (SELECT book_id FROM sync_books WHERE library = ?1 AND sync_id = ?3)"
                ),
                params![lib.id.to_string(), at, subject_of(&key)],
            )?;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::apply::tests::{device, highlight, publish_next, ANNO};
    use crate::sync::apply::{apply_pending, receive, Batch, Received};
    use crate::sync::ids::Id16;

    const HOUR: i64 = 60 * 60 * 1000;

    /// `n` highlights with notes, saved on `s`.
    fn highlights(s: &mut Store, book: &str, n: usize) -> Vec<String> {
        (0..n)
            .map(|i| {
                let mut h = highlight(book, Some(&format!("note {i}")), 1_760_000_001_000);
                h.id = uuid::Uuid::new_v4().to_string();
                h.quote_exact = format!("q{i}");
                s.save_annotation(&h).unwrap();
                h.id
            })
            .collect()
    }

    /// Receives `batch`, letting it through if the reader is asked.
    fn let_through(s: &Store, lib: &Library, batch: &Batch, now: i64) {
        if receive(s.conn(), lib, batch, now).unwrap() == Received::Asks {
            answer(s.conn(), lib, Answer::Apply, now).unwrap();
            assert!(matches!(
                receive(s.conn(), lib, batch, now).unwrap(),
                Received::Applied { .. }
            ));
        }
    }

    /// A's `n` highlights (with notes), applied on B.
    fn shared(n: usize, now: i64) -> (Store, Library, Store, Library, Vec<String>) {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let ids = highlights(&mut a, "book-a", n);
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        let_through(&b, &lb, &batch, now);
        assert_eq!(b.annotations("book-b").unwrap().len(), n);
        // As if the reader had last answered then: the counting starts afresh.
        b.conn()
            .execute(
                "INSERT INTO sync_question (library, asking, approved, answered_at) VALUES (?1, 0, 0, ?2)
                 ON CONFLICT(library) DO UPDATE SET answered_at = ?2",
                params![lb.id.to_string(), now],
            )
            .unwrap();
        (a, la, b, lb, ids)
    }

    fn delete(a: &mut Store, la: &Library, ids: &[String]) -> Batch {
        for id in ids {
            a.delete_annotation(id).unwrap();
        }
        publish_next(a.conn(), la).unwrap().unwrap()
    }

    #[test]
    fn many_deletions_in_one_change_wait_for_the_reader() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(30, now);
        let later = now + HOUR;
        // 30 highlights and their 30 notes: 60 deleted, past 50.
        let batch = delete(&mut a, &la, &ids);
        assert_eq!(
            receive(b.conn(), &lb, &batch, later).unwrap(),
            Received::Asks
        );
        assert_eq!(
            b.annotations("book-b").unwrap().len(),
            30,
            "nothing applied"
        );
        assert_eq!(asking(b.conn(), &lb).unwrap().map(|c| c.deleted), Some(60));
        // Nothing else is applied meanwhile, from any device.
        assert_eq!(apply_pending(b.conn(), &lb, later).unwrap(), 0);
        answer(b.conn(), &lb, Answer::Apply, later).unwrap();
        assert!(matches!(
            receive(b.conn(), &lb, &batch, later + 1).unwrap(),
            Received::Applied { .. }
        ));
        assert!(b.annotations("book-b").unwrap().is_empty());
        assert!(asking(b.conn(), &lb).unwrap().is_none(), "answered");
    }

    #[test]
    fn deletions_paced_across_changes_are_counted_together() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(40, now);
        // 20 + 20 notes each time: 40, then 80 in the day.
        let first = delete(&mut a, &la, &ids[..20]);
        assert!(matches!(
            receive(b.conn(), &lb, &first, now + HOUR).unwrap(),
            Received::Applied { .. }
        ));
        let second = delete(&mut a, &la, &ids[20..]);
        assert_eq!(
            receive(b.conn(), &lb, &second, now + 2 * HOUR).unwrap(),
            Received::Asks
        );
    }

    #[test]
    fn totals_since_the_last_answer_are_counted_past_a_day() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(40, now);
        let first = delete(&mut a, &la, &ids[..20]);
        receive(b.conn(), &lb, &first, now + HOUR).unwrap();
        // Two days later: 40 in the day, but 80 since the reader last answered.
        let second = delete(&mut a, &la, &ids[20..]);
        assert_eq!(
            receive(b.conn(), &lb, &second, now + 50 * HOUR).unwrap(),
            Received::Asks
        );
    }

    #[test]
    fn stop_syncing_puts_back_what_other_devices_changed() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(40, now);
        let first = delete(&mut a, &la, &ids[..20]);
        receive(b.conn(), &lb, &first, now + HOUR).unwrap();
        assert_eq!(b.annotations("book-b").unwrap().len(), 20);
        let second = delete(&mut a, &la, &ids[20..]);
        assert_eq!(
            receive(b.conn(), &lb, &second, now + 2 * HOUR).unwrap(),
            Received::Asks
        );
        let back = answer(b.conn(), &lb, Answer::StopSyncing, now + 3 * HOUR).unwrap();
        // 20 highlights and their 20 notes, as they were, text and all.
        assert_eq!(back, 40);
        let rows = b.annotations("book-b").unwrap();
        assert_eq!(rows.len(), 40);
        assert!(rows
            .iter()
            .all(|r| r.note.as_deref().is_some_and(|n| n.starts_with("note "))));
        assert!(paused(b.conn(), &lb).unwrap());
        assert_eq!(
            receive(b.conn(), &lb, &second, now + 3 * HOUR).unwrap(),
            Received::Paused
        );
        // Put back as this device's own versions, sent when the reader resumes.
        assert!(journal::unsent(b.conn(), &lb).unwrap().len() >= 40);
    }

    #[test]
    fn stop_syncing_leaves_what_the_reader_changed_since() {
        let now = crate::store::now_ms();
        let (mut a, la, mut b, lb, ids) = shared(30, now);
        // A rewrites one note; B's reader edits it again afterwards.
        let mut h = highlight("book-a", Some("A's rewrite"), 1_760_000_002_000);
        h.id = ids[0].clone();
        h.quote_exact = "q0".into();
        a.save_annotation(&h).unwrap();
        let rewrite = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &rewrite, now + HOUR).unwrap();
        let mut mine = h.clone();
        mine.book_id = "book-b".into();
        mine.note = Some("the reader's own".into());
        b.save_annotation(&mine).unwrap();
        let batch = delete(&mut a, &la, &ids[1..]);
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + 2 * HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::StopSyncing, now + 3 * HOUR).unwrap();
        let row = b
            .annotations("book-b")
            .unwrap()
            .into_iter()
            .find(|r| r.id == ids[0])
            .unwrap();
        assert_eq!(row.note.as_deref(), Some("the reader's own"));
    }

    #[test]
    fn the_undo_journal_keeps_a_week_and_what_no_answer_has_covered() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(10, now);
        let batch = delete(&mut a, &la, &ids[..5]);
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        let rows = |s: &Store| -> i64 {
            s.conn()
                .query_row("SELECT COUNT(*) FROM sync_undo", [], |r| r.get(0))
                .unwrap()
        };
        // 10 highlights and notes added (answered), then 5 of each deleted.
        assert_eq!(rows(&b), 30);
        prune(b.conn(), &lb, now + 6 * DAY_MS).unwrap();
        assert_eq!(rows(&b), 30, "a week");
        // Past the week, what was changed since the last answer stays: the counting
        // that hasn't been answered may still call for Stop syncing.
        prune(b.conn(), &lb, now + 8 * DAY_MS).unwrap();
        assert_eq!(rows(&b), 10);
        b.conn()
            .execute(
                "UPDATE sync_question SET answered_at = ?1",
                [now + 8 * DAY_MS],
            )
            .unwrap();
        prune(b.conn(), &lb, now + 8 * DAY_MS).unwrap();
        assert_eq!(rows(&b), 0);
    }

    #[test]
    fn one_undo_entry_a_record_since_the_last_answer() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        for (i, text) in ["second", "third"].iter().enumerate() {
            let mut h = highlight("book-a", Some(text), 1_760_000_002_000 + i as i64);
            h.id = ids[0].clone();
            h.quote_exact = "q0".into();
            a.save_annotation(&h).unwrap();
            let batch = publish_next(a.conn(), &la).unwrap().unwrap();
            receive(b.conn(), &lb, &batch, now + (i as i64 + 1) * HOUR).unwrap();
        }
        let before: Vec<Option<Vec<u8>>> = b
            .conn()
            .prepare("SELECT before FROM sync_undo WHERE key LIKE 'note:%' AND at > ?1")
            .unwrap()
            .query_map([now], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(before.len(), 1, "merged");
        assert_eq!(
            before[0].as_deref(),
            Some(&br#"{"text":"note 0"}"#[..]),
            "what it was first"
        );
    }

    #[test]
    fn a_books_place_is_counted_once_a_day() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, _) = shared(1, now);
        for i in 0..5 {
            a.save_position("book-a", &format!("epubcfi(/6/{})", 2 * i + 2), 0.1, None)
                .unwrap();
            let batch = publish_next(a.conn(), &la).unwrap().unwrap();
            receive(b.conn(), &lb, &batch, now + (i + 1) * 60_000).unwrap();
        }
        let rows: i64 = b
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_counted WHERE kind = 'places' AND at > ?1",
                [now],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn an_approved_pass_applies_only_what_the_reader_was_asked_about() {
        let now = crate::store::now_ms();
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        // B doesn't have the book yet: A's 110 highlights and notes wait.
        b.conn().execute("DELETE FROM sync_books", []).unwrap();
        highlights(&mut a, "book-a", 110);
        let mut batch = publish_next(a.conn(), &la).unwrap().unwrap();
        // One note's body comes late.
        let at = batch
            .bodies
            .iter()
            .position(|b| b.starts_with(br#"{"text""#))
            .unwrap();
        let late = batch.bodies.remove(at);
        assert!(matches!(
            receive(b.conn(), &lb, &batch, now).unwrap(),
            Received::Applied { applied: 0, .. }
        ));
        // A place written more than a day ahead (it waits for the clock) comes too.
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let mut ahead = publish_next(a.conn(), &la).unwrap().unwrap();
        for v in &mut ahead.versions {
            v["clock"] = serde_json::json!([now as u64 + 30 * HOUR as u64, 0]);
        }
        receive(b.conn(), &lb, &ahead, now).unwrap();
        // The book arrives: 219 records would apply, past 200 added.
        b.conn()
            .execute(
                "INSERT INTO sync_books (library, sync_id, book_id) VALUES (?1, ?2, 'book-b')",
                params![lb.id.to_string(), crate::sync::apply::tests::SYNC_BOOK],
            )
            .unwrap();
        assert_eq!(apply_pending(b.conn(), &lb, now + HOUR).unwrap(), 0);
        assert_eq!(asking(b.conn(), &lb).unwrap().map(|c| c.added), Some(219));
        // No body arrives meanwhile to make more ready.
        assert!(!crate::sync::apply::receive_body(b.conn(), &lb, &late).unwrap());
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        // Two days on, the place is due, but it wasn't in the question.
        assert_eq!(apply_pending(b.conn(), &lb, now + 2 * DAY_MS).unwrap(), 219);
        assert!(b.position("book-b").unwrap().is_none());
        assert!(asking(b.conn(), &lb).unwrap().is_none());
        assert!(crate::sync::apply::receive_body(b.conn(), &lb, &late).unwrap());
        apply_pending(b.conn(), &lb, now + 2 * DAY_MS).unwrap();
        assert!(
            b.position("book-b").unwrap().is_some(),
            "it applies in the next pass"
        );
    }

    #[test]
    fn a_change_the_reader_let_through_isnt_counted_against_the_next() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(40, now);
        let batch = delete(&mut a, &la, &ids[..30]);
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR + 1).unwrap();
        let next = delete(&mut a, &la, &ids[30..]);
        assert!(matches!(
            receive(b.conn(), &lb, &next, now + 2 * HOUR).unwrap(),
            Received::Applied { .. }
        ));
    }

    #[test]
    fn many_notes_rewritten_wait_for_the_reader() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(60, now);
        for (i, id) in ids.iter().enumerate().take(51) {
            let mut h = highlight("book-a", Some(&format!("rewritten {i}")), 1_760_000_002_000);
            h.id = id.clone();
            h.quote_exact = format!("q{i}");
            a.save_annotation(&h).unwrap();
        }
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Asks
        );
        assert_eq!(asking(b.conn(), &lb).unwrap().map(|c| c.changed), Some(51));
    }

    #[test]
    fn highlights_added_to_one_book_are_counted_by_their_bytes() {
        let (b, lb) = device(2, "book-b");
        let row = |book: &str, bytes: i64| {
            b.conn()
                .execute(
                    "INSERT INTO sync_counted (library, at, what, kind, book, bytes, approved)
                     VALUES (?1, 10, 'added', 'notes', ?2, ?3, 0)",
                    params![lb.id.to_string(), book, bytes],
                )
                .unwrap();
        };
        // 30 MB to each of two books: neither is past 50 MB.
        row("x", 30 << 20);
        row("y", 30 << 20);
        assert_eq!(over(b.conn(), &lb, 20).unwrap(), None);
        row("x", 30 << 20);
        let (counts, _) = over(b.conn(), &lb, 20).unwrap().unwrap();
        assert_eq!(counts.book_bytes, 60 << 20);
    }

    #[test]
    fn places_of_many_books_wait_for_the_reader() {
        let now = crate::store::now_ms();
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        // 21 more books, matched on both devices.
        for i in 0..21 {
            let sync_id = uuid::Uuid::new_v4().to_string();
            for (s, lib, n) in [(&a, &la, 1), (&b, &lb, 2)] {
                let book = format!("book-{n}-{i}");
                s.conn()
                    .execute(
                        "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                         VALUES (?1, ?2, 'x.epub', 'T', 'package', 0)",
                        params![book, format!("h{n}-{i}")],
                    )
                    .unwrap();
                s.conn()
                    .execute(
                        "INSERT INTO sync_books (library, sync_id, book_id) VALUES (?1, ?2, ?3)",
                        params![lib.id.to_string(), sync_id, book],
                    )
                    .unwrap();
            }
        }
        for i in 0..21 {
            a.save_position(&format!("book-1-{i}"), "epubcfi(/6/2)", 0.1, None)
                .unwrap();
        }
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Asks
        );
        assert_eq!(
            asking(b.conn(), &lb).unwrap().map(|c| c.place_books),
            Some(21)
        );
    }

    #[test]
    fn the_totals_are_asked_about_at_most_once_a_week() {
        let (b, lb) = device(2, "book-b");
        let deleted = |at: i64, n: usize| {
            for _ in 0..n {
                b.conn()
                    .execute(
                        "INSERT INTO sync_counted (library, at, what, kind, book, bytes, approved)
                         VALUES (?1, ?2, 'deleted', 'notes', NULL, 0, 0)",
                        params![lb.id.to_string(), at],
                    )
                    .unwrap();
            }
        };
        // 30 a day apart: never 50 in a day, but 60 in all.
        deleted(10, 30);
        deleted(2 * DAY_MS, 30);
        let (counts, totals) = over(b.conn(), &lb, 2 * DAY_MS + 1).unwrap().unwrap();
        assert!(totals);
        assert_eq!(counts.deleted, 60);
        ask(b.conn(), &lb, None, &[], &counts, totals, 2 * DAY_MS + 1).unwrap();
        settle(b.conn(), &lb).unwrap(); // as if answered, without resetting the count
        deleted(4 * DAY_MS, 1);
        assert_eq!(
            over(b.conn(), &lb, 4 * DAY_MS + 1).unwrap(),
            None,
            "asked this week"
        );
        assert!(over(b.conn(), &lb, 10 * DAY_MS)
            .unwrap()
            .is_some_and(|(_, t)| t));
    }

    #[test]
    fn stop_syncing_after_an_approval_leaves_what_was_approved() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(40, now);
        let batch = delete(&mut a, &la, &ids[..30]);
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        receive(b.conn(), &lb, &batch, now + 2 * HOUR).unwrap();
        let more = delete(&mut a, &la, &ids[30..]);
        receive(b.conn(), &lb, &more, now + 3 * HOUR).unwrap();
        assert!(b.annotations("book-b").unwrap().is_empty());
        // Stop syncing puts back the 10 since the answer, not the 30 approved.
        answer(b.conn(), &lb, Answer::StopSyncing, now + 4 * HOUR).unwrap();
        assert_eq!(b.annotations("book-b").unwrap().len(), 10);
    }

    #[test]
    fn places_rewritten_again_after_an_approval_are_counted_again() {
        let now = crate::store::now_ms();
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let mut books = Vec::new();
        for i in 0..21 {
            let sync_id = uuid::Uuid::new_v4().to_string();
            for (s, lib, n) in [(&a, &la, 1), (&b, &lb, 2)] {
                let book = format!("book-{n}-{i}");
                s.conn()
                    .execute(
                        "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                         VALUES (?1, ?2, 'x.epub', 'T', 'package', 0)",
                        params![book, format!("h{n}-{i}")],
                    )
                    .unwrap();
                s.conn()
                    .execute(
                        "INSERT INTO sync_books (library, sync_id, book_id) VALUES (?1, ?2, ?3)",
                        params![lib.id.to_string(), sync_id, book],
                    )
                    .unwrap();
            }
            books.push(format!("book-1-{i}"));
        }
        let places = |a: &mut Store, cfi: &str| {
            for book in &books {
                a.save_position(book, cfi, 0.1, None).unwrap();
            }
        };
        places(&mut a, "epubcfi(/6/2)");
        let first = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &first, now + HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        receive(b.conn(), &lb, &first, now + 2 * HOUR).unwrap();
        places(&mut a, "epubcfi(/6/4)");
        let second = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &second, now + 3 * HOUR).unwrap(),
            Received::Asks
        );
    }

    #[test]
    fn what_resolves_with_an_approved_change_is_counted() {
        let (b, lb, a2, _) = resolving_on_retry(1);
        let now = crate::store::now_ms();
        assert_eq!(
            receive(b.conn(), &lb, &a2, now + HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        // Two days on, C's note is due and resolves with A's change.
        assert!(matches!(
            receive(b.conn(), &lb, &a2, now + 2 * DAY_MS).unwrap(),
            Received::Applied { .. }
        ));
        let counted: i64 = b
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_counted WHERE approved = 0 AND at > ?1",
                [now + HOUR],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            counted, 1,
            "C's note wasn't in the question, so it is counted"
        );
    }

    #[test]
    fn a_second_question_lets_through_what_resolved_on_the_retry() {
        let (b, lb, a2, ids) = resolving_on_retry(51);
        let now = crate::store::now_ms();
        assert_eq!(
            receive(b.conn(), &lb, &a2, now + HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        // C's 51 rewrites resolve on the retry: past 50, asked again.
        let later = now + 2 * DAY_MS;
        assert_eq!(receive(b.conn(), &lb, &a2, later).unwrap(), Received::Asks);
        assert_eq!(asking(b.conn(), &lb).unwrap().map(|c| c.changed), Some(51));
        answer(b.conn(), &lb, Answer::Apply, later).unwrap();
        assert!(matches!(
            receive(b.conn(), &lb, &a2, later + 1).unwrap(),
            Received::Applied { .. }
        ));
        let rows = b.annotations("book-b").unwrap();
        let rewritten = rows
            .iter()
            .filter(|r| ids.contains(&r.id) && r.note.as_deref() == Some("C's"))
            .count();
        assert_eq!(rewritten, 51);
    }

    /// A's 30 deletions (past 50 records) reach B after C's rewrite of `n` other
    /// notes, written more than a day ahead, which descend from A's change: those
    /// resolve only once both A's change is applied and their clock is due.
    /// Returns B, A's change and the rewritten highlights.
    fn resolving_on_retry(n: usize) -> (Store, Library, Batch, Vec<String>) {
        let now = crate::store::now_ms();
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let (mut c, lc) = device(3, "book-c");
        let ids = highlights(&mut a, "book-a", 30 + n);
        let a1 = publish_next(a.conn(), &la).unwrap().unwrap();
        let_through(&b, &lb, &a1, now);
        let_through(&c, &lc, &a1, now);
        b.conn()
            .execute("UPDATE sync_question SET answered_at = ?1", [now])
            .unwrap();
        // A also touches the notes C will rewrite, so C's rewrites descend from A's
        // change.
        for (i, id) in ids[30..].iter().enumerate() {
            let mut h = highlight("book-a", Some(&format!("A's {i}")), 1_760_000_002_000);
            h.id = id.clone();
            h.quote_exact = format!("q{}", 30 + i);
            a.save_annotation(&h).unwrap();
        }
        let a2 = delete(&mut a, &la, &ids[..30]);
        let_through(&c, &lc, &a2, now);
        for (i, id) in ids[30..].iter().enumerate() {
            let mut h = highlight("book-c", Some("C's"), 1_760_000_003_000);
            h.id = id.clone();
            h.quote_exact = format!("q{}", 30 + i);
            c.save_annotation(&h).unwrap();
        }
        let mut c1 = publish_next(c.conn(), &lc).unwrap().unwrap();
        for v in &mut c1.versions {
            v["clock"] = serde_json::json!([now as u64 + 30 * HOUR as u64, 0]);
        }
        receive(b.conn(), &lb, &c1, now + HOUR).unwrap();
        let mut rewritten = ids[30..].to_vec();
        rewritten.sort();
        (b, lb, a2, rewritten)
    }

    #[test]
    fn an_approval_is_for_the_change_the_reader_was_shown() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(30, now);
        let batch = delete(&mut a, &la, &ids);
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        // Another change at the same sequence isn't covered by the approval.
        let mut other = batch.clone();
        other.hash = super::super::ids::Hash32([0xee; 32]);
        assert_eq!(
            receive(b.conn(), &lb, &other, now + HOUR).unwrap(),
            Received::Asks
        );
        assert!(matches!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Applied { .. }
        ));
    }

    #[test]
    fn a_change_in_the_answers_millisecond_is_in_the_new_window() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(10, now);
        // Applied in the same millisecond as the last answer.
        let batch = delete(&mut a, &la, &ids);
        receive(b.conn(), &lb, &batch, now).unwrap();
        assert!(b.annotations("book-b").unwrap().is_empty());
        answer(b.conn(), &lb, Answer::StopSyncing, now + 1).unwrap();
        assert_eq!(b.annotations("book-b").unwrap().len(), 10, "put back");
    }

    #[test]
    fn stop_syncing_follows_a_later_version_of_the_same_content() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let a2 = delete(&mut a, &la, &ids);
        receive(b.conn(), &lb, &a2, now + HOUR).unwrap();
        // C deletes the note again, after A: the same content (a tombstone).
        let c = Id16([3; 16]);
        let key = format!("note:{}", ids[0]);
        let (raw, hash): (String, String) = b
            .conn()
            .query_row(
                "SELECT raw, hash FROM sync_versions
                 WHERE key = ?1 AND device = ?2 AND json_extract(raw, '$.deleted') IS NOT NULL",
                params![key, la.device.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let seq: i64 = b
            .conn()
            .query_row(
                "SELECT seq FROM sync_versions WHERE key = ?1 AND hash = ?2",
                params![key, hash],
                |r| r.get(0),
            )
            .unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        v["device"] = serde_json::json!(c.to_string());
        v["clock"] = serde_json::json!([now as u64 + 60_000, 0]);
        v["history"] = serde_json::json!([{ "d": la.device.to_string(), "s": seq, "h": hash }]);
        let c1 = Batch {
            device: c,
            seq: 1,
            hash: super::super::ids::Hash32([3; 32]),
            versions: vec![v],
            bodies: vec![],
        };
        receive(b.conn(), &lb, &c1, now + 2 * HOUR).unwrap();
        answer(b.conn(), &lb, Answer::StopSyncing, now + 3 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(row.note.as_deref(), Some("note 0"), "the note is put back");
    }

    #[test]
    fn what_in_an_approved_change_wasnt_counted_is_counted_on_the_retry() {
        // One change of A's: 30 deletions ready, and 51 rewrites written more than a
        // day ahead. The question counts the deletions only.
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(81, now);
        for (i, id) in ids[30..].iter().enumerate() {
            let mut h = highlight("book-a", Some("rewritten"), 1_760_000_002_000);
            h.id = id.clone();
            h.quote_exact = format!("q{}", 30 + i);
            a.save_annotation(&h).unwrap();
        }
        let mut batch = delete(&mut a, &la, &ids[..30]);
        for v in &mut batch.versions {
            if v.get("body").is_some() {
                v["clock"] = serde_json::json!([now as u64 + 30 * HOUR as u64, 0]);
            }
        }
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Asks
        );
        assert_eq!(asking(b.conn(), &lb).unwrap().map(|c| c.changed), Some(0));
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        // Two days on, the rewrites are due: counted, so asked again.
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + 2 * DAY_MS).unwrap(),
            Received::Asks
        );
        assert_eq!(asking(b.conn(), &lb).unwrap().map(|c| c.changed), Some(51));
    }

    #[test]
    fn stop_syncing_puts_back_notes_whatever_the_book_holds_meanwhile() {
        // Within the window: B's notes on one highlight deleted, another highlight's
        // note grown near the book's cap; putting back the first isn't held back
        // by the second, which is put back too.
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(2, now);
        let mut big = highlight("book-a", Some(&"x".repeat(400 * 1024)), 1_760_000_002_000);
        big.id = ids[1].clone();
        big.quote_exact = "q1".into();
        a.save_annotation(&big).unwrap();
        let mut gone = highlight("book-a", None, 1_760_000_002_000);
        gone.id = ids[0].clone();
        gone.quote_exact = "q0".into();
        a.save_annotation(&gone).unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        // Pretend the book is full, as other devices' content might make it.
        b.conn()
            .execute(
                "UPDATE annotations SET quote_prefix = ?1 WHERE id = ?2",
                params!["p".repeat(120 * 1024), ids[1]],
            )
            .unwrap();
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        let rows = b.annotations("book-b").unwrap();
        let first = rows.iter().find(|r| r.id == ids[0]).unwrap();
        assert_eq!(first.note.as_deref(), Some("note 0"), "put back");
    }

    #[test]
    fn a_highlight_deleted_without_its_note_tombstone_counts_its_note() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(30, now);
        let mut batch = delete(&mut a, &la, &ids);
        // Only the highlights' tombstones: their notes are hidden all the same.
        batch.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        assert_eq!(
            receive(b.conn(), &lb, &batch, now + HOUR).unwrap(),
            Received::Asks
        );
        assert_eq!(asking(b.conn(), &lb).unwrap().map(|c| c.deleted), Some(60));
    }

    #[test]
    fn stop_syncing_keeps_an_edit_the_reader_made_after_a_remote_change() {
        let now = crate::store::now_ms();
        let (mut a, la, mut b, lb, ids) = shared(1, now);
        // A rewrites the note; B's reader edits it and publishes.
        let mut h = highlight("book-a", Some("A's"), 1_760_000_002_000);
        h.id = ids[0].clone();
        h.quote_exact = "q0".into();
        a.save_annotation(&h).unwrap();
        let a2 = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &a2, now + HOUR).unwrap();
        let mut mine = h.clone();
        mine.book_id = "book-b".into();
        mine.note = Some("mine".into());
        b.save_annotation(&mine).unwrap();
        let b1 = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(a.conn(), &la, &b1, now + 2 * HOUR).unwrap();
        // A sends a version with the reader's very text, after it.
        let key = format!("note:{}", ids[0]);
        let (raw, b_seq, b_hash): (String, i64, String) = b
            .conn()
            .query_row(
                "SELECT raw, seq, hash FROM sync_versions WHERE key = ?1 AND device = ?2 AND seq IS NOT NULL",
                params![key, lb.device.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let (a_seq, a_hash): (i64, String) = b
            .conn()
            .query_row(
                "SELECT seq, hash FROM sync_versions WHERE key = ?1 AND device = ?2 ORDER BY seq DESC",
                params![key, la.device.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        v["device"] = serde_json::json!(la.device.to_string());
        v["clock"] = serde_json::json!([now as u64 + 3 * HOUR as u64, 0]);
        v["history"] = serde_json::json!([
            { "d": la.device.to_string(), "s": a_seq, "h": a_hash },
            { "d": lb.device.to_string(), "s": b_seq, "h": b_hash },
        ]);
        let body = journal::body(
            b.conn(),
            &super::super::ids::Hash32::parse(v["body"]["hash"].as_str().unwrap()).unwrap(),
        )
        .unwrap()
        .unwrap();
        let a3 = Batch {
            device: la.device,
            seq: a2.seq + 1,
            hash: super::super::ids::Hash32([0xa3; 32]),
            versions: vec![v],
            bodies: vec![body],
        };
        receive(b.conn(), &lb, &a3, now + 3 * HOUR).unwrap();
        answer(b.conn(), &lb, Answer::StopSyncing, now + 4 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(row.note.as_deref(), Some("mine"));
    }

    #[test]
    fn stop_syncing_puts_back_a_local_place_a_remote_one_replaced() {
        let now = crate::store::now_ms();
        let (mut a, la, mut b, lb, _) = shared(1, now);
        b.save_position("book-b", "epubcfi(/6/8)", 0.4, None)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        assert_eq!(b.position("book-b").unwrap().unwrap().0, "epubcfi(/6/2)");
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        assert_eq!(b.position("book-b").unwrap().unwrap().0, "epubcfi(/6/8)");
    }

    #[test]
    fn books_opened_elsewhere_are_counted_and_put_back() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, _) = shared(1, now);
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let first = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &first, now + HOUR).unwrap();
        // The reader answers after that: the window starts here.
        b.conn()
            .execute(
                "UPDATE sync_question SET answered_at = ?1",
                [now + HOUR + 1],
            )
            .unwrap();
        let opened = |s: &Store| -> Option<i64> {
            s.conn()
                .query_row("SELECT opened_at FROM books WHERE id = 'book-b'", [], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        let was = opened(&b);
        assert!(was.is_some());
        // The same place saved again: only the book's last opened changes.
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let second = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &second, now + 2 * HOUR).unwrap();
        assert_ne!(opened(&b), was);
        let counted: i64 = b
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_counted WHERE kind = 'places' AND at > ?1",
                [now + HOUR + 1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(counted, 1, "the reopening is counted");
        answer(b.conn(), &lb, Answer::StopSyncing, now + 3 * HOUR).unwrap();
        assert_eq!(opened(&b), was, "put back");
        // As a new version of B's own, sent when the reader resumes.
        assert!(journal::unsent(b.conn(), &lb).unwrap().iter().any(|v| v
            .key
            .as_str()
            .starts_with("bookRead:")
            && v.key.as_str().ends_with(":opened")));
    }

    #[test]
    fn a_highlight_restored_elsewhere_doesnt_bring_back_a_note_by_itself() {
        let now = crate::store::now_ms();
        let (mut a, la, mut b, lb, ids) = shared(1, now);
        b.delete_annotation(&ids[0]).unwrap();
        let gone = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(a.conn(), &la, &gone, now + HOUR).unwrap();
        // A brings the highlight back, but sends no note version with it.
        a.restore_annotation(&ids[0]).unwrap();
        let mut back = publish_next(a.conn(), &la).unwrap().unwrap();
        back.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &back, now + 2 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(
            row.note, None,
            "the note kept on B's deleted row stays hidden"
        );
    }

    #[test]
    fn a_book_finished_again_elsewhere_is_counted() {
        let now = crate::store::now_ms();
        let (_a, la, b, lb, _) = shared(1, now);
        let finished = |at: u64, seq: u64| Batch {
            device: la.device,
            seq,
            hash: super::super::ids::Hash32([seq as u8; 32]),
            versions: vec![serde_json::json!({
                "format": 1,
                "key": format!("bookRead:{}:finished", crate::sync::apply::tests::SYNC_BOOK),
                "device": la.device.to_string(), "clock": [at, 0], "history": [],
                "body": { "hash": super::super::ids::identity_hash(br#"{"finished":true}"#).to_string(), "size": 17 },
            })],
            bodies: vec![br#"{"finished":true}"#.to_vec()],
        };
        let first = finished(now as u64 + 1000, 2);
        receive(b.conn(), &lb, &first, now + HOUR).unwrap();
        // The reader answers after that: only the second finish is in the window.
        b.conn()
            .execute(
                "UPDATE sync_question SET answered_at = ?1",
                [now + HOUR + 1],
            )
            .unwrap();
        let mut second = finished(now as u64 + 2000, 3);
        second.versions[0]["history"] = serde_json::json!([{
            "d": la.device.to_string(), "s": 2,
            "h": crate::sync::record::version_hash(&first.versions[0]).to_string(),
        }]);
        receive(b.conn(), &lb, &second, now + 2 * HOUR).unwrap();
        let in_window = |table: &str| -> i64 {
            b.conn()
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE at > ?1"),
                    [now + HOUR + 1],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(in_window("sync_undo"), 1, "journalled");
        assert_eq!(in_window("sync_counted"), 1, "counted");
    }

    #[test]
    fn stop_syncing_brings_back_a_note_an_unpaired_deletion_hid() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let mut batch = delete(&mut a, &la, &ids);
        batch.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        assert!(b.annotations("book-b").unwrap().is_empty());
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(row.note.as_deref(), Some("note 0"));
    }

    #[test]
    fn stop_syncing_clears_a_first_opening_from_elsewhere() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, _) = shared(1, now);
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        let opened = || -> Option<i64> {
            b.conn()
                .query_row("SELECT opened_at FROM books WHERE id = 'book-b'", [], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        assert!(opened().is_some());
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        assert_eq!(opened(), None, "never opened here");
    }

    #[test]
    fn a_first_opening_undone_is_cleared_on_the_other_devices_too() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, _) = shared(1, now);
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        // The reader resumes: B's restoration reaches A.
        let back = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(a.conn(), &la, &back, now + 3 * HOUR).unwrap();
        let opened: Option<i64> = a
            .conn()
            .query_row("SELECT opened_at FROM books WHERE id = 'book-a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(opened, None);
    }

    #[test]
    fn stop_syncing_settles_a_conflict_holding_this_devices_version() {
        let now = crate::store::now_ms();
        let (mut a, la, mut b, lb, ids) = shared(1, now);
        // B edits the note and publishes; apart, A deletes the highlight.
        let mut mine = highlight("book-b", Some("B's edit"), 1_760_000_002_000);
        mine.id = ids[0].clone();
        mine.quote_exact = "q0".into();
        b.save_annotation(&mine).unwrap();
        publish_next(b.conn(), &lb).unwrap().unwrap();
        let gone = delete(&mut a, &la, &ids);
        receive(b.conn(), &lb, &gone, now + HOUR).unwrap();
        assert!(
            b.annotations("book-b").unwrap().is_empty(),
            "deleted until chosen"
        );
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(row.note.as_deref(), Some("B's edit"));
        assert!(!journal::held(
            b.conn(),
            &lb,
            &Key::parse(&format!("note:{}", ids[0])).unwrap().0
        )
        .unwrap());
    }

    #[test]
    fn a_losing_place_that_wins_later_is_counted() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, _) = shared(1, now);
        // A's place is shown at B; X's older place arrives and loses.
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let a2 = publish_next(a.conn(), &la).unwrap().unwrap();
        let place =
            |v: &serde_json::Value| v["key"].as_str().is_some_and(|k| k.starts_with("place:"));
        let shown = a2.versions.iter().find(|v| place(v)).unwrap().clone();
        let x = Id16([5; 16]);
        let lost_body = br#"{"cfi":"epubcfi(/6/40)","fraction":0.9}"#.to_vec();
        let mut lost = shown.clone();
        lost["device"] = serde_json::json!(x.to_string());
        lost["clock"] = serde_json::json!([1_000, 0]);
        lost["history"] = serde_json::json!([]);
        lost["body"] = serde_json::json!({
            "hash": super::super::ids::identity_hash(&lost_body).to_string(), "size": lost_body.len()
        });
        let x1 = Batch {
            device: x,
            seq: 1,
            hash: super::super::ids::Hash32([5; 32]),
            versions: vec![lost.clone()],
            bodies: vec![lost_body],
        };
        receive(b.conn(), &lb, &a2, now + HOUR).unwrap();
        receive(b.conn(), &lb, &x1, now + HOUR).unwrap();
        assert_eq!(b.position("book-b").unwrap().unwrap().0, "epubcfi(/6/2)");
        // A's place is then superseded by an older-clocked descendant: X's wins.
        let mut tomb = shown.clone();
        tomb["clock"] = serde_json::json!([500, 0]);
        tomb["history"] = serde_json::json!([{
            "d": la.device.to_string(), "s": a2.seq,
            "h": crate::sync::record::version_hash(&shown).to_string(),
        }]);
        tomb.as_object_mut().unwrap().remove("body");
        tomb["deleted"] = serde_json::json!({ "covers": [] });
        let a3 = Batch {
            device: la.device,
            seq: a2.seq + 1,
            hash: super::super::ids::Hash32([6; 32]),
            versions: vec![tomb],
            bodies: vec![],
        };
        receive(b.conn(), &lb, &a3, now + 2 * HOUR).unwrap();
        assert_eq!(b.position("book-b").unwrap().unwrap().0, "epubcfi(/6/40)");
        let undo: i64 = b
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_undo WHERE key LIKE 'place:%' AND device = ?1",
                [x.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(undo, 1, "X's place, shown now, is journalled");
    }

    #[test]
    fn a_highlight_restored_elsewhere_takes_the_note_the_library_shows() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        // A's highlight tombstone alone (its note record stays live), then a restore.
        let mut gone = delete(&mut a, &la, &ids);
        gone.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &gone, now + HOUR).unwrap();
        a.restore_annotation(&ids[0]).unwrap();
        let mut back = publish_next(a.conn(), &la).unwrap().unwrap();
        back.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &back, now + 2 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(
            row.note.as_deref(),
            Some("note 0"),
            "the note record is still live"
        );
    }

    #[test]
    fn no_body_is_kept_while_sync_is_paused() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, _) = shared(1, now);
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let mut batch = publish_next(a.conn(), &la).unwrap().unwrap();
        let place = batch
            .bodies
            .iter()
            .position(|x| x.starts_with(br#"{"cfi""#))
            .unwrap();
        let late = batch.bodies.remove(place);
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        assert!(!crate::sync::apply::receive_body(b.conn(), &lb, &late).unwrap());
    }

    #[test]
    fn notes_rewritten_larger_count_their_growth() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let mut h = highlight("book-a", Some(&"x".repeat(100 * 1024)), 1_760_000_002_000);
        h.id = ids[0].clone();
        h.quote_exact = "q0".into();
        a.save_annotation(&h).unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        let grown = counts(b.conn(), &lb, now).unwrap().book_bytes;
        assert!(grown > 100 * 1024 - 100, "{grown}");
    }

    #[test]
    fn a_restored_highlight_keeps_the_readers_unsent_note() {
        let now = crate::store::now_ms();
        let (mut a, la, mut b, lb, ids) = shared(1, now);
        // B's reader edits the note (not sent yet); A's unpaired deletion hides it,
        // then A's restore arrives without a note version.
        let mut mine = highlight("book-b", Some("B's newest"), 1_760_000_002_000);
        mine.id = ids[0].clone();
        mine.quote_exact = "q0".into();
        b.save_annotation(&mine).unwrap();
        let mut gone = delete(&mut a, &la, &ids);
        gone.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &gone, now + HOUR).unwrap();
        a.restore_annotation(&ids[0]).unwrap();
        let mut back = publish_next(a.conn(), &la).unwrap().unwrap();
        back.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &back, now + 2 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(row.note.as_deref(), Some("B's newest"));
    }

    /// What was counted after `at`: (what, rows, bytes).
    fn counted_after(s: &Store, at: i64) -> Vec<(String, i64, i64)> {
        s.conn()
            .prepare(
                "SELECT what, COUNT(*), SUM(bytes) FROM sync_counted WHERE at > ?1 AND kind = 'notes'
                 GROUP BY what ORDER BY what",
            )
            .unwrap()
            .query_map([at], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn a_highlight_restored_with_its_note_counts_both() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let mut gone = delete(&mut a, &la, &ids);
        gone.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &gone, now + HOUR).unwrap();
        a.restore_annotation(&ids[0]).unwrap();
        let mut back = publish_next(a.conn(), &la).unwrap().unwrap();
        back.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &back, now + 2 * HOUR).unwrap();
        let added = counted_after(&b, now + HOUR + 1);
        let (what, rows, bytes) = &added[0];
        assert_eq!(
            (what.as_str(), *rows),
            ("added", 2),
            "the highlight and its note"
        );
        // The row's bytes, note included, once.
        let key = Key::parse(&format!("annotation:{}", ids[0])).unwrap().0;
        assert_eq!(*bytes, row(b.conn(), &key).unwrap().unwrap().bytes);
    }

    #[test]
    fn a_highlight_moved_to_another_book_counts_its_note() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let other = "0b1c2d3e-4f50-4a61-8b72-93a4b5c6d7e8";
        for (s, lib, book) in [(&a, &la, "book-a2"), (&b, &lb, "book-b2")] {
            s.conn()
                .execute(
                    "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                     VALUES (?1, ?1, 'y.epub', 'Typee', 'package', 0)",
                    [book],
                )
                .unwrap();
            s.conn()
                .execute(
                    "INSERT INTO sync_books (library, sync_id, book_id) VALUES (?1, ?2, ?3)",
                    params![lib.id.to_string(), other, book],
                )
                .unwrap();
        }
        let mut h = highlight("book-a2", Some("note 0"), 1_760_000_002_000);
        h.id = ids[0].clone();
        h.quote_exact = "q0".into();
        a.save_annotation(&h).unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        assert_eq!(b.annotations("book-b2").unwrap().len(), 1);
        let changed = counted_after(&b, now);
        let (what, rows, bytes) = &changed[0];
        assert_eq!(
            (what.as_str(), *rows),
            ("changed", 2),
            "the highlight and its note"
        );
        assert!(*bytes > 0, "its bytes come to the new book");
    }

    #[test]
    fn a_place_changed_again_stays_in_the_day() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, _) = shared(1, now);
        for (i, at) in [now + HOUR, now + 23 * HOUR].into_iter().enumerate() {
            a.save_position("book-a", &format!("epubcfi(/6/{})", 2 * i + 2), 0.1, None)
                .unwrap();
            let batch = publish_next(a.conn(), &la).unwrap().unwrap();
            receive(b.conn(), &lb, &batch, at).unwrap();
        }
        // A day after the first change, the book is still in the last 24 hours.
        let day = counts(b.conn(), &lb, now + 25 * HOUR - DAY_MS).unwrap();
        assert_eq!(day.place_books, 1);
    }

    #[test]
    fn a_note_deleted_after_its_highlight_was_answered_counts() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        // A's highlight deletion alone, then the reader answers.
        let mut all = delete(&mut a, &la, &ids);
        let notes: Vec<serde_json::Value> = all
            .versions
            .iter()
            .filter(|v| v["key"].as_str().is_some_and(|k| k.starts_with("note:")))
            .cloned()
            .collect();
        all.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &all, now + HOUR).unwrap();
        b.conn()
            .execute(
                "UPDATE sync_question SET answered_at = ?1",
                [now + HOUR + 1],
            )
            .unwrap();
        // The hidden note's tombstone, in the next window, counts.
        let later = Batch {
            device: la.device,
            seq: all.seq + 1,
            hash: super::super::ids::Hash32([0x77; 32]),
            versions: notes,
            bodies: vec![],
        };
        receive(b.conn(), &lb, &later, now + 2 * HOUR).unwrap();
        let deleted = counted_after(&b, now + HOUR + 1);
        assert_eq!(deleted, vec![("deleted".to_owned(), 1, 0)]);
    }

    #[test]
    fn a_hidden_note_added_again_then_deleted_counts() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let mut gone = delete(&mut a, &la, &ids);
        let note_tomb = gone
            .versions
            .iter()
            .find(|v| v["key"].as_str().is_some_and(|k| k.starts_with("note:")))
            .unwrap()
            .clone();
        gone.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &gone, now + HOUR).unwrap();
        // The hidden note gets a new text, then its tombstone: both after the
        // highlight's deletion, in the same window.
        let text = br#"{"text":"again"}"#.to_vec();
        let key = note_tomb["key"].as_str().unwrap().to_owned();
        let (prev_seq, prev_hash): (i64, String) = b
            .conn()
            .query_row(
                "SELECT seq, hash FROM sync_versions WHERE key = ?1 AND applied >= 1 ORDER BY seq DESC",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let again = serde_json::json!({
            "format": 1, "key": key, "device": la.device.to_string(),
            "clock": [now as u64 + 10 * HOUR as u64 / 10, 0],
            "history": [{ "d": la.device.to_string(), "s": prev_seq, "h": prev_hash }],
            "body": { "hash": super::super::ids::identity_hash(&text).to_string(), "size": text.len() },
        });
        let seq2 = gone.seq + 1;
        let b2 = Batch {
            device: la.device,
            seq: seq2,
            hash: super::super::ids::Hash32([0x71; 32]),
            versions: vec![again.clone()],
            bodies: vec![text],
        };
        receive(b.conn(), &lb, &b2, now + 2 * HOUR).unwrap();
        let mut tomb = again.clone();
        tomb.as_object_mut().unwrap().remove("body");
        tomb["deleted"] = serde_json::json!({ "covers": [] });
        tomb["clock"] = serde_json::json!([now as u64 + 2 * HOUR as u64, 0]);
        tomb["history"] = serde_json::json!([{
            "d": la.device.to_string(), "s": seq2,
            "h": crate::sync::record::version_hash(&again).to_string(),
        }]);
        let b3 = Batch {
            device: la.device,
            seq: seq2 + 1,
            hash: super::super::ids::Hash32([0x72; 32]),
            versions: vec![tomb],
            bodies: vec![],
        };
        receive(b.conn(), &lb, &b3, now + 3 * HOUR).unwrap();
        let deleted: i64 = b
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_counted WHERE what = 'deleted' AND at > ?1",
                [now + 2 * HOUR],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 1, "the note's own deletion, after it came back");
    }

    #[test]
    fn a_hidden_note_added_again_in_the_same_millisecond_counts() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let mut gone = delete(&mut a, &la, &ids);
        let note_tomb = gone
            .versions
            .iter()
            .find(|v| v["key"].as_str().is_some_and(|k| k.starts_with("note:")))
            .unwrap()
            .clone();
        gone.versions.retain(|v| {
            v["key"]
                .as_str()
                .is_some_and(|k| k.starts_with("annotation:"))
        });
        receive(b.conn(), &lb, &gone, now + HOUR).unwrap();
        // The hidden note gets a new text, then its tombstone: both after the
        // highlight's deletion, in the same window.
        let text = br#"{"text":"again"}"#.to_vec();
        let key = note_tomb["key"].as_str().unwrap().to_owned();
        let (prev_seq, prev_hash): (i64, String) = b
            .conn()
            .query_row(
                "SELECT seq, hash FROM sync_versions WHERE key = ?1 AND applied >= 1 ORDER BY seq DESC",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let again = serde_json::json!({
            "format": 1, "key": key, "device": la.device.to_string(),
            "clock": [now as u64 + 10 * HOUR as u64 / 10, 0],
            "history": [{ "d": la.device.to_string(), "s": prev_seq, "h": prev_hash }],
            "body": { "hash": super::super::ids::identity_hash(&text).to_string(), "size": text.len() },
        });
        let seq2 = gone.seq + 1;
        let b2 = Batch {
            device: la.device,
            seq: seq2,
            hash: super::super::ids::Hash32([0x71; 32]),
            versions: vec![again.clone()],
            bodies: vec![text],
        };
        receive(b.conn(), &lb, &b2, now + HOUR).unwrap();
        let mut tomb = again.clone();
        tomb.as_object_mut().unwrap().remove("body");
        tomb["deleted"] = serde_json::json!({ "covers": [] });
        tomb["clock"] = serde_json::json!([now as u64 + 2 * HOUR as u64, 0]);
        tomb["history"] = serde_json::json!([{
            "d": la.device.to_string(), "s": seq2,
            "h": crate::sync::record::version_hash(&again).to_string(),
        }]);
        let b3 = Batch {
            device: la.device,
            seq: seq2 + 1,
            hash: super::super::ids::Hash32([0x72; 32]),
            versions: vec![tomb],
            bodies: vec![],
        };
        receive(b.conn(), &lb, &b3, now + HOUR).unwrap();
        let deleted: i64 = b
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_counted WHERE what = 'deleted' AND at > ?1",
                [now],
                |r| r.get(0),
            )
            .unwrap();
        // The highlight and its note, then the note's own deletion after it came
        // back: all in one millisecond.
        assert_eq!(deleted, 3, "the note's own deletion counts too");
    }

    #[test]
    fn stop_syncing_puts_back_an_unsent_edit_a_deletion_held() {
        let now = crate::store::now_ms();
        let (mut a, la, mut b, lb, ids) = shared(1, now);
        // B's reader edits the note, unsent; apart, A deletes the highlight.
        let mut mine = highlight("book-b", Some("B's unsent"), 1_760_000_002_000);
        mine.id = ids[0].clone();
        mine.quote_exact = "q0".into();
        b.save_annotation(&mine).unwrap();
        let gone = delete(&mut a, &la, &ids);
        receive(b.conn(), &lb, &gone, now + HOUR).unwrap();
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(row.note.as_deref(), Some("B's unsent"));
    }

    #[test]
    fn stop_syncing_puts_back_a_highlights_time_too() {
        let now = crate::store::now_ms();
        let (mut a, la, b, lb, ids) = shared(1, now);
        let mut h = highlight("book-a", Some("note 0"), 1_760_000_009_000);
        h.id = ids[0].clone();
        h.quote_exact = "q0".into();
        h.color = "blue".into();
        a.save_annotation(&h).unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &batch, now + HOUR).unwrap();
        answer(b.conn(), &lb, Answer::StopSyncing, now + 2 * HOUR).unwrap();
        // Back as it was: its colour, and the time its body had (`highlights`).
        let row = &b.annotations("book-b").unwrap()[0];
        assert_eq!(
            (row.color.as_str(), row.updated_at),
            ("yellow", 1_760_000_001_000)
        );
    }

    #[test]
    fn a_change_already_applied_isnt_counted_again() {
        let now = crate::store::now_ms();
        let (_a, _la, b, lb, _ids) = shared(10, now);
        let counted = |s: &Store| -> i64 {
            s.conn()
                .query_row("SELECT COUNT(*) FROM sync_counted", [], |r| r.get(0))
                .unwrap()
        };
        let n = counted(&b);
        apply_pending(b.conn(), &lb, now).unwrap();
        assert_eq!(counted(&b), n);
        let _ = ANNO;
    }
}
