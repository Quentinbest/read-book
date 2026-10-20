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
/// `displaced` is the body of this device's unsent version the tombstone replaced,
/// which its history can't name (it was never numbered).
pub(crate) fn seen(
    conn: &Connection,
    lib: &Library,
    v: &Version,
    seq: Option<u64>,
    displaced: Option<(String, Vec<u8>)>,
    now_ms: i64,
) -> Result<()> {
    if !matches!(v.content, Content::Deleted { .. }) {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO sync_deleted
             (library, key, device, hash, seq, first_seen, displaced_raw, displaced)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            lib.id.to_string(),
            v.key.as_str(),
            v.device.to_string(),
            v.hash.to_string(),
            seq.map(|s| s as i64),
            now_ms,
            displaced.as_ref().map(|(raw, _)| raw.clone()),
            displaced.map(|(_, body)| body)
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

/// The version the tombstone displaced: the newest, in SY10's order, of those it
/// descends from (its history for this key, and this device's unnumbered one it
/// replaced) that still has a body here.
fn displaced(conn: &Connection, lib: &Library, tomb: &Placed) -> Result<Option<Vec<u8>>> {
    // This device's unnumbered version the tombstone replaced, kept with the
    // deletion, competes with the rest in SY10's order (clock, then device).
    let kept: Option<(Option<String>, Option<Vec<u8>>)> = conn
        .query_row(
            "SELECT displaced_raw, displaced FROM sync_deleted
             WHERE library = ?1 AND key = ?2 AND device = ?3 AND hash = ?4",
            params![
                lib.id.to_string(),
                tomb.version.key.as_str(),
                tomb.version.device.to_string(),
                tomb.version.hash.to_string()
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let mut best: Option<(super::clock::Stamp, Vec<u8>)> = None;
    if let Some((Some(raw), Some(body))) = kept {
        if let Some(Ok(super::record::Checked::Version(v))) =
            serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .map(|v| super::record::check_version(v, lib.device))
        {
            best = Some((
                Placed {
                    version: v,
                    seq: u64::MAX,
                }
                .stamp(),
                body,
            ));
        }
    }
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
        let stamp = Placed {
            version: v,
            seq: e.seq,
        }
        .stamp();
        if best.as_ref().map_or(true, |(c, _)| stamp > *c) {
            best = Some((stamp, body));
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
    // A held tombstone (this device's, hidden in a conflict) is replaced the same way.
    let wall = now_ms.max(0) as u64;
    let as_choice = |body: Vec<u8>| -> Result<Version> {
        let v = journal::hold_local(conn, lib, key, LocalContent::Body(body), wall)?;
        journal::release_held(conn, lib, key)?;
        Ok(v)
    };
    let v = if journal::held(conn, lib, key)? {
        as_choice(body)?
    } else {
        match journal::record_local(conn, lib, key, LocalContent::Body(body.clone()), wall) {
            Err(JournalError::Conflicted(_)) => as_choice(body)?,
            other => other?,
        }
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
    use rusqlite::params;

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
    fn an_edit_deleted_before_it_was_sent_is_restorable() {
        let (_a, _la, mut b, lb) = shared();
        // B edits the note (unsent), then deletes the highlight.
        b.save_annotation(&highlight("book-b", Some("B's edit"), 1_760_000_002_000))
            .unwrap();
        b.delete_annotation(ANNO).unwrap();
        let now = crate::store::now_ms();
        assert!(restore(b.conn(), &lb, &key("annotation"), now + DAY).unwrap());
        assert_eq!(note_of(&b, "book-b"), Some(Some("B's edit".into())));
    }

    #[test]
    fn a_deletion_reauthored_after_a_rotation_stays_restorable() {
        let (_a, _la, mut b, lb) = shared();
        b.delete_annotation(ANNO).unwrap();
        // B takes a new identity before sending: its unsent versions are re-authored.
        let rotated = Library {
            id: lb.id,
            device: crate::sync::ids::Id16([0x2b; 16]),
        };
        journal::set_device(
            b.conn(),
            &rotated,
            true,
            None,
            crate::store::now_ms() as u64,
        )
        .unwrap();
        let now = crate::store::now_ms();
        assert!(recently_deleted(b.conn(), &rotated, now + DAY)
            .unwrap()
            .iter()
            .any(|d| d.key == key("annotation")));
        assert!(restore(b.conn(), &rotated, &key("annotation"), now + DAY).unwrap());
    }

    #[test]
    fn restoring_a_held_deletion_descends_from_every_head() {
        let (mut a, la, mut b, lb) = shared();
        let now = crate::store::now_ms();
        // B deletes (unsent); apart, A edits: B's deletion is shown, A's waits in the
        // conflict. B restores: the restored version names A's too.
        b.delete_annotation(ANNO).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut h = highlight("book-a", Some("A's"), 1_760_000_002_000);
        h.color = "blue".into();
        a.save_annotation(&h).unwrap();
        let edit = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &edit, now).unwrap();
        // Hold B's tombstone, as if the reader's choice were pending.
        b.conn()
            .execute(
                "UPDATE sync_versions SET applied = -3 WHERE seq IS NULL AND key = ?1",
                [key("annotation").as_str()],
            )
            .unwrap();
        assert!(restore(b.conn(), &lb, &key("annotation"), now + DAY).unwrap());
        let raw: String = b
            .conn()
            .query_row(
                "SELECT raw FROM sync_versions WHERE seq IS NULL AND key = ?1",
                [key("annotation").as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        // The entry for A names A's edit, the head that waits in the conflict.
        let a_entry = v["history"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["d"] == la.device.to_string())
            .cloned();
        assert_eq!(a_entry.and_then(|e| e["s"].as_u64()), Some(edit.seq), "{v}");
    }

    #[test]
    fn restore_brings_back_the_newest_of_tied_versions_by_device() {
        let (_a, _la, b, lb) = shared();
        let k = key("note");
        // Two versions with one clock, from devices 3 and 4, and a tombstone naming
        // both: SY10 orders 4's after 3's.
        let mut history = Vec::new();
        for (d, text) in [(3u8, "from 3"), (4u8, "from 4")] {
            let body = format!(r#"{{"text":"{text}"}}"#).into_bytes();
            let hash = journal::put_body(b.conn(), &body).unwrap();
            let v = serde_json::json!({
                "format": 1, "key": k.as_str(), "device": crate::sync::ids::Id16([d; 16]).to_string(),
                "clock": [1_760_000_009_000u64, 0], "history": [],
                "body": { "hash": hash.to_string(), "size": body.len() },
            });
            let vh = crate::sync::record::version_hash(&v);
            b.conn()
                .execute(
                    "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied)
                     VALUES (?1, ?2, ?3, 1, ?4, ?5, 1)",
                    params![
                        lb.id.to_string(),
                        k.as_str(),
                        crate::sync::ids::Id16([d; 16]).to_string(),
                        vh.to_string(),
                        v.to_string()
                    ],
                )
                .unwrap();
            history.push(serde_json::json!({ "d": crate::sync::ids::Id16([d; 16]).to_string(), "s": 1, "h": vh.to_string() }));
        }
        let tomb = serde_json::json!({
            "format": 1, "key": k.as_str(), "device": crate::sync::ids::Id16([5; 16]).to_string(),
            "clock": [1_760_000_010_000u64, 0], "history": history, "deleted": { "covers": [] },
        });
        let crate::sync::record::Checked::Version(tv) =
            crate::sync::record::check_version(tomb, crate::sync::ids::Id16([5; 16])).unwrap()
        else {
            unreachable!()
        };
        let body = displaced(
            b.conn(),
            &lb,
            &Placed {
                version: tv,
                seq: 1,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(body, br#"{"text":"from 4"}"#.to_vec());
    }

    #[test]
    fn a_held_older_edit_doesnt_win_over_the_newer_one_it_waited_on() {
        let (mut a, la, mut b, lb) = shared();
        let now = crate::store::now_ms();
        // B's unsent edit, then A's later one: B's is held. B deletes: the newest
        // the deletion displaced is A's.
        b.save_annotation(&highlight("book-b", Some("B's older"), 1_760_000_002_000))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.save_annotation(&highlight("book-a", Some("A's newer"), 1_760_000_003_000))
            .unwrap();
        let edit = publish_next(a.conn(), &la).unwrap().unwrap();
        receive(b.conn(), &lb, &edit, now).unwrap();
        b.delete_annotation(ANNO).unwrap();
        assert!(restore(b.conn(), &lb, &key("annotation"), now + DAY).unwrap());
        assert_eq!(note_of(&b, "book-b"), Some(Some("A's newer".into())));
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
