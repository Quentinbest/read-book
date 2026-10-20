//! SY7, SY9, SY29, SP11, SP15: the sync journal in the library's database (migration 6).
//!
//! A local change is journalled as a version without a sequence; the device numbers
//! and signs it only when it publishes it (SY7, SP15). Until then, a later change to
//! the same record replaces it, so only the latest goes out. Remote versions are kept
//! with their sequence, exactly as received. Bodies live by their plain hash (SP4).
//!
//! A new local version descends from what the reader saw: the record's settled
//! version, or the one shown while a conflict waits, and the writer's own previous
//! version (SP11). It never descends from a version waiting in a conflict, so editing
//! the shown text doesn't silently discard the other one (SY13).

use super::clock::{Clock, Hlc};
use super::ids::{identity_hash, DeviceId, Hash32, Id16};
use super::merge::{Outcome, Placed, Record};
use super::record::{check_version, Checked, Entry, Key, RecordError, Version, FORMAT};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("a stored version no longer checks: {0}")]
    Record(#[from] RecordError),
    #[error("library {0} isn't set up for sync")]
    NoLibrary(Id16),
    #[error("the device store's last sequence ({0}) is behind the library's")]
    StoreBehind(u64),
    #[error("{0} waits for the reader to choose between versions")]
    Conflicted(Key),
    #[error("library {0} lives in another storage")]
    OtherStorage(Id16),
}

pub type Result<T> = std::result::Result<T, JournalError>;

/// A library this device has joined, and this device's ID in it (SP28).
#[derive(Clone, Copy, Debug)]
pub struct Library {
    pub id: Id16,
    pub device: DeviceId,
}

/// What a local change makes a record hold.
#[derive(Clone, Debug)]
pub enum LocalContent {
    /// A body: its bytes are kept here and only its hash goes in the version (SP12).
    Body(Vec<u8>),
    /// SP13: a tombstone with its causal frontier and, for a book, its identity.
    Deleted {
        covers: Vec<Entry>,
        identity: Option<Value>,
    },
    /// SP12: a choice's inline body.
    Inline(Value),
}

