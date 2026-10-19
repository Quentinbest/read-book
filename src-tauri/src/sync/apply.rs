//! SY12, SY13, SY14, SY21, SY28, SP11: other devices' versions, merged and applied.
//!
//! A batch is one device's change, in sequence order. Receiving it checks every
//! version (SY28), keeps the good ones, and applies what each touched record now
//! comes to, all in one transaction: a kill leaves either the whole batch or none of
//! it (plan §6.3). Applying writes the library's tables directly, never through the
//! journal, so it is never sent back (SY12). A version whose body, book or highlight
//! hasn't arrived waits; nothing missing ever deletes anything (SY21).
//!
//! Stage 1b exchanges batches in memory; Stage 1c gives them their signed change-file
//! form (SP14) and Stage 2 their storage.

use super::a9;
use super::ids::{identity_hash, DeviceId, Hash32};
use super::journal::{self, JournalError, Library, Which};
use super::merge::{
    check_frontier, check_history, reader_decides, Chains, HistoryCheck, Lookup, Outcome, Placed,
    Prior,
};
use super::record::{check_body, check_version, Checked, Content, Key, Kind, Version};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;

type Result<T> = std::result::Result<T, JournalError>;

/// One device's change: its versions, in order, and the bodies they name.
#[derive(Clone, Debug)]
pub struct Batch {
    pub device: DeviceId,
    pub seq: u64,
    /// The change's payload hash (SP5); a stand-in until Stage 1c signs changes.
    pub hash: Hash32,
    pub versions: Vec<Value>,
    pub bodies: Vec<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    /// Kept, and `applied` records changed in the library.
    Applied { applied: usize, set_aside: usize },
    /// Already received.
    Repeat,
    /// A gap before it: it waits for the changes in between (SP14).
    Waits,
    /// SY28: this device has set aside as much from that one as it keeps, so it
    /// stops receiving from it; nothing of the change is kept.
    Stopped,
}

/// SY28 (prov., item 106): what is kept set aside from one device before this one
/// stops receiving from it.
pub const ASIDE_ITEMS: i64 = 1000;
pub const ASIDE_BYTES: i64 = 16 * 1024 * 1024;

