//! SP26, SP30: revert and hold a removed or quarantined device's late changes.
//!
//! Pure library operations; no sync-pass or publication caller yet. The journal,
//! including superseded versions, supplies ancestry and bodies, never sync_undo.

use super::apply::{self, Tally};
use super::guard::stamp_column;
use super::ids::{DeviceId, Hash32};
use super::journal::{self, JournalError, Library, LocalContent, Which};
use super::merge::{Outcome, Placed, Record};
use super::record::{check_version, Checked, Content, Entry, Key, Kind, RecordError, Version};
use super::trust::Mark;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// SP26: at most this many late versions per device.
pub const MAX_LATE: usize = 500;
/// SP26: at most this many body bytes per device, including pending Keeps.
pub const MAX_LATE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum LateError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error("late-change reason must be removed or quarantined")]
    Reason,
    #[error("ancestor or late version {0:?} of {1} is not held")]
    MissingVersion(Entry, Key),
    #[error("body {0} is not held")]
    MissingBody(Hash32),
    #[error("restoration of {0} could not be applied")]
    NotWritten(Key),
    #[error("published restorations of {0:?} need discard-choice merging before recovery")]
    PublishedRestorations(Vec<Key>),
}

pub type Result<T> = std::result::Result<T, LateError>;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reverted {
    /// Records written back (several late versions can belong to one record).
    pub reverted: usize,
    /// Late versions whose record was left alone because another writer changed it.
    pub left: usize,
    pub listed: usize,
    pub dropped: usize,
}

fn checked(raw: &str, device: DeviceId) -> Result<Version> {
    let raw = serde_json::from_str(raw).map_err(|_| RecordError::NotAnObject)?;
    match check_version(raw, device)? {
        Checked::Version(v) => Ok(v),
        Checked::NewerFormat(_) => Err(RecordError::Field("format").into()),
    }
}

// SP11: superseded ancestors are held but are deliberately absent from merging.
fn version(conn: &Connection, lib: &Library, key: &Key, id: Entry) -> Result<Placed> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT raw FROM sync_versions WHERE library = ?1 AND device = ?2
             AND seq = ?3 AND key = ?4 AND hash = ?5 AND applied IN (1, 2, -5)",
            params![
                lib.id.to_string(),
                id.device.to_string(),
                id.seq as i64,
                key.as_str(),
                id.hash.to_string()
            ],
            |r| r.get(0),
        )
        .optional()?;
    let raw = raw.ok_or_else(|| LateError::MissingVersion(id, key.clone()))?;
    Ok(Placed {
        version: checked(&raw, id.device)?,
        seq: id.seq,
    })
}

