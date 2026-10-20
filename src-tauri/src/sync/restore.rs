//! SY17, SY19: a deletion's age on this device, and Restore.
//!
//! Each device measures a deletion's age from when it first saw it (SY17): a local
//! deletion when it is made, another device's when it is applied here. Restore is
//! offered while that age is under a week (prov., item 111). It makes a new version
//! descending from the tombstone, holding what the deletion displaced, so the thing
//! comes back on every device; a highlight comes back with the note deleted with
//! it, while a note deleted on its own, earlier, stays deleted (SY19). Books come
//! with Stage 4 (SY18), Recently deleted with Stage 5. The time passed in is the
//! time this device can vouch for (`retention::Vouched`, kept by Stage 3).

use super::apply::{self, Tally};
use super::journal::{self, JournalError, Library, LocalContent, Which};
use super::merge::{Outcome, Placed};
use super::record::{Content, Key, Kind, Version};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

type Result<T> = std::result::Result<T, JournalError>;

/// SY17 (prov., item 111): Restore is offered while this device's own age for a
/// deletion is under this. PROVISIONAL.
pub const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// A deletion this device saw: its tombstone (`seq` once published, for a remote
/// one) and when it was first seen here.
pub(crate) fn seen(
    conn: &Connection,
    lib: &Library,
    v: &Version,
    seq: Option<u64>,
    now_ms: i64,
) -> Result<()> {
    if !matches!(v.content, Content::Deleted { .. }) {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO sync_deleted (library, key, device, hash, seq, first_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            lib.id.to_string(),
            v.key.as_str(),
            v.device.to_string(),
            v.hash.to_string(),
            seq.map(|s| s as i64),
            now_ms
        ],
    )?;
    Ok(())
}

/// A deletion Restore can be offered for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deleted {
    pub key: Key,
    /// How long ago this device first saw it.
    pub age_ms: i64,
}

/// What the library shows of `key`, this device's unsent or held edit included.
fn shown(conn: &Connection, lib: &Library, key: &Key) -> Result<Option<Placed>> {
    let mut record = journal::record_of(conn, lib, key, Which::Applied)?;
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
            seq: u64::MAX,
        });
    }
    Ok(match record.outcome(&HashSet::new()) {
        Some(Outcome::Settled(p)) => Some(p.clone()),
        Some(Outcome::Conflict { shown, .. }) => Some(shown.clone()),
        None => None,
    })
}

/// The age of the deletion `key` shows, if it shows one this device has seen.
fn age(conn: &Connection, lib: &Library, key: &Key, now_ms: i64) -> Result<Option<(Placed, i64)>> {
    let Some(tomb) = shown(conn, lib, key)? else {
        return Ok(None);
    };
    if !tomb.is_deleted() {
        return Ok(None);
    }
    let first: Option<i64> = conn
        .query_row(
            "SELECT first_seen FROM sync_deleted WHERE library = ?1 AND key = ?2 AND device = ?3 AND hash = ?4",
            params![
                lib.id.to_string(),
                key.as_str(),
                tomb.version.device.to_string(),
                tomb.version.hash.to_string()
            ],
            |r| r.get(0),
        )
        .optional()?;
    Ok(first.map(|f| (tomb, now_ms - f)))
}

/// The version the tombstone displaced: the newest of those it descends from (its
/// history for this key) that still has a body here.
fn displaced(conn: &Connection, lib: &Library, tomb: &Placed) -> Result<Option<Vec<u8>>> {
    let mut best: Option<(super::clock::Hlc, Vec<u8>)> = None;
    for e in &tomb.version.history {
        let raw: Option<String> = conn
            .query_row(
                "SELECT raw FROM sync_versions
                 WHERE library = ?1 AND key = ?2 AND device = ?3 AND seq = ?4 AND hash = ?5",
                params![
                    lib.id.to_string(),
                    tomb.version.key.as_str(),
                    e.device.to_string(),
                    e.seq as i64,
                    e.hash.to_string()
                ],
                |r| r.get(0),
            )
            .optional()?;
        let Some(Ok(super::record::Checked::Version(v))) = raw
            .as_deref()
            .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
            .map(|v| super::record::check_version(v, e.device))
        else {
            continue;
        };
        let Content::Body { hash, .. } = &v.content else {
            continue;
        };
        let Some(body) = journal::body(conn, hash)? else {
            continue;
        };
        if best.as_ref().map_or(true, |(c, _)| v.clock > *c) {
            best = Some((v.clock, body));
        }
    }
    Ok(best.map(|(_, b)| b))
}

