//! SP26, SP30: revert and hold a removed or quarantined device's late changes.
//!
//! Pure library operations; no sync-pass or publication caller yet. The journal,
//! including superseded versions, supplies ancestry and bodies, never sync_undo.

use super::apply::{self, Tally};
use super::ids::{DeviceId, Hash32};
use super::journal::{self, JournalError, Library, LocalContent, Which};
use super::merge::{Outcome, Placed, Record};
use super::record::{check_version, Checked, Content, Entry, Key, RecordError, Version};
use super::trust::Mark;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
    #[error("local choice for {0} could not be applied")]
    NotWritten(Key),
}

pub type Result<T> = std::result::Result<T, LateError>;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reverted {
    /// Records whose shown library content changed (several versions may share one).
    pub reverted: usize,
    /// Late versions whose shown record still descends from them through a writer
    /// not being reverted. Concurrent surviving versions do not count.
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
             AND seq = ?3 AND key = ?4 AND hash = ?5 AND applied IN (1, 2, -5, -6)",
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
    let local: Option<(String, i64)> = conn.query_row(
        "SELECT raw, applied FROM sync_versions WHERE library = ?1 AND key = ?2 AND seq IS NULL",
        params![lib.id.to_string(), key.as_str()], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional()?;
    let mut held = None;
    if let Some((raw, state)) = local {
        let p = Placed {
            version: checked(&raw, lib.device)?,
            seq: u64::MAX,
        };
        if state == -3 {
            held = Some(p.clone());
        }
        record.add(p);
    }
    let current = outcome(&record).cloned();
    // SY13: match recompute/apply_key's held-edit rule when counting `left`.
    if held.is_some()
        && !current
            .as_ref()
            .is_some_and(|p| p.version.device != lib.device && p.is_deleted())
    {
        return Ok(held);
    }
    Ok(current)
}

fn outcome(record: &Record) -> Option<&Placed> {
    match record.outcome(&HashSet::new()) {
        Some(Outcome::Settled(p)) | Some(Outcome::Conflict { shown: p, .. }) => Some(p),
        None => None,
    }
}

// SP11, SP26: losing a checked ancestor is not permission to erase the
// record. Recompute checks the body it needs, not bodies of superseded versions.
fn check_ancestors(
    conn: &Connection,
    lib: &Library,
    p: &Placed,
    visited: &mut HashSet<Entry>,
) -> Result<()> {
    let mut pending = vec![p.id()];
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let raw: Option<String> = conn
            .query_row(
                "SELECT raw FROM sync_versions WHERE library = ?1 AND device = ?2
             AND seq = ?3 AND key = ?4 AND hash = ?5",
                params![
                    lib.id.to_string(),
                    id.device.to_string(),
                    id.seq as i64,
                    p.version.key.as_str(),
                    id.hash.to_string()
                ],
                |r| r.get(0),
            )
            .optional()?;
        let raw = raw.ok_or_else(|| LateError::MissingVersion(id, p.version.key.clone()))?;
        let ancestor = Placed {
            version: checked(&raw, id.device)?,
            seq: id.seq,
        };
        pending.extend(ancestor.version.history);
    }
    Ok(())
}