// SY13: unsent and held edits count too, as in restore::shown. The sentinel is
// only for local ordering and never becomes a history entry or a published ID.
fn shown(conn: &Connection, lib: &Library, key: &Key) -> Result<Option<Placed>> {
    let mut record = journal::record_of(conn, lib, key, Which::Applied)?;
    let local: Option<String> = conn
        .query_row(
            "SELECT raw FROM sync_versions WHERE library = ?1 AND key = ?2 AND seq IS NULL",
            params![lib.id.to_string(), key.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(raw) = local {
        record.add(Placed {
            version: checked(&raw, lib.device)?,
            seq: u64::MAX,
        });
    }
    Ok(outcome(&record).cloned())
}

fn outcome(record: &Record) -> Option<&Placed> {
    match record.outcome(&HashSet::new()) {
        Some(Outcome::Settled(p)) | Some(Outcome::Conflict { shown: p, .. }) => Some(p),
        None => None,
    }
}

// SP26: only exact restorations made here stand for their source. A reader's
// subsequent edit has another hash, even if it repeats the restored content.
fn represented(conn: &Connection, lib: &Library, mut p: Placed) -> Result<Option<Placed>> {
    loop {
        let source: Option<(Option<String>, Option<i64>, Option<String>)> = conn
            .query_row(
                "SELECT source_device, source_seq, source_hash FROM sync_late_restored
                 WHERE library = ?1 AND key = ?2 AND restoration = ?3",
                params![
                    lib.id.to_string(),
                    p.version.key.as_str(),
                    p.version.hash.to_string()
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match source {
            None => return Ok(Some(p)),
            Some((None, None, None)) => return Ok(None),
            Some((Some(device), Some(seq), Some(hash))) => {
                p = version(
                    conn,
                    lib,
                    &p.version.key,
                    Entry {
                        device: DeviceId::parse(&device).ok_or(RecordError::Field("device"))?,
                        seq: seq as u64,
                        hash: Hash32::parse(&hash).ok_or(RecordError::Field("hash"))?,
                    },
                )?;
            }
            _ => return Err(RecordError::Field("restoration").into()),
        }
    }
}

fn past(p: &Placed, cutoffs: &BTreeMap<DeviceId, u64>) -> bool {
    cutoffs
        .get(&p.version.device)
        .is_some_and(|cutoff| p.seq > *cutoff)
}

// SP26, SP30: retain provenance after publication and recovery too. Keep does
// not call this: it is a reader decision, not an automatic restoration.
fn remember(conn: &Connection, lib: &Library, v: &Version, source: Option<&Placed>) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_late_restored
         (library, key, restoration, source_device, source_seq, source_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            lib.id.to_string(),
            v.key.as_str(),
            v.hash.to_string(),
            source.map(|p| p.version.device.to_string()),
            source.map(|p| p.seq as i64),
            source.map(|p| p.version.hash.to_string())
        ],
    )?;
    Ok(())
}

// SP11, SP26: follow removed ancestors until reaching surviving ones. History
// contains transitive entries too; heads eliminate those further back. Concurrent
// surviving ancestors use the same ordering/conflict rule as the reader's record.
fn before(
    conn: &Connection,
    lib: &Library,
    p: &Placed,
    cutoffs: &BTreeMap<DeviceId, u64>,
) -> Result<Option<Placed>> {
    let mut pending = p.version.history.clone();
    let mut visited = HashSet::new();
    let mut surviving = Record::default();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(ancestor) = represented(conn, lib, version(conn, lib, &p.version.key, id)?)?
        else {
            continue;
        };
        if past(&ancestor, cutoffs) {
            pending.extend(ancestor.version.history.iter().copied());
        } else {
            surviving.add(ancestor);
        }
    }
    Ok(outcome(&surviving).cloned())
}

fn content(conn: &Connection, lib: &Library, p: Option<&Placed>) -> Result<LocalContent> {
    Ok(match p.map(|p| &p.version.content) {
        Some(Content::Body { hash, .. }) => {
            LocalContent::Body(journal::body(conn, hash)?.ok_or(LateError::MissingBody(*hash))?)
        }
        Some(Content::Inline(v)) => LocalContent::Inline(v.clone()),
        Some(Content::Deleted { covers, identity }) => LocalContent::Deleted {
            covers: covers.clone(),
            identity: identity.clone(),
        },
        None => LocalContent::Deleted {
            covers: super::local::covers(conn, lib)?,
            identity: None,
        },
    })
}

fn local(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    content: LocalContent,
    now_ms: i64,
) -> Result<Version> {
    let wall = now_ms.max(0) as u64;
    Ok(
        match journal::record_local(conn, lib, key, content.clone(), wall) {
            // SY13: the restoration/choice settles an otherwise hidden local head,
            // following guard::revert's hold-and-release pattern.
            Err(JournalError::Conflicted(_)) => {
                let v = journal::hold_local(conn, lib, key, content, wall)?;
                journal::release_held(conn, lib, key)?;
                v
            }
            other => other?,
        },
    )
}

// SP26: a Keep descends explicitly from the late version, even when it is no
// longer in the shown head's ancestry. Only the newly written, unnumbered local
// row is amended; remote bytes and numbered versions are never changed.
fn put(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    source: Option<&Placed>,
    parent: &Placed,
    now_ms: i64,
) -> Result<Version> {
    let content = content(conn, lib, source)?;
    journal::observe(conn, lib, parent.version.clock, now_ms.max(0) as u64)?;
    let mut v = local(conn, lib, key, content, now_ms)?;
    let mut history = BTreeMap::<DeviceId, Entry>::new();
    for e in v
        .history
        .iter()
        .chain(parent.version.history.iter())
        .copied()
        .chain([parent.id()])
    {
        let slot = history.entry(e.device).or_insert(e);
        if e.seq > slot.seq {
            *slot = e;
        }
    }
    v.raw["history"] = Value::Array(history.values().map(entry).collect());
    v = checked(&v.raw.to_string(), lib.device)?;
    conn.execute(
        "UPDATE sync_versions SET hash = ?3, raw = ?4 WHERE library = ?1 AND key = ?2 AND seq IS NULL",
        params![lib.id.to_string(), key.as_str(), v.hash.to_string(), v.raw.to_string()],
    )?;
    let mut tally = Tally::default();
    tally.restoring = true;
    if !apply::write(conn, lib, &v, &mut tally)? {
        return Err(LateError::NotWritten(key.clone()));
    }
    Ok(v)
}

fn entry(id: &Entry) -> Value {
    json!({ "d": id.device.to_string(), "s": id.seq, "h": id.hash.to_string() })
}

// SY26, SP26: reading-state bodies carry state, not the old time. As in
// guard::revert, only this library gets the ancestor's time; peers see the new
// restoration version's time. Missing content is an error, never an empty state.
fn restore_time(conn: &Connection, lib: &Library, key: &Key, prior: Option<&Placed>) -> Result<()> {
    let Some(column) = stamp_column(key.kind()) else {
        return Ok(());
    };
    let at = match (prior, content(conn, lib, prior)?) {
        (Some(p), LocalContent::Body(body)) => {
            let finished = key.kind() != Kind::BookFinished
                || serde_json::from_slice::<Value>(&body).map_err(|_| RecordError::Body)?
                    ["finished"]
                    == true;
            finished.then_some(p.version.clock.ms() as i64)
        }
        _ => None,
    };
    conn.execute(
        &format!(
            "UPDATE books SET {column} = ?2 WHERE id IN
            (SELECT book_id FROM sync_books WHERE library = ?1 AND sync_id = ?3)"
        ),
        params![lib.id.to_string(), at, key.as_str().split(':').nth(1)],
    )?;
    Ok(())
}

/// SP26, SP29, SP30: stop all removed writers together and revert their applied
/// changes beyond each writer's later cutoff. All marks must already be verified.
/// Unapplied versions are held outside merging/rechecks until Keep or reapply.
/// Repeats at the same cutoff are no-ops, including after recovery. Reader-decided
/// versions remain excluded at every cutoff.
pub fn revert_past(
    conn: &Connection,
    lib: &Library,
    removed: &BTreeMap<DeviceId, Option<Mark>>,
    accepted_by_all: &BTreeMap<DeviceId, Mark>,
    reason: &str,
    now_ms: i64,
) -> Result<Reverted> {
    if !matches!(reason, "removed" | "quarantined") {
        return Err(LateError::Reason);
    }
    let cutoffs: BTreeMap<_, _> = removed
        .iter()
        .map(|(&device, cutoff)| {
            let cutoff = cutoff
                .iter()
                .chain(accepted_by_all.get(&device))
                .map(|m| m.s)
                .max()
                .unwrap_or(0);
            (device, cutoff)
        })
        .collect();
    let tx = conn.unchecked_transaction()?;
    let mut result = Reverted::default();
    let mut restored = BTreeMap::<Key, Option<String>>::new();
    for (&device, &cutoff) in &cutoffs {
        let dropped_before = result.dropped;
        apply::stop(&tx, lib, device, reason, now_ms)?;
        // SP26, SP30: resolved recovery rows are removed. Remember this cutoff's
        // processed prefix separately so repeating the old call cannot undo recovery;
        // a different cutoff can still process those versions again.
        let processed: Option<i64> = tx
        .query_row(
            "SELECT seq FROM sync_late_passes WHERE library = ?1 AND device = ?2 AND cutoff = ?3",
            params![lib.id.to_string(), device.to_string(), cutoff as i64],
            |r| r.get(0),
        )
        .optional()?;
        let rows: Vec<(String, i64, String, i64)> = tx
        .prepare(
            "SELECT v.key, v.seq, v.hash, v.applied FROM sync_versions v
         WHERE v.library = ?1 AND v.device = ?2 AND v.applied IN (1, 2, 0, -1, -2, -4) AND v.seq > ?3
         AND NOT EXISTS (SELECT 1 FROM sync_late_seen s WHERE s.library = v.library
             AND s.device = v.device AND s.seq = v.seq AND s.key = v.key)
         ORDER BY v.seq, v.key",
        )?
        .query_map(
            params![lib.id.to_string(), device.to_string(), (cutoff as i64).max(processed.unwrap_or(0))],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?
        .collect::<std::result::Result<_, _>>()?;
        let through: Option<i64> = tx.query_row(
            "SELECT MAX(seq) FROM sync_versions WHERE library = ?1 AND device = ?2 AND seq > ?3
         AND applied IN (1, 2, 0, -1, -2, -4, -5)",
            params![lib.id.to_string(), device.to_string(), cutoff as i64],
            |r| r.get(0),
        )?;
        let (mut count, mut bytes): (i64, i64) = tx.query_row(
        "SELECT COUNT(*), COALESCE(SUM(bytes), 0) FROM sync_late WHERE library = ?1 AND device = ?2",
        params![lib.id.to_string(), device.to_string()], |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
        // SP26: once the list overflows, its entire later tail stays unlisted, even
        // if a later version is small or the reader frees room by discarding offers.
        let mut overflow: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_late_over WHERE library = ?1 AND device = ?2 AND dropped > 0)",
        params![lib.id.to_string(), device.to_string()], |r| r.get(0),
    )?;
        for (key, seq, hash, applied) in rows {
            let key = Key::parse(&key).ok_or(RecordError::Key)?.0;
            let id = Entry {
                device,
                seq: seq as u64,
                hash: Hash32::parse(&hash).ok_or(RecordError::Field("hash"))?,
            };
            let pending = applied <= 0;
            if pending {
                // SP26, SP30: block even overflow versions. Process only new rows:
                // an already resolved prefix must not be held again on a repeat call.
                tx.execute(
                    "UPDATE sync_versions SET applied = -5 WHERE library = ?1
                AND device = ?2 AND seq = ?3 AND key = ?4 AND applied IN (0, -1, -2, -4)",
                    params![lib.id.to_string(), device.to_string(), seq, key.as_str()],
                )?;
            }
            let late = version(&tx, lib, &key, id)?;
            if !pending && !restored.contains_key(&key) {
                let current = shown(&tx, lib, &key)?;
                let old_hash = current.as_ref().map(|p| p.version.hash.to_string());
                let current = current
                    .map(|p| represented(&tx, lib, p))
                    .transpose()?
                    .flatten();
                let marker = match current {
                    Some(p) if past(&p, &cutoffs) => {
                        let prior = before(&tx, lib, &p, &cutoffs)?;
                        let v = put(&tx, lib, &key, prior.as_ref(), &p, now_ms)?;
                        remember(&tx, lib, &v, prior.as_ref())?;
                        tx.execute(
                        "UPDATE sync_late_seen SET restored = ?4 WHERE library = ?1 AND key = ?2
                         AND restored = ?3 AND decided = 0 AND was IS NULL",
                        params![lib.id.to_string(), key.as_str(), old_hash, v.hash.to_string()],
                    )?;
                        restore_time(&tx, lib, &key, prior.as_ref())?;
                        result.reverted += 1;
                        Some(v.hash.to_string())
                    }
                    _ => None,
                };
                restored.insert(key.clone(), marker);
            }
            let marker = if pending {
                None
            } else {
                restored[&key].as_deref()
            };
            if !pending && marker.is_none() {
                result.left += 1;
            }
            let size = match &late.version.content {
                Content::Body { size, .. } => *size,
                Content::Inline(v) => v.to_string().len() as u64,
                Content::Deleted { .. } => 0,
            };
            if !overflow && count < MAX_LATE as i64 && bytes + size as i64 <= MAX_LATE_BYTES as i64
            {
                tx.execute(
                    "INSERT INTO sync_late (library, device, key, seq, hash, bytes, reason, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        lib.id.to_string(),
                        device.to_string(),
                        key.as_str(),
                        seq,
                        hash,
                        size as i64,
                        reason,
                        now_ms
                    ],
                )?;
                count += 1;
                bytes += size as i64;
                result.listed += 1;
            } else {
                overflow = true;
                result.dropped += 1;
            }
            tx.execute(
                "INSERT INTO sync_late_seen (library, device, seq, key, was, restored)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    lib.id.to_string(),
                    device.to_string(),
                    seq,
                    key.as_str(),
                    pending.then_some(applied),
                    marker
                ],
            )?;
        }
        if let Some(through) = through {
            tx.execute(
            "INSERT INTO sync_late_passes (library, device, cutoff, seq) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(library, device, cutoff) DO UPDATE SET seq = MAX(seq, excluded.seq)",
            params![
                lib.id.to_string(),
                device.to_string(),
                cutoff as i64,
                through
            ],
        )?;
        }
        let dropped = result.dropped - dropped_before;
        if dropped > 0 {
            tx.execute(
                "INSERT INTO sync_late_over (library, device, dropped) VALUES (?1, ?2, ?3)
            ON CONFLICT(library, device) DO UPDATE SET dropped = dropped + excluded.dropped",
                params![lib.id.to_string(), device.to_string(), dropped as i64],
            )?;
        }
    }
    tx.commit()?;
    Ok(result)
}

struct Late {
    key: Key,
    id: Entry,
    state: String,
    #[cfg(test)]
    restored: Option<String>,
    kept: Option<String>,
    at: i64,
    #[cfg(test)]
    pending: bool,
}