/// SY19: what Restore can be offered for now: highlights and notes whose
/// deletion this device first saw less than a week ago, with what it displaced
/// still here.
pub fn recently_deleted(conn: &Connection, lib: &Library, now_ms: i64) -> Result<Vec<Deleted>> {
    let keys: Vec<String> = conn
        .prepare("SELECT DISTINCT key FROM sync_deleted WHERE library = ?1 AND first_seen > ?2")?
        .query_map(params![lib.id.to_string(), now_ms - WEEK_MS], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for k in keys {
        let Some((key, _)) = Key::parse(&k) else {
            continue;
        };
        if !matches!(key.kind(), Kind::Annotation | Kind::Note) {
            continue;
        }
        if let Some((tomb, age_ms)) = age(conn, lib, &key, now_ms)? {
            if age_ms < WEEK_MS && displaced(conn, lib, &tomb)?.is_some() {
                out.push(Deleted { key, age_ms });
            }
        }
    }
    out.sort_by(|a, b| a.key.as_str().cmp(b.key.as_str()));
    Ok(out)
}

/// SY19: restores `key` (a highlight or a note): a new version descending from its
/// tombstone, holding what the deletion displaced, written here and sent like any
/// edit. A highlight comes back with the note deleted with it (in the same
/// change). `false` when it isn't offered: not deleted, past its week, or what it
/// displaced is gone.
pub fn restore(conn: &Connection, lib: &Library, key: &Key, now_ms: i64) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    // The note's tombstone, read before the highlight comes back.
    let note = Key::parse(&format!("note:{}", subject(key))).map(|(k, _)| k);
    let note_tomb = match (&note, key.kind()) {
        (Some(note), Kind::Annotation) => age(&tx, lib, note, now_ms)?,
        _ => None,
    };
    let Some(tomb) = restore_one(&tx, lib, key, now_ms)? else {
        return Ok(false);
    };
    if let (Some(note), Some((note_tomb, _))) = (note, note_tomb) {
        if same_change(&tomb, &note_tomb) {
            restore_one(&tx, lib, &note, now_ms)?;
        }
    }
    tx.commit()?;
    Ok(true)
}

fn subject(key: &Key) -> &str {
    key.as_str().split(':').nth(1).unwrap_or_default()
}

/// Whether two tombstones came in the same change: the same writer and sequence,
/// or both this device's own, not yet sent.
fn same_change(a: &Placed, b: &Placed) -> bool {
    a.version.device == b.version.device && a.seq == b.seq
}

