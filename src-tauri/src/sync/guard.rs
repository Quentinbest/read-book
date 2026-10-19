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
use super::ids::DeviceId;
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
    /// The most bytes of highlights and notes added to one book.
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
    answered_at: Option<i64>,
    totals_asked_at: Option<i64>,
}

fn question(conn: &Connection, lib: &Library) -> Result<Option<Question>> {
    Ok(conn
        .query_row(
            "SELECT asking, approved, device, seq, answered_at, totals_asked_at
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

/// Whether a change from `from` (a device's change, or `None` for a pass over
/// pending versions) may be applied now.
pub(crate) fn gate(
    conn: &Connection,
    lib: &Library,
    from: Option<(DeviceId, u64)>,
) -> Result<Gate> {
    let Some(q) = question(conn, lib)? else {
        return Ok(Gate::Open);
    };
    if !q.asking {
        return Ok(Gate::Open);
    }
    let same = match from {
        Some((d, s)) => {
            q.device.as_deref() == Some(d.to_string().as_str()) && q.seq == Some(s as i64)
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
         WHERE library = ?1 AND at > ?2 AND approved = 0 AND kind = 'notes' AND what = 'added'
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

/// SY26: the question is open; `from` is the change that waits for it, or for a
/// pass over pending versions, `keys` are the records it would have applied: an
/// approval lets through those and no others.
pub(crate) fn ask(
    conn: &Connection,
    lib: &Library,
    from: Option<(DeviceId, u64)>,
    keys: &[Key],
    counts: &Counts,
    totals: bool,
    now_ms: i64,
) -> Result<()> {
    let counts = serde_json::to_string(counts).expect("counts serialize");
    let keys = serde_json::to_string(&keys.iter().map(Key::as_str).collect::<Vec<_>>())
        .expect("keys serialize");
    conn.execute(
        "INSERT INTO sync_question (library, asking, approved, asked_at, device, seq, counts, totals_asked_at, keys)
         VALUES (?1, 1, 0, ?2, ?3, ?4, ?5, CASE WHEN ?6 THEN ?2 END, ?7)
         ON CONFLICT(library) DO UPDATE SET asking = 1, approved = 0, asked_at = ?2, device = ?3,
             seq = ?4, counts = ?5,
             totals_asked_at = CASE WHEN ?6 THEN ?2 ELSE totals_asked_at END, keys = ?7",
        params![
            lib.id.to_string(),
            now_ms,
            from.map(|(d, _)| d.to_string()),
            from.map(|(_, s)| s as i64),
            counts,
            totals,
            keys
        ],
    )?;
    Ok(())
}

/// The records an approved pass over pending versions may apply.
pub(crate) fn approved_keys(conn: &Connection, lib: &Library) -> Result<Vec<Key>> {
    let keys: Option<Option<String>> = conn
        .query_row(
            "SELECT keys FROM sync_question WHERE library = ?1",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let keys: Vec<String> = keys
        .flatten()
        .and_then(|k| serde_json::from_str(&k).ok())
        .unwrap_or_default();
    Ok(keys
        .iter()
        .filter_map(|k| Key::parse(k).map(|(k, _)| k))
        .collect())
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

/// What the library shows of `key` before a remote version replaces it: its body,
/// or `None` when it shows nothing (never there, or deleted). This device's unsent
/// edit counts, at sequence `next`.
pub(crate) fn before(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    next: u64,
) -> Result<Option<Vec<u8>>> {
    let mut record = journal::record_of(conn, lib, key, Which::Applied)?;
    if let Some(v) = journal::unsent_of(conn, lib, key)? {
        record.add(Placed {
            version: v,
            seq: next,
        });
    }
    let shown = match record.outcome(&HashSet::new()) {
        Some(Outcome::Settled(p)) => p,
        Some(Outcome::Conflict { shown, .. }) => shown,
        None => return Ok(None),
    };
    match &shown.version.content {
        Content::Body { hash, .. } => journal::body(conn, hash),
        _ => Ok(None),
    }
}

/// Whether a remote version hasn't been applied here yet (so applying it is a
/// change to count, not a repeat).
pub(crate) fn fresh(conn: &Connection, lib: &Library, p: &Placed) -> Result<bool> {
    let applied: Option<i64> = conn
        .query_row(
            "SELECT applied FROM sync_versions WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
            params![
                lib.id.to_string(),
                p.version.device.to_string(),
                p.seq as i64,
                p.version.key.as_str()
            ],
            |r| r.get(0),
        )
        .optional()?;
    Ok(applied == Some(0))
}

/// A remote version has just been applied over `before`: the undo journal keeps
/// what it replaced, and the change is counted (unless the reader let it through).
pub(crate) fn applied(
    conn: &Connection,
    lib: &Library,
    p: &Placed,
    before: Option<Vec<u8>>,
    approved: bool,
    now_ms: i64,
) -> Result<()> {
    let after = match &p.version.content {
        Content::Body { hash, .. } => journal::body(conn, hash)?,
        _ => None,
    };
    let what = match (&before, &after) {
        (None, Some(_)) => "added",
        (Some(_), None) => "deleted",
        (Some(b), Some(a)) if b != a => "changed",
        _ => return Ok(()),
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
    // Since the last answer, one entry a record: what it was before the first
    // change (kept), and the last change (moved on). So the journal holds what Stop
    // syncing needs however long the counting runs, bounded by the records changed.
    // What the reader let through is kept as of the answer, outside the window Stop
    // syncing puts back.
    let since = question(conn, lib)?
        .and_then(|q| q.answered_at)
        .unwrap_or(0);
    let at = if approved { since } else { now_ms };
    let moved = if approved {
        0
    } else {
        conn.execute(
            "UPDATE sync_undo SET device = ?3, seq = ?4, hash = ?5, at = ?6
         WHERE library = ?1 AND key = ?2 AND at > ?7",
            params![
                lib.id.to_string(),
                p.version.key.as_str(),
                p.version.device.to_string(),
                p.seq as i64,
                p.version.hash.to_string(),
                now_ms,
                since
            ],
        )?
    };
    if moved == 0 {
        conn.execute(
            "INSERT INTO sync_undo (library, key, device, seq, hash, before, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                lib.id.to_string(),
                p.version.key.as_str(),
                p.version.device.to_string(),
                p.seq as i64,
                p.version.hash.to_string(),
                before,
                at
            ],
        )?;
    }
    // Places count by book: one row a book a day is enough.
    if kind == "places" {
        let counted: bool = conn
            .query_row(
                "SELECT 1 FROM sync_counted
                 WHERE library = ?1 AND kind = 'places' AND book = ?2 AND at > ?3 AND approved = ?4",
                params![lib.id.to_string(), book, since.max(now_ms - DAY_MS), approved],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if counted {
            return Ok(());
        }
    }
    let bytes = if what == "added" {
        after.map_or(0, |a| a.len() as i64)
    } else {
        0
    };
    conn.execute(
        "INSERT INTO sync_counted (library, at, what, kind, book, bytes, approved)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            lib.id.to_string(),
            now_ms,
            what,
            kind,
            book,
            bytes,
            approved
        ],
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
        if shown.version.device.to_string() != e.device
            || shown.version.hash.to_string() != e.hash
            || journal::unsent_of(conn, lib, &key)?.is_some()
            || journal::held(conn, lib, &key)?
        {
            continue;
        }
        let content = match e.before {
            Some(body) => LocalContent::Body(body),
            None => LocalContent::Deleted {
                covers: super::local::covers(conn, lib)?,
                identity: None,
            },
        };
        match journal::record_local(conn, lib, &key, content, now_ms.max(0) as u64) {
            Ok(v) => {
                if apply::write(conn, lib, &v, &mut Tally::default())? {
                    n += 1;
                }
            }
            Err(JournalError::Conflicted(_)) => continue,
            Err(err) => return Err(err),
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
        let now = crate::store::now_ms();
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let (mut c, lc) = device(3, "book-c");
        let ids = highlights(&mut a, "book-a", 31);
        let a1 = publish_next(a.conn(), &la).unwrap().unwrap();
        let_through(&b, &lb, &a1, now);
        let_through(&c, &lc, &a1, now);
        b.conn()
            .execute("UPDATE sync_question SET answered_at = ?1", [now])
            .unwrap();
        // A deletes 30 (past 50 records) and rewrites the last one's note.
        let mut h = highlight("book-a", Some("A's"), 1_760_000_002_000);
        h.id = ids[30].clone();
        h.quote_exact = "q30".into();
        a.save_annotation(&h).unwrap();
        let a2 = delete(&mut a, &la, &ids[..30]);
        let_through(&c, &lc, &a2, now);
        // C rewrites that note after A: at B it waits for A's change.
        let mut h = highlight("book-c", Some("C's"), 1_760_000_003_000);
        h.id = ids[30].clone();
        h.quote_exact = "q30".into();
        c.save_annotation(&h).unwrap();
        let c1 = publish_next(c.conn(), &lc).unwrap().unwrap();
        receive(b.conn(), &lb, &c1, now + HOUR).unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &a2, now + HOUR).unwrap(),
            Received::Asks
        );
        answer(b.conn(), &lb, Answer::Apply, now + HOUR).unwrap();
        assert!(matches!(
            receive(b.conn(), &lb, &a2, now + 2 * HOUR).unwrap(),
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
            "C's note, which resolved with A's change, is counted"
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