fn content(conn: &Connection, p: &Placed) -> Result<LocalContent> {
    Ok(match &p.version.content {
        Content::Body { hash, .. } => {
            LocalContent::Body(journal::body(conn, hash)?.ok_or(LateError::MissingBody(*hash))?)
        }
        Content::Inline(v) => LocalContent::Inline(v.clone()),
        Content::Deleted { covers, identity } => LocalContent::Deleted {
            covers: covers.clone(),
            identity: identity.clone(),
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
            // SY13: the choice settles an otherwise hidden local head,
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
    parent: &Placed,
    now_ms: i64,
) -> Result<Version> {
    let content = content(conn, parent)?;
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
    // SP30: a cutoff names a change by its hash, not only its sequence. Where this
    // device holds another change at that sequence (the device equivocated, and
    // showed the remover a different branch), the cutoff isn't on this chain: the
    // accepted-by-all mark, which a cutoff is never earlier than, stands instead.
    let mut cutoffs = BTreeMap::new();
    for (&device, cutoff) in removed {
        let on_chain = match cutoff {
            Some(m) => conn
                .query_row(
                    "SELECT hash FROM sync_changes WHERE library = ?1 AND device = ?2 AND seq = ?3",
                    params![lib.id.to_string(), device.to_string(), m.s as i64],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .map_or(true, |h| Hash32::parse(&h) == Some(m.h)),
            None => true,
        };
        let cutoff = cutoff
            .iter()
            .filter(|_| on_chain)
            .chain(accepted_by_all.get(&device))
            .map(|m| m.s)
            .max()
            .unwrap_or(0);
        cutoffs.insert(device, cutoff);
    }
    let tx = conn.unchecked_transaction()?;
    let mut result = Reverted::default();
    let mut affected = BTreeMap::<Key, Vec<Placed>>::new();
    let mut visited = HashSet::new();
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
         AND applied IN (1, 2, 0, -1, -2, -4, -5, -6)",
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
            // SP26: both applied and pending late versions leave the merge,
            // including the unlisted tail. Keep the original state for recovery.
            tx.execute(
                "UPDATE sync_versions SET applied = ?5 WHERE library = ?1
                 AND device = ?2 AND seq = ?3 AND key = ?4",
                params![
                    lib.id.to_string(),
                    device.to_string(),
                    seq,
                    key.as_str(),
                    if applied > 0 { -6 } else { -5 }
                ],
            )?;
            let late = version(&tx, lib, &key, id)?;
            if applied > 0 {
                check_ancestors(&tx, lib, &late, &mut visited)?;
                affected.entry(key.clone()).or_default().push(late.clone());
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
                "INSERT INTO sync_late_seen (library, device, seq, key, was)
             VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    lib.id.to_string(),
                    device.to_string(),
                    seq,
                    key.as_str(),
                    applied
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
    // SP26: remove every writer first, then merge what survives together.
    let mut tally = Tally::default();
    tally.restoring = true;
    for (key, late) in affected {
        result.reverted += usize::from(apply::recompute(&tx, lib, &key, now_ms, &mut tally)?);
        if let Some(current) = shown(&tx, lib, &key)? {
            if !removed.contains_key(&current.version.device) {
                result.left += late.iter().filter(|p| current.descends_from(p)).count();
            }
        }
    }
    tx.commit()?;
    Ok(result)
}

struct Late {
    key: Key,
    id: Entry,
    state: String,
    kept: Option<String>,
    at: i64,
    #[cfg(test)]
    pending: bool,
}

fn rows(conn: &Connection, lib: &Library, device: Option<DeviceId>) -> Result<Vec<Late>> {
    let raw = conn
        .prepare(
            "SELECT l.key, l.device, l.seq, l.hash, l.state, l.kept, l.at, s.was <= 0
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
                    r.get::<_, i64>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    raw.into_iter()
        .map(|(key, device, seq, hash, state, kept, at, _pending)| {
            Ok(Late {
                key: Key::parse(&key).ok_or(RecordError::Key)?.0,
                id: Entry {
                    device: DeviceId::parse(&device).ok_or(RecordError::Field("device"))?,
                    seq,
                    hash: Hash32::parse(&hash).ok_or(RecordError::Field("hash"))?,
                },
                state,
                kept,
                at,
                #[cfg(test)]
                pending: _pending,
            })
        })
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

/// SP30: recover undecided versions through the resolved prefix, including
/// overflow. Previously applied versions return to the merge; pending versions
/// regain their original state for apply_pending. Returns the number of applied
/// versions recovered, even if a surviving reader edit still shows.
pub fn reapply_upto(
    conn: &Connection,
    lib: &Library,
    device: DeviceId,
    upto: Mark,
    now_ms: i64,
) -> Result<usize> {
    // SP30: the resolved predecessor names a change by its hash. Unless this device
    // holds that very change at that sequence, what it applied up to there is
    // another branch (the device equivocated): nothing is released, and those
    // versions stay held for the reader.
    let on_chain = conn
        .query_row(
            "SELECT hash FROM sync_changes WHERE library = ?1 AND device = ?2 AND seq = ?3",
            params![lib.id.to_string(), device.to_string(), upto.s as i64],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|h| Hash32::parse(&h))
        == Some(upto.h);
    if !on_chain {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let recovering: Vec<(String, i64, i64)> = tx
        .prepare(
            "SELECT key, seq, was FROM sync_late_seen WHERE library = ?1 AND device = ?2
         AND seq <= ?3 AND decided = 0 ORDER BY seq, key",
        )?
        .query_map(
            params![lib.id.to_string(), device.to_string(), upto.s as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?
        .collect::<std::result::Result<_, _>>()?;
    let mut affected = BTreeSet::new();
    let mut n = 0;
    for (key, seq, was) in recovering {
        let key = Key::parse(&key).ok_or(RecordError::Key)?.0;
        let released = tx.execute(
            "UPDATE sync_versions SET applied = ?5 WHERE library = ?1 AND device = ?2
             AND seq = ?3 AND key = ?4 AND applied = ?6",
            params![
                lib.id.to_string(),
                device.to_string(),
                seq,
                key.as_str(),
                if was > 0 { 1 } else { was },
                if was > 0 { -6 } else { -5 }
            ],
        )?;
        if was > 0 && released > 0 {
            affected.insert(key.clone());
            n += 1;
        }
        let listed = tx.execute(
            "DELETE FROM sync_late WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
            params![lib.id.to_string(), device.to_string(), seq, key.as_str()],
        )?;
        if listed == 0 {
            // SP30: only the resolved unlisted tail reduces overflow.
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
    let mut tally = Tally::default();
    tally.restoring = true;
    for key in affected {
        apply::recompute(&tx, lib, &key, now_ms, &mut tally)?;
    }
    tx.commit()?;
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
        let v = put(&tx, lib, key, &p, now_ms)?;
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
    use crate::sync::record::Kind;

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
        assert!(publish_next(t.conn(), &lt).unwrap().is_none());
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
                reverted: 0,
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
        assert!(offers.iter().all(|r| r.pending));
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
            2
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
    fn an_applied_descendant_can_be_reverted_without_an_unneeded_ancestor_body() {
        for remove_descendant in [false, true] {
            let (mut d, ld) = device(1, "d");
            let (t, lt) = device(2, "t");
            notes(&mut d, "d", 1);
            let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
            accept(&t, &lt, &initial);
            edit(&d, &ld, &key(0), "missing body");
            let mut pending = publish_next(d.conn(), &ld).unwrap().unwrap();
            pending.bodies.clear();
            accept(&t, &lt, &pending);
            edit(&d, &ld, &key(0), "applied descendant");
            let late = publish_next(d.conn(), &ld).unwrap().unwrap();
            accept(&t, &lt, &late);
            assert_eq!(text(&t, 0), "applied descendant");
            // SP11: superseded by the descendant without ever needing its own body.
            assert_eq!(version_state(&t, ld.device, pending.seq as i64), 2);
            edit(&d, &ld, &key(0), "latest late");
            accept(&t, &lt, &publish_next(d.conn(), &ld).unwrap().unwrap());
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(
                    ld.device,
                    Some(mark(if remove_descendant { &initial } else { &late })),
                )]),
                &BTreeMap::new(),
                "removed",
                now(),
            )
            .unwrap();
            assert_eq!(
                text(&t, 0),
                if remove_descendant {
                    "before"
                } else {
                    "applied descendant"
                }
            );
            assert_eq!(
                version_state(&t, ld.device, pending.seq as i64),
                if remove_descendant { -6 } else { 2 }
            );
            assert_eq!(
                version_state(&t, ld.device, late.seq as i64),
                if remove_descendant { -6 } else { 1 }
            );
        }
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
                matches!(
                    error,
                    LateError::Journal(JournalError::Record(RecordError::Body))
                )
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
    fn new_records_disappear_and_reading_times_are_restored() {
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
        assert!(shown(t.conn(), &lt, &key(0)).unwrap().is_none());
        assert!(publish_next(t.conn(), &lt).unwrap().is_none());
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
    fn reverting_a_conflict_shows_the_surviving_local_head() {
        let (mut d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        notes(&mut d, "d", 1);
        let initial = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &initial);
        edit(&t, &lt, &key(0), "local concurrent edit");
        let _own = publish_next(t.conn(), &lt).unwrap().unwrap();
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
        assert_eq!(text(&t, 0), "local concurrent edit");
        assert!(!journal::held(t.conn(), &lt, &key(0)).unwrap());
        assert!(journal::unsent_of(t.conn(), &lt, &key(0))
            .unwrap()
            .is_none());
        assert_eq!(version_state(&t, ld.device, remote.seq as i64), -6);
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
                left: 0,
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
        // The journal was seeded directly; record the changes it names, so the
        // resolved predecessor is a change this device holds (SP30).
        let upto = |s: u64| {
            t.conn()
                .execute(
                    "INSERT OR IGNORE INTO sync_changes (library, device, seq, hash)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        lt.id.to_string(),
                        ld.device.to_string(),
                        s as i64,
                        Hash32([0; 32]).to_string()
                    ],
                )
                .unwrap();
            Mark {
                s,
                h: Hash32([0; 32]),
            }
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

    fn at(store: &Store, device: DeviceId, seq: u64) -> Mark {
        let hash: String = store
            .conn()
            .query_row(
                "SELECT hash FROM sync_versions WHERE device = ?1 AND seq = ?2 AND key = ?3",
                params![device.to_string(), seq as i64, key(0).as_str()],
                |r| r.get(0),
            )
            .unwrap();
        Mark {
            s: seq,
            h: Hash32::parse(&hash).unwrap(),
        }
    }

    /// SP29, SP30: a cutoff or resolved predecessor names a change, by its
    /// payload hash (`sync_changes`), not a version.
    fn change_mark(store: &Store, device: DeviceId, seq: u64) -> Mark {
        let hash: String = store
            .conn()
            .query_row(
                "SELECT hash FROM sync_changes WHERE device = ?1 AND seq = ?2",
                params![device.to_string(), seq as i64],
                |r| r.get(0),
            )
            .unwrap();
        Mark {
            s: seq,
            h: Hash32::parse(&hash).unwrap(),
        }
    }

    #[test]
    fn reapplying_b_then_a_ends_on_bs_version() {
        let (t, lt, la, lb, initial) = cascade(1, 2);
        let removed = BTreeMap::from([(la.device, Some(mark(&initial))), (lb.device, None)]);
        revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
        for (device, seq, was) in [(la.device, 2, 2), (lb.device, 1, 1)] {
            assert_eq!(version_state(&t, device, seq), -6);
            let original: i64 = t
                .conn()
                .query_row(
                    "SELECT was FROM sync_late_seen WHERE device = ?1 AND seq = ?2 AND key = ?3",
                    params![device.to_string(), seq, key(0).as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(original, was);
            let p = version(
                t.conn(),
                &lt,
                &key(0),
                Entry {
                    device,
                    seq: seq as u64,
                    hash: at(&t, device, seq as u64).h,
                },
            )
            .unwrap();
            if let Content::Body { hash, .. } = p.version.content {
                journal::drop_body_if_unnamed(t.conn(), &hash).unwrap();
                assert!(journal::has_body(t.conn(), &hash).unwrap());
            }
        }
        reapply_upto(
            t.conn(),
            &lt,
            lb.device,
            change_mark(&t, lb.device, 1),
            now(),
        )
        .unwrap();
        reapply_upto(
            t.conn(),
            &lt,
            la.device,
            change_mark(&t, la.device, 2),
            now(),
        )
        .unwrap();
        assert_eq!(text(&t, 0), "B late");
        assert!(publish_next(t.conn(), &lt).unwrap().is_none());
    }

    #[test]
    fn reapplying_one_device_keeps_another_devices_late_version_out() {
        let (t, lt, la, lb, initial) = cascade(1, 2);
        let parent = version(
            t.conn(),
            &lt,
            &key(0),
            Entry {
                device: la.device,
                seq: 2,
                hash: at(&t, la.device, 2).h,
            },
        )
        .unwrap();
        let bytes = br#"{"text":"A held"}"#;
        let hash = journal::put_body(t.conn(), bytes).unwrap();
        let raw = json!({ "format": 1, "key": key(0).as_str(), "device": la.device.to_string(),
            "clock": [now(), 0], "history": [entry(&parent.id())],
            "body": { "hash": hash.to_string(), "size": bytes.len() } });
        journal::store_remote(
            t.conn(),
            &lt,
            &Placed {
                version: checked(&raw.to_string(), la.device).unwrap(),
                seq: 3,
            },
        )
        .unwrap();
        let removed = BTreeMap::from([(la.device, Some(mark(&initial))), (lb.device, None)]);
        revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
        // A's change 3 was stored directly above; record it as received, so the
        // resolved predecessor names a change this device holds (SP30).
        t.conn()
            .execute(
                "INSERT INTO sync_changes (library, device, seq, hash) VALUES (?1, ?2, 3, ?3)",
                params![
                    lt.id.to_string(),
                    la.device.to_string(),
                    Hash32([3; 32]).to_string()
                ],
            )
            .unwrap();
        reapply_upto(
            t.conn(),
            &lt,
            la.device,
            change_mark(&t, la.device, 3),
            now(),
        )
        .unwrap();
        assert_eq!(text(&t, 0), "A late");
        assert_eq!(version_state(&t, lb.device, 1), -6);
        assert_eq!(version_state(&t, la.device, 3), 0);
        assert!(publish_next(t.conn(), &lt).unwrap().is_none());
    }

    #[test]
    fn a_reader_edit_after_a_late_change_stays_after_the_revert() {
        for (held, cutoff) in [(false, true), (true, true), (true, false)] {
            let (t, lt, ld, batches) = fork();
            edit(&t, &lt, &key(0), "reader's edit");
            if held {
                t.conn()
                    .execute(
                        "UPDATE sync_versions SET applied = -3 WHERE seq IS NULL",
                        [],
                    )
                    .unwrap();
            }
            let own: String = t
                .conn()
                .query_row("SELECT raw FROM sync_versions WHERE seq IS NULL", [], |r| {
                    r.get(0)
                })
                .unwrap();
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(ld.device, cutoff.then(|| mark(&batches[0])))]),
                &BTreeMap::new(),
                "removed",
                now(),
            )
            .unwrap();
            assert_eq!(text(&t, 0), "reader's edit");
            let after: String = t
                .conn()
                .query_row(
                    "SELECT raw FROM sync_versions WHERE seq IS NULL AND key = ?1",
                    [key(0).as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(after, own);
            assert_eq!(version_state(&t, ld.device, 2), -6);
            // SY13, #109: the version it was held against is out of the merge, so the
            // edit is the reader's ordinary unsent one again.
            assert!(!journal::held(t.conn(), &lt, &key(0)).unwrap());
        }
    }

    #[test]
    fn a_keep_of_a_held_version_applies_on_another_device_holding_it() {
        // SP26: D's version reached two devices before its body; both hold it after
        // the removal. One keeps it once the body arrives; the other applies that
        // Keep, since the held version's ancestry was checked before it was held.
        let (mut d, ld) = device(1, "d");
        let (t1, l1) = device(2, "t1");
        let (t2, l2) = device(3, "t2");
        notes(&mut d, "d", 1);
        let first = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t1, &l1, &first);
        accept(&t2, &l2, &first);
        edit(&d, &ld, &key(0), "late");
        let mut late = publish_next(d.conn(), &ld).unwrap().unwrap();
        let body = late.bodies.pop().unwrap();
        accept(&t1, &l1, &late);
        accept(&t2, &l2, &late);
        let removed = BTreeMap::from([(ld.device, Some(mark(&first)))]);
        for (t, l) in [(&t1, &l1), (&t2, &l2)] {
            revert_past(t.conn(), l, &removed, &BTreeMap::new(), "removed", now()).unwrap();
            assert_eq!(version_state(t, ld.device, 2), -5);
        }
        journal::put_body(t1.conn(), &body).unwrap();
        keep(t1.conn(), &l1, ld.device, &key(0), 2, now()).unwrap();
        assert_eq!(text(&t1, 0), "late");
        accept(&t2, &l2, &publish_next(t1.conn(), &l1).unwrap().unwrap());
        assert_eq!(text(&t2, 0), "late");
        assert_eq!(version_state(&t2, l1.device, 1), 1);
    }

    #[test]
    fn a_cutoff_naming_another_branch_falls_back_to_what_everyone_accepted() {
        // SP30: D equivocated at 2. The removal's cutoff names a change 2 that isn't
        // the one this device applied, so the accepted-by-all mark (1) is the cutoff
        // and this device's change 2 is reverted too.
        let (t, lt, ld, batches) = fork();
        let other_branch = Mark {
            s: 2,
            h: Hash32([7; 32]),
        };
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(other_branch))]),
            &BTreeMap::from([(ld.device, mark(&batches[0]))]),
            "removed",
            now(),
        )
        .unwrap();
        assert_eq!(version_state(&t, ld.device, 2), -6);
        // The same sequence with this device's own change 2 keeps it.
        let (t, lt, ld, batches) = fork();
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&batches[1])))]),
            &BTreeMap::from([(ld.device, mark(&batches[0]))]),
            "removed",
            now(),
        )
        .unwrap();
        assert!(version_state(&t, ld.device, 2) > 0);
        let third: i64 = t
            .conn()
            .query_row(
                "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = 3",
                [ld.device.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(third, -6);
    }

    #[test]
    fn a_resolved_predecessor_on_another_branch_releases_nothing() {
        // SP30: the proof's branches resolve to change 3, but this device's change 3
        // is another one (the device equivocated): nothing is released.
        let (t, lt, ld, batches) = fork();
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, None)]),
            &BTreeMap::from([(ld.device, mark(&batches[0]))]),
            "quarantined",
            now(),
        )
        .unwrap();
        let other_branch = Mark {
            s: 3,
            h: Hash32([7; 32]),
        };
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, other_branch, now()).unwrap(),
            0
        );
        assert_eq!(version_state(&t, ld.device, 2), -6);
        assert_eq!(text(&t, 0), "before");
        // This device's own change 3 releases 2 and 3.
        assert!(reapply_upto(t.conn(), &lt, ld.device, mark(&batches[2]), now()).unwrap() > 0);
        assert!(version_state(&t, ld.device, 2) > 0);
    }

    #[test]
    fn a_hold_made_while_waiting_for_ancestry_is_checked_when_it_arrives() {
        // SP26, SP30: D's version reached this device before X's version it builds
        // on, so it was waiting (-1) when the removal held it. Once X's arrives, the
        // hold counts as checked ancestry, and stays out of the merge.
        let (mut x, lx) = device(1, "x");
        let (d, ld) = device(2, "d");
        let (t, lt) = device(3, "t");
        notes(&mut x, "x", 1);
        let from_x = publish_next(x.conn(), &lx).unwrap().unwrap();
        accept(&d, &ld, &from_x);
        edit(&d, &ld, &key(0), "D's edit");
        accept(&t, &lt, &publish_next(d.conn(), &ld).unwrap().unwrap());
        assert_eq!(version_state(&t, ld.device, 1), -1);
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, None)]),
            &BTreeMap::new(),
            "removed",
            now(),
        )
        .unwrap();
        let was = |t: &Store| -> i64 {
            t.conn()
                .query_row("SELECT was FROM sync_late_seen WHERE seq = 1", [], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        assert_eq!(was(&t), -1);
        accept(&t, &lt, &from_x);
        apply_pending(t.conn(), &lt, now()).unwrap();
        assert_eq!(was(&t), 0);
        assert_eq!(version_state(&t, ld.device, 1), -5);
    }

    #[test]
    fn a_reader_edit_hidden_by_a_reverted_deletion_comes_back() {
        // SY13, #109: D's deletion hid the reader's unsent edit, which was held for
        // the reader's choice; once the deletion is reverted, the edit shows.
        let (d, ld) = device(1, "d");
        let (t, lt) = device(2, "t");
        let mut d = d;
        notes(&mut d, "d", 1);
        let first = publish_next(d.conn(), &ld).unwrap().unwrap();
        accept(&t, &lt, &first);
        edit(&t, &lt, &key(0), "reader's edit");
        let covers = super::super::local::covers(d.conn(), &ld).unwrap();
        let v = journal::record_local(
            d.conn(),
            &ld,
            &key(0),
            LocalContent::Deleted {
                covers,
                identity: None,
            },
            now() as u64 + 10,
        )
        .unwrap();
        let mut tally = Tally::default();
        tally.restoring = true;
        apply::write(d.conn(), &ld, &v, &mut tally).unwrap();
        accept(&t, &lt, &publish_next(d.conn(), &ld).unwrap().unwrap());
        assert!(journal::held(t.conn(), &lt, &key(0)).unwrap());
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(ld.device, Some(mark(&first)))]),
            &BTreeMap::new(),
            "removed",
            now(),
        )
        .unwrap();
        assert_eq!(text(&t, 0), "reader's edit");
        assert!(!journal::held(t.conn(), &lt, &key(0)).unwrap());
    }

    #[test]
    fn a_later_version_by_a_device_not_removed_still_applies_on_a_reverted_one() {
        let (mut a, la) = device(1, "a");
        let (b, lb) = device(2, "b");
        let (t, lt) = device(3, "t");
        notes(&mut a, "a", 1);
        let initial = publish_next(a.conn(), &la).unwrap().unwrap();
        accept(&b, &lb, &initial);
        accept(&t, &lt, &initial);
        edit(&a, &la, &key(0), "A late");
        let late = publish_next(a.conn(), &la).unwrap().unwrap();
        accept(&b, &lb, &late);
        accept(&t, &lt, &late);
        revert_past(
            t.conn(),
            &lt,
            &BTreeMap::from([(la.device, Some(mark(&initial)))]),
            &BTreeMap::new(),
            "removed",
            now(),
        )
        .unwrap();
        edit(&b, &lb, &key(0), "B later");
        let later = publish_next(b.conn(), &lb).unwrap().unwrap();
        accept(&t, &lt, &later);
        assert_eq!(text(&t, 0), "B later");
        assert_eq!(version_state(&t, lb.device, later.seq as i64), 1);
        assert_eq!(version_state(&t, la.device, late.seq as i64), -6);
        assert!(publish_next(t.conn(), &lt).unwrap().is_none());
    }

    #[test]
    fn a_restored_highlight_does_not_bring_back_a_still_reverted_note() {
        for reapply in [false, true] {
            let (mut a, la) = device(1, "a");
            let (b, lb) = device(2, "b");
            let (t, lt) = device(3, "t");
            let (mut c, lc) = device(4, "c");
            let mut annotation = highlight("a", None, now());
            annotation.id = key(0).as_str().strip_prefix("note:").unwrap().to_owned();
            a.save_annotation(&annotation).unwrap();
            let initial = publish_next(a.conn(), &la).unwrap().unwrap();
            for (store, lib) in [(&b, &lb), (&t, &lt), (&c, &lc)] {
                accept(store, lib, &initial);
            }
            edit(&b, &lb, &key(0), "B late note");
            let late = publish_next(b.conn(), &lb).unwrap().unwrap();
            accept(&t, &lt, &late);
            revert_past(
                t.conn(),
                &lt,
                &BTreeMap::from([(la.device, None), (lb.device, None)]),
                &BTreeMap::new(),
                "removed",
                now(),
            )
            .unwrap();
            assert!(t.annotations("t").unwrap().is_empty());
            if reapply {
                reapply_upto(t.conn(), &lt, la.device, mark(&initial), now()).unwrap();
            } else {
                let mut changed = c.annotations("c").unwrap().remove(0);
                changed.color = "green".into();
                c.save_annotation(&changed).unwrap();
                accept(&t, &lt, &publish_next(c.conn(), &lc).unwrap().unwrap());
            }
            assert_eq!(t.annotations("t").unwrap()[0].note, None);
            assert_eq!(version_state(&t, lb.device, late.seq as i64), -6);
        }
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
    fn a_cascade_reapplies_for_each_device() {
        // #163: A and B have late versions of one record. Re-applying A, then B,
        // ends on B's version without creating a local edit.
        let (t, lt, la, lb, initial) = cascade(1, 2);
        let removed = BTreeMap::from([(la.device, Some(mark(&initial))), (lb.device, None)]);
        revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
        assert_eq!(text(&t, 0), "before");
        let at = |d: DeviceId, s: i64| -> Mark {
            let h: String = t
                .conn()
                .query_row(
                    "SELECT hash FROM sync_changes WHERE device = ?1 AND seq = ?2",
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
    fn a_related_device_removed_later_is_recomputed() {
        for published in [false, true] {
            for reader_edit in [false, true] {
                let (t, lt, la, lb, initial) = cascade(1, 2);
                let mut removed = BTreeMap::from([(lb.device, None)]);
                revert_past(t.conn(), &lt, &removed, &BTreeMap::new(), "removed", now()).unwrap();
                assert_eq!(text(&t, 0), "A late");
                if published {
                    assert!(publish_next(t.conn(), &lt).unwrap().is_none());
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
    fn a_cutoff_that_moves_earlier_recomputes_again() {
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
                assert!(publish_next(t.conn(), &lt).unwrap().is_none());
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
    fn a_released_newer_version_supersedes_the_reapplied_version() {
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
    fn recovery_recomputes_all_records_and_releases_pending_versions() {
        // SP30: a publication attempt cannot block recovery of any record;
        // the missing-body successor waits through the normal pending path.
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
        assert!(publish_next(t.conn(), &lt).unwrap().is_none());
        assert_eq!(
            reapply_upto(t.conn(), &lt, ld.device, mark(&third), now()).unwrap(),
            2
        );
        assert_eq!(text(&t, 1), "other 2");
        assert_eq!(text(&t, 0), "version 2");
        assert_eq!(version_state(&t, ld.device, 3), 0);
        assert_eq!(apply_pending(t.conn(), &lt, now()).unwrap(), 0);
    }

    #[test]
    fn migration_eleven_fixture_matches_and_preserves_late_bookkeeping() {
        // Shared migration tests enumerate older tables; cover the additions here.
        let fixture = Connection::open_in_memory().unwrap();
        fixture
            .execute_batch(include_str!("../../tests/fixtures/schema-v11.sql"))
            .unwrap();
        let migrated = Store::open_in_memory().unwrap();
        assert_eq!(
            migrated
                .conn()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            11
        );
        assert_eq!(
            migrated
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = 'sync_late_restored'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        for table in [
            "sync_late",
            "sync_late_over",
            "sync_late_seen",
            "sync_late_passes",
        ] {
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
        for table in [
            "sync_late",
            "sync_late_over",
            "sync_late_seen",
            "sync_late_passes",
        ] {
            assert_eq!(count(&reopened, table), 1);
        }
    }
}