/// Restores one record; the tombstone it restored over, if it did.
fn restore_one(conn: &Connection, lib: &Library, key: &Key, now_ms: i64) -> Result<Option<Placed>> {
    let Some((tomb, age_ms)) = age(conn, lib, key, now_ms)? else {
        return Ok(None);
    };
    if age_ms >= WEEK_MS {
        return Ok(None);
    }
    let Some(body) = displaced(conn, lib, &tomb)? else {
        return Ok(None);
    };
    // The reader's own choice: where this device's version waits in a conflict, the
    // restored one descends from every head, as a choice does, and is sent.
    let wall = now_ms.max(0) as u64;
    let v = match journal::record_local(conn, lib, key, LocalContent::Body(body.clone()), wall) {
        Err(JournalError::Conflicted(_)) => {
            let v = journal::hold_local(conn, lib, key, LocalContent::Body(body), wall)?;
            journal::release_held(conn, lib, key)?;
            v
        }
        other => other?,
    };
    super::guard::changed_here(conn, lib, key)?;
    let mut restoring = Tally::default();
    restoring.restoring = true;
    apply::write(conn, lib, &v, &mut restoring)?;
    Ok(Some(tomb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::apply::receive;
    use crate::sync::apply::tests::{device, highlight, note_of, publish_next, send, ANNO};

    const DAY: i64 = 24 * 60 * 60 * 1000;

    fn key(k: &str) -> Key {
        Key::parse(&format!("{k}:{ANNO}")).unwrap().0
    }

    /// A's highlight with a note, on B too.
    fn shared() -> (Store, Library, Store, Library) {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("the note"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        (a, la, b, lb)
    }

    #[test]
    fn a_highlight_deleted_elsewhere_comes_back_with_its_note_everywhere() {
        let (mut a, la, b, lb) = shared();
        let now = crate::store::now_ms();
        a.delete_annotation(ANNO).unwrap();
        let gone = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &gone, now).unwrap();
        assert!(b.annotations("book-b").unwrap().is_empty());
        let offered: Vec<String> = recently_deleted(b.conn(), &lb, now + DAY)
            .unwrap()
            .iter()
            .map(|d| d.key.as_str().to_owned())
            .collect();
        assert_eq!(
            offered,
            vec![key("annotation").as_str(), key("note").as_str()]
        );
        assert!(restore(b.conn(), &lb, &key("annotation"), now + DAY).unwrap());
        assert_eq!(note_of(&b, "book-b"), Some(Some("the note".into())));
        // It comes back on the device that deleted it too.
        let back = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(a.conn(), &la, &back, now + DAY).unwrap();
        assert_eq!(note_of(&a, "book-a"), Some(Some("the note".into())));
    }

    #[test]
    fn a_note_deleted_on_its_own_earlier_stays_deleted() {
        let (mut a, la, b, lb) = shared();
        let now = crate::store::now_ms();
        // The note cleared in one change, the highlight deleted in a later one.
        a.save_annotation(&highlight("book-a", None, 1_760_000_002_000))
            .unwrap();
        let cleared = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &cleared, now).unwrap();
        a.delete_annotation(ANNO).unwrap();
        let gone = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &gone, now).unwrap();
        assert!(restore(b.conn(), &lb, &key("annotation"), now + DAY).unwrap());
        assert_eq!(note_of(&b, "book-b"), Some(None));
    }

    #[test]
    fn restore_isnt_offered_after_the_week() {
        let (mut a, la, b, lb) = shared();
        let now = crate::store::now_ms();
        a.delete_annotation(ANNO).unwrap();
        let gone = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &gone, now).unwrap();
        assert!(recently_deleted(b.conn(), &lb, now + 8 * DAY)
            .unwrap()
            .is_empty());
        assert!(!restore(b.conn(), &lb, &key("annotation"), now + 8 * DAY).unwrap());
        assert!(b.annotations("book-b").unwrap().is_empty());
    }

    #[test]
    fn a_deletion_made_here_is_restorable_here() {
        let (_a, _la, mut b, lb) = shared();
        b.delete_annotation(ANNO).unwrap();
        let now = crate::store::now_ms();
        assert!(restore(b.conn(), &lb, &key("annotation"), now + DAY).unwrap());
        assert_eq!(note_of(&b, "book-b"), Some(Some("the note".into())));
    }

    #[test]
    fn a_note_deleted_while_its_highlight_stays_is_restorable() {
        let (mut a, la, b, lb) = shared();
        let now = crate::store::now_ms();
        a.save_annotation(&highlight("book-a", None, 1_760_000_002_000))
            .unwrap();
        let cleared = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &cleared, now).unwrap();
        assert_eq!(note_of(&b, "book-b"), Some(None));
        assert!(restore(b.conn(), &lb, &key("note"), now + DAY).unwrap());
        assert_eq!(note_of(&b, "book-b"), Some(Some("the note".into())));
    }
}
