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

/// SY29: join a library in a storage, making it the active one. Nothing else is
/// written to the sync tables before this.
pub fn join(conn: &Connection, library: Id16, storage: &str, now_ms: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE sync_libraries SET active = 0 WHERE active = 1", [])?;
    tx.execute(
        "INSERT INTO sync_libraries (library, storage, state, active, joined_at)
         VALUES (?1, ?2, 'on', 1, ?3)
         ON CONFLICT(library) DO UPDATE SET active = 1, state = 'on'",
        params![library.to_string(), storage, now_ms],
    )?;
    tx.commit()?;
    Ok(())
}

/// The library sync is on for, if any (item 109: one at a time).
pub fn active(conn: &Connection) -> Result<Option<Id16>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT library FROM sync_libraries WHERE active = 1 AND state = 'on'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id.and_then(|s| Id16::parse(&s)))
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
/// and those received from others. Unsent local versions aren't in it.
pub fn record_of(conn: &Connection, lib: &Library, key: &Key, which: Which) -> Result<Record> {
    let mut stmt = conn.prepare(
        "SELECT device, seq, raw FROM sync_versions
         WHERE library = ?1 AND key = ?2 AND seq IS NOT NULL AND (?3 = 0 OR applied = 1)",
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
    let base = match record.outcome(&HashSet::new()) {
        Some(Outcome::Settled(p)) => Some(p),
        Some(Outcome::Conflict { shown, .. }) => Some(shown),
        None => None,
    };
    if let Some(p) = base {
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

/// SY7: journal a local change to `key`, replacing an earlier unsent one. Call it in
/// the same transaction as the change itself, so the two can't part.
pub fn record_local(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    content: LocalContent,
    wall_ms: u64,
) -> Result<Version> {
    let clock = tick(conn, lib, wall_ms)?;
    // Only what was applied: a version still waiting (for its body, a book or a
    // guard) was never seen by the reader, so nothing may claim to descend from it.
    let record = record_of(conn, lib, key, Which::Applied)?;
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

/// The local versions waiting to be published (SY7), oldest key first.
pub fn unsent(conn: &Connection, lib: &Library) -> Result<Vec<Version>> {
    let mut stmt = conn.prepare(
        "SELECT raw FROM sync_versions WHERE library = ?1 AND seq IS NULL ORDER BY rowid",
    )?;
    let rows = stmt.query_map([lib.id.to_string()], |r| r.get::<_, String>(0))?;
    rows.map(|raw| check(&raw?, lib.device)).collect()
}

/// SP15: once the change file numbered `seq` holding these versions is signed and
/// kept, they take that sequence. Each is named by its hash, so an edit made since
/// (which replaced the unsent row) stays unsent and goes in a later change.
pub fn mark_published(
    conn: &Connection,
    lib: &Library,
    versions: &[(Key, Hash32)],
    seq: u64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (key, hash) in versions {
        tx.execute(
            "UPDATE sync_versions SET seq = ?3
             WHERE library = ?1 AND key = ?2 AND hash = ?4 AND seq IS NULL",
            params![
                lib.id.to_string(),
                key.as_str(),
                seq as i64,
                hash.to_string()
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
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
        join(store.conn(), lib.id, "icloud-drive", 0).unwrap();
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
        assert_eq!(active(store.conn()).unwrap(), None);
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
        mark_published(c, &lib, &[(k.clone(), ours.hash)], 1).unwrap();
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
    }

    #[test]
    fn an_edit_made_while_publishing_stays_unsent() {
        let (store, lib) = setup();
        let c = store.conn();
        let k = key(PLACE);
        let signed = record_local(c, &lib, &k, place(1), 1000).unwrap();
        // The reader edits again after the change file was signed, before it is marked.
        let newer = record_local(c, &lib, &k, place(2), 1100).unwrap();
        mark_published(c, &lib, &[(k.clone(), signed.hash)], 1).unwrap();
        let left = unsent(c, &lib).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].hash, newer.hash, "the newer edit still goes out");
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
        join(c, Id16([8; 16]), "google-drive", 10).unwrap();
        assert_eq!(active(c).unwrap(), Some(Id16([8; 16])));
        join(c, lib.id, "icloud-drive", 20).unwrap();
        assert_eq!(active(c).unwrap(), Some(lib.id));
    }
}