/// SY29: join a library in a storage as `lib.device`, making it the active one and
/// turning sync on. Only the reader's own join, or Resume, calls this; a start calls
/// `set_device`, which leaves the state as it is. Nothing else is written to the sync
/// tables before a join.
pub fn join(conn: &Connection, lib: &Library, storage: &str, now_ms: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    // SY29: each storage holds its own library, with its own ID, members and naming
    // key, so switching storage joins a different library; one library never moves.
    let existing: Option<String> = tx
        .query_row(
            "SELECT storage FROM sync_libraries WHERE library = ?1",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some_and(|s| s != storage) {
        return Err(JournalError::OtherStorage(lib.id));
    }
    tx.execute("UPDATE sync_libraries SET active = 0 WHERE active = 1", [])?;
    tx.execute(
        "INSERT INTO sync_libraries (library, storage, device, state, active, joined_at)
         VALUES (?1, ?2, ?3, 'on', 1, ?4)
         ON CONFLICT(library) DO UPDATE SET active = 1, state = 'on'",
        params![lib.id.to_string(), storage, lib.device.to_string(), now_ms],
    )?;
    tx.commit()?;
    set_device(conn, lib, false, None, now_ms as u64)
}

/// SY7, SP28: at each start, this device's ID in the library, from its device store.
/// The library's state (on, off, paused) is left as it is. When the ID changed (a
/// rotation, or enrolling as a new device), the local versions not yet published are
/// re-authored under the new identity, with the old identity's versions kept as
/// their ancestry; with `fresh_clock` (SY10: rotating because this clock ran ahead)
/// the clock starts again from the system clock and they are stamped afresh.
/// `old_last_seq` is the last sequence the old identity signed (its device store),
/// or `None` when there is nothing to recover (a join):
/// a change numbered after it but never signed (a crash in between) can never be
/// signed now, so its versions go back to unsent and are re-authored with the rest,
/// unless a later local edit to the same record already replaces them.
pub fn set_device(
    conn: &Connection,
    lib: &Library,
    fresh_clock: bool,
    old_last_seq: Option<u64>,
    wall_ms: u64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let stored: Option<String> = tx
        .query_row(
            "SELECT device FROM sync_libraries WHERE library = ?1",
            [lib.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Err(JournalError::NoLibrary(lib.id));
    };
    if stored == lib.device.to_string() {
        return Ok(());
    }
    tx.execute(
        "UPDATE sync_libraries SET device = ?2 WHERE library = ?1",
        params![lib.id.to_string(), lib.device.to_string()],
    )?;
    if fresh_clock {
        tx.execute(
            "UPDATE sync_libraries SET clock_ms = 0, clock_n = 0 WHERE library = ?1",
            [lib.id.to_string()],
        )?;
    }
    // Numbered by the old identity but never signed: back to unsent, or dropped
    // where a later unsent edit to the same record replaces them.
    if let Some(last) = old_last_seq {
        tx.execute(
            "DELETE FROM sync_versions
             WHERE library = ?1 AND device = ?2 AND seq > ?3
               AND key IN (SELECT key FROM sync_versions WHERE library = ?1 AND seq IS NULL)",
            params![lib.id.to_string(), stored, last as i64],
        )?;
        tx.execute(
            "UPDATE sync_versions SET seq = NULL WHERE library = ?1 AND device = ?2 AND seq > ?3",
            params![lib.id.to_string(), stored, last as i64],
        )?;
    }
    let pending: Vec<(i64, String, String)> = tx
        .prepare(
            "SELECT rowid, raw, hash FROM sync_versions WHERE library = ?1 AND seq IS NULL ORDER BY rowid",
        )?
        .query_map([lib.id.to_string()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let old = Id16::parse(&stored);
    for (rowid, raw, old_hash) in pending {
        let mut v: Value = serde_json::from_str(&raw).map_err(|_| RecordError::NotAnObject)?;
        v["device"] = json!(lib.device.to_string());
        // No ancestor may be a version the old identity never signed: such an entry
        // falls back to the old identity's latest signed version of the record (SP11).
        if let Some(last) = old_last_seq {
            let key = v["key"].as_str().unwrap_or_default().to_owned();
            let unsigned = |e: &Value| {
                e["d"].as_str() == Some(stored.as_str())
                    && e["s"].as_u64().is_some_and(|s| s > last)
            };
            if v["history"]
                .as_array()
                .is_some_and(|h| h.iter().any(unsigned))
            {
                let signed: Option<(i64, String)> = tx
                    .query_row(
                        "SELECT seq, hash FROM sync_versions
                         WHERE library = ?1 AND device = ?2 AND key = ?3 AND seq <= ?4
                         ORDER BY seq DESC LIMIT 1",
                        params![lib.id.to_string(), stored, key, last as i64],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some(h) = v["history"].as_array_mut() {
                    h.retain(|e| !unsigned(e));
                    if let Some((s, hash)) = signed {
                        h.push(json!({ "d": stored, "s": s, "h": hash }));
                    }
                }
            }
        }
        if fresh_clock {
            // The fresh clock still sorts after every applied ancestor from another
            // device, whose clocks are good; only the old identity's is left out.
            if let Some(key) = v["key"].as_str().and_then(Key::parse).map(|(k, _)| k) {
                for p in record_of(&tx, lib, &key, Which::Applied)?.versions() {
                    if Some(p.version.device) != old {
                        observe(&tx, lib, p.version.clock, wall_ms)?;
                    }
                }
            }
            v["clock"] = json!(tick(&tx, lib, wall_ms)?);
        }
        let Checked::Version(version) = check_version(v.clone(), lib.device)? else {
            unreachable!("this Linen wrote it")
        };
        tx.execute(
            "UPDATE sync_versions SET device = ?2, hash = ?3, raw = ?4 WHERE rowid = ?1",
            params![
                rowid,
                lib.device.to_string(),
                version.hash.to_string(),
                v.to_string()
            ],
        )?;
        // SY17: a re-authored tombstone keeps its deletion's age and what it replaced.
        tx.execute(
            "UPDATE sync_deleted SET device = ?3, hash = ?4
             WHERE library = ?1 AND key = ?2 AND device = ?5 AND hash = ?6",
            params![
                lib.id.to_string(),
                version.key.as_str(),
                lib.device.to_string(),
                version.hash.to_string(),
                stored,
                old_hash
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// The library sync is on for, and this device's ID in it, if any (item 109: one
/// at a time).
pub fn active(conn: &Connection) -> Result<Option<Library>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT library, device FROM sync_libraries WHERE active = 1 AND state = 'on'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(l, d)| {
        Some(Library {
            id: Id16::parse(&l)?,
            device: Id16::parse(&d)?,
        })
    }))
}

/// Keeps a body by its plain hash; the same bytes are kept once.
pub fn put_body(conn: &Connection, bytes: &[u8]) -> Result<Hash32> {
    let hash = identity_hash(bytes);
    conn.execute(
        "INSERT OR IGNORE INTO sync_bodies (hash, body) VALUES (?1, ?2)",
        params![hash.to_string(), bytes],
    )?;
    Ok(hash)
}

pub fn body(conn: &Connection, hash: &Hash32) -> Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row(
            "SELECT body FROM sync_bodies WHERE hash = ?1",
            [hash.to_string()],
            |r| r.get(0),
        )
        .optional()?)
}

fn check(raw: &str, device: DeviceId) -> Result<Version> {
    let v: Value = serde_json::from_str(raw).map_err(|_| RecordError::NotAnObject)?;
    match check_version(v, device)? {
        Checked::Version(v) => Ok(v),
        Checked::NewerFormat(_) => Err(RecordError::Field("format").into()),
    }
}

/// Which of a record's versions to load.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// Every version held, for merging.
    All,
    /// Only those applied to the library: what the reader can have seen.
    Applied,
}

/// The versions of `key` this device holds with a sequence: its own published ones
/// and those received from others. Unsent local versions aren't in it, nor are
/// received ones still waiting for their ancestry to be checked (`applied = -1`).
/// The states of `sync_versions.applied`: 1 applied; 2 applied and superseded (a
/// version applied here descends from it, so it is never a head again and isn't
/// loaded to merge); 0 pending; −1 waiting for its ancestry; −2 its clock too far
/// ahead; −3 a local edit held for the reader (#109); −4 excluded for its body.
pub fn record_of(conn: &Connection, lib: &Library, key: &Key, which: Which) -> Result<Record> {
    let mut stmt = conn.prepare(
        "SELECT device, seq, raw FROM sync_versions
         WHERE library = ?1 AND key = ?2 AND seq IS NOT NULL
           AND ((?3 = 0 AND applied = 0) OR applied = 1)",
    )?;
    let applied_only = which == Which::Applied;
    let rows = stmt.query_map(
        params![lib.id.to_string(), key.as_str(), applied_only],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut record = Record::default();
    for row in rows {
        let (device, seq, raw) = row?;
        let device = Id16::parse(&device).ok_or(RecordError::Field("device"))?;
        record.add(Placed {
            version: check(&raw, device)?,
            seq: seq as u64,
        });
    }
    Ok(record)
}

/// SP11: the history a new local version of `key` carries.
fn history_for(record: &Record, device: DeviceId) -> Vec<Entry> {
    let mut latest: BTreeMap<DeviceId, (u64, Hash32)> = BTreeMap::new();
    let mut take = |e: Entry| {
        let slot = latest.entry(e.device).or_insert((0, e.hash));
        if e.seq > slot.0 {
            *slot = (e.seq, e.hash);
        }
    };
    // Settled: every head (equal contents, or the newest of those that aren't read
    // by the reader), so none stays concurrent with the edit. In a conflict, only the
    // one shown: the others wait for the reader (SY13).
    let ignored = HashSet::new();
    let base: Vec<&Placed> = match record.outcome(&ignored) {
        Some(Outcome::Settled(_)) => record.heads(&ignored),
        Some(Outcome::Conflict { shown, .. }) => vec![shown],
        None => vec![],
    };
    for p in base {
        take(p.id());
        p.version.history.iter().copied().for_each(&mut take);
    }
    if let Some(own) = record.latest_of(device) {
        take(own.id());
        own.version.history.iter().copied().for_each(&mut take);
    }
    latest
        .into_iter()
        .map(|(device, (seq, hash))| Entry { device, seq, hash })
        .collect()
}

fn tick(conn: &Connection, lib: &Library, wall_ms: u64) -> Result<Hlc> {
    let (ms, n): (i64, i64) = conn
        .query_row(
            "SELECT clock_ms, clock_n FROM sync_libraries WHERE library = ?1",
            [lib.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(JournalError::NoLibrary(lib.id))?;
    let mut clock = Clock::new(Hlc(ms as u64, n as u16));
    let now = clock.tick(wall_ms);
    conn.execute(
        "UPDATE sync_libraries SET clock_ms = ?2, clock_n = ?3 WHERE library = ?1",
        params![lib.id.to_string(), now.ms() as i64, now.n() as i64],
    )?;
    Ok(now)
}

/// SY10: a remote version applied moves this device's clock to its reading, so a
/// change made after it always sorts after it. (A version far enough ahead to be set
/// aside is never applied, and so never observed.)
pub fn observe(conn: &Connection, lib: &Library, remote: Hlc, wall_ms: u64) -> Result<()> {
    let (ms, n) = clock_of(conn, lib)?;
    let mut clock = Clock::new(Hlc(ms as u64, n as u16));
    clock.observe(remote, wall_ms);
    let now = clock.last();
    conn.execute(
        "UPDATE sync_libraries SET clock_ms = ?2, clock_n = ?3 WHERE library = ?1",
        params![lib.id.to_string(), now.ms() as i64, now.n() as i64],
    )?;
    Ok(())
}

fn clock_of(conn: &Connection, lib: &Library) -> Result<(i64, i64)> {
    conn.query_row(
        "SELECT clock_ms, clock_n FROM sync_libraries WHERE library = ?1",
        [lib.id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .ok_or(JournalError::NoLibrary(lib.id))
}

/// SY7: journal a local change to `key`, replacing an earlier unsent one. Call it in
/// the same transaction as the change itself, so the two can't part.
pub fn record_local(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    content: LocalContent,
    wall_ms: u64,
) -> Result<Version> {
    // Only what was applied: a version still waiting (for its body, a book or a
    // guard) was never seen by the reader, so nothing may claim to descend from it.
    let record = record_of(conn, lib, key, Which::Applied)?;
    // SY13: when this device's own version is one the reader hasn't chosen yet (a
    // conflict head that isn't shown), an ordinary edit would have to descend from it
    // (SP11) and so settle the conflict without the reader. It waits instead: the
    // reader's choice (Stage 5) settles it, descending from every head.
    if let Some(Outcome::Conflict { shown, all }) = record.outcome(&HashSet::new()) {
        let own_hidden = all
            .iter()
            .any(|h| h.version.device == lib.device && h.id() != shown.id());
        if own_hidden {
            return Err(JournalError::Conflicted(key.clone()));
        }
    }
    // SY10: after everything it descends from, whatever the system clock says.
    for p in record.versions() {
        observe(conn, lib, p.version.clock, wall_ms)?;
    }
    let clock = tick(conn, lib, wall_ms)?;
    let history: Vec<Value> = history_for(&record, lib.device)
        .iter()
        .map(|e| json!({ "d": e.device.to_string(), "s": e.seq, "h": e.hash.to_string() }))
        .collect();
    let mut v = json!({
        "format": FORMAT,
        "key": key.as_str(),
        "device": lib.device.to_string(),
        "clock": clock,
        "history": history,
    });
    match content {
        LocalContent::Body(bytes) => {
            let hash = put_body(conn, &bytes)?;
            v["body"] = json!({ "hash": hash.to_string(), "size": bytes.len() });
        }
        LocalContent::Deleted { covers, identity } => {
            let covers: Vec<Value> = covers
                .iter()
                .map(|e| json!({ "d": e.device.to_string(), "s": e.seq, "h": e.hash.to_string() }))
                .collect();
            let mut d = json!({ "covers": covers });
            if let Some(id) = identity {
                d["identity"] = id;
            }
            v["deleted"] = d;
        }
        LocalContent::Inline(i) => v["inline"] = i,
    }
    // What this device writes passes the same checks as what it receives (SY28).
    let Checked::Version(version) = check_version(v.clone(), lib.device)? else {
        unreachable!("this Linen writes its own format")
    };
    conn.execute(
        "DELETE FROM sync_versions WHERE library = ?1 AND key = ?2 AND seq IS NULL",
        params![lib.id.to_string(), key.as_str()],
    )?;
    conn.execute(
        "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, 1)",
        params![
            lib.id.to_string(),
            key.as_str(),
            lib.device.to_string(),
            version.hash.to_string(),
            v.to_string()
        ],
    )?;
    Ok(version)
}

/// SY13, #109: an edit made while this device's own version waits, unshown, in a
/// conflict. It is kept as the reader's candidate choice (`applied = -3`): it
/// descends from every head, as a choice does, but it is not published and not
/// merged until the reader confirms it (Stage 5). A later edit to the same record
/// replaces it.
pub fn hold_local(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    content: LocalContent,
    wall_ms: u64,
) -> Result<Version> {
    // SY13: every head in the conflict, those still unapplied (hidden) included;
    // never one waiting for its ancestry, a clock or a bad body (not loaded).
    let record = record_of(conn, lib, key, Which::All)?;
    // SY10: only what was applied moves the clock, so a hidden head whose body
    // hasn't arrived (or proved good) doesn't. The held edit is still after every
    // head by descent, which is what settles a record the reader decides (SY13).
    for p in record_of(conn, lib, key, Which::Applied)?.versions() {
        observe(conn, lib, p.version.clock, wall_ms)?;
    }
    let clock = tick(conn, lib, wall_ms)?;
    let mut latest: BTreeMap<DeviceId, (u64, Hash32)> = BTreeMap::new();
    for h in record.heads(&HashSet::new()) {
        for e in std::iter::once(h.id()).chain(h.version.history.iter().copied()) {
            let slot = latest.entry(e.device).or_insert((0, e.hash));
            if e.seq > slot.0 {
                *slot = (e.seq, e.hash);
            }
        }
    }
    let history: Vec<Value> = latest
        .iter()
        .map(|(d, (s, h))| json!({ "d": d.to_string(), "s": s, "h": h.to_string() }))
        .collect();
    let mut v = json!({
        "format": FORMAT, "key": key.as_str(), "device": lib.device.to_string(),
        "clock": clock, "history": history,
    });
    match content {
        LocalContent::Body(bytes) => {
            let hash = put_body(conn, &bytes)?;
            v["body"] = json!({ "hash": hash.to_string(), "size": bytes.len() });
        }
        LocalContent::Deleted { covers, identity } => {
            let covers: Vec<Value> = covers
                .iter()
                .map(|e| json!({ "d": e.device.to_string(), "s": e.seq, "h": e.hash.to_string() }))
                .collect();
            let mut d = json!({ "covers": covers });
            if let Some(id) = identity {
                d["identity"] = id;
            }
            v["deleted"] = d;
        }
        LocalContent::Inline(i) => v["inline"] = i,
    }
    let Checked::Version(version) = check_version(v.clone(), lib.device)? else {
        unreachable!("this Linen writes its own format")
    };
    conn.execute(
        "DELETE FROM sync_versions WHERE library = ?1 AND key = ?2 AND seq IS NULL",
        params![lib.id.to_string(), key.as_str()],
    )?;
    conn.execute(
        "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, -3)",
        params![
            lib.id.to_string(),
            key.as_str(),
            lib.device.to_string(),
            version.hash.to_string(),
            v.to_string()
        ],
    )?;
    Ok(version)
}

/// Whether an edit to `key` is held for the reader's choice (`hold_local`).
pub fn held(conn: &Connection, lib: &Library, key: &Key) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_versions WHERE library = ?1 AND key = ?2 AND applied = -3",
            params![lib.id.to_string(), key.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The local versions waiting to be published (SY7), oldest key first.
/// Whether a body is kept here.
pub fn has_body(conn: &Connection, hash: &Hash32) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_bodies WHERE hash = ?1",
            [hash.to_string()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Deletes a body no version names any more (`sync_versions_body`, migration 7).
pub fn drop_body_if_unnamed(conn: &Connection, hash: &Hash32) -> Result<()> {
    conn.execute(
        "DELETE FROM sync_bodies WHERE hash = ?1 AND NOT EXISTS
         (SELECT 1 FROM sync_versions WHERE json_extract(raw, '$.body.hash') = ?1)",
        [hash.to_string()],
    )?;
    Ok(())
}

/// SY28: this device's unsent versions of one key are dropped, held or not: a later
/// local value, over its limit, replaced them and stays on this device.
pub fn drop_unsent(conn: &Connection, lib: &Library, key: &Key) -> Result<()> {
    let hashes: Vec<String> = conn
        .prepare(
            "SELECT json_extract(raw, '$.body.hash') FROM sync_versions
             WHERE library = ?1 AND key = ?2 AND seq IS NULL AND json_extract(raw, '$.body.hash') IS NOT NULL",
        )?
        .query_map(params![lib.id.to_string(), key.as_str()], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    conn.execute(
        "DELETE FROM sync_versions WHERE library = ?1 AND key = ?2 AND seq IS NULL",
        params![lib.id.to_string(), key.as_str()],
    )?;
    for h in hashes.iter().filter_map(|h| Hash32::parse(h)) {
        drop_body_if_unnamed(conn, &h)?;
    }
    Ok(())
}

/// SY26: a held version becomes this device's ordinary unsent one, to be published:
/// the reader's own decision (Stop syncing) settles the conflict it descends from.
pub fn release_held(conn: &Connection, lib: &Library, key: &Key) -> Result<()> {
    conn.execute(
        "UPDATE sync_versions SET applied = 1 WHERE library = ?1 AND key = ?2 AND seq IS NULL AND applied = -3",
        params![lib.id.to_string(), key.as_str()],
    )?;
    Ok(())
}

/// This device's unnumbered version of one key (unsent, or held for a choice), as
/// written, with its body, if it has one.
pub fn unnumbered(
    conn: &Connection,
    lib: &Library,
    key: &Key,
) -> Result<Option<(String, Vec<u8>)>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT raw FROM sync_versions WHERE library = ?1 AND key = ?2 AND seq IS NULL",
            params![lib.id.to_string(), key.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    match check(&raw, lib.device)?.content {
        super::record::Content::Body { hash, .. } => Ok(body(conn, &hash)?.map(|b| (raw, b))),
        _ => Ok(None),
    }
}

/// This device's unsent version of one key, if any (not one held, #109).
pub fn unsent_of(conn: &Connection, lib: &Library, key: &Key) -> Result<Option<Version>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT raw FROM sync_versions
             WHERE library = ?1 AND key = ?2 AND seq IS NULL AND applied != -3",
            params![lib.id.to_string(), key.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|r| check(&r, lib.device)).transpose()
}

pub fn unsent(conn: &Connection, lib: &Library) -> Result<Vec<Version>> {
    let mut stmt = conn.prepare(
        "SELECT raw FROM sync_versions
         WHERE library = ?1 AND seq IS NULL AND applied != -3 ORDER BY rowid",
    )?;
    let rows = stmt.query_map([lib.id.to_string()], |r| r.get::<_, String>(0))?;
    rows.map(|raw| check(&raw?, lib.device)).collect()
}

/// SY7, SP15: number every unsent version as this device's next change, in one
/// transaction, and return the sequence and the versions in it. `last_seq` is the
/// last sequence this device signed, from its device store (SP28): only it knows
/// changes that held no version (a reservation, a hold) or whose rows were compacted,
/// so only it can say which sequence is next. From then on the
/// change's content is fixed by these rows: signing it (Stage 1c) is deterministic,
/// so after a crash the same file is signed again, never a different one for the
/// same sequence. A local edit made afterwards is a new unsent version that
/// descends from these (SP11).
pub fn take_unsent(
    conn: &Connection,
    lib: &Library,
    last_seq: u64,
) -> Result<Option<(u64, Vec<Version>)>> {
    let tx = conn.unchecked_transaction()?;
    let journalled = tx
        .query_row(
            "SELECT MAX(seq) FROM sync_versions WHERE library = ?1 AND device = ?2 AND seq IS NOT NULL",
            params![lib.id.to_string(), lib.device.to_string()],
            |r| r.get::<_, Option<i64>>(0),
        )?
        .unwrap_or(0) as u64;
    if journalled == last_seq + 1 {
        // Numbered before a crash, but never signed: the same change again, from the
        // same rows, before anything new is numbered.
        let mut stmt = tx.prepare(
            "SELECT raw FROM sync_versions WHERE library = ?1 AND device = ?2 AND seq = ?3 ORDER BY rowid",
        )?;
        let again = stmt
            .query_map(
                params![
                    lib.id.to_string(),
                    lib.device.to_string(),
                    journalled as i64
                ],
                |r| r.get::<_, String>(0),
            )?
            .map(|raw| check(&raw?, lib.device))
            .collect::<Result<Vec<_>>>()?;
        return Ok(Some((journalled, again)));
    }
    if journalled > last_seq {
        // The library holds a later signed change of ours than the device store: a
        // store rolled back. SY7's start check rotates before anything is published.
        return Err(JournalError::StoreBehind(last_seq));
    }
    let versions = unsent(&tx, lib)?;
    if versions.is_empty() {
        return Ok(None);
    }
    let seq = last_seq + 1;
    tx.execute(
        // Exactly the rows `unsent` chose: never an edit held for the reader (#109).
        "UPDATE sync_versions SET seq = ?2 WHERE library = ?1 AND seq IS NULL AND applied != -3",
        params![lib.id.to_string(), seq as i64],
    )?;
    tx.commit()?;
    Ok(Some((seq, versions)))
}

/// Keeps a checked remote version; a repeat delivery changes nothing. Returns whether
/// it was new.
pub fn store_remote(conn: &Connection, lib: &Library, p: &Placed) -> Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO sync_versions (library, key, device, seq, hash, raw, applied)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
        params![
            lib.id.to_string(),
            p.version.key.as_str(),
            p.version.device.to_string(),
            p.seq as i64,
            p.version.hash.to_string(),
            p.version.raw.to_string()
        ],
    )?;
    Ok(n == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    const PLACE: &str = "place:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";

    fn setup() -> (Store, Library) {
        let store = Store::open_in_memory().unwrap();
        let lib = Library {
            id: Id16([7; 16]),
            device: Id16([1; 16]),
        };
        join(store.conn(), &lib, "icloud-drive", 0).unwrap();
        (store, lib)
    }

    fn key(s: &str) -> Key {
        Key::parse(s).unwrap().0
    }

    fn place(n: u8) -> LocalContent {
        LocalContent::Body(format!(r#"{{"cfi":"epubcfi(/6/{n})","fraction":0.5}}"#).into_bytes())
    }

    #[test]
    fn nothing_is_journalled_before_a_library_is_joined() {
        let store = Store::open_in_memory().unwrap();
        assert!(active(store.conn()).unwrap().is_none());
        let lib = Library {
            id: Id16([7; 16]),
            device: Id16([1; 16]),
        };
        assert!(matches!(
            record_local(store.conn(), &lib, &key(PLACE), place(1), 1000),
            Err(JournalError::NoLibrary(_))
        ));
    }

    #[test]
    fn a_later_local_change_replaces_an_unsent_one() {
        let (store, lib) = setup();
        let c = store.conn();
        record_local(c, &lib, &key(PLACE), place(1), 1000).unwrap();
        let second = record_local(c, &lib, &key(PLACE), place(2), 1000).unwrap();
        let unsent = unsent(c, &lib).unwrap();
        assert_eq!(unsent.len(), 1, "only the latest goes out");
        assert_eq!(unsent[0].hash, second.hash);
        assert!(
            unsent[0].clock > crate::sync::clock::Hlc(1000, 0),
            "the clock rose"
        );
        let crate::sync::record::Content::Body { hash, .. } = unsent[0].content else {
            panic!()
        };
        assert!(
            body(c, &hash).unwrap().is_some(),
            "the body is kept by its hash"
        );
    }

    #[test]
    fn a_local_version_descends_from_what_it_replaced_and_its_own_last() {
        let (store, lib) = setup();
        let c = store.conn();
        let k = key(PLACE);
        // Publish one of our own, then receive another device's that descends from it.
        let ours = record_local(c, &lib, &k, place(1), 1000).unwrap();
        assert_eq!(
            take_unsent(c, &lib, 0)
                .unwrap()
                .map(|(s, v)| (s, v[0].hash)),
            Some((1, ours.hash))
        );
        let other = Id16([2; 16]);
        let theirs = json!({
            "format": 1, "key": PLACE, "device": other.to_string(), "clock": [2000, 0],
            "history": [{ "d": lib.device.to_string(), "s": 1, "h": ours.hash.to_string() }],
            "body": { "hash": Hash32([5; 32]).to_string(), "size": 3 },
        });
        let Checked::Version(theirs) = check_version(theirs, other).unwrap() else {
            panic!()
        };
        let placed = Placed {
            version: theirs.clone(),
            seq: 4,
        };
        assert!(store_remote(c, &lib, &placed).unwrap());
        assert!(
            !store_remote(c, &lib, &placed).unwrap(),
            "a repeat changes nothing"
        );
        // Not yet applied: a local edit can't descend from it.
        let early = record_local(c, &lib, &k, place(2), 2500).unwrap();
        assert!(
            early.history.iter().all(|e| e.device != other),
            "not seen, not descended from"
        );
        c.execute(
            "UPDATE sync_versions SET applied = 1 WHERE device = ?1",
            [other.to_string()],
        )
        .unwrap();
        // Our next edit names both: theirs, and our own previous version (SP11).
        let next = record_local(c, &lib, &k, place(3), 3000).unwrap();
        let mut h: Vec<(DeviceId, u64)> = next.history.iter().map(|e| (e.device, e.seq)).collect();
        h.sort();
        assert_eq!(h, vec![(lib.device, 1), (other, 4)]);
        assert!(next.clock > theirs.clock, "after what it saw");
        // Even when theirs was ahead of this system clock (under SY10's 24 hours).
        let ahead = json!({
            "format": 1, "key": PLACE, "device": other.to_string(), "clock": [10_000_000u64, 0],
            "history": [{ "d": lib.device.to_string(), "s": 1, "h": ours.hash.to_string() }],
            "body": { "hash": Hash32([6; 32]).to_string(), "size": 3 },
        });
        let Checked::Version(ahead) = check_version(ahead, other).unwrap() else {
            panic!()
        };
        store_remote(
            c,
            &lib,
            &Placed {
                version: ahead.clone(),
                seq: 5,
            },
        )
        .unwrap();
        c.execute("UPDATE sync_versions SET applied = 1", [])
            .unwrap();
        let later = record_local(c, &lib, &k, place(4), 3500).unwrap();
        assert!(
            later.clock > ahead.clock,
            "a local edit sorts after what it saw"
        );
    }

    #[test]
    fn an_edit_made_after_numbering_goes_in_the_next_change() {
        let (store, lib) = setup();
        let c = store.conn();
        let k = key(PLACE);
        let first = record_local(c, &lib, &k, place(1), 1000).unwrap();
        let (seq, taken) = take_unsent(c, &lib, 0).unwrap().unwrap();
        assert_eq!((seq, taken[0].hash), (1, first.hash));
        // The reader edits again: it waits for the next change and names the first (SP11).
        let newer = record_local(c, &lib, &k, place(2), 1100).unwrap();
        assert!(newer
            .history
            .iter()
            .any(|e| e.device == lib.device && e.seq == 1 && e.hash == first.hash));
        // The device store says change 2 held only a hold: the next is 3, not 2.
        let (seq, taken) = take_unsent(c, &lib, 2).unwrap().unwrap();
        assert_eq!((seq, taken.len(), taken[0].hash), (3, 1, newer.hash));
        assert!(take_unsent(c, &lib, 3).unwrap().is_none());
        // A crash after numbering change 3, before it was signed: the same change again.
        let (seq, again) = take_unsent(c, &lib, 2).unwrap().unwrap();
        assert_eq!((seq, again[0].hash), (3, newer.hash));
        // A store behind the library is refused, never reused (SP15).
        record_local(c, &lib, &k, place(3), 1200).unwrap();
        assert!(matches!(
            take_unsent(c, &lib, 1),
            Err(JournalError::StoreBehind(1))
        ));
    }

    #[test]
    fn a_start_keeps_the_state_and_a_rotation_reauthors_unsent_versions() {
        let (store, lib) = setup();
        let c = store.conn();
        c.execute("UPDATE sync_libraries SET state = 'paused'", [])
            .unwrap();
        set_device(c, &lib, false, None, 0).unwrap();
        assert!(
            active(c).unwrap().is_none(),
            "a paused library stays paused across a start"
        );
        c.execute("UPDATE sync_libraries SET state = 'on'", [])
            .unwrap();
        // A good remote version ahead of the system clock (under SY10's 24 hours).
        let other = Id16([2; 16]);
        let good = json!({
            "format": 1, "key": PLACE, "device": other.to_string(), "clock": [5_000_000u64, 0],
            "history": [], "body": { "hash": Hash32([5; 32]).to_string(), "size": 3 },
        });
        let Checked::Version(good) = check_version(good, other).unwrap() else {
            panic!()
        };
        store_remote(
            c,
            &lib,
            &Placed {
                version: good.clone(),
                seq: 1,
            },
        )
        .unwrap();
        c.execute("UPDATE sync_versions SET applied = 1", [])
            .unwrap();
        // An edit far in the future, then a rotation for the clock (SY10).
        let ahead = record_local(c, &lib, &key(PLACE), place(1), 9_000_000_000_000).unwrap();
        let rotated = Library {
            id: lib.id,
            device: Id16([9; 16]),
        };
        set_device(c, &rotated, true, Some(0), 1_000).unwrap();
        let left = unsent(c, &rotated).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(
            left[0].device, rotated.device,
            "published under the new identity (SY7)"
        );
        assert!(left[0].clock < ahead.clock, "with a fresh clock");
        assert!(left[0].clock > good.clock, "still after its good ancestor");
        assert_eq!(active(c).unwrap().map(|l| l.device), Some(rotated.device));
    }

    #[test]
    fn an_edit_after_equal_contents_descends_from_both() {
        // Two devices wrote the same note apart: settled, and an edit after it names both.
        let (store, lib) = setup();
        let c = store.conn();
        let note = "note:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
        for (d, ms) in [(2u8, 100u64), (3, 200)] {
            let v = json!({
                "format": 1, "key": note, "device": Id16([d; 16]).to_string(),
                "clock": [ms, 0], "history": [],
                "body": { "hash": Hash32([1; 32]).to_string(), "size": 3 },
            });
            let Checked::Version(v) = check_version(v, Id16([d; 16])).unwrap() else {
                panic!()
            };
            store_remote(c, &lib, &Placed { version: v, seq: 1 }).unwrap();
        }
        c.execute("UPDATE sync_versions SET applied = 1", [])
            .unwrap();
        let edit = record_local(
            c,
            &lib,
            &key(note),
            LocalContent::Body(br#"{"text":"mine"}"#.to_vec()),
            300,
        )
        .unwrap();
        let mut named: Vec<DeviceId> = edit.history.iter().map(|e| e.device).collect();
        named.sort();
        assert_eq!(named, vec![Id16([2; 16]), Id16([3; 16])]);
    }

    #[test]
    fn an_edit_cant_settle_a_conflict_holding_this_devices_hidden_version() {
        let (store, lib) = setup();
        let c = store.conn();
        let note = "note:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
        let k = key(note);
        let ours = record_local(
            c,
            &lib,
            &k,
            LocalContent::Body(br#"{"text":"ours"}"#.to_vec()),
            100,
        )
        .unwrap();
        take_unsent(c, &lib, 0).unwrap();
        let other = Id16([2; 16]);
        let v = json!({
            "format": 1, "key": note, "device": other.to_string(), "clock": [200u64, 0],
            "history": [], "body": { "hash": Hash32([2; 32]).to_string(), "size": 3 },
        });
        let Checked::Version(v) = check_version(v, other).unwrap() else {
            panic!()
        };
        store_remote(c, &lib, &Placed { version: v, seq: 1 }).unwrap();
        c.execute("UPDATE sync_versions SET applied = 1", [])
            .unwrap();
        // Theirs is shown (changed last); ours is hidden: an edit must wait (SY13).
        let r = record_local(
            c,
            &lib,
            &k,
            LocalContent::Body(br#"{"text":"edit"}"#.to_vec()),
            300,
        );
        assert!(matches!(r, Err(JournalError::Conflicted(_))));
        let _ = ours;
    }

    #[test]
    fn a_change_numbered_but_never_signed_is_reauthored_on_rotation() {
        let (store, lib) = setup();
        let c = store.conn();
        record_local(c, &lib, &key(PLACE), place(1), 1000).unwrap();
        take_unsent(c, &lib, 0).unwrap(); // numbered 1, then a crash before signing
        let note = "note:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
        record_local(
            c,
            &lib,
            &key(note),
            LocalContent::Body(br#"{"text":"n"}"#.to_vec()),
            1100,
        )
        .unwrap();
        take_unsent(c, &lib, 1).unwrap(); // numbered 2, never signed either
        record_local(
            c,
            &lib,
            &key(note),
            LocalContent::Body(br#"{"text":"later"}"#.to_vec()),
            1200,
        )
        .unwrap();
        let rotated = Library {
            id: lib.id,
            device: Id16([9; 16]),
        };
        // The old identity's device store had signed only change 0: nothing.
        set_device(c, &rotated, false, Some(0), 1300).unwrap();
        let left = unsent(c, &rotated).unwrap();
        let keys: Vec<&str> = left.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(
            keys.len(),
            2,
            "the place comes back, the replaced note doesn't: {keys:?}"
        );
        assert!(left.iter().all(|v| v.device == rotated.device));
        assert!(
            left.iter()
                .all(|v| v.history.iter().all(|e| e.device != lib.device)),
            "no unsigned ancestor"
        );
    }

    #[test]
    fn a_pruned_ancestor_falls_back_to_the_last_signed_one() {
        let (store, lib) = setup();
        let c = store.conn();
        let note = "note:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
        let k = key(note);
        let v1 = record_local(
            c,
            &lib,
            &k,
            LocalContent::Body(br#"{"text":"1"}"#.to_vec()),
            1000,
        )
        .unwrap();
        take_unsent(c, &lib, 0).unwrap(); // signed: 1
        record_local(
            c,
            &lib,
            &k,
            LocalContent::Body(br#"{"text":"2"}"#.to_vec()),
            1100,
        )
        .unwrap();
        take_unsent(c, &lib, 1).unwrap(); // numbered 2, never signed
        record_local(
            c,
            &lib,
            &k,
            LocalContent::Body(br#"{"text":"3"}"#.to_vec()),
            1200,
        )
        .unwrap();
        let rotated = Library {
            id: lib.id,
            device: Id16([9; 16]),
        };
        set_device(c, &rotated, false, Some(1), 1300).unwrap();
        let left = unsent(c, &rotated).unwrap();
        assert_eq!(left.len(), 1);
        let old: Vec<(u64, Hash32)> = left[0]
            .history
            .iter()
            .filter(|e| e.device == lib.device)
            .map(|e| (e.seq, e.hash))
            .collect();
        assert_eq!(
            old,
            vec![(1, v1.hash)],
            "descends from the signed version 1"
        );
    }

    #[test]
    fn editing_the_shown_note_keeps_the_conflict() {
        // SY13: an edit descends from the shown version only, so the other stays.
        let (store, lib) = setup();
        let c = store.conn();
        let note = "note:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
        let mk = |d: u8, ms: u64, b: u8| {
            let v = json!({
                "format": 1, "key": note, "device": Id16([d; 16]).to_string(),
                "clock": [ms, 0], "history": [],
                "body": { "hash": Hash32([b; 32]).to_string(), "size": 3 },
            });
            let Checked::Version(v) = check_version(v, Id16([d; 16])).unwrap() else {
                panic!()
            };
            Placed { version: v, seq: 1 }
        };
        store_remote(c, &lib, &mk(2, 100, 1)).unwrap();
        store_remote(c, &lib, &mk(3, 200, 2)).unwrap();
        c.execute("UPDATE sync_versions SET applied = 1", [])
            .unwrap();
        let edit = record_local(
            c,
            &lib,
            &key(note),
            LocalContent::Body(br#"{"text":"mine"}"#.to_vec()),
            300,
        )
        .unwrap();
        let named: Vec<DeviceId> = edit.history.iter().map(|e| e.device).collect();
        assert_eq!(
            named,
            vec![Id16([3; 16])],
            "descends from the shown one only"
        );
    }

    #[test]
    fn joining_another_library_makes_it_the_only_active_one() {
        let (store, lib) = setup();
        let c = store.conn();
        assert!(matches!(
            join(c, &lib, "google-drive", 5),
            Err(JournalError::OtherStorage(_))
        ));
        let other = Library {
            id: Id16([8; 16]),
            device: Id16([3; 16]),
        };
        join(c, &other, "google-drive", 10).unwrap();
        assert_eq!(
            active(c).unwrap().map(|l| (l.id, l.device)),
            Some((other.id, other.device))
        );
        join(c, &lib, "icloud-drive", 20).unwrap();
        assert_eq!(active(c).unwrap().map(|l| l.id), Some(lib.id));
    }
}