/// SY10, SY28: whether keeping one more item of `extra_bytes` from one device would
/// pass what this device keeps of it set aside: set-aside items, and versions waiting
/// for their ancestry or a clock ahead, or excluded for their body (`applied` −1,
/// −2, −4), with the real size of the bodies they name. Valid versions waiting only
/// for a book or a highlight are held to `pending_full` instead, after applying.
fn aside_full(
    conn: &Connection,
    lib: &Library,
    device: DeviceId,
    extra_items: i64,
    extra_bytes: i64,
) -> Result<bool> {
    let (n, bytes): (i64, Option<i64>) = conn.query_row(
        "SELECT COUNT(*), SUM(size) FROM sync_aside WHERE library = ?1 AND device = ?2",
        params![lib.id.to_string(), device.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (m, held): (i64, Option<i64>) = conn.query_row(
        "SELECT COUNT(*), SUM(length(v.raw) + COALESCE(length(b.body), 0))
         FROM sync_versions v
         LEFT JOIN sync_bodies b ON b.hash = json_extract(v.raw, '$.body.hash')
         WHERE v.library = ?1 AND v.device = ?2 AND v.applied IN (-1, -2, -4)",
        params![lib.id.to_string(), device.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(n + m + extra_items > ASIDE_ITEMS
        || bytes.unwrap_or(0) + held.unwrap_or(0) + extra_bytes > ASIDE_BYTES)
}

/// SY2, SY28 (prov.): valid versions of one device still pending (`applied = 0`,
/// waiting for a book, a highlight or the reader) are held to this many bytes,
/// bodies included. Checked after a change is applied, so versions that apply in it
/// aren't charged: a large first sync passes.
#[cfg(not(test))]
pub const PENDING_BYTES: i64 = 256 * 1024 * 1024;
/// Smaller in tests, so the limit can be reached quickly.
#[cfg(test)]
pub const PENDING_BYTES: i64 = 1024 * 1024;

fn pending_full(conn: &Connection, lib: &Library, device: DeviceId) -> Result<bool> {
    let held: Option<i64> = conn.query_row(
        "SELECT SUM(length(v.raw) + COALESCE(length(b.body), 0))
         FROM sync_versions v
         LEFT JOIN sync_bodies b ON b.hash = json_extract(v.raw, '$.body.hash')
         WHERE v.library = ?1 AND v.device = ?2 AND v.applied = 0",
        params![lib.id.to_string(), device.to_string()],
        |r| r.get(0),
    )?;
    Ok(held.unwrap_or(0) > PENDING_BYTES)
}

fn own_seq(conn: &Connection, lib: &Library) -> Result<u64> {
    let s: Option<i64> = conn.query_row(
        "SELECT MAX(seq) FROM sync_versions WHERE library = ?1 AND device = ?2",
        params![lib.id.to_string(), lib.device.to_string()],
        |r| r.get(0),
    )?;
    Ok(s.unwrap_or(0) as u64)
}

/// SY7, SP15: number this device's unsent versions as its next change, after
/// `last_seq`, the last sequence its device store says it signed (SP28).
pub fn publish(conn: &Connection, lib: &Library, last_seq: u64) -> Result<Option<Batch>> {
    let Some((seq, unsent)) = journal::take_unsent(conn, lib, last_seq)? else {
        return Ok(None);
    };
    let mut bodies = Vec::new();
    for v in &unsent {
        if let Content::Body { hash, .. } = &v.content {
            if let Some(b) = journal::body(conn, hash)? {
                bodies.push(b);
            }
        }
    }
    let versions: Vec<Value> = unsent.iter().map(|v| v.raw.clone()).collect();
    let hash = identity_hash(Value::Array(versions.clone()).to_string().as_bytes());
    conn.execute(
        "INSERT OR REPLACE INTO sync_changes (library, device, seq, hash) VALUES (?1, ?2, ?3, ?4)",
        params![
            lib.id.to_string(),
            lib.device.to_string(),
            seq as i64,
            hash.to_string()
        ],
    )?;
    Ok(Some(Batch {
        device: lib.device,
        seq,
        hash,
        versions,
        bodies,
    }))
}

/// SP11, SP13: the device chains as this journal holds them.
struct JournalChains<'a> {
    conn: &'a Connection,
    lib: &'a Library,
}

impl JournalChains<'_> {
    /// How far this device holds `device`'s chain.
    fn held(&self, device: DeviceId) -> u64 {
        if device == self.lib.device {
            own_seq(self.conn, self.lib).unwrap_or(0)
        } else {
            seen(self.conn, self.lib, device).unwrap_or(0)
        }
    }
}

impl Chains for JournalChains<'_> {
    fn version_at(&self, device: DeviceId, seq: u64, key: &str) -> Lookup {
        if self.held(device) < seq {
            return Lookup::Unknown;
        }
        let row: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT hash, applied FROM sync_versions
                 WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
                params![self.lib.id.to_string(), device.to_string(), seq as i64, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .ok()
            .flatten();
        match row {
            // Still waiting itself (its ancestry, or a clock ahead): unresolved.
            // Waiting itself (its ancestry, or a clock ahead): unresolved. A version
            // excluded for its body (-4) is still in its writer's chain.
            Some((_, applied)) if applied < 0 && applied != -4 => Lookup::Unknown,
            Some((h, _)) => Hash32::parse(&h).map_or(Lookup::Absent, Lookup::Present),
            None => Lookup::Absent,
        }
    }

    fn change_at(&self, device: DeviceId, seq: u64) -> Lookup {
        let hash: Option<String> = self
            .conn
            .query_row(
                "SELECT hash FROM sync_changes WHERE library = ?1 AND device = ?2 AND seq = ?3",
                params![self.lib.id.to_string(), device.to_string(), seq as i64],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        match hash.as_deref().and_then(Hash32::parse) {
            Some(h) => Lookup::Present(h),
            None if self.held(device) >= seq => Lookup::Absent,
            None => Lookup::Unknown,
        }
    }

    fn prior_version(&self, device: DeviceId, key: &str, before: u64) -> Prior {
        if self.held(device) + 1 < before {
            return Prior::Unknown;
        }
        let row: Option<(i64, String, i64)> = self
            .conn
            .query_row(
                "SELECT seq, hash, applied FROM sync_versions
                 WHERE library = ?1 AND device = ?2 AND key = ?3 AND seq < ?4
                 ORDER BY seq DESC LIMIT 1",
                params![
                    self.lib.id.to_string(),
                    device.to_string(),
                    key,
                    before as i64
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .ok()
            .flatten();
        match row {
            Some((_, _, applied)) if applied < 0 && applied != -4 => Prior::Unknown,
            Some((s, h, _)) => Hash32::parse(&h).map_or(Prior::Unknown, |h| Prior::At(s as u64, h)),
            None => Prior::None,
        }
    }
}

fn too_far_ahead(v: &Version, now_ms: i64) -> bool {
    v.clock.ms().saturating_sub(now_ms.max(0) as u64) > super::clock::SET_ASIDE_MS
}

/// A received version kept but not merged: -1 waits for its ancestry, -2 for this
/// clock to catch up with it.
fn set_waiting(conn: &Connection, lib: &Library, p: &Placed, state: i64) -> Result<()> {
    conn.execute(
        "UPDATE sync_versions SET applied = ?5
         WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
        params![
            lib.id.to_string(),
            p.version.device.to_string(),
            p.seq as i64,
            p.version.key.as_str(),
            state
        ],
    )?;
    Ok(())
}

/// SY9, SP11, SP13: a version's history and frontier against the chains held.
fn ancestry(conn: &Connection, lib: &Library, p: &Placed) -> HistoryCheck {
    let chains = JournalChains { conn, lib };
    match (check_history(p, &chains), check_frontier(p, &chains)) {
        (HistoryCheck::False, _) | (_, HistoryCheck::False) => HistoryCheck::False,
        (HistoryCheck::Wait, _) | (_, HistoryCheck::Wait) => HistoryCheck::Wait,
        _ => HistoryCheck::Ok,
    }
}

fn seen(conn: &Connection, lib: &Library, device: DeviceId) -> Result<u64> {
    let s: Option<i64> = conn
        .query_row(
            "SELECT seq FROM sync_seen WHERE library = ?1 AND device = ?2",
            params![lib.id.to_string(), device.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(s.unwrap_or(0) as u64)
}

/// SY28: keeps a set-aside item, counted once. The budget is checked by `receive`,
/// once the whole change is kept.
fn set_aside(
    conn: &Connection,
    lib: &Library,
    device: DeviceId,
    raw: &[u8],
    reason: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_aside (library, device, hash, reason, size, raw, first_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            lib.id.to_string(),
            device.to_string(),
            identity_hash(raw).to_string(),
            reason,
            raw.len() as i64,
            raw,
            now_ms
        ],
    )?;
    Ok(())
}

/// SY28, SP14: receive one device's change, in order, and apply what it changed.
pub fn receive(conn: &Connection, lib: &Library, batch: &Batch, now_ms: i64) -> Result<Received> {
    let have = seen(conn, lib, batch.device)?;
    if batch.seq <= have {
        return Ok(Received::Repeat);
    }
    if batch.seq != have + 1 {
        return Ok(Received::Waits);
    }
    // SY28: nothing more is received from a device whose budget is spent.
    if aside_full(conn, lib, batch.device, 1, 0)? {
        return Ok(Received::Stopped);
    }
    let tx = conn.unchecked_transaction()?;
    let mut keys = Vec::new();
    let mut aside = 0;
    let mut named = HashSet::new();
    let lengths: std::collections::HashMap<Hash32, usize> = batch
        .bodies
        .iter()
        .map(|b| (identity_hash(b), b.len()))
        .collect();
    // SP11: a change file holds at most one version of a key. Every version of a
    // key named twice is set aside, so neither is kept in place of the other.
    let mut times: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for raw in &batch.versions {
        if let Some(k) = raw.get("key").and_then(Value::as_str) {
            *times.entry(k).or_default() += 1;
        }
    }
    for raw in &batch.versions {
        // SY28: a body must be the size its version declares.
        let checked = check_version(raw.clone(), batch.device).and_then(|c| match c {
            Checked::Version(v) if times.get(v.key.as_str()).is_some_and(|&n| n > 1) => {
                Err(super::record::RecordError::DuplicateKey)
            }
            Checked::Version(v) => match &v.content {
                Content::Body { hash, size }
                    if lengths.get(hash).is_some_and(|&l| l as u64 != *size) =>
                {
                    Err(super::record::RecordError::Field("body"))
                }
                _ => Ok(Checked::Version(v)),
            },
            other => Ok(other),
        });
        match checked {
            // SY10: more than 24 hours ahead of this clock: kept with its place in the
            // chain (`applied = -2`) but never merged, applied or observed until the
            // clock catches up; then it is checked like any other.
            Ok(Checked::Version(v)) if too_far_ahead(&v, now_ms) => {
                let placed = Placed {
                    version: v,
                    seq: batch.seq,
                };
                journal::store_remote(&tx, lib, &placed)?;
                set_waiting(&tx, lib, &placed, -2)?;
                if let Content::Body { hash, .. } = &placed.version.content {
                    named.insert(*hash);
                }
                aside += 1;
            }
            Ok(Checked::Version(v)) => {
                let placed = Placed {
                    version: v,
                    seq: batch.seq,
                };
                match ancestry(&tx, lib, &placed) {
                    // SY9: a forged or future ancestor is refused.
                    HistoryCheck::False => {
                        set_aside(
                            &tx,
                            lib,
                            batch.device,
                            raw.to_string().as_bytes(),
                            "false ancestry",
                            now_ms,
                        )?;
                        aside += 1;
                        continue;
                    }
                    HistoryCheck::Wait => {
                        journal::store_remote(&tx, lib, &placed)?;
                        tx.execute(
                            "UPDATE sync_versions SET applied = -1
                             WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
                            params![
                                lib.id.to_string(),
                                batch.device.to_string(),
                                batch.seq as i64,
                                placed.version.key.as_str()
                            ],
                        )?;
                    }
                    HistoryCheck::Ok => {
                        keys.push(placed.version.key.clone());
                        journal::store_remote(&tx, lib, &placed)?;
                    }
                }
                if let Content::Body { hash, .. } = &placed.version.content {
                    named.insert(*hash);
                }
            }
            // SY9: a newer format is kept aside whole, and passed on.
            Ok(Checked::NewerFormat(_)) => {
                set_aside(
                    &tx,
                    lib,
                    batch.device,
                    raw.to_string().as_bytes(),
                    "newer format",
                    now_ms,
                )?;
                aside += 1;
            }
            Err(e) => {
                set_aside(
                    &tx,
                    lib,
                    batch.device,
                    raw.to_string().as_bytes(),
                    &e.to_string(),
                    now_ms,
                )?;
                aside += 1;
            }
        }
    }
    // Only bodies a version in the change names are kept; their type is checked
    // when they are applied.
    for b in &batch.bodies {
        if named.contains(&identity_hash(b)) {
            journal::put_body(&tx, b)?;
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO sync_changes (library, device, seq, hash) VALUES (?1, ?2, ?3, ?4)",
        params![
            lib.id.to_string(),
            batch.device.to_string(),
            batch.seq as i64,
            batch.hash.to_string()
        ],
    )?;
    tx.execute(
        "INSERT INTO sync_seen (library, device, seq, hash) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(library, device) DO UPDATE SET seq = excluded.seq, hash = excluded.hash",
        params![
            lib.id.to_string(),
            batch.device.to_string(),
            batch.seq as i64,
            batch.hash.to_string()
        ],
    )?;
    // Versions that waited for their ancestry may resolve now.
    keys.extend(recheck_waiting(&tx, lib, now_ms)?);
    let mut applied = 0;
    for k in keys {
        if apply_key(&tx, lib, &k, now_ms)? {
            applied += 1;
        }
    }
    // SY28: once everything the change brings is kept and applied, what is left of
    // that device unapplied must fit both budgets; if not, the whole change is
    // rolled back (the transaction is dropped) and receiving from it stops. A change
    // file is bounded (SP8), so this is the only check needed, once per change.
    if aside_full(&tx, lib, batch.device, 0, 0)? || pending_full(&tx, lib, batch.device)? {
        return Ok(Received::Stopped);
    }
    tx.commit()?;
    Ok(Received::Applied {
        applied,
        set_aside: aside,
    })
}

/// Versions waiting for their ancestry, checked again until a pass resolves none:
/// one version resolving can resolve the next of its chain. A loop, not recursion,
/// and each pass drops its rows; waiting versions count against SY28's budget, so
/// the work is bounded. Returns the keys that joined the merge.
fn recheck_waiting(conn: &Connection, lib: &Library, now_ms: i64) -> Result<Vec<Key>> {
    let mut joined = Vec::new();
    loop {
        let more = recheck_pass(conn, lib, now_ms)?;
        if more.is_empty() {
            return Ok(joined);
        }
        joined.extend(more);
    }
}

/// One pass over the waiting versions: those that resolve join the merge, those
/// found false are set aside.
fn recheck_pass(conn: &Connection, lib: &Library, now_ms: i64) -> Result<Vec<Key>> {
    let waiting: Vec<(i64, String, i64, String)> = conn
        .prepare(
            "SELECT rowid, device, seq, raw FROM sync_versions
             WHERE library = ?1 AND applied IN (-1, -2) ORDER BY seq",
        )?
        .query_map([lib.id.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut joined = Vec::new();
    for (rowid, device, seq, raw) in waiting {
        let Some(device) = super::ids::Id16::parse(&device) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Ok(Checked::Version(v)) = check_version(value, device) else {
            continue;
        };
        let placed = Placed {
            version: v,
            seq: seq as u64,
        };
        if too_far_ahead(&placed.version, now_ms) {
            continue;
        }
        match ancestry(conn, lib, &placed) {
            HistoryCheck::Ok => {
                conn.execute(
                    "UPDATE sync_versions SET applied = 0 WHERE rowid = ?1",
                    [rowid],
                )?;
                joined.push(placed.version.key);
            }
            HistoryCheck::False => {
                set_aside(conn, lib, device, raw.as_bytes(), "false ancestry", now_ms)?;
                conn.execute("DELETE FROM sync_versions WHERE rowid = ?1", [rowid])?;
                // Its body goes too, unless another version still names it.
                if let Content::Body { hash, .. } = &placed.version.content {
                    conn.execute(
                        "DELETE FROM sync_bodies WHERE hash = ?1 AND NOT EXISTS
                         (SELECT 1 FROM sync_versions WHERE json_extract(raw, '$.body.hash') = ?1)",
                        [hash.to_string()],
                    )?;
                }
            }
            HistoryCheck::Wait => {
                set_waiting(conn, lib, &placed, -1)?;
            }
        }
    }
    Ok(joined)
}

/// SY13: note versions made apart from a highlight's deletion, outside its causal
/// frontier. The highlight stays deleted (item 102) and the note's text is kept on
/// the row; these are what Stage 5's “Changed on two devices” lists for the reader.
pub fn dependent_conflicts(
    conn: &Connection,
    lib: &Library,
    annotation_id: &str,
) -> Result<Vec<Placed>> {
    let Some((akey, _)) = Key::parse(&format!("annotation:{annotation_id}")) else {
        return Ok(vec![]);
    };
    let Some((nkey, _)) = Key::parse(&format!("note:{annotation_id}")) else {
        return Ok(vec![]);
    };
    let ignored = HashSet::new();
    let annotation = journal::record_of(conn, lib, &akey, Which::All)?;
    // The deletion shown in the library: settled, or shown while a change made
    // apart to the highlight itself waits too.
    let (Some(Outcome::Settled(tombstone))
    | Some(Outcome::Conflict {
        shown: tombstone, ..
    })) = annotation.outcome(&ignored)
    else {
        return Ok(vec![]);
    };
    if !tombstone.is_deleted() {
        return Ok(vec![]);
    }
    let notes = journal::record_of(conn, lib, &nkey, Which::All)?;
    Ok(notes
        .heads(&ignored)
        .into_iter()
        .filter(|n| !n.is_deleted() && super::merge::outside_frontier(tombstone, n))
        .cloned()
        .collect())
}

/// Apply what still waits: bodies or books that have arrived since.
pub fn apply_pending(conn: &Connection, lib: &Library, now_ms: i64) -> Result<usize> {
    {
        let tx = conn.unchecked_transaction()?;
        recheck_waiting(&tx, lib, now_ms)?;
        tx.commit()?;
    }
    let keys: Vec<String> = conn
        .prepare("SELECT DISTINCT key FROM sync_versions WHERE library = ?1 AND applied = 0")?
        .query_map([lib.id.to_string()], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for k in keys {
        if let Some((key, _)) = Key::parse(&k) {
            if apply_key(&tx, lib, &key, now_ms)? {
                n += 1;
            }
        }
    }
    tx.commit()?;
    Ok(n)
}

fn local_book(conn: &Connection, lib: &Library, sync_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT book_id FROM sync_books WHERE library = ?1 AND sync_id = ?2 AND book_id IS NOT NULL",
            params![lib.id.to_string(), sync_id],
            |r| r.get(0),
        )
        .optional()?)
}

fn subject(key: &Key) -> &str {
    key.as_str().split(':').nth(1).unwrap_or_default()
}

/// Writes one version's state into the library. `false` when it must wait.
fn write(conn: &Connection, lib: &Library, v: &Version) -> Result<bool> {
    let ms = v.clock.ms() as i64;
    let body = match &v.content {
        Content::Body { hash, size } => match journal::body(conn, hash)? {
            // SY28: a body must be the size its version declares, also when it
            // arrives after its version.
            Some(bytes) => match check_body(&v.key, &bytes).and_then(|b| {
                if bytes.len() as u64 == *size {
                    Ok(b)
                } else {
                    Err(super::record::RecordError::Field("body"))
                }
            }) {
                Ok(b) => Some(b),
                Err(e) => {
                    // SY28: a body that fails is set aside and its version taken out
                    // of merging (`applied = -4`), never applied.
                    // The body stays counted under its version (see `aside_full`); the
                    // aside entry only records why.
                    set_aside(
                        conn,
                        lib,
                        v.device,
                        hash.to_string().as_bytes(),
                        &e.to_string(),
                        ms,
                    )?;
                    conn.execute(
                        "UPDATE sync_versions SET applied = -4
                         WHERE library = ?1 AND device = ?2 AND key = ?3 AND hash = ?4 AND seq IS NOT NULL",
                        params![lib.id.to_string(), v.device.to_string(), v.key.as_str(), v.hash.to_string()],
                    )?;
                    return Ok(false);
                }
            },
            None => return Ok(false),
        },
        Content::Deleted { .. } => None,
        Content::Inline(_) => return Ok(false),
    };
    // SY28: a highlight over its limit stays on this device, unsynced; nothing from
    // another device replaces or deletes it.
    if v.kind == Kind::Annotation {
        if let Some((row, _)) = crate::store::annotation_row(conn, subject(&v.key))? {
            let book = super::local::sync_id(conn, lib, &row.book_id)?;
            if a9::body_of(&row, &book).to_string().len() as u64 > Kind::Annotation.body_limit() {
                return Ok(false);
            }
        }
    }
    match v.kind {
        Kind::Place | Kind::BookOpened | Kind::BookFinished => {
            let Some(book) = local_book(conn, lib, subject(&v.key))? else {
                return Ok(false);
            };
            match (v.kind, &body) {
                (Kind::Place, Some(b)) => {
                    conn.execute(
                        "INSERT INTO positions (book_id, cfi, fraction, updated_at, chapter_label)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(book_id) DO UPDATE SET cfi = excluded.cfi, fraction = excluded.fraction,
                         updated_at = excluded.updated_at, chapter_label = excluded.chapter_label",
                        params![book, b["cfi"].as_str(), b["fraction"].as_f64(), ms, b["chapter"].as_str()],
                    )?;
                }
                (Kind::Place, None) => {
                    conn.execute("DELETE FROM positions WHERE book_id = ?1", [&book])?;
                }
                (Kind::BookOpened, Some(_)) => {
                    conn.execute(
                        "UPDATE books SET opened_at = ?2 WHERE id = ?1",
                        params![book, ms],
                    )?;
                }
                (Kind::BookFinished, Some(b)) => {
                    let at = b["finished"].as_bool().unwrap_or(false).then_some(ms);
                    conn.execute(
                        "UPDATE books SET finished_at = ?2 WHERE id = ?1",
                        params![book, at],
                    )?;
                }
                _ => {}
            }
        }
        Kind::Annotation => match &body {
            Some(b) => {
                let Some(book) = a9::book_of(b).map(str::to_owned) else {
                    return Ok(false);
                };
                let Some(local) = local_book(conn, lib, &book)? else {
                    return Ok(false);
                };
                let Some(a) = a9::row_of(b, &local) else {
                    return Ok(false);
                };
                conn.execute(
                    "INSERT INTO annotations (id, book_id, anchored_content_hash, color, cfi_range, quote_exact,
                         quote_prefix, quote_suffix, note, created_at, updated_at, deleted_at, anchor_status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10, NULL, ?11)
                     ON CONFLICT(id) DO UPDATE SET anchored_content_hash = excluded.anchored_content_hash,
                         color = excluded.color, cfi_range = excluded.cfi_range, quote_exact = excluded.quote_exact,
                         quote_prefix = excluded.quote_prefix, quote_suffix = excluded.quote_suffix,
                         updated_at = excluded.updated_at, deleted_at = NULL, anchor_status = excluded.anchor_status",
                    params![a.id, a.book_id, a.anchored_content_hash, a.color, a.cfi_range, a.quote_exact,
                        a.quote_prefix, a.quote_suffix, a.created_at, a.updated_at, a.anchor_status],
                )?;
            }
            None => {
                conn.execute(
                    "UPDATE annotations SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                    params![subject(&v.key), ms],
                )?;
            }
        },
        Kind::Note => {
            let text = body
                .as_ref()
                .and_then(|b| b["text"].as_str().map(str::to_owned));
            let local: Option<Option<String>> = conn
                .query_row(
                    "SELECT note FROM annotations WHERE id = ?1",
                    [subject(&v.key)],
                    |r| r.get(0),
                )
                .optional()?;
            match local {
                // A tombstone with no highlight here has nothing to delete; if the
                // highlight arrives later, it arrives without the note.
                None if text.is_none() => return Ok(true),
                None => return Ok(false), // its highlight hasn't arrived
                // SY28: a note over its limit stays on this device, unsynced; nothing
                // from another device replaces it.
                Some(Some(n))
                    if serde_json::json!({ "text": n }).to_string().len() as u64
                        > Kind::Note.body_limit() =>
                {
                    return Ok(false)
                }
                Some(_) => {}
            }
            conn.execute(
                "UPDATE annotations SET note = ?2 WHERE id = ?1",
                params![subject(&v.key), text],
            )?;
        }
        // Settings (item 103), books and their files (Stage 4) and choices (Stage 5)
        // aren't applied yet; they wait.
        _ => return Ok(false),
    }
    Ok(true)
}

fn mark_applied(conn: &Connection, lib: &Library, p: &Placed) -> Result<()> {
    conn.execute(
        "UPDATE sync_versions SET applied = 1 WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
        params![lib.id.to_string(), p.version.device.to_string(), p.seq as i64, p.version.key.as_str()],
    )?;
    Ok(())
}

/// What one record comes to now, written into the library. `true` if it changed.
fn apply_key(conn: &Connection, lib: &Library, key: &Key, now_ms: i64) -> Result<bool> {
    let mut record = journal::record_of(conn, lib, key, Which::All)?;
    // A local change not yet published counts as this device's next version.
    let next = own_seq(conn, lib)? + 1;
    if let Some(v) = journal::unsent_of(conn, lib, key)? {
        record.add(Placed {
            version: v,
            seq: next,
        });
    }
    let (winner, heads) = match record.outcome(&HashSet::new()) {
        Some(Outcome::Settled(p)) => (p, record.heads(&HashSet::new())),
        Some(Outcome::Conflict { shown, all }) => (shown, all),
        None => return Ok(false),
    };
    // SY13, #109: this device's unsent edit, now hidden in a conflict, is held for the
    // reader's choice rather than left to be replaced by the next edit.
    if let Some(Outcome::Conflict { .. }) = record.outcome(&HashSet::new()) {
        let hidden_unsent = heads
            .iter()
            .any(|h| h.version.device == lib.device && h.seq == next && h.id() != winner.id());
        if hidden_unsent {
            conn.execute(
                "UPDATE sync_versions SET applied = -3 WHERE library = ?1 AND key = ?2 AND seq IS NULL",
                params![lib.id.to_string(), key.as_str()],
            )?;
        }
    }
    // SY10: every head accepted here has been seen, shown or not, so a change made
    // after it sorts after it.
    for h in &heads {
        if h.version.device != lib.device {
            journal::observe(conn, lib, h.version.clock, now_ms.max(0) as u64)?;
        }
    }
    if journal::held(conn, lib, key)? {
        // #109: an edit is held for the reader's choice; nothing overwrites it.
        return Ok(false);
    }
    let changed = if winner.version.device == lib.device {
        false // the library already holds this device's own state
    } else if write(conn, lib, &winner.version)? {
        true
    } else {
        return Ok(false);
    };
    // What the library now shows, and everything it descends from, has been seen.
    // For newest-wins records the losing heads are superseded too; for those the
    // reader decides, they stay unapplied, waiting in the conflict (SY13).
    // Settled (one head, equal contents, or newest-wins): every head is superseded or
    // the same, so all have been seen. In a conflict only the shown one has.
    let settled = matches!(record.outcome(&HashSet::new()), Some(Outcome::Settled(_)));
    let all_seen = settled || !reader_decides(winner.version.kind);
    for p in record.versions() {
        if winner.descends_from(p) || (all_seen && heads.iter().any(|h| h.descends_from(p))) {
            mark_applied(conn, lib, p)?;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AnnotationRow, Store};
    use crate::sync::ids::Id16;
    use crate::sync::local;

    const LIB: Id16 = Id16([7; 16]);
    const SYNC_BOOK: &str = "7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
    const ANNO: &str = "1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d";
    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// A device with one book, joined to the library. The book is matched to the
    /// same sync ID on every device, as Stage 4's book records will do.
    fn device(n: u8, book: &str) -> (Store, Library) {
        let store = Store::open_in_memory().unwrap();
        let lib = Library {
            id: LIB,
            device: Id16([n; 16]),
        };
        let c = store.conn();
        c.execute(
            "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
             VALUES (?1, ?2, 'x.epub', 'Moby-Dick', 'package', 0)",
            params![book, format!("{HASH}{n}")],
        )
        .unwrap();
        journal::join(c, &lib, "icloud-drive", crate::store::now_ms()).unwrap();
        c.execute(
            "INSERT INTO sync_books (library, sync_id, book_id) VALUES (?1, ?2, ?3)",
            params![LIB.to_string(), SYNC_BOOK, book],
        )
        .unwrap();
        (store, lib)
    }

    /// The tests stand in for the device store with the library's own count.
    fn publish_next(conn: &Connection, lib: &Library) -> Result<Option<Batch>> {
        publish(conn, lib, own_seq(conn, lib)?)
    }

    fn send(from: &Store, fl: &Library, to: &Store, tl: &Library) -> Option<Received> {
        let batch = publish_next(from.conn(), fl).unwrap()?;
        Some(receive(to.conn(), tl, &batch, crate::store::now_ms()).unwrap())
    }

    fn highlight(book: &str, note: Option<&str>, at: i64) -> AnnotationRow {
        AnnotationRow {
            id: ANNO.into(),
            book_id: book.into(),
            anchored_content_hash: HASH.into(),
            color: "yellow".into(),
            cfi_range: "epubcfi(/6/4!/4/2,/1:0,/1:4)".into(),
            quote_exact: "Call".into(),
            quote_prefix: "".into(),
            quote_suffix: " me".into(),
            note: note.map(str::to_owned),
            created_at: 1_760_000_000_000,
            updated_at: at,
            anchor_status: "anchored".into(),
        }
    }

    fn note_of(s: &Store, book: &str) -> Option<Option<String>> {
        s.annotations(book).unwrap().first().map(|a| a.note.clone())
    }

    #[test]
    fn a_place_arrives_and_isnt_sent_back() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/14!/4/2/1:0)", 0.31, Some("Loomings"))
            .unwrap();
        assert!(matches!(
            send(&a, &la, &b, &lb),
            Some(Received::Applied {
                applied: 2,
                set_aside: 0
            })
        ));
        assert_eq!(
            b.position("book-b").unwrap(),
            Some(("epubcfi(/6/14!/4/2/1:0)".into(), 0.31))
        );
        // SY12: applying a remote place is not a local change.
        assert!(publish_next(b.conn(), &lb).unwrap().is_none());
    }

    #[test]
    fn a_highlight_and_its_note_arrive_and_an_edit_comes_back() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("Ishmael"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        assert_eq!(note_of(&b, "book-b"), Some(Some("Ishmael".into())));
        b.save_annotation(&highlight(
            "book-b",
            Some("Ishmael, again"),
            1_760_000_002_000,
        ))
        .unwrap();
        send(&b, &lb, &a, &la).unwrap();
        assert_eq!(note_of(&a, "book-a"), Some(Some("Ishmael, again".into())));
    }

    #[test]
    fn notes_changed_apart_keep_both_and_show_the_latest_on_both() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        a.save_annotation(&highlight("book-a", Some("from A"), 1_760_000_002_000))
            .unwrap();
        b.save_annotation(&highlight("book-b", Some("from B"), 1_760_000_003_000))
            .unwrap();
        let ba = publish_next(a.conn(), &la).unwrap().unwrap();
        let bb = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(b.conn(), &lb, &ba, crate::store::now_ms()).unwrap();
        receive(a.conn(), &la, &bb, crate::store::now_ms()).unwrap();
        // SY13: both devices show the same one, and keep both versions for the reader.
        assert_eq!(note_of(&a, "book-a"), note_of(&b, "book-b"));
        let key = Key::parse(&format!("note:{ANNO}")).unwrap().0;
        for (s, l) in [(&a, &la), (&b, &lb)] {
            let r = journal::record_of(s.conn(), l, &key, Which::All).unwrap();
            assert!(
                matches!(r.outcome(&HashSet::new()), Some(Outcome::Conflict { all, .. }) if all.len() == 2)
            );
        }
    }

    #[test]
    fn a_deletion_and_a_change_made_apart_stay_deleted_until_the_reader_chooses() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", None, 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        a.delete_annotation(ANNO).unwrap();
        let mut changed = highlight("book-b", None, 1_760_000_003_000);
        changed.color = "rose".into();
        b.save_annotation(&changed).unwrap();
        let ba = publish_next(a.conn(), &la).unwrap().unwrap();
        let bb = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(b.conn(), &lb, &ba, crate::store::now_ms()).unwrap();
        receive(a.conn(), &la, &bb, crate::store::now_ms()).unwrap();
        // Item 102: deleted on both, the change waiting in the conflict.
        assert!(a.annotations("book-a").unwrap().is_empty());
        assert!(b.annotations("book-b").unwrap().is_empty());
    }

    #[test]
    fn the_newest_place_wins_on_both_devices() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        b.save_position("book-b", "epubcfi(/6/20)", 0.6, None)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.save_position("book-a", "epubcfi(/6/10)", 0.2, None)
            .unwrap();
        let ba = publish_next(a.conn(), &la).unwrap().unwrap();
        let bb = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(b.conn(), &lb, &ba, crate::store::now_ms()).unwrap();
        receive(a.conn(), &la, &bb, crate::store::now_ms()).unwrap();
        // SY12, item 105: the later one, even though it is earlier in the book.
        assert_eq!(a.position("book-a").unwrap().unwrap().0, "epubcfi(/6/10)");
        assert_eq!(b.position("book-b").unwrap().unwrap().0, "epubcfi(/6/10)");
    }

    #[test]
    fn gaps_wait_repeats_change_nothing_and_bad_versions_are_set_aside() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let first = publish_next(a.conn(), &la).unwrap().unwrap();
        a.save_position("book-a", "epubcfi(/6/4)", 0.2, None)
            .unwrap();
        let second = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &second, crate::store::now_ms()).unwrap(),
            Received::Waits
        );
        assert!(b.position("book-b").unwrap().is_none());
        receive(b.conn(), &lb, &first, crate::store::now_ms()).unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &first, crate::store::now_ms()).unwrap(),
            Received::Repeat
        );
        let mut forged = second.clone();
        forged
            .versions
            .push(serde_json::json!({ "format": 1, "key": "place:x" }));
        assert!(matches!(
            receive(b.conn(), &lb, &forged, crate::store::now_ms()).unwrap(),
            Received::Applied { set_aside: 1, .. }
        ));
        assert_eq!(b.position("book-b").unwrap().unwrap().0, "epubcfi(/6/4)");
    }

    #[test]
    fn a_version_more_than_a_day_ahead_is_set_aside() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        // Received by a device whose clock reads two days earlier.
        let two_days = 2 * 24 * 60 * 60 * 1000;
        let early = crate::store::now_ms() - two_days;
        assert!(matches!(
            receive(b.conn(), &lb, &batch, early).unwrap(),
            Received::Applied {
                applied: 0,
                set_aside: 2
            }
        ));
        assert!(b.position("book-b").unwrap().is_none());
    }

    #[test]
    fn a_version_claiming_an_edit_its_writer_never_saw_is_refused() {
        // SY9: B's version names a change of A's that A's chain shows never existed.
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let (mut c, lc) = device(3, "book-c");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        send(&a, &la, &c, &lc).unwrap();
        c.save_position("book-c", "epubcfi(/6/4)", 0.2, None)
            .unwrap();
        let mut forged = publish_next(c.conn(), &lc).unwrap().unwrap();
        for v in &mut forged.versions {
            if v["key"].as_str().is_some_and(|k| k.starts_with("place:")) {
                let wrong = Hash32([9; 32]).to_string();
                v["history"] =
                    serde_json::json!([{ "d": la.device.to_string(), "s": 1, "h": wrong }]);
            }
        }
        // B holds A's change 1 first, so the false entry can be checked.
        let first = {
            a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
                .unwrap();
            publish_next(a.conn(), &la).unwrap().unwrap()
        };
        let _ = first;
        let a1: Batch = {
            let rows: Vec<String> = a
                .conn()
                .prepare("SELECT raw FROM sync_versions WHERE seq = 1")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            let hash: String = a
                .conn()
                .query_row("SELECT hash FROM sync_changes WHERE seq = 1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            let bodies = rows
                .iter()
                .filter_map(|r| {
                    let v: Value = serde_json::from_str(r).unwrap();
                    let h = Hash32::parse(v["body"]["hash"].as_str()?)?;
                    journal::body(a.conn(), &h).unwrap()
                })
                .collect();
            Batch {
                device: la.device,
                seq: 1,
                hash: Hash32::parse(&hash).unwrap(),
                versions: rows
                    .iter()
                    .map(|r| serde_json::from_str(r).unwrap())
                    .collect(),
                bodies,
            }
        };
        receive(b.conn(), &lb, &a1, crate::store::now_ms()).unwrap();
        let got = receive(b.conn(), &lb, &forged, crate::store::now_ms()).unwrap();
        assert!(
            matches!(got, Received::Applied { set_aside: 1, .. }),
            "{got:?}"
        );
        assert_eq!(
            b.position("book-b").unwrap().unwrap().0,
            "epubcfi(/6/2)",
            "A's place stands"
        );
    }

    #[test]
    fn a_version_whose_ancestor_hasnt_arrived_waits_then_applies() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let (mut c, lc) = device(3, "book-c");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let from_a = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(c.conn(), &lc, &from_a, crate::store::now_ms()).unwrap();
        c.save_annotation(&highlight("book-c", Some("second"), 1_760_000_002_000))
            .unwrap();
        let from_c = publish_next(c.conn(), &lc).unwrap().unwrap();
        // B gets C's edit first: it names A's version, which B doesn't hold yet.
        receive(b.conn(), &lb, &from_c, crate::store::now_ms()).unwrap();
        assert!(
            b.annotations("book-b").unwrap().is_empty(),
            "waits for its ancestry"
        );
        receive(b.conn(), &lb, &from_a, crate::store::now_ms()).unwrap();
        assert_eq!(
            note_of(&b, "book-b"),
            Some(Some("second".into())),
            "then applies"
        );
    }

    #[test]
    fn a_note_over_its_limit_is_saved_but_stays_on_this_device() {
        // SY28: the local save goes ahead; only the note isn't journalled.
        let (mut a, la) = device(1, "book-a");
        let long = "x".repeat(5 * 1024 * 1024);
        a.save_annotation(&highlight("book-a", Some(&long), 1_760_000_001_000))
            .unwrap();
        assert_eq!(
            a.annotations("book-a").unwrap()[0]
                .note
                .as_deref()
                .map(str::len),
            Some(long.len())
        );
        let keys: Vec<String> = journal::unsent(a.conn(), &la)
            .unwrap()
            .iter()
            .map(|v| v.key.as_str().to_owned())
            .collect();
        assert!(
            keys.iter().any(|k| k.starts_with("annotation:"))
                && !keys.iter().any(|k| k.starts_with("note:"))
        );
    }

    #[test]
    fn a_version_set_aside_for_its_clock_applies_once_the_clock_catches_up() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        let two_days = 2 * 24 * 60 * 60 * 1000;
        receive(b.conn(), &lb, &batch, crate::store::now_ms() - two_days).unwrap();
        assert!(b.position("book-b").unwrap().is_none());
        // SY10: applied once this clock catches up.
        assert!(apply_pending(b.conn(), &lb, crate::store::now_ms()).unwrap() >= 1);
        assert_eq!(b.position("book-b").unwrap().unwrap().0, "epubcfi(/6/2)");
    }

    #[test]
    fn a_successor_of_a_waiting_version_waits_too_and_both_apply_later() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let (mut c, lc) = device(3, "book-c");
        a.save_annotation(&highlight("book-a", Some("from A"), 1_760_000_001_000))
            .unwrap();
        let from_a = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(c.conn(), &lc, &from_a, crate::store::now_ms()).unwrap();
        c.save_annotation(&highlight("book-c", Some("C one"), 1_760_000_002_000))
            .unwrap();
        let c1 = publish_next(c.conn(), &lc).unwrap().unwrap();
        c.save_annotation(&highlight("book-c", Some("C two"), 1_760_000_003_000))
            .unwrap();
        let c2 = publish_next(c.conn(), &lc).unwrap().unwrap();
        receive(b.conn(), &lb, &c1, crate::store::now_ms()).unwrap();
        let got = receive(b.conn(), &lb, &c2, crate::store::now_ms()).unwrap();
        assert!(
            matches!(got, Received::Applied { set_aside: 0, .. }),
            "waits, isn't refused: {got:?}"
        );
        receive(b.conn(), &lb, &from_a, crate::store::now_ms()).unwrap();
        assert_eq!(note_of(&b, "book-b"), Some(Some("C two".into())));
    }

    #[test]
    fn a_waiting_version_found_false_takes_its_body_with_it() {
        // C's note names A's change with the wrong hash: it waits for A, then is false.
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        let (mut c, lc) = device(3, "book-c");
        a.save_annotation(&highlight("book-a", Some("from A"), 1_760_000_001_000))
            .unwrap();
        let from_a = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(c.conn(), &lc, &from_a, crate::store::now_ms()).unwrap();
        c.save_annotation(&highlight(
            "book-c",
            Some("C's own words"),
            1_760_000_002_000,
        ))
        .unwrap();
        let mut c1 = publish_next(c.conn(), &lc).unwrap().unwrap();
        let mut body = None;
        for v in &mut c1.versions {
            if v["key"].as_str().is_some_and(|k| k.starts_with("note:")) {
                for e in v["history"].as_array_mut().unwrap() {
                    if e["d"] == la.device.to_string() {
                        e["h"] = serde_json::json!(Hash32([9; 32]).to_string());
                    }
                }
                body = Some(v["body"]["hash"].as_str().unwrap().to_owned());
            }
        }
        let body = body.unwrap();
        let kept = |s: &Store| -> i64 {
            s.conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_bodies WHERE hash = ?1",
                    [&body],
                    |r| r.get(0),
                )
                .unwrap()
        };
        receive(b.conn(), &lb, &c1, crate::store::now_ms()).unwrap();
        assert_eq!(kept(&b), 1, "kept while it waits");
        receive(b.conn(), &lb, &from_a, crate::store::now_ms()).unwrap();
        assert_eq!(
            kept(&b),
            0,
            "SY28: not kept outside the budget once found false"
        );
        assert_eq!(note_of(&b, "book-b"), Some(Some("from A".into())));
    }

    #[test]
    fn bad_bodies_past_the_budget_in_one_change_leave_nothing_kept() {
        // Five 4 MB highlight bodies that aren't highlights: 20 MB set aside, over 16.
        let (b, lb) = device(2, "book-b");
        let other = Id16([4; 16]);
        let mut versions = Vec::new();
        let mut bodies = Vec::new();
        for i in 0..5u8 {
            let mut body = vec![b'x'; 4 * 1024 * 1024 - 1];
            body[0] = b'0' + i;
            versions.push(serde_json::json!({
                "format": 1, "key": format!("annotation:{}", uuid::Uuid::new_v4()),
                "device": other.to_string(), "clock": [crate::store::now_ms() as u64, i], "history": [],
                "body": { "hash": identity_hash(&body).to_string(), "size": body.len() },
            }));
            bodies.push(body);
        }
        let batch = Batch {
            device: other,
            seq: 1,
            hash: Hash32([1; 32]),
            versions,
            bodies,
        };
        assert_eq!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap(),
            Received::Stopped
        );
        let kept: i64 = b
            .conn()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM sync_bodies) + (SELECT COUNT(*) FROM sync_aside)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 0, "the whole change is rolled back");
    }

    #[test]
    fn two_versions_of_one_key_in_a_change_are_both_set_aside() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let mut batch = publish_next(a.conn(), &la).unwrap().unwrap();
        let place = batch
            .versions
            .iter()
            .find(|v| v["key"].as_str().is_some_and(|k| k.starts_with("place:")))
            .unwrap()
            .clone();
        let mut twin = place.clone();
        twin["clock"][1] = serde_json::json!(7);
        batch.versions.push(twin);
        assert!(matches!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap(),
            Received::Applied { set_aside: 2, .. }
        ));
        assert!(b.position("book-b").unwrap().is_none());
    }

    #[test]
    fn a_hidden_head_still_moves_the_clock_on() {
        // SY10: A's deletion is shown over B's later edit; B's clock is still seen.
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", None, 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        a.delete_annotation(ANNO).unwrap();
        let mut h = highlight("book-b", None, 1_760_000_002_000);
        h.color = "blue".into();
        b.save_annotation(&h).unwrap();
        let mut from_b = publish_next(b.conn(), &lb).unwrap().unwrap();
        let ahead = crate::store::now_ms() as u64 + 60 * 60 * 1000;
        for v in &mut from_b.versions {
            v["clock"] = serde_json::json!([ahead, 0]);
        }
        receive(a.conn(), &la, &from_b, crate::store::now_ms()).unwrap();
        let ms: i64 = a
            .conn()
            .query_row("SELECT clock_ms FROM sync_libraries", [], |r| r.get(0))
            .unwrap();
        assert!(ms as u64 >= ahead, "{ms} < {ahead}");
    }

    #[test]
    fn an_oversized_local_highlight_isnt_replaced_from_another_device() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        let mut big = highlight("book-a", None, 1_760_000_001_000);
        big.quote_exact = "x".repeat(5 * 1024 * 1024);
        a.save_annotation(&big).unwrap();
        b.save_annotation(&highlight("book-b", None, 1_760_000_002_000))
            .unwrap();
        send(&b, &lb, &a, &la).unwrap();
        let kept = a.annotations("book-a").unwrap();
        assert_eq!(
            kept[0].quote_exact.len(),
            big.quote_exact.len(),
            "SY28: stays"
        );
    }

    #[test]
    fn a_body_arriving_late_is_held_to_its_declared_size() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let mut batch = publish_next(a.conn(), &la).unwrap().unwrap();
        for v in &mut batch.versions {
            if v["key"].as_str().is_some_and(|k| k.starts_with("place:")) {
                v["body"]["size"] = serde_json::json!(3);
            }
        }
        let bodies = std::mem::take(&mut batch.bodies);
        receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap();
        for body in &bodies {
            journal::put_body(b.conn(), body).unwrap();
        }
        apply_pending(b.conn(), &lb, crate::store::now_ms()).unwrap();
        assert!(b.position("book-b").unwrap().is_none());
    }

    #[test]
    fn a_newest_wins_record_settles_once() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        b.save_position("book-b", "epubcfi(/6/20)", 0.6, None)
            .unwrap();
        a.save_position("book-a", "epubcfi(/6/10)", 0.2, None)
            .unwrap();
        let ba = publish_next(a.conn(), &la).unwrap().unwrap();
        let bb = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(b.conn(), &lb, &ba, crate::store::now_ms()).unwrap();
        receive(a.conn(), &la, &bb, crate::store::now_ms()).unwrap();
        assert_eq!(
            apply_pending(a.conn(), &la, crate::store::now_ms()).unwrap(),
            0
        );
        assert_eq!(
            apply_pending(b.conn(), &lb, crate::store::now_ms()).unwrap(),
            0
        );
    }

    #[test]
    fn a_note_added_apart_to_a_deleted_highlight_is_a_conflict_for_the_reader() {
        // SY13: B adds the first note while A deletes the highlight, apart.
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", None, 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        a.delete_annotation(ANNO).unwrap();
        b.save_annotation(&highlight(
            "book-b",
            Some("a first note"),
            1_760_000_001_000, // only the note changes
        ))
        .unwrap();
        let ba = publish_next(a.conn(), &la).unwrap().unwrap();
        let bb = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(b.conn(), &lb, &ba, crate::store::now_ms()).unwrap();
        receive(a.conn(), &la, &bb, crate::store::now_ms()).unwrap();
        for (s, l) in [(&a, &la), (&b, &lb)] {
            assert_eq!(
                dependent_conflicts(s.conn(), l, ANNO).unwrap().len(),
                1,
                "the note waits for the reader"
            );
        }
    }

    #[test]
    fn an_edit_held_in_a_conflict_isnt_published_or_overwritten() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        a.save_annotation(&highlight("book-a", Some("A's"), 1_760_000_002_000))
            .unwrap();
        b.save_annotation(&highlight("book-b", Some("B's later"), 1_760_000_003_000))
            .unwrap();
        let ba = publish_next(a.conn(), &la).unwrap().unwrap();
        let bb = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(a.conn(), &la, &bb, crate::store::now_ms()).unwrap();
        receive(b.conn(), &lb, &ba, crate::store::now_ms()).unwrap();
        // On A, B's later version is shown and A's own is hidden: A's next edit is held.
        a.save_annotation(&highlight(
            "book-a",
            Some("A edits again"),
            1_760_000_004_000,
        ))
        .unwrap();
        assert_eq!(note_of(&a, "book-a"), Some(Some("A edits again".into())));
        let key = Key::parse(&format!("note:{ANNO}")).unwrap().0;
        assert!(journal::held(a.conn(), &la, &key).unwrap());
        assert!(
            journal::unsent(a.conn(), &la)
                .unwrap()
                .iter()
                .all(|v| v.key != key),
            "not published"
        );
        // Something else arriving for that note doesn't overwrite the held edit.
        b.save_annotation(&highlight("book-b", Some("B again"), 1_760_000_005_000))
            .unwrap();
        send(&b, &lb, &a, &la).unwrap();
        assert_eq!(note_of(&a, "book-a"), Some(Some("A edits again".into())));
    }

    #[test]
    fn receiving_stops_once_a_devices_set_aside_budget_is_spent() {
        // SY28: at most 1,000 items or 16 MB set aside from one device.
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        for i in 0..ASIDE_ITEMS {
            b.conn()
                .execute(
                    "INSERT INTO sync_aside (library, device, hash, reason, size, raw, first_at) VALUES (?1, ?2, ?3, 'x', 1, X'00', 0)",
                    params![LIB.to_string(), la.device.to_string(), format!("{i:064}")],
                )
                .unwrap();
        }
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap(),
            Received::Stopped
        );
        assert!(b.position("book-b").unwrap().is_none());
    }

    #[test]
    fn clock_ahead_versions_count_against_the_budget() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        // 1,000 versions already held from A for a clock ahead (SY10).
        for i in 0..ASIDE_ITEMS {
            b.conn()
                .execute(
                    "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied)
                     VALUES (?1, 'place:x', ?2, ?3, 'h', '{}', -2)",
                    params![LIB.to_string(), la.device.to_string(), i + 1],
                )
                .unwrap();
        }
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        assert_eq!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap(),
            Received::Stopped
        );
    }

    #[test]
    fn a_held_edit_isnt_numbered_with_other_changes() {
        let (_, la) = device(1, "book-a");
        let (mut s, _) = device(1, "book-a2");
        let _ = la;
        let lib = journal::active(s.conn()).unwrap().unwrap();
        let note = Key::parse(&format!("note:{ANNO}")).unwrap().0;
        journal::hold_local(
            s.conn(),
            &lib,
            &note,
            journal::LocalContent::Body(br#"{"text":"held"}"#.to_vec()),
            1000,
        )
        .unwrap();
        s.save_position("book-a2", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let (_, taken) = journal::take_unsent(s.conn(), &lib, 0).unwrap().unwrap();
        assert!(taken.iter().all(|v| v.key != note));
        // A crash replay of change 1 doesn't pick it up either.
        let (seq, again) = journal::take_unsent(s.conn(), &lib, 0).unwrap().unwrap();
        assert_eq!(seq, 1);
        assert!(again.iter().all(|v| v.key != note));
        assert!(journal::held(s.conn(), &lib, &note).unwrap());
    }

    #[test]
    fn a_deletion_doesnt_cover_a_change_still_waiting_to_apply() {
        // SY13: B's note edit arrives at A but waits (here: its body is missing).
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        b.save_annotation(&highlight("book-b", Some("B's edit"), 1_760_000_002_000))
            .unwrap();
        let mut from_b = publish_next(b.conn(), &lb).unwrap().unwrap();
        from_b.bodies.clear();
        receive(a.conn(), &la, &from_b, crate::store::now_ms()).unwrap();
        let frontier = crate::sync::local::covers(a.conn(), &la).unwrap();
        assert!(
            frontier.iter().all(|e| e.device != lb.device),
            "B's waiting change isn't covered: {frontier:?}"
        );
    }

    #[test]
    fn a_body_that_would_pass_the_budget_isnt_kept() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        // 16 MB less a little already kept aside from A.
        b.conn()
            .execute(
                "INSERT INTO sync_aside (library, device, hash, reason, size, raw, first_at) VALUES (?1, ?2, 'x', 'x', ?3, X'00', 0)",
                params![LIB.to_string(), la.device.to_string(), ASIDE_BYTES - 10],
            )
            .unwrap();
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        let two_days = 2 * 24 * 60 * 60 * 1000;
        assert_eq!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms() - two_days).unwrap(),
            Received::Stopped
        );
    }

    #[test]
    fn a_version_excluded_for_its_body_is_still_in_its_writers_chain() {
        let (b, lb) = device(2, "book-b");
        let other = Id16([4; 16]);
        let h = Hash32([3; 32]);
        b.conn()
            .execute(
                "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied) VALUES (?1, ?2, ?3, 1, ?4, '{}', -4)",
                params![LIB.to_string(), format!("note:{ANNO}"), other.to_string(), h.to_string()],
            )
            .unwrap();
        b.conn()
            .execute(
                "INSERT INTO sync_seen (library, device, seq, hash) VALUES (?1, ?2, 1, ?3)",
                params![LIB.to_string(), other.to_string(), h.to_string()],
            )
            .unwrap();
        let chains = JournalChains {
            conn: b.conn(),
            lib: &lb,
        };
        assert_eq!(
            chains.version_at(other, 1, &format!("note:{ANNO}")),
            Lookup::Present(h)
        );
        assert_eq!(
            chains.prior_version(other, &format!("note:{ANNO}"), 2),
            Prior::At(1, h)
        );
    }

    #[test]
    fn an_oversized_local_note_isnt_replaced_from_another_device() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("short"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        let long = "x".repeat(5 * 1024 * 1024);
        b.save_annotation(&highlight("book-b", Some(&long), 1_760_000_002_000))
            .unwrap();
        a.save_annotation(&highlight("book-a", Some("A edits"), 1_760_000_003_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        assert_eq!(
            note_of(&b, "book-b").flatten().map(|n| n.len()),
            Some(long.len()),
            "SY28: stays"
        );
    }

    #[test]
    fn a_note_tombstone_without_its_highlight_settles() {
        let (b, lb) = device(2, "book-b");
        let other = Id16([4; 16]);
        let v = serde_json::json!({
            "format": 1, "key": format!("note:{ANNO}"), "device": other.to_string(),
            "clock": [crate::store::now_ms() as u64, 0], "history": [], "deleted": { "covers": [] },
        });
        let batch = Batch {
            device: other,
            seq: 1,
            hash: Hash32([1; 32]),
            versions: vec![v],
            bodies: vec![],
        };
        assert!(matches!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap(),
            Received::Applied { applied: 1, .. }
        ));
        assert_eq!(
            apply_pending(b.conn(), &lb, crate::store::now_ms()).unwrap(),
            0
        );
    }

    #[test]
    fn a_body_larger_than_declared_is_refused() {
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let mut batch = publish_next(a.conn(), &la).unwrap().unwrap();
        for v in &mut batch.versions {
            if v["key"].as_str().is_some_and(|k| k.starts_with("place:")) {
                v["body"]["size"] = serde_json::json!(3);
            }
        }
        assert!(matches!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap(),
            Received::Applied { set_aside: 1, .. }
        ));
        assert!(b.position("book-b").unwrap().is_none());
    }

    #[test]
    fn versions_waiting_for_a_highlight_count_against_the_budget() {
        // A device sending notes for highlights that never come is stopped once what
        // waits passes the pending budget (PENDING_BYTES).
        let (b, lb) = device(2, "book-b");
        let other = Id16([4; 16]);
        let big = "x".repeat(200 * 1024);
        let mut stopped = false;
        for seq in 1..=20u64 {
            let text = format!(r#"{{"text":"{big}{seq}"}}"#).into_bytes();
            let id = uuid::Uuid::new_v4();
            let v = serde_json::json!({
                "format": 1, "key": format!("note:{id}"), "device": other.to_string(),
                "clock": [crate::store::now_ms() as u64, 0], "history": [],
                "body": { "hash": identity_hash(&text).to_string(), "size": text.len() },
            });
            let batch = Batch {
                device: other,
                seq,
                hash: Hash32([seq as u8; 32]),
                versions: vec![v],
                bodies: vec![text],
            };
            if receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap() == Received::Stopped
            {
                stopped = true;
                break;
            }
        }
        assert!(stopped);
    }

    #[test]
    fn a_large_first_sync_isnt_charged_to_the_set_aside_budget() {
        // 600 highlights with notes, in one change, all applied.
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        for i in 0..600 {
            let mut h = highlight("book-a", Some("n"), 1_760_000_001_000);
            h.id = uuid::Uuid::new_v4().to_string();
            h.quote_exact = format!("q{i}");
            a.save_annotation(&h).unwrap();
        }
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        assert!(batch.versions.len() > ASIDE_ITEMS as usize);
        assert!(matches!(
            receive(b.conn(), &lb, &batch, crate::store::now_ms()).unwrap(),
            Received::Applied { .. }
        ));
        assert_eq!(b.annotations("book-b").unwrap().len(), 600);
    }

    #[test]
    fn an_unsent_edit_hidden_by_a_remote_conflict_is_held() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        // A edits and doesn't publish yet; B's later edit arrives and is shown.
        a.save_annotation(&highlight("book-a", Some("A's unsent"), 1_760_000_001_000))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        b.save_annotation(&highlight("book-b", Some("B's"), 1_760_000_001_000))
            .unwrap();
        send(&b, &lb, &a, &la).unwrap();
        let key = Key::parse(&format!("note:{ANNO}")).unwrap().0;
        assert!(
            journal::held(a.conn(), &la, &key).unwrap(),
            "held for the reader, not lost"
        );
        assert_eq!(note_of(&a, "book-a"), Some(Some("A's unsent".into())));
    }

    #[test]
    fn equal_notes_written_apart_settle_once_on_both() {
        let (mut a, la) = device(1, "book-a");
        let (mut b, lb) = device(2, "book-b");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        send(&a, &la, &b, &lb).unwrap();
        a.save_annotation(&highlight("book-a", Some("same"), 1_760_000_001_000))
            .unwrap();
        b.save_annotation(&highlight("book-b", Some("same"), 1_760_000_001_000))
            .unwrap();
        let ba = publish_next(a.conn(), &la).unwrap().unwrap();
        let bb = publish_next(b.conn(), &lb).unwrap().unwrap();
        receive(b.conn(), &lb, &ba, crate::store::now_ms()).unwrap();
        receive(a.conn(), &la, &bb, crate::store::now_ms()).unwrap();
        for (s, l) in [(&a, &la), (&b, &lb)] {
            assert_eq!(
                apply_pending(s.conn(), l, crate::store::now_ms()).unwrap(),
                0
            );
            let left: i64 = s
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_versions WHERE applied = 0",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(left, 0, "nothing left pending");
        }
    }

    #[test]
    fn a_failure_while_applying_leaves_nothing_half_done() {
        // Plan §6.3: a batch is applied in one transaction, all or nothing.
        let (mut a, la) = device(1, "book-a");
        let (b, lb) = device(2, "book-b");
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        let batch = publish_next(a.conn(), &la).unwrap().unwrap();
        b.conn().execute_batch("DROP TABLE positions").unwrap();
        assert!(receive(b.conn(), &lb, &batch, crate::store::now_ms()).is_err());
        let kept: i64 = b
            .conn()
            .query_row("SELECT COUNT(*) FROM sync_versions", [], |r| r.get(0))
            .unwrap();
        let seen: i64 = b
            .conn()
            .query_row("SELECT COUNT(*) FROM sync_seen", [], |r| r.get(0))
            .unwrap();
        assert_eq!((kept, seen), (0, 0));
    }

    #[test]
    fn joining_publishes_the_whole_library() {
        // SY29: what this device had becomes its own new changes.
        let (mut a, la) = device(1, "book-a");
        journal::set_device(a.conn(), &la, false, None, 0).unwrap();
        a.conn().execute("DELETE FROM sync_libraries", []).unwrap();
        a.save_position("book-a", "epubcfi(/6/2)", 0.1, None)
            .unwrap();
        a.save_annotation(&highlight("book-a", Some("kept"), 1_760_000_001_000))
            .unwrap();
        assert!(
            journal::unsent(a.conn(), &la).unwrap().is_empty(),
            "nothing before a join (SY6)"
        );
        journal::join(a.conn(), &la, "icloud-drive", crate::store::now_ms()).unwrap();
        assert_eq!(local::publish_library(a.conn(), 1_000).unwrap(), 2);
        let keys: HashSet<String> = journal::unsent(a.conn(), &la)
            .unwrap()
            .iter()
            .map(|v| v.key.as_str().to_owned())
            .collect();
        assert!(keys.iter().any(|k| k.starts_with("place:")));
        assert!(
            keys.contains(&format!("annotation:{ANNO}")) && keys.contains(&format!("note:{ANNO}"))
        );
    }
}