fn rows(conn: &Connection, lib: &Library, device: Option<DeviceId>) -> Result<Vec<Late>> {
    let raw = conn
        .prepare(
            "SELECT l.key, l.device, l.seq, l.hash, l.state, s.restored, l.kept, l.at, s.was IS NOT NULL
         FROM sync_late l JOIN sync_late_seen s
           ON s.library = l.library AND s.device = l.device AND s.seq = l.seq AND s.key = l.key
         WHERE l.library = ?1 AND (?2 IS NULL OR l.device = ?2) ORDER BY l.seq, l.key",
        )?
        .query_map(
            params![lib.id.to_string(), device.map(|d| d.to_string())],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? as u64,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, bool>(8)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    raw.into_iter()
        .map(
            |(key, device, seq, hash, state, _restored, kept, at, _pending)| {
                Ok(Late {
                    key: Key::parse(&key).ok_or(RecordError::Key)?.0,
                    id: Entry {
                        device: DeviceId::parse(&device).ok_or(RecordError::Field("device"))?,
                        seq,
                        hash: Hash32::parse(&hash).ok_or(RecordError::Field("hash"))?,
                    },
                    state,
                    #[cfg(test)]
                    restored: _restored,
                    kept,
                    at,
                    #[cfg(test)]
                    pending: _pending,
                })
            },
        )
        .collect()
}

fn remove(conn: &Connection, lib: &Library, row: &Late) -> Result<()> {
    conn.execute(
        "DELETE FROM sync_late WHERE library = ?1 AND device = ?2 AND key = ?3 AND seq = ?4",
        params![
            lib.id.to_string(),
            row.id.device.to_string(),
            row.key.as_str(),
            row.id.seq as i64
        ],
    )?;
    Ok(())
}

/// SP30: recover the resolved prefix, including versions beyond the offer cap.
/// Applied content returns only while our restoration still shows. Never-applied
/// versions regain their original waiting state and aren't counted as writes.
/// Reader decisions are left alone; no choice is made here. The count is applied
/// versions recovered, though each record is written only once.
/// A published restoration cannot yield to a released newer version until merging
/// supports Discard choices. Such a record is left as it is, its versions still
/// held and listed for a later call; every other record is recovered, and then
/// `PublishedRestorations` names the records left.
pub fn reapply_upto(
    conn: &Connection,
    lib: &Library,
    device: DeviceId,
    upto: Mark,
    now_ms: i64,
) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    let mut left = Vec::new();
    let recovering: Vec<(String, i64)> = tx
        .prepare(
            "SELECT key, seq FROM sync_late_seen WHERE library = ?1 AND device = ?2
         AND seq <= ?3 AND decided = 0 ORDER BY seq, key",
        )?
        .query_map(
            params![lib.id.to_string(), device.to_string(), upto.s as i64],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<std::result::Result<_, _>>()?;
    struct Resolved {
        p: Placed,
        was: Option<i64>,
        restored: Option<String>,
    }
    let mut records = BTreeMap::<Key, Vec<Resolved>>::new();
    for (key, seq) in recovering {
        let key = Key::parse(&key).ok_or(RecordError::Key)?.0;
        let (was, restored): (Option<i64>, Option<String>) = tx.query_row(
            "SELECT was, restored FROM sync_late_seen WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
            params![lib.id.to_string(), device.to_string(), seq, key.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let hash: String = tx.query_row(
            "SELECT hash FROM sync_versions WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
            params![lib.id.to_string(), device.to_string(), seq, key.as_str()], |r| r.get(0),
        )?;
        let id = Entry {
            device,
            seq: seq as u64,
            hash: Hash32::parse(&hash).ok_or(RecordError::Field("hash"))?,
        };
        records.entry(key.clone()).or_default().push(Resolved {
            p: version(&tx, lib, &key, id)?,
            was,
            restored,
        });
    }
    for (key, resolved) in records {
        let current = shown(&tx, lib, &key)?;
        let applied: Vec<_> = resolved.iter().filter(|r| r.was.is_none()).collect();
        let newest = applied.last();
        let ours = current.as_ref().is_some_and(|p| {
            applied
                .iter()
                .any(|r| r.restored.as_deref() == Some(&p.version.hash.to_string()))
        });
        if ours {
            let newest = newest.expect("restoration belongs to an applied late version");
            let released_newer = resolved
                .iter()
                .any(|r| r.was.is_some() && r.p.seq > newest.p.seq);
            if released_newer {
                if current.as_ref().is_some_and(|p| p.seq != u64::MAX) {
                    left.push(key);
                    continue;
                }
                journal::drop_unsent(&tx, lib, &key)?;
                for row in &applied {
                    tx.execute(
                        "UPDATE sync_versions SET applied = 1 WHERE library = ?1 AND device = ?2
                         AND key = ?3 AND seq = ?4 AND applied = 2",
                        params![
                            lib.id.to_string(),
                            device.to_string(),
                            key.as_str(),
                            row.p.seq as i64
                        ],
                    )?;
                }
                // SP30: use the normal writer and its 1/2 head bookkeeping while
                // the released successors are still held outside merging.
                let mut tally = Tally::default();
                tally.restoring = true;
                if !apply::apply_key(&tx, lib, &key, now_ms, &mut tally)? {
                    return Err(LateError::NotWritten(key));
                }
            } else {
                let v = put(&tx, lib, &key, Some(&newest.p), &newest.p, now_ms)?;
                remember(&tx, lib, &v, Some(&newest.p))?;
                // SP30: a later resolved prefix can still replace this restoration.
                // Every device's rows naming the replaced restoration, as in
                // `revert_past`: a cascade's devices can share one (#163).
                tx.execute(
                    "UPDATE sync_late_seen SET restored = ?4 WHERE library = ?1
                     AND key = ?2 AND restored = ?3 AND decided = 0 AND was IS NULL",
                    params![
                        lib.id.to_string(),
                        key.as_str(),
                        current.as_ref().map(|p| p.version.hash.to_string()),
                        v.hash.to_string()
                    ],
                )?;
            }
            n += applied.len();
        }
        for row in resolved {
            let seq = row.p.seq as i64;
            if let Some(was) = row.was {
                tx.execute(
                    "UPDATE sync_versions SET applied = ?5 WHERE library = ?1 AND device = ?2
                     AND seq = ?3 AND key = ?4 AND applied = -5",
                    params![
                        lib.id.to_string(),
                        device.to_string(),
                        seq,
                        key.as_str(),
                        was
                    ],
                )?;
            }
            let listed = tx.execute(
            "DELETE FROM sync_late WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
            params![lib.id.to_string(), device.to_string(), seq, key.as_str()],
        )?;
            if listed == 0 {
                // SP30: an undecided version without an offer was over the cap.
                // Once resolved it is no longer late, so it no longer blocks cleanup.
                tx.execute("UPDATE sync_late_over SET dropped = dropped - 1 WHERE library = ?1 AND device = ?2 AND dropped > 0",
                params![lib.id.to_string(), device.to_string()])?;
                tx.execute(
                    "DELETE FROM sync_late_over WHERE library = ?1 AND device = ?2 AND dropped = 0",
                    params![lib.id.to_string(), device.to_string()],
                )?;
            }
            tx.execute("DELETE FROM sync_late_seen WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
            params![lib.id.to_string(), device.to_string(), seq, key.as_str()])?;
        }
    }
    tx.commit()?;
    if !left.is_empty() {
        return Err(LateError::PublishedRestorations(left));
    }
    Ok(n)
}

/// SP26, SP12: write the late content locally, explicitly descending from it.
/// Keep stays pending until its replacement has a real publication sequence.
pub fn keep(
    conn: &Connection,
    lib: &Library,
    device: DeviceId,
    key: &Key,
    seq: u64,
    now_ms: i64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if let Some(row) = rows(&tx, lib, Some(device))?
        .into_iter()
        .find(|r| &r.key == key && r.id.seq == seq && r.state == "held")
    {
        let p = version(&tx, lib, key, row.id)?;
        let v = put(&tx, lib, key, Some(&p), &p, now_ms)?;
        tx.execute(
            "UPDATE sync_late SET state = 'kept', kept = ?5, at = ?6
            WHERE library = ?1 AND device = ?2 AND key = ?3 AND seq = ?4",
            params![
                lib.id.to_string(),
                device.to_string(),
                key.as_str(),
                seq as i64,
                v.hash.to_string(),
                now_ms
            ],
        )?;
        decided(&tx, lib, &row)?;
    }
    tx.commit()?;
    Ok(())
}

fn decided(conn: &Connection, lib: &Library, row: &Late) -> Result<()> {
    conn.execute("UPDATE sync_late_seen SET decided = 1 WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
        params![lib.id.to_string(), row.id.device.to_string(), row.id.seq as i64, row.key.as_str()])?;
    Ok(())
}

fn choice(
    conn: &Connection,
    lib: &Library,
    row: &Late,
    decision: &str,
    replacement: Entry,
    now_ms: i64,
) -> Result<()> {
    let key = Key::parse(&format!(
        "choice:{}.{}.{}",
        row.id.device, row.id.seq, row.id.hash
    ))
    .ok_or(RecordError::Key)?
    .0;
    local(
        conn,
        lib,
        &key,
        LocalContent::Inline(json!({ "choice": decision, "version": entry(&replacement) })),
        now_ms,
    )?;
    decided(conn, lib, row)?;
    remove(conn, lib, row)
}

/// SP12: finish pending Keeps whose exact replacement now has a sequence.
/// No caller is wired yet; Stage 2 will call this after publication. A replacement
/// overwritten while unsent remains pending; never substitute a different edit.
pub fn finish_keeps(conn: &Connection, lib: &Library) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for row in rows(&tx, lib, None)?
        .into_iter()
        .filter(|r| r.state == "kept")
    {
        let replacement: Option<(String, i64, String)> = tx
            .query_row(
                "SELECT device, seq, hash FROM sync_versions WHERE library = ?1 AND key = ?2
             AND hash = ?3 AND seq IS NOT NULL AND applied IN (1, 2)",
                params![lib.id.to_string(), row.key.as_str(), row.kept],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((d, seq, h)) = replacement {
            let id = Entry {
                device: DeviceId::parse(&d).ok_or(RecordError::Field("device"))?,
                seq: seq as u64,
                hash: Hash32::parse(&h).ok_or(RecordError::Field("hash"))?,
            };
            choice(&tx, lib, &row, "keep", id, row.at)?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// SP12, SP26: Discard records the late version's known ID and clears its offer.
pub fn discard(
    conn: &Connection,
    lib: &Library,
    device: DeviceId,
    key: &Key,
    seq: u64,
    now_ms: i64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if let Some(row) = rows(&tx, lib, Some(device))?
        .into_iter()
        .find(|r| &r.key == key && r.id.seq == seq)
    {
        choice(&tx, lib, &row, "discard", row.id, now_ms)?;
    }
    tx.commit()?;
    Ok(())
}

/// SY34: discard each listed offer with an ordinary SP12 Discard choice.
/// This does not clear overflow: still-late, unlisted changes need SP12's
/// code-signed choice:late. (Stage 3) before storage cleanup. SP30 recovery removes
/// only versions that are no longer late from that count.
pub fn discard_listed(
    conn: &Connection,
    lib: &Library,
    device: DeviceId,
    now_ms: i64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for row in rows(&tx, lib, Some(device))? {
        choice(&tx, lib, &row, "discard", row.id, now_ms)?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::apply::tests::{device, highlight, publish_next, SYNC_BOOK};
    use crate::sync::apply::{apply_pending, receive, receive_body, Batch, Received};
    use crate::sync::ids::Id16;

    fn now() -> i64 {
        crate::store::now_ms()
    }

    fn key(i: usize) -> Key {
        Key::parse(&format!("note:00000000-0000-4000-8000-{i:012x}"))
            .unwrap()
            .0
    }

    fn notes(store: &mut Store, book: &str, count: usize) {
        for i in 0..count {
            let mut h = highlight(book, Some("before"), now());
            h.id = key(i).as_str().strip_prefix("note:").unwrap().to_owned();
            store.save_annotation(&h).unwrap();
        }
    }

    fn edit(store: &Store, lib: &Library, key: &Key, text: &str) {
        let v = journal::record_local(
            store.conn(),
            lib,
            key,
            LocalContent::Body(json!({ "text": text }).to_string().into_bytes()),
            now() as u64,
        )
        .unwrap();
        let mut tally = Tally::default();
        tally.restoring = true;
        assert!(apply::write(store.conn(), lib, &v, &mut tally).unwrap());
    }

    fn accept(store: &Store, lib: &Library, batch: &Batch) {
        assert!(matches!(
            receive(store.conn(), lib, batch, now()).unwrap(),
            Received::Applied { .. }
        ));
    }

    fn text(store: &Store, i: usize) -> String {
        store
            .conn()
            .query_row(
                "SELECT note FROM annotations WHERE id = ?1",
                [key(i).as_str().strip_prefix("note:").unwrap()],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn count(store: &Store, table: &str) -> usize {
        store
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap() as usize
    }

    fn mark(batch: &Batch) -> Mark {
        Mark {
            s: batch.seq,
            h: batch.hash,
        }
    }

    fn choice_body(store: &Store, lib: &Library, id: Entry) -> Value {
        let key = Key::parse(&format!("choice:{}.{}.{}", id.device, id.seq, id.hash))
            .unwrap()
            .0;
        match journal::unsent_of(store.conn(), lib, &key)
            .unwrap()
            .unwrap()
            .content
        {
            Content::Inline(body) => body,
            _ => panic!("not a choice"),
        }
    }

    #[test]
    fn a_removed_devices_change_past_the_cutoff_is_reverted_and_offered() {
        let (mut a, la) = device(1, "a");
        let (d, ld) = device(2, "d");
        let (t, lt) = device(3, "t");
        notes(&mut a, "a", 2);
        let initial = publish_next(a.conn(), &la).unwrap().unwrap();
        accept(&d, &ld, &initial);
        accept(&t, &lt, &initial);
        edit(&d, &ld, &key(0), "D's first");
        edit(&d, &ld, &key(1), "D's second");
        let late = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &late);
        accept(&a, &la, &late);
        edit(&a, &la, &key(1), "A changed it afterwards");
        accept(&t, &lt, &publish_next(a.conn(), &la).unwrap().unwrap());
        assert_eq!(
            t.conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND key = ?2",
                    params![ld.device.to_string(), key(1).as_str()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, None)]),
                &BTreeMap::new(),
                "removed",
                now() + 1
            )
            .unwrap(),
            Reverted {
                reverted: 1,
                left: 1,
                listed: 2,
                dropped: 0
            }
        );
        assert_eq!(text(&t, 0), "before");
        assert_eq!(text(&t, 1), "A changed it afterwards");
        assert!(apply::stopped(t.conn(), &lt, ld.device).unwrap());
        let restored = shown(t.conn(), &lt, &key(0)).unwrap().unwrap();
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, None)]),
                &BTreeMap::new(),
                "removed",
                now() + 2
            )
            .unwrap(),
            Reverted::default()
        );
        assert_eq!(shown(t.conn(), &lt, &key(0)).unwrap().unwrap(), restored);
        let offers = rows(t.conn(), &lt, Some(ld.device)).unwrap();
        let first = offers.iter().find(|r| r.key == key(0)).unwrap().id;
        let second = offers.iter().find(|r| r.key == key(1)).unwrap().id;
        keep(t.conn(), &lt, ld.device, &key(0), late.seq, now() + 3).unwrap();
        assert_eq!(text(&t, 0), "D's first");
        assert_eq!(count(&t, "sync_late"), 2, "Keep remains pending");
        let kept = journal::unsent_of(t.conn(), &lt, &key(0)).unwrap().unwrap();
        assert_eq!(kept.device, lt.device);
        assert_ne!(kept.hash, first.hash);
        assert!(kept.history.contains(&first));
        keep(t.conn(), &lt, ld.device, &key(0), late.seq, now() + 4).unwrap();
        assert_eq!(
            journal::unsent_of(t.conn(), &lt, &key(0)).unwrap().unwrap(),
            kept
        );
        assert_eq!(finish_keeps(t.conn(), &lt).unwrap(), 0);
        let numbered = publish_next(t.conn(), &lt).unwrap().unwrap();
        assert_eq!(finish_keeps(t.conn(), &lt).unwrap(), 1);
        assert_eq!(
            choice_body(&t, &lt, first),
            json!({ "choice": "keep", "version": {
                "d": lt.device.to_string(), "s": numbered.seq, "h": kept.hash.to_string()
            }})
        );
        assert_eq!(finish_keeps(t.conn(), &lt).unwrap(), 0);
        discard(t.conn(), &lt, ld.device, &key(1), late.seq, now() + 5).unwrap();
        assert_eq!(
            choice_body(&t, &lt, second),
            json!({ "choice": "discard", "version": entry(&second) })
        );
        assert_eq!(count(&t, "sync_late"), 0);
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, None)]),
                &BTreeMap::new(),
                "removed",
                now() + 6
            )
            .unwrap(),
            Reverted::default()
        );
        edit(&d, &ld, &key(0), "must not arrive");
        assert_eq!(
            receive(
                t.conn(),
                &lt,
                &publish_next(d.conn(), &ld).unwrap().unwrap(),
                now() + 7
            )
            .unwrap(),
            Received::Stopped
        );
        assert_eq!(text(&t, 0), "D's first");
    }

    fn fork() -> (Store, Library, Library, Vec<Batch>) {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 3);
        let mut batches = vec![publish_next(d.conn(), &ld).unwrap().unwrap()];
        for i in 0..3 {
            edit(&d, &ld, &key(i), &format!("fork A change {}", i + 2));
            batches.push(publish_next(d.conn(), &ld).unwrap().unwrap());
        }
        for b in &batches {
            accept(&t, &lt, b);
        }
        (t, lt, ld, batches)
    }

    #[test]
    fn a_fork_applied_before_its_proof_is_reverted_when_the_proof_arrives() {
        let (t, lt, ld, batches) = fork();
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, None)]),
                &BTreeMap::from([(ld.device, mark(&batches[0]))]),
                "quarantined",
                now() + 1
            )
            .unwrap(),
            Reverted {
                reverted: 3,
                left: 0,
                listed: 3,
                dropped: 0
            }
        );
        for i in 0..3 {
            assert_eq!(text(&t, i), "before");
        }
        assert_eq!(
            t.conn()
                .query_row("SELECT reason FROM sync_stopped", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "quarantined"
        );
        publish_next(t.conn(), &lt).unwrap().unwrap();
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&batches[2]), now() + 2).unwrap(),
            2
        );
        assert_eq!(text(&t, 0), "fork A change 2");
        assert_eq!(text(&t, 1), "fork A change 3");
        assert_eq!(text(&t, 2), "before");
        let held = rows(t.conn(), &lt, Some(ld.device)).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].id.seq, 4);
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&batches[2]), now() + 3).unwrap(),
            0
        );
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, None)]),
                &BTreeMap::from([(ld.device, mark(&batches[0]))]),
                "quarantined",
                now() + 4
            )
            .unwrap(),
            Reverted::default()
        );
        assert!(journal::unsent(t.conn(), &lt)
            .unwrap()
            .iter()
            .all(|v| v.kind != Kind::Choice));
    }

    // SP26 cap fixtures isolate the late-list budget from receive's separate
    // guard/per-book limits. Versions and bodies pass the normal format checks.
    fn seed(conn: &Connection, lib: &Library, device: DeviceId, n: usize, bytes: &[u8]) {
        let hash = journal::put_body(conn, bytes).unwrap();
        for i in 0..n {
            let raw = json!({ "format": 1, "key": key(i).as_str(), "device": device.to_string(),
                "clock": [now(), 0], "history": [], "body": { "hash": hash.to_string(), "size": bytes.len() } });
            let p = Placed {
                version: checked(&raw.to_string(), device).unwrap(),
                seq: i as u64 + 1,
            };
            journal::store_remote(conn, lib, &p).unwrap();
        }
        conn.execute(
            "UPDATE sync_versions SET applied = 1 WHERE library = ?1 AND device = ?2",
            params![lib.id.to_string(), device.to_string()],
        )
        .unwrap();
    }

    #[test]
    fn a_removed_devices_late_changes_stop_at_the_cap() {
        let (t, lt) = device(2, "t");
        let d = Id16([1; 16]);
        seed(t.conn(), &lt, d, MAX_LATE + 1, br#"{"text":"late"}"#);
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(d, None)]),
                &BTreeMap::new(),
                "removed",
                now() + 1
            )
            .unwrap(),
            Reverted {
                reverted: 501,
                left: 0,
                listed: 500,
                dropped: 1
            }
        );
        assert_eq!(count(&t, "sync_late"), MAX_LATE);
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(d, None)]),
                &BTreeMap::new(),
                "removed",
                now() + 2
            )
            .unwrap(),
            Reverted::default()
        );
        assert_eq!(
            t.conn()
                .query_row("SELECT dropped FROM sync_late_over", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        discard_listed(t.conn(), &lt, d, now() + 3).unwrap();
        assert_eq!(count(&t, "sync_late"), 0);
        assert_eq!(
            t.conn()
                .query_row("SELECT dropped FROM sync_late_over", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            journal::unsent(t.conn(), &lt)
                .unwrap()
                .iter()
                .filter(|v| v.kind == Kind::Choice)
                .count(),
            MAX_LATE
        );
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(d, None)]),
                &BTreeMap::new(),
                "removed",
                now() + 4
            )
            .unwrap(),
            Reverted::default()
        );
        let (b, lb) = device(3, "b");
        // Real limits: one shared 4 MiB body charged to each version. Sixteen
        // fit exactly; the seventeenth exceeds 64 MiB. No test-only relaxation.
        let big = json!({ "text": "x".repeat(Kind::Note.body_limit() as usize - 11) })
            .to_string()
            .into_bytes();
        assert_eq!(big.len() as u64, Kind::Note.body_limit());
        seed(b.conn(), &lb, d, 17, &big);
        let result = revert_past(
            b.conn(),
            &lb,
            &BTreeMap::from([(d, None)]),
            &BTreeMap::new(),
            "removed",
            now() + 1,
        )
        .unwrap();
        assert_eq!((result.listed, result.dropped), (16, 1));
        assert_eq!(
            b.conn()
                .query_row("SELECT SUM(bytes) FROM sync_late", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            MAX_LATE_BYTES as i64
        );
        let other = Id16([4; 16]);
        seed(b.conn(), &lb, other, 1, &big);
        assert_eq!(
            revert_past(
                b.conn(),
                &lb,
                &BTreeMap::from([(other, None)]),
                &BTreeMap::new(),
                "removed",
                now() + 2
            )
            .unwrap()
            .listed,
            1
        );
    }

    #[test]
    fn a_version_received_before_the_removal_is_held_not_applied() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 1);
        let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &initial);
        edit(&d, &ld, &key(0), "pending content");
        let mut late = publish_next(d.conn(), &ld).unwrap().unwrap();
        let body = late.bodies.pop().unwrap();
        assert!(late.bodies.is_empty());
        accept(&t, &lt, &late);
        let state = || {
            t.conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = ?2 AND key = ?3",
                    params![ld.device.to_string(), late.seq as i64, key(0).as_str()],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(state(), 0);
        assert_eq!(text(&t, 0), "before");
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
                &BTreeMap::new(),
                "removed",
                now()
            )
            .unwrap(),
            Reverted {
                reverted: 0,
                left: 0,
                listed: 1,
                dropped: 0
            }
        );
        assert_eq!(state(), -5);
        let offers = rows(t.conn(), &lt, Some(ld.device)).unwrap();
        assert!(offers[0].pending);
        assert!(offers[0].restored.is_none());
        assert!(
            !receive_body(t.conn(), &lt, &body).unwrap(),
            "normal body fetching excludes late holds"
        );
        // The body becomes available locally, but the held version must stay out
        // of both merging and apply_pending until its prefix is resolved.
        journal::put_body(t.conn(), &body).unwrap();
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
        assert_eq!(text(&t, 0), "before");
        assert_eq!(state(), -5);
        assert!(journal::record_of(t.conn(), &lt, &key(0), Which::All)
            .unwrap()
            .versions()
            .all(|p| p.seq != late.seq));
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&late), now()).unwrap(),
            0,
            "release queues work; it writes no record itself"
        );
        assert_eq!(state(), 0);
        assert_eq!(count(&t, "sync_late"), 0);
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
                &BTreeMap::new(),
                "removed",
                now()
            )
            .unwrap(),
            Reverted::default()
        );
        assert_eq!(
            state(),
            0,
            "repeat removal must not re-hold the resolved prefix"
        );
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 1);
        assert_eq!(text(&t, 0), "pending content");
        assert_eq!(state(), 1);
    }

    #[test]
    fn every_pending_state_is_held_and_can_be_kept_or_discarded() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 4);
        let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &initial);
        for i in 0..4 {
            edit(&d, &ld, &key(i), &format!("pending {i}"));
        }
        let mut late = publish_next(d.conn(), &ld).unwrap().unwrap();
        let bodies = std::mem::take(&mut late.bodies);
        accept(&t, &lt, &late);
        // Valid received versions in each waiting state, with their bodies now
        // available. A recheck would release -1/-2 if the removal didn't hold them.
        for (i, state) in [0, -1, -2, -4].into_iter().enumerate() {
            t.conn().execute("UPDATE sync_versions SET applied = ?1 WHERE device = ?2 AND seq = ?3 AND key = ?4",
                params![state, ld.device.to_string(), late.seq as i64, key(i).as_str()]).unwrap();
        }
        for body in bodies {
            journal::put_body(t.conn(), &body).unwrap();
        }
        let result = revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
            &BTreeMap::new(),
            "quarantined",
            now(),
        )
        .unwrap();
        assert_eq!(
            result,
            Reverted {
                reverted: 0,
                left: 0,
                listed: 4,
                dropped: 0
            }
        );
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
        assert_eq!(
            t.conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_versions WHERE applied = -5",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            4
        );
        let offers = rows(t.conn(), &lt, Some(ld.device)).unwrap();
        assert!(offers.iter().all(|r| r.pending && r.restored.is_none()));
        for i in 0..4 {
            assert_eq!(text(&t, i), "before");
        }
        keep(t.conn(), &lt, ld.device, &key(0), late.seq, now()).unwrap();
        assert_eq!(text(&t, 0), "pending 0");
        let kept = journal::unsent_of(t.conn(), &lt, &key(0)).unwrap().unwrap();
        assert!(kept.history.contains(&offers[0].id));
        assert_eq!(finish_keeps(t.conn(), &lt).unwrap(), 0);
        let numbered = publish_next(t.conn(), &lt).unwrap().unwrap();
        assert_eq!(finish_keeps(t.conn(), &lt).unwrap(), 1);
        assert_eq!(
            choice_body(&t, &lt, offers[0].id),
            json!({ "choice": "keep", "version": {
                "d": lt.device.to_string(), "s": numbered.seq, "h": kept.hash.to_string()
            }})
        );
        discard(t.conn(), &lt, ld.device, &key(1), late.seq, now()).unwrap();
        assert_eq!(
            choice_body(&t, &lt, offers[1].id),
            json!({ "choice": "discard", "version": entry(&offers[1].id) })
        );
        assert_eq!(text(&t, 1), "before");
        assert_eq!(count(&t, "sync_late"), 2);
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
    }

    #[test]
    fn a_late_overflow_drops_the_entire_tail_even_after_room_is_freed() {
        let (t, lt) = device(2, "t");
        let d = Id16([1; 16]);
        // Sixteen bodies leave 16 bytes free. The next large body overflows,
        // while a subsequent 12-byte body would fit if the cap weren't one-way.
        let big = json!({ "text": "x".repeat(Kind::Note.body_limit() as usize - 12) })
            .to_string()
            .into_bytes();
        seed(t.conn(), &lt, d, 17, &big);
        let small = |seq: u64| {
            let body = br#"{"text":"x"}"#;
            let hash = journal::put_body(t.conn(), body).unwrap();
            let raw = json!({ "format": 1, "key": key(seq as usize).as_str(), "device": d.to_string(),
                "clock": [now(), 0], "history": [], "body": { "hash": hash.to_string(), "size": body.len() } });
            journal::store_remote(
                t.conn(),
                &lt,
                &Placed {
                    version: checked(&raw.to_string(), d).unwrap(),
                    seq,
                },
            )
            .unwrap();
        };
        small(18);
        let first = revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(d, None)]),
            &BTreeMap::new(),
            "removed",
            now(),
        )
        .unwrap();
        assert_eq!((first.listed, first.dropped), (16, 2));
        assert!(rows(t.conn(), &lt, Some(d))
            .unwrap()
            .iter()
            .all(|r| r.id.seq <= 16));
        assert_eq!(
            t.conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = 18",
                    [d.to_string()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            -5
        );
        discard_listed(t.conn(), &lt, d, now()).unwrap();
        assert_eq!(count(&t, "sync_late"), 0);
        assert_eq!(
            t.conn()
                .query_row("SELECT dropped FROM sync_late_over", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        small(19);
        let next = revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(d, None)]),
            &BTreeMap::new(),
            "removed",
            now(),
        )
        .unwrap();
        assert_eq!(
            next,
            Reverted {
                reverted: 0,
                left: 0,
                listed: 0,
                dropped: 1
            }
        );
        assert_eq!(
            t.conn()
                .query_row("SELECT dropped FROM sync_late_over", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(d, None)]),
                &BTreeMap::new(),
                "removed",
                now()
            )
            .unwrap(),
            Reverted::default()
        );
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
        assert_eq!(
            t.conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_versions WHERE applied = -5",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
    }

    #[test]
    fn a_removal_during_recovery_reverts_nothing_everyone_accepted() {
        let (t, lt, ld, batches) = fork();
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&batches[0])))]),
                &BTreeMap::from([(ld.device, mark(&batches[2]))]),
                "removed",
                now() + 1
            )
            .unwrap(),
            Reverted {
                reverted: 1,
                left: 0,
                listed: 1,
                dropped: 0
            }
        );
        assert_eq!(text(&t, 0), "fork A change 2");
        assert_eq!(text(&t, 1), "fork A change 3");
        assert_eq!(text(&t, 2), "before");
        assert!(rows(t.conn(), &lt, Some(ld.device))
            .unwrap()
            .iter()
            .all(|r| r.id.seq > 3));
        let (u, lu, du, batches) = fork();
        assert_eq!(
            revert_past(
                u.conn(),
                &lu,
                &BTreeMap::from([(du.device, Some(mark(&batches[3])))]),
                &BTreeMap::from([(du.device, mark(&batches[0]))]),
                "removed",
                now() + 1
            )
            .unwrap(),
            Reverted::default()
        );
        assert_eq!(text(&u, 2), "fork A change 4");
    }

    #[test]
    fn reapply_preserves_intervening_local_and_remote_edits() {
        let (t, lt, ld, batches) = fork();
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, None)]),
            &BTreeMap::from([(ld.device, mark(&batches[0]))]),
            "quarantined",
            now() + 1,
        )
        .unwrap();
        edit(&t, &lt, &key(0), "reader changed it");
        let (r, lr) = device(3, "r");
        for b in &batches {
            accept(&r, &lr, b);
        }
        accept(&r, &lr, &publish_next(t.conn(), &lt).unwrap().unwrap());
        edit(&r, &lr, &key(1), "another device changed it");
        accept(&t, &lt, &publish_next(r.conn(), &lr).unwrap().unwrap());
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&batches[2]), now() + 2).unwrap(),
            0
        );
        assert_eq!(text(&t, 0), "reader changed it");
        assert_eq!(text(&t, 1), "another device changed it");
        assert_eq!(count(&t, "sync_late"), 1);
    }

    #[test]
    fn several_late_versions_of_one_record_reapply_in_order() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 1);
        let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &initial);
        let mut changes = vec![];
        for s in 2..=4 {
            edit(&d, &ld, &key(0), &format!("change {s}"));
            let b = publish_next(d.conn(), &ld).unwrap().unwrap();
            accept(&t, &lt, &b);
            changes.push(b);
        }
        let result = revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
            &BTreeMap::new(),
            "quarantined",
            now() + 1,
        )
        .unwrap();
        assert_eq!((result.reverted, result.listed), (1, 3));
        assert_eq!(text(&t, 0), "before");
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&changes[1]), now() + 2).unwrap(),
            2
        );
        assert_eq!(text(&t, 0), "change 3");
        assert_eq!(count(&t, "sync_late"), 1);
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&changes[2]), now() + 3).unwrap(),
            1
        );
        assert_eq!(text(&t, 0), "change 4");
    }

    #[test]
    fn missing_ancestry_or_body_rolls_back_the_entire_revert() {
        for missing_body in [false, true] {
            let (t, lt, ld, batches) = fork();
            let original = batches[0]
                .versions
                .iter()
                .find(|v| v["key"] == key(0).as_str())
                .unwrap();
            if missing_body {
                t.conn()
                    .execute(
                        "DELETE FROM sync_bodies WHERE hash = ?1",
                        [original["body"]["hash"].as_str().unwrap()],
                    )
                    .unwrap();
            } else {
                t.conn()
                    .execute(
                        "DELETE FROM sync_versions WHERE device = ?1 AND seq = 1 AND key = ?2",
                        params![ld.device.to_string(), key(0).as_str()],
                    )
                    .unwrap();
            }
            let error = revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&batches[0])))]),
                &BTreeMap::new(),
                "removed",
                now() + 1,
            )
            .unwrap_err();
            assert!(if missing_body {
                matches!(error, LateError::MissingBody(_))
            } else {
                matches!(error, LateError::MissingVersion(..))
            });
            assert_eq!(text(&t, 0), "fork A change 2");
            assert_eq!(count(&t, "sync_late"), 0);
            assert_eq!(count(&t, "sync_late_seen"), 0);
            assert!(!apply::stopped(t.conn(), &lt, ld.device).unwrap());
        }
    }

    #[test]
    fn new_records_become_deletions_and_reading_times_are_restored() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 1);
        accept(&t, &lt, &publish_next(d.conn(), &ld).unwrap().unwrap());
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, None)]),
            &BTreeMap::new(),
            "removed",
            now() + 1,
        )
        .unwrap();
        assert!(t.annotations("t").unwrap().is_empty());
        assert!(shown(t.conn(), &lt, &key(0)).unwrap().unwrap().is_deleted());
        for kind in ["opened", "finished"] {
            let (a, la) = device(1, "a");
            let (b, lb) = device(2, "b");
            let key = Key::parse(&format!("bookRead:{SYNC_BOOK}:{kind}"))
                .unwrap()
                .0;
            let body = if kind == "opened" {
                json!({})
            } else {
                json!({ "finished": true })
            };
            journal::record_local(
                a.conn(),
                &la,
                &key,
                LocalContent::Body(body.to_string().into_bytes()),
                now() as u64,
            )
            .unwrap();
            let first = publish_next(a.conn(), &la).unwrap().unwrap();
            let original_time = first.versions[0]["clock"][0].as_i64().unwrap();
            accept(&b, &lb, &first);
            journal::record_local(
                a.conn(),
                &la,
                &key,
                LocalContent::Body(body.to_string().into_bytes()),
                now() as u64 + 100,
            )
            .unwrap();
            accept(&b, &lb, &publish_next(a.conn(), &la).unwrap().unwrap());
            revert_past(
                b.conn(),
                &lb,
                &BTreeMap::from([(la.device, Some(mark(&first)))]),
                &BTreeMap::new(),
                "removed",
                now() + 200,
            )
            .unwrap();
            assert_eq!(
                b.conn()
                    .query_row(
                        &format!("SELECT {kind}_at FROM books WHERE id = 'b'"),
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                original_time
            );
        }
    }

    #[test]
    fn a_restoration_settles_a_conflict_with_a_hidden_local_head() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 1);
        let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &initial);
        edit(&t, &lt, &key(0), "local concurrent edit");
        let own = publish_next(t.conn(), &lt).unwrap().unwrap();
        journal::record_local(
            d.conn(),
            &ld,
            &key(0),
            LocalContent::Body(br#"{"text":"remote concurrent edit"}"#.to_vec()),
            now() as u64 + 100,
        )
        .unwrap();
        let remote = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &remote);
        assert_eq!(text(&t, 0), "remote concurrent edit");
        let record = journal::record_of(t.conn(), &lt, &key(0), Which::Applied).unwrap();
        assert!(matches!(
            record.outcome(&HashSet::new()),
            Some(Outcome::Conflict { .. })
        ));
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
                &BTreeMap::new(),
                "removed",
                now() + 200
            )
            .unwrap()
            .reverted,
            1
        );
        assert_eq!(text(&t, 0), "before");
        assert!(!journal::held(t.conn(), &lt, &key(0)).unwrap());
        let restored = journal::unsent_of(t.conn(), &lt, &key(0)).unwrap().unwrap();
        assert!(restored
            .history
            .iter()
            .any(|e| e.device == lt.device && e.seq == own.seq));
        assert!(restored
            .history
            .iter()
            .any(|e| e.device == ld.device && e.seq == remote.seq));
    }

    #[test]
    fn keeping_a_hidden_late_head_explicitly_descends_from_it() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        let (e, le) = device(3, "e");
        notes(&mut d, "d", 1);
        let first = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &first);
        accept(&e, &le, &first);
        edit(&d, &ld, &key(0), "late content");
        let late = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &late);
        journal::record_local(
            e.conn(),
            &le,
            &key(0),
            LocalContent::Body(br#"{"text":"concurrent content"}"#.to_vec()),
            now() as u64 + 100,
        )
        .unwrap();
        accept(&t, &lt, &publish_next(e.conn(), &le).unwrap().unwrap());
        assert_eq!(text(&t, 0), "concurrent content");
        assert_eq!(
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&first)))]),
                &BTreeMap::new(),
                "removed",
                now() + 200
            )
            .unwrap(),
            Reverted {
                reverted: 0,
                left: 1,
                listed: 1,
                dropped: 0
            }
        );
        let id = rows(t.conn(), &lt, Some(ld.device)).unwrap()[0].id;
        keep(t.conn(), &lt, ld.device, &key(0), late.seq, now() + 201).unwrap();
        let replacement = journal::unsent_of(t.conn(), &lt, &key(0)).unwrap().unwrap();
        assert!(replacement.history.contains(&id));
        assert!(replacement.history.iter().any(|e| e.device == le.device));
        assert_eq!(text(&t, 0), "late content");
    }

    #[test]
    fn an_unsent_reader_edit_is_left_and_an_overwritten_keep_is_not_finished() {
        let (t, lt, ld, batches) = fork();
        edit(&t, &lt, &key(0), "unsent reader edit");
        let result = revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&batches[0])))]),
            &BTreeMap::new(),
            "removed",
            now() + 1,
        )
        .unwrap();
        assert_eq!((result.reverted, result.left, result.listed), (2, 1, 3));
        assert_eq!(text(&t, 0), "unsent reader edit");
        keep(t.conn(), &lt, ld.device, &key(1), 3, now() + 2).unwrap();
        edit(&t, &lt, &key(1), "edited after Keep");
        publish_next(t.conn(), &lt).unwrap().unwrap();
        assert_eq!(finish_keeps(t.conn(), &lt).unwrap(), 0);
        assert_eq!(text(&t, 1), "edited after Keep");
        assert_eq!(
            rows(t.conn(), &lt, Some(ld.device))
                .unwrap()
                .iter()
                .filter(|r| r.state == "kept")
                .count(),
            1
        );
        assert!(journal::unsent(t.conn(), &lt)
            .unwrap()
            .iter()
            .all(|v| v.kind != Kind::Choice));
    }

    #[test]
    fn a_released_version_keeps_waiting_for_its_ancestry() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        let (e, le) = device(3, "e");
        notes(&mut d, "d", 1);
        let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &initial);
        accept(&e, &le, &initial);
        edit(&e, &le, &key(0), "missing ancestor");
        let ancestor = publish_next(e.conn(), &le).unwrap().unwrap();
        accept(&d, &ld, &ancestor);
        edit(&d, &ld, &key(0), "descendant");
        let late = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &late);
        let state = || {
            t.conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = ?2 AND key = ?3",
                    params![ld.device.to_string(), late.seq as i64, key(0).as_str()],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(state(), -1);
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
            &BTreeMap::new(),
            "quarantined",
            now(),
        )
        .unwrap();
        assert_eq!(state(), -5);
        assert_eq!(
            t.conn()
                .query_row("SELECT was FROM sync_late_seen", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            -1
        );
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&late), now()).unwrap(),
            0
        );
        assert_eq!(state(), -1);
        assert_eq!(count(&t, "sync_late"), 0);
        assert_eq!(count(&t, "sync_late_seen"), 0);
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
        assert_eq!(state(), -1);
        assert_eq!(text(&t, 0), "before");
        accept(&t, &lt, &ancestor);
        assert_eq!(text(&t, 0), "descendant");
        assert_eq!(state(), 1);
    }

    #[test]
    fn a_resolved_prefix_past_the_cap_applies_again() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 3);
        let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &initial);
        let base = |k: &Key| Placed {
            version: checked(
                &initial
                    .versions
                    .iter()
                    .find(|v| v["key"] == k.as_str())
                    .unwrap()
                    .to_string(),
                ld.device,
            )
            .unwrap(),
            seq: initial.seq,
        };
        // As in the cap fixture above, seed journal states to isolate SP26 from
        // receive's separate guard budgets. Use the real 500-version offer cap.
        let stage = |k: &Key, seq: u64, text: &str, parent: Option<&Placed>, applied: bool| {
            let body = json!({ "text": text }).to_string().into_bytes();
            let hash = journal::put_body(t.conn(), &body).unwrap();
            let history: Vec<Value> = parent.into_iter().map(|p| entry(&p.id())).collect();
            let raw = json!({ "format": 1, "key": k.as_str(), "device": ld.device.to_string(),
                "clock": [now() as u64 + seq, 0], "history": history,
                "body": { "hash": hash.to_string(), "size": body.len() } });
            let p = Placed {
                version: checked(&raw.to_string(), ld.device).unwrap(),
                seq,
            };
            assert!(journal::store_remote(t.conn(), &lt, &p).unwrap());
            if applied {
                let mut tally = Tally::default();
                tally.restoring = true;
                assert!(apply::write(t.conn(), &lt, &p.version, &mut tally).unwrap());
                t.conn().execute("UPDATE sync_versions SET applied = 2 WHERE device = ?1 AND key = ?2 AND seq < ?3 AND applied IN (1, 2)",
                    params![ld.device.to_string(), k.as_str(), seq as i64]).unwrap();
                t.conn().execute("UPDATE sync_versions SET applied = 1 WHERE device = ?1 AND key = ?2 AND seq = ?3",
                    params![ld.device.to_string(), k.as_str(), seq as i64]).unwrap();
            }
            p
        };
        for i in 0..MAX_LATE {
            stage(&key(1000 + i), i as u64 + 2, "filler", None, false);
        }
        let first = MAX_LATE as u64 + 2;
        let a = stage(
            &key(0),
            first,
            "over cap applied one",
            Some(&base(&key(0))),
            true,
        );
        stage(&key(0), first + 1, "over cap applied two", Some(&a), true);
        stage(
            &key(1),
            first + 2,
            "over cap pending one",
            Some(&base(&key(1))),
            false,
        );
        stage(
            &key(2),
            first + 3,
            "over cap pending two",
            Some(&base(&key(2))),
            false,
        );
        let result = revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
            &BTreeMap::new(),
            "quarantined",
            now(),
        )
        .unwrap();
        assert_eq!(
            (result.reverted, result.listed, result.dropped),
            (1, MAX_LATE, 4)
        );
        assert_eq!(text(&t, 0), "before");
        let over = || {
            t.conn()
                .query_row("SELECT dropped FROM sync_late_over", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
        };
        let pending_state = |seq: u64| {
            t.conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = ?2",
                    params![ld.device.to_string(), seq as i64],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(pending_state(first + 2), -5);
        // A removed offer is not overflow when the reader decided it.
        discard(t.conn(), &lt, ld.device, &key(1000), 2, now()).unwrap();
        let upto = |s| Mark {
            s,
            h: Hash32([0; 32]),
        };
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, upto(first), now()).unwrap(),
            1
        );
        assert_eq!(text(&t, 0), "over cap applied one");
        assert_eq!(over(), 3);
        assert_eq!(count(&t, "sync_late"), 0);
        assert_eq!(count(&t, "sync_late_seen"), 4);
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, upto(first + 2), now()).unwrap(),
            1
        );
        assert_eq!(text(&t, 0), "over cap applied two");
        assert_eq!(pending_state(first + 2), 0);
        assert_eq!(pending_state(first + 3), -5);
        assert_eq!(over(), 1);
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 1);
        assert_eq!(text(&t, 1), "over cap pending one");
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, upto(first + 3), now()).unwrap(),
            0
        );
        assert_eq!(count(&t, "sync_late_over"), 0);
        assert_eq!(
            count(&t, "sync_late_seen"),
            1,
            "only the reader's decision remains"
        );
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 1);
        assert_eq!(text(&t, 2), "over cap pending two");
        // Deleting recovered seen rows permits a different cutoff to process
        // those versions again; the old cutoff's watermark does not block it.
        let again = revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(upto(first + 1)))]),
            &BTreeMap::new(),
            "quarantined",
            now(),
        )
        .unwrap();
        assert_eq!((again.reverted, again.listed, again.dropped), (2, 2, 0));
    }

    #[test]
    fn a_discarded_late_change_isnt_reapplied() {
        for all in [false, true] {
            let (t, lt, ld, batches) = fork();
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&batches[0])))]),
                &BTreeMap::new(),
                "quarantined",
                now(),
            )
            .unwrap();
            if all {
                discard_listed(t.conn(), &lt, ld.device, now()).unwrap();
            } else {
                discard(t.conn(), &lt, ld.device, &key(0), 2, now()).unwrap();
                keep(t.conn(), &lt, ld.device, &key(1), 3, now()).unwrap();
            }
            let decisions: i64 = t
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_late_seen WHERE decided = 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(decisions, if all { 3 } else { 2 });
            assert_eq!(
                reapply_upto(t.conn(), &lt, ld.device, mark(&batches[3]), now()).unwrap(),
                usize::from(!all)
            );
            assert_eq!(text(&t, 0), "before");
            assert_eq!(text(&t, 1), if all { "before" } else { "fork A change 3" });
            assert_eq!(text(&t, 2), if all { "before" } else { "fork A change 4" });
            assert_eq!(count(&t, "sync_late_seen"), decisions as usize);
            assert_eq!(count(&t, "sync_late_over"), 0);
            if !all {
                assert_eq!(
                    count(&t, "sync_late"),
                    1,
                    "Keep remains pending publication"
                );
                publish_next(t.conn(), &lt).unwrap().unwrap();
                assert_eq!(finish_keeps(t.conn(), &lt).unwrap(), 1);
            }
            assert_eq!(count(&t, "sync_late"), 0);
            assert_eq!(
                reapply_upto(t.conn(), &lt, ld.device, mark(&batches[3]), now()).unwrap(),
                0
            );
        }
    }

    #[test]
    fn released_clock_and_body_exclusions_are_preserved() {
        for was in [-2, -4] {
            let (mut d, ld) = device(1, "d");
            let (t, lt) = device(2, "t");
            notes(&mut d, "d", 1);
            let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
            accept(&t, &lt, &initial);
            let future = now() as u64 + super::super::clock::SET_ASIDE_MS + 60_000;
            let (body, wall) = if was == -2 {
                (br#"{"text":"ahead"}"#.to_vec(), future)
            } else {
                (br#"{"invalid":true}"#.to_vec(), now() as u64)
            };
            journal::record_local(d.conn(), &ld, &key(0), LocalContent::Body(body), wall).unwrap();
            let late = publish_next(d.conn(), &ld).unwrap().unwrap();
            accept(&t, &lt, &late);
            let state = || {
                t.conn().query_row(
                "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = ?2 AND key = ?3",
                params![ld.device.to_string(), late.seq as i64, key(0).as_str()], |r| r.get::<_, i64>(0)).unwrap()
            };
            assert_eq!(state(), was);
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, Some(mark(&initial)))]),
                &BTreeMap::new(),
                "quarantined",
                now(),
            )
            .unwrap();
            assert_eq!(state(), -5);
            reapply_upto(t.conn(), &lt, ld.device, mark(&late), now()).unwrap();
            assert_eq!(state(), was);
            assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
            assert_eq!(state(), was);
            assert_eq!(text(&t, 0), "before");
            if was == -2 {
                assert_eq!(apply_pending(t.conn(), &lt, future as i64).unwrap(), 1);
                assert_eq!(state(), 1);
                assert_eq!(text(&t, 0), "ahead");
            }
        }
    }

    // SP26: the second removed writer descends from the first one's late edit.
    fn cascade(a_id: u8, b_id: u8) -> (Store, Library, Library, Library, Batch) {
        let (mut a, la) = device(a_id, "a");
        let (b, lb) = device(b_id, "b");
        let (t, lt) = device(3, "t");
        notes(&mut a, "a", 1);
        let initial = publish_next(a.conn(), &la).unwrap().unwrap();
        accept(&b, &lb, &initial);
        accept(&t, &lt, &initial);
        edit(&a, &la, &key(0), "A late");
        let late = publish_next(a.conn(), &la).unwrap().unwrap();
        accept(&b, &lb, &late);
        accept(&t, &lt, &late);
        edit(&b, &lb, &key(0), "B late");
        accept(&t, &lt, &publish_next(b.conn(), &lb).unwrap().unwrap());
        (t, lt, la, lb, initial)
    }

    #[test]
    fn a_cascade_removal_restores_what_came_before_both() {
        // SP26: neither device iteration order may leave the other's late edit.
        for (a, b) in [(1, 2), (2, 1)] {
            let (t, lt, la, lb, initial) = cascade(a, b);
            let removed = BTreeMap::from([(la.device, Some(mark(&initial))), (lb.device, None)]);
            assert_eq!(text(&t, 0), "B late");
            let result =
                revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
            assert_eq!(
                result,
                Reverted {
                    reverted: 1,
                    left: 0,
                    listed: 2,
                    dropped: 0
                }
            );
            assert_eq!(text(&t, 0), "before");
            for d in [la.device, lb.device] {
                assert!(apply::stopped(t.conn(), &lt, d).unwrap());
                assert_eq!(rows(t.conn(), &lt, Some(d)).unwrap().len(), 1);
            }
            assert_eq!(count(&t, "sync_late_passes"), 2);
            assert_eq!(
                revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap(),
                Reverted::default()
            );
        }
    }

    #[test]
    fn a_cascades_shared_restoration_reapplies_for_each_device() {
        // #163: A and B have late versions of one record, so one restoration stands
        // for both. Re-applying A, then B, ends on B's version.
        let (t, lt, la, lb, initial) = cascade(1, 2);
        let removed = BTreeMap::from([(la.device, Some(mark(&initial))), (lb.device, None)]);
        revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
        assert_eq!(text(&t, 0), "before");
        let at = |d: DeviceId, s: i64| -> Mark {
            let h: String = t
                .conn()
                .query_row(
                    "SELECT hash FROM sync_versions WHERE device = ?1 AND seq = ?2",
                    params![d.to_string(), s],
                    |r| r.get(0),
                )
                .unwrap();
            Mark {
                s: s as u64,
                h: Hash32::parse(&h).unwrap(),
            }
        };
        reapply_upto(t.conn(), &lt, la.device, at(la.device, 2), now()).unwrap();
        assert_eq!(text(&t, 0), "A late");
        reapply_upto(t.conn(), &lt, lb.device, at(lb.device, 1), now()).unwrap();
        assert_eq!(text(&t, 0), "B late");
        for d in [la.device, lb.device] {
            assert!(rows(t.conn(), &lt, Some(d)).unwrap().is_empty());
        }
    }

    #[test]
    fn a_removed_writers_restoration_is_reverted_when_a_related_device_is_removed_later() {
        for published in [false, true] {
            for reader_edit in [false, true] {
                let (t, lt, la, lb, initial) = cascade(1, 2);
                let mut removed = BTreeMap::from([(lb.device, None)]);
                revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
                assert_eq!(text(&t, 0), "A late");
                if published {
                    publish_next(t.conn(), &lt).unwrap().unwrap();
                }
                if reader_edit {
                    edit(&t, &lt, &key(0), "reader's later edit");
                }
                removed.insert(la.device, Some(mark(&initial)));
                let result =
                    revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now())
                        .unwrap();
                assert_eq!(result.reverted, usize::from(!reader_edit));
                assert_eq!(
                    text(&t, 0),
                    if reader_edit {
                        "reader's later edit"
                    } else {
                        "before"
                    }
                );
                assert_eq!(
                    revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now())
                        .unwrap(),
                    Reverted::default()
                );
            }
        }
    }

    #[test]
    fn a_cutoff_that_moves_earlier_reverts_the_restoration_again() {
        for published in [false, true] {
            let (mut d, ld) = device(1, "d");
            let (t, lt) = device(2, "t");
            notes(&mut d, "d", 1);
            let first = publish_next(d.conn(), &ld).unwrap().unwrap();
            accept(&t, &lt, &first);
            edit(&d, &ld, &key(0), "version 2");
            let second = publish_next(d.conn(), &ld).unwrap().unwrap();
            accept(&t, &lt, &second);
            edit(&d, &ld, &key(0), "version 3");
            accept(&t, &lt, &publish_next(d.conn(), &ld).unwrap().unwrap());
            let mut removed = BTreeMap::from([(ld.device, Some(mark(&second)))]);
            revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
            assert_eq!(text(&t, 0), "version 2");
            if published {
                publish_next(t.conn(), &lt).unwrap().unwrap();
            }
            removed.insert(ld.device, Some(mark(&first)));
            assert_eq!(
                revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now())
                    .unwrap()
                    .reverted,
                1
            );
            assert_eq!(text(&t, 0), "before");
            assert_eq!(rows(t.conn(), &lt, Some(ld.device)).unwrap().len(), 2);
            assert_eq!(
                revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap(),
                Reverted::default()
            );
        }
    }

    // SP30: version 2 applied, version 3 received without its body, then held.
    fn applied_then_held() -> (Store, Library, Library, Batch, Vec<u8>) {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 1);
        let first = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &first);
        edit(&d, &ld, &key(0), "version 2");
        accept(&t, &lt, &publish_next(d.conn(), &ld).unwrap().unwrap());
        edit(&d, &ld, &key(0), "version 3");
        let mut third = publish_next(d.conn(), &ld).unwrap().unwrap();
        let body = third.bodies.pop().unwrap();
        assert!(third.bodies.is_empty());
        accept(&t, &lt, &third);
        assert_eq!(text(&t, 0), "version 2");
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&first)))]),
            &BTreeMap::new(),
            "quarantined",
            now(),
        )
        .unwrap();
        assert_eq!(text(&t, 0), "before");
        assert_eq!(version_state(&t, ld.device, 3), -5);
        (t, lt, ld, third, body)
    }

    fn version_state(store: &Store, device: DeviceId, seq: i64) -> i64 {
        store
            .conn()
            .query_row(
                "SELECT applied FROM sync_versions WHERE device = ?1 AND key = ?2 AND seq = ?3",
                params![device.to_string(), key(0).as_str(), seq],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_released_newer_version_supersedes_the_restoration() {
        let (t, lt, ld, third, body) = applied_then_held();
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&third), now()).unwrap(),
            1
        );
        assert_eq!(text(&t, 0), "version 2");
        assert!(journal::unsent_of(t.conn(), &lt, &key(0))
            .unwrap()
            .is_none());
        assert_eq!(version_state(&t, ld.device, 2), 1);
        assert_eq!(version_state(&t, ld.device, 3), 0);
        assert_eq!(count(&t, "sync_late"), 0);
        assert_eq!(count(&t, "sync_late_seen"), 0);
        assert!(receive_body(t.conn(), &lt, &body).unwrap());
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 1);
        assert_eq!(text(&t, 0), "version 3");
        let record = journal::record_of(t.conn(), &lt, &key(0), Which::Applied).unwrap();
        assert!(
            matches!(record.outcome(&HashSet::new()), Some(Outcome::Settled(p)) if p.seq == third.seq && p.version.device == ld.device)
        );
        assert_eq!(version_state(&t, ld.device, 2), 2);
        assert_eq!(version_state(&t, ld.device, 3), 1);
    }

    #[test]
    fn a_published_restoration_leaves_only_its_own_record() {
        // SP30: one record whose restoration was published waits; the call still
        // recovers every other record.
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 2);
        let first = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &first);
        edit(&d, &ld, &key(0), "version 2");
        edit(&d, &ld, &key(1), "other 2");
        accept(&t, &lt, &publish_next(d.conn(), &ld).unwrap().unwrap());
        edit(&d, &ld, &key(0), "version 3");
        let mut third = publish_next(d.conn(), &ld).unwrap().unwrap();
        third.bodies.clear();
        accept(&t, &lt, &third);
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&first)))]),
            &BTreeMap::new(),
            "quarantined",
            now(),
        )
        .unwrap();
        assert_eq!(
            (text(&t, 0), text(&t, 1)),
            ("before".into(), "before".into())
        );
        while publish_next(t.conn(), &lt).unwrap().is_some() {}
        assert!(matches!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&third), now()),
            Err(LateError::PublishedRestorations(k)) if k == vec![key(0)]
        ));
        assert_eq!(text(&t, 1), "other 2");
        assert_eq!(text(&t, 0), "before");
        assert_eq!(version_state(&t, ld.device, 3), -5);
    }

    #[test]
    fn a_published_restoration_isnt_superseded_silently() {
        let (t, lt, ld, third, body) = applied_then_held();
        publish_next(t.conn(), &lt).unwrap().unwrap();
        // SP30: compare all table contents, including the clock and bookkeeping.
        let snapshot = || {
            let tables: Vec<String> = t
                .conn()
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            tables
                .into_iter()
                .map(|table| {
                    let mut stmt = t
                        .conn()
                        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                        .unwrap();
                    let columns = stmt.column_count();
                    stmt.query_map([], |r| {
                        (0..columns)
                            .map(|i| r.get::<_, rusqlite::types::Value>(i))
                            .collect::<std::result::Result<Vec<_>, _>>()
                    })
                    .unwrap()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .unwrap()
                })
                .collect::<Vec<_>>()
        };
        let before = snapshot();
        assert!(
            matches!(reapply_upto(t.conn(), &lt, ld.device, mark(&third), now()), Err(LateError::PublishedRestorations(k)) if k == vec![key(0)])
        );
        assert_eq!(snapshot(), before);
        journal::put_body(t.conn(), &body).unwrap();
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
        assert_eq!(text(&t, 0), "before");
        assert_eq!(version_state(&t, ld.device, 3), -5);
    }

    #[test]
    fn migration_eleven_fixture_matches_and_preserves_late_bookkeeping() {
        // Shared migration tests enumerate older tables; cover the additions here.
        let fixture = Connection::open_in_memory().unwrap();
        fixture
            .execute_batch(include_str!("../../tests/fixtures/schema-v11.sql"))
            .unwrap();
        let migrated = Store::open_in_memory().unwrap();
        for table in ["sync_late", "sync_late_over", "sync_late_seen"] {
            let schema = |c: &Connection| -> String {
                c.query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap()
            };
            assert_eq!(schema(&fixture), schema(migrated.conn()));
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fixture.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch(include_str!("../../tests/fixtures/schema-v11.sql"))
            .unwrap();
        let reopened = Store::open(&path).unwrap();
        for table in ["sync_late", "sync_late_over", "sync_late_seen"] {
            assert_eq!(count(&reopened, table), 1);
        }
    }
    #[test]
    fn migration_twelve_fixture_matches_and_preserves_restorations() {
        // SP26 (#163): migration 12 adds what each restoration stands for.
        let fixture = Connection::open_in_memory().unwrap();
        fixture
            .execute_batch(include_str!("../../tests/fixtures/schema-v12.sql"))
            .unwrap();
        let migrated = Store::open_in_memory().unwrap();
        let schema = |c: &Connection| -> String {
            c.query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'sync_late_restored'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(schema(&fixture), schema(migrated.conn()));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("fixture.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch(include_str!("../../tests/fixtures/schema-v12.sql"))
            .unwrap();
        let reopened = Store::open(&path).unwrap();
        assert_eq!(count(&reopened, "sync_late_restored"), 1);
    }
}
