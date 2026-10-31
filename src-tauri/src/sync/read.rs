//! SP18: read an enrolled peer from its authenticated view and subsequent files.

use super::apply::{self, Batch, Received};
use super::chain::{self, ChainError};
use super::device_store::Floor;
use super::ids::{DeviceId, Hash32};
use super::journal::{JournalError, Library};
use super::record::Entry;
use super::view::View;
use rusqlite::{params, Connection};

/// SP35: the authenticated view's marks are carried for the caller. They are
/// not consumed here; the receive outcome tells the caller whether it landed.
#[derive(Debug, PartialEq, Eq)]
pub struct ViewReceived {
    /// When needed, the view's outcome comes first; its suffix follows only
    /// after acceptance. When already held, only the suffix outcomes are returned.
    pub received: Vec<Received>,
    pub marks: Vec<Entry>,
}

/// SP18: receive a view opened with view::open for this library/device/key.
/// The caller supplies whether the peer is removed; a removed device's view is
/// refused, since SP18 reads only its change files after the cutoff.
/// The view's open and week claims are not consumed here (#176).
/// After acceptance, persist its generation with device_file::raise_view_generation.
/// This function neither fetches files nor deletes anything from storage.
#[allow(clippy::too_many_arguments)]
pub fn receive_view(
    conn: &Connection,
    lib: &Library,
    view: &View,
    device: &DeviceId,
    key: &[u8; 32],
    floor: &Floor,
    removed: bool,
    bodies: Vec<Vec<u8>>,
    files: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
    now_ms: i64,
) -> Result<ViewReceived, ChainError> {
    if removed {
        return Err(ChainError::Removed);
    }
    if view.gen < floor.gen {
        return Err(ChainError::StaleView);
    }
    let checkpoints = floor.checkpoints()?;
    let marks = view
        .shards
        .iter()
        .flat_map(|s| s.marks.iter().copied())
        .collect();
    // SP18: every held sequence, not merely its head, must agree.
    let held: Vec<(i64, String)> = conn
        .prepare(
            "SELECT seq, hash FROM sync_changes
             WHERE library = ?1 AND device = ?2 AND seq <= ?3",
        )
        .map_err(JournalError::from)?
        .query_map(
            params![
                lib.id.to_string(),
                device.to_string(),
                view.through.s as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(JournalError::from)?
        .collect::<Result<_, _>>()
        .map_err(JournalError::from)?;
    let spine_hash = |s: u64| {
        s.checked_sub(1)
            .and_then(|i| view.spine.get(i as usize))
            .filter(|e| e.s == s)
            .map(|e| e.h)
    };
    for (s, hash) in held {
        if spine_hash(s as u64) != Hash32::parse(&hash) {
            return Err(ChainError::Link);
        }
    }
    for &(s, h) in checkpoints.iter().filter(|(s, _)| *s <= view.through.s) {
        if spine_hash(s) != Some(h) {
            return Err(ChainError::Link);
        }
    }
    let have = apply::seen(conn, lib, *device)?;
    if have >= view.through.s {
        return Ok(ViewReceived {
            received: chain::receive_run(conn, lib, device, key, &checkpoints, files, now_ms)?,
            marks,
        });
    }
    // SP18: before applying anything, prove the lowest floor checkpoint beyond
    // what is held, using the spine followed by the linked suffix.
    if let Some(lowest) = checkpoints
        .iter()
        .map(|(s, _)| *s)
        .filter(|s| *s > have)
        .min()
    {
        let mut seq = view.through.s;
        let mut hash = view.through.h;
        let mut reached = seq >= lowest;
        for (bytes, _) in &files {
            let c = chain::open_change(bytes, &lib.id, device, key)?;
            if c.seq != seq + 1 {
                break;
            }
            if c.prev != Some(hash) || checkpoints.iter().any(|(s, h)| *s == c.seq && *h != c.hash)
            {
                return Err(ChainError::Link);
            }
            seq = c.seq;
            hash = c.hash;
            reached |= seq == lowest;
        }
        if !reached {
            return Ok(ViewReceived {
                received: (0..=files.len()).map(|_| Received::Waits).collect(),
                marks,
            });
        }
    }
    let batch = Batch {
        device: *device,
        seq: view.through.s,
        hash: view.through.h,
        versions: view
            .shards
            .iter()
            .flat_map(|s| s.records.iter().map(|r| r.version.clone()))
            .collect(),
        bodies,
    };
    let got = apply::receive_view(conn, lib, &batch, now_ms, view)?;
    let accepted = matches!(got, Received::Applied { .. });
    let mut received = vec![got];
    if accepted {
        received.extend(chain::receive_run(
            conn,
            lib,
            device,
            key,
            &checkpoints,
            files,
            now_ms,
        )?);
    }
    Ok(ViewReceived { received, marks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::apply::tests::{highlight, note_of, ANNO};
    use crate::sync::compact::tests::Device;
    use crate::sync::device_store::{Checkpoint, DeviceStore, Keystore, StoreError};
    use crate::sync::guard;
    use crate::sync::ids::identity_hash;
    use crate::sync::journal::{self, Which};
    use crate::sync::merge::Outcome;
    use crate::sync::record::{self, Key};
    use crate::sync::{device_file, restore};
    use serde_json::{json, Value};
    use std::collections::HashSet;

    fn now() -> i64 {
        crate::store::now_ms()
    }

    fn key(kind: &str) -> Key {
        Key::parse(&format!("{kind}:{ANNO}")).unwrap().0
    }

    fn edit(a: &mut Device, text: &str, n: i64) -> chain::Change {
        a.db.save_annotation(&highlight("book", Some(text), 1_760_000_001_000 + n))
            .unwrap();
        a.change()
    }

    fn file(a: &Device, c: &chain::Change) -> (Vec<u8>, Vec<Vec<u8>>) {
        let signed = chain::sign_change(&a.key(), &a.lib.id, &c.writer, c.seq, c.prev, &c.versions);
        assert_eq!(signed.hash, c.hash);
        let bodies = c
            .versions
            .iter()
            .filter_map(|v| v["body"]["hash"].as_str())
            .map(|h| {
                journal::body(a.db.conn(), &Hash32::parse(h).unwrap())
                    .unwrap()
                    .unwrap()
            })
            .collect();
        (signed.bytes, bodies)
    }

    fn send(c: &Device, a: &Device, change: &chain::Change) -> Received {
        let (bytes, bodies) = file(a, change);
        chain::receive_change(
            c.db.conn(),
            &c.lib,
            &bytes,
            &a.lib.device,
            &a.key().public(),
            &[],
            bodies,
            now(),
        )
        .unwrap()
    }

    fn view(a: &mut Device, changes: &[chain::Change]) -> View {
        let built = a.build(None, changes, &|_| false);
        let written = a.save(&built);
        a.open(&written)
    }

    fn take(
        c: &Device,
        a: &Device,
        v: &View,
        floor: &Floor,
        later: &[chain::Change],
    ) -> Result<ViewReceived, ChainError> {
        let bodies = v
            .shards
            .iter()
            .flat_map(|s| &s.records)
            .filter_map(|r| r.version["body"]["hash"].as_str())
            .map(|h| {
                journal::body(a.db.conn(), &Hash32::parse(h).unwrap())
                    .unwrap()
                    .unwrap()
            })
            .collect();
        receive_view(
            c.db.conn(),
            &c.lib,
            v,
            &a.lib.device,
            &a.key().public(),
            floor,
            false,
            bodies,
            later.iter().map(|ch| file(a, ch)).collect(),
            now(),
        )
    }

    fn held(c: &Device, a: &Device) -> u64 {
        apply::seen(c.db.conn(), &c.lib, a.lib.device).unwrap()
    }

    fn count(c: &Device, table: &str) -> i64 {
        c.db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_device_behind_resumes_from_the_view_and_later_changes() {
        let mut a = Device::new();
        let mut changes = Vec::new();
        for n in 1..=6 {
            changes.push(edit(&mut a, &format!("note {n}"), n));
            // An independent key written only before compaction must survive too.
            if n == 1 {
                a.note("note:00000000-0000-4000-8000-000000000099", "unchanged");
            }
        }
        let v = view(&mut a, &changes[..4]);
        for prior in [0, 2, 4, 6] {
            let c = Device::new();
            for change in &changes[..prior] {
                send(&c, &a, change);
            }
            let result = take(&c, &a, &v, &Floor::default(), &changes[4..]).unwrap();
            assert!(result
                .received
                .iter()
                .all(|r| matches!(r, Received::Applied { .. } | Received::Repeat)));
            assert_eq!(held(&c, &a), 6);
            assert_eq!(note_of(&c.db, "book"), Some(Some("note 6".into())));
            let latest: std::collections::BTreeMap<_, _> = changes
                .iter()
                .flat_map(|c| &c.versions)
                .map(|v| (v["key"].as_str().unwrap(), record::version_hash(v)))
                .collect();
            for (key, expected) in latest {
                let actual: String = c.db.conn().query_row(
                    "SELECT hash FROM sync_versions WHERE library = ?1 AND device = ?2 AND key = ?3
                     ORDER BY seq DESC LIMIT 1",
                    params![c.lib.id.to_string(), a.lib.device.to_string(), key],
                    |r| r.get(0),
                ).unwrap();
                assert_eq!(actual, expected.to_string(), "{key}");
            }
            // Compacted change 3 is still usable as a frontier checkpoint.
            assert_eq!(
                chain::held_hash(c.db.conn(), &c.lib, &a.lib.device, 3).unwrap(),
                Some(changes[2].hash)
            );
        }
    }

    fn branch(c: &Device, b: &Device, ancestor: &Value, seq: u64, hash: Hash32) -> Received {
        let body = br#"{"text":"branch"}"#.to_vec();
        let raw = json!({
            "format": 1, "key": ancestor["key"], "device": b.lib.device.to_string(),
            "clock": [1_760_000_003_000u64, 0],
            "history": [{"d": ancestor["device"], "s": seq, "h": hash.to_string()}],
            "body": {"hash": identity_hash(&body).to_string(), "size": body.len()}
        });
        let signed = chain::sign_change(
            &b.key(),
            &b.lib.id,
            &b.store.library(&b.lib.id).unwrap().writer_id().unwrap(),
            1,
            None,
            &[raw],
        );
        chain::receive_change(
            c.db.conn(),
            &c.lib,
            &signed.bytes,
            &b.lib.device,
            &b.key().public(),
            &[],
            vec![body],
            now(),
        )
        .unwrap()
    }

    // SP11: sign a view whose note claims ancestry in another key. Its
    // signatures and spine are valid; receiving must check the history itself.
    fn view_claiming_other_key(a: &mut Device, b: &Device, hash: Hash32) -> (View, Value) {
        a.db.save_annotation(&highlight("book", Some("refused note"), 1_760_000_001_000))
            .unwrap();
        let mut versions: Vec<_> = journal::unsent(a.db.conn(), &a.lib)
            .unwrap()
            .into_iter()
            .map(|v| v.raw)
            .collect();
        let note = versions
            .iter_mut()
            .find(|v| v["key"] == key("note").as_str())
            .unwrap();
        note["history"] = json!([{"d": b.lib.device.to_string(), "s": 1, "h": hash.to_string()}]);
        let note = note.clone();
        let writer = a.store.library(&a.lib.id).unwrap().writer_id().unwrap();
        let signed = chain::sign_change(&a.key(), &a.lib.id, &writer, 1, None, &versions);
        let change =
            chain::open_change(&signed.bytes, &a.lib.id, &a.lib.device, &a.key().public()).unwrap();
        let records: Vec<_> = versions
            .into_iter()
            .map(|version| crate::sync::view::Record {
                key: Key::parse(version["key"].as_str().unwrap()).unwrap().0,
                version,
                week: vec![],
            })
            .collect();
        let written = crate::sync::view::write(
            &a.key(),
            &a.lib.id,
            &writer,
            1,
            None,
            crate::sync::trust::Mark {
                s: 1,
                h: change.hash,
            },
            &records,
            &[],
            &[crate::sync::view::SpineEntry::from_change(&change)],
            &crate::sync::view::Open::default(),
        )
        .unwrap();
        (a.open(&written), note)
    }

    fn assert_descendant_refused(c: &Device, refused: &Value) {
        let descendant = Device::new();
        assert!(matches!(
            branch(c, &descendant, refused, 1, record::version_hash(refused)),
            Received::Applied {
                applied: 0,
                set_aside: 1
            }
        ));
        let reason: String =
            c.db.conn()
                .query_row(
                    "SELECT reason FROM sync_aside WHERE library = ?1 AND device = ?2",
                    params![c.lib.id.to_string(), descendant.lib.device.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(reason, "false ancestry");
        assert_eq!(note_of(&c.db, "book"), Some(None));
    }

    #[test]
    fn a_refused_view_record_vouches_for_nothing() {
        let mut b = Device::new();
        let other = b.note("note:00000000-0000-4000-8000-000000000099", "other key");
        let change = b.change();
        let mut a = Device::new();
        let (view, refused) = view_claiming_other_key(&mut a, &b, other.hash);
        let c = Device::new();
        send(&c, &b, &change);
        assert!(matches!(
            take(&c, &a, &view, &Floor::default(), &[])
                .unwrap()
                .received[0],
            Received::Applied {
                applied: 1,
                set_aside: 1
            }
        ));
        assert_descendant_refused(&c, &refused);
    }

    #[test]
    fn a_waiting_view_record_later_refused_vouches_for_nothing() {
        let mut b = Device::new();
        let other = b.note("note:00000000-0000-4000-8000-000000000099", "other key");
        let change = b.change();
        let mut a = Device::new();
        let (view, refused) = view_claiming_other_key(&mut a, &b, other.hash);
        let c = Device::new();
        take(&c, &a, &view, &Floor::default(), &[]).unwrap();
        let state: i64 = c
            .db
            .conn()
            .query_row(
                "SELECT applied FROM sync_versions WHERE library = ?1 AND device = ?2 AND key = ?3",
                params![
                    c.lib.id.to_string(),
                    a.lib.device.to_string(),
                    key("note").as_str()
                ],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, -1);
        send(&c, &b, &change);
        let reason: String =
            c.db.conn()
                .query_row(
                    "SELECT reason FROM sync_aside WHERE library = ?1 AND device = ?2",
                    params![c.lib.id.to_string(), a.lib.device.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(reason, "false ancestry");
        let remaining: i64 = c.db.conn().query_row(
            "SELECT COUNT(*) FROM sync_versions WHERE library = ?1 AND device = ?2 AND key = ?3",
            params![c.lib.id.to_string(), a.lib.device.to_string(), key("note").as_str()],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(remaining, 0);
        assert_descendant_refused(&c, &refused);
    }

    fn is_conflict(c: &Device) -> bool {
        let r = journal::record_of(c.db.conn(), &c.lib, &key("note"), Which::All).unwrap();
        matches!(r.outcome(&HashSet::new()), Some(Outcome::Conflict { .. }))
    }

    #[test]
    fn compaction_then_a_late_branch() {
        let mut a = Device::new();
        let changes: Vec<_> = (1..=4)
            .map(|n| edit(&mut a, &format!("generation {n}"), n))
            .collect();
        let v = view(&mut a, &changes);
        for n in [2, 4] {
            let mut b = Device::new();
            for ch in &changes[..n] {
                send(&b, &a, ch);
            }
            let c = Device::new();
            take(&c, &a, &v, &Floor::default(), &[]).unwrap();
            b.db.save_annotation(&highlight("book", Some("B edited"), 1_760_000_002_000))
                .unwrap();
            let edit = b.change();
            assert!(matches!(
                send(&c, &b, &edit),
                Received::Applied { set_aside: 0, .. }
            ));
            assert_eq!(is_conflict(&c), n == 2);
            if n == 4 {
                assert_eq!(note_of(&c.db, "book"), Some(Some("B edited".into())));
            }
        }
    }

    #[test]
    fn a_branch_from_a_version_compacted_away_long_ago_is_verified_by_the_spine() {
        let mut a = Device::new();
        let changes: Vec<_> = (1..=3)
            .map(|n| edit(&mut a, &format!("generation {n}"), n))
            .collect();
        // Three published generations retain ancestry after its full records
        // have disappeared from the view.
        let mut previous = None;
        for ch in &changes {
            let built = a.build(previous.as_ref(), std::slice::from_ref(ch), &|_| false);
            let written = a.save(&built);
            previous = Some(a.open(&written));
            a.acknowledge(&written);
        }
        let v = previous.unwrap();
        assert_eq!(v.gen, 3);
        let old = changes[0]
            .versions
            .iter()
            .find(|v| v["key"] == key("note").as_str())
            .unwrap();
        assert!(!v
            .shards
            .iter()
            .flat_map(|s| &s.records)
            .any(|r| record::version_hash(&r.version) == record::version_hash(old)));
        for valid in [true, false] {
            let c = Device::new();
            let b = Device::new();
            take(&c, &a, &v, &Floor::default(), &[]).unwrap();
            let h = if valid {
                record::version_hash(old)
            } else {
                Hash32([9; 32])
            };
            let got = branch(&c, &b, old, 1, h);
            assert!(
                matches!(got, Received::Applied { set_aside, .. } if set_aside == usize::from(!valid))
            );
            assert_eq!(is_conflict(&c), valid);
            assert_eq!(count(&c, "sync_aside"), i64::from(!valid));
        }
        // SP11: a real hash under another key prefix is absent, not ancestry.
        let c = Device::new();
        let b = Device::new();
        take(&c, &a, &v, &Floor::default(), &[]).unwrap();
        let mut absent = old.clone();
        absent["key"] = json!("note:00000000-0000-4000-8000-000000000099");
        assert!(matches!(
            branch(&c, &b, &absent, 1, record::version_hash(old)),
            Received::Applied { set_aside: 1, .. }
        ));
    }

    /// The book-level Restore case waits for Stage 4.
    #[test]
    fn compaction_then_restore() {
        let mut a = Device::new();
        let c = Device::new();
        let first = edit(&mut a, "whole paired note", 1);
        send(&c, &a, &first);
        let before = c.db.annotations("book").unwrap();
        a.db.delete_annotation(ANNO).unwrap();
        let deleted = a.change();
        let v = view(&mut a, &[first, deleted]);
        take(&c, &a, &v, &Floor::default(), &[]).unwrap();
        assert!(c.db.annotations("book").unwrap().is_empty());
        assert!(restore::restore(c.db.conn(), &c.lib, &key("annotation"), now()).unwrap());
        let mut restored = c.db.annotations("book").unwrap();
        assert!(restored[0].updated_at >= before[0].updated_at);
        // Restore is a new edit; all content and original creation time survive.
        restored[0].updated_at = before[0].updated_at;
        assert_eq!(restored, before);
        assert_eq!(
            note_of(&c.db, "book"),
            Some(Some("whole paired note".into()))
        );
    }

    #[test]
    fn a_view_older_than_the_floor_is_refused() {
        let mut a = Device::new();
        let ch = edit(&mut a, "one", 1);
        let v = view(&mut a, &[ch]);
        let c = Device::new();
        assert!(matches!(
            take(
                &c,
                &a,
                &v,
                &Floor {
                    gen: v.gen + 1,
                    ..Floor::default()
                },
                &[]
            ),
            Err(ChainError::StaleView)
        ));
        assert_eq!(held(&c, &a), 0);
    }

    #[test]
    fn a_spine_disagreeing_with_a_held_change_is_refused() {
        let mut a = Device::new();
        let changes: Vec<_> = (1..=3).map(|n| edit(&mut a, &format!("{n}"), n)).collect();
        let v = view(&mut a, &changes);
        let c = Device::new();
        for ch in &changes[..2] {
            send(&c, &a, ch);
        }
        // Head 2 agrees; the earlier held sequence must still be checked.
        c.db.conn()
            .execute(
                "UPDATE sync_changes SET hash = ?1 WHERE seq = 1",
                [Hash32([9; 32]).to_string()],
            )
            .unwrap();
        assert!(matches!(
            take(&c, &a, &v, &Floor::default(), &[]),
            Err(ChainError::Link)
        ));
        assert_eq!(held(&c, &a), 2);
        let floor = Floor {
            s: 1,
            h: Some(Hash32([8; 32]).to_string()),
            ..Floor::default()
        };
        let empty = Device::new();
        assert!(matches!(
            take(&empty, &a, &v, &floor, &[]),
            Err(ChainError::Link)
        ));
    }

    #[test]
    fn a_view_below_a_floor_checkpoint_waits_until_the_later_files_reach_it() {
        let mut a = Device::new();
        let changes: Vec<_> = (1..=6).map(|n| edit(&mut a, &format!("{n}"), n)).collect();
        let v = view(&mut a, &changes[..4]);
        let c = Device::new();
        let floor = Floor {
            s: 6,
            h: Some(changes[5].hash.to_string()),
            earlier: vec![Checkpoint {
                s: 3,
                h: changes[2].hash.to_string(),
            }],
            ..Floor::default()
        };
        // The lowest checkpoint is 3: the spine itself reaches it.
        assert!(matches!(
            take(&c, &a, &v, &floor, &[]).unwrap().received[0],
            Received::Applied { .. }
        ));
        let c = Device::new();
        let floor = Floor {
            earlier: Vec::new(),
            ..floor
        };
        assert_eq!(
            take(&c, &a, &v, &floor, &changes[4..5]).unwrap().received,
            vec![Received::Waits, Received::Waits]
        );
        assert_eq!(held(&c, &a), 0);
        assert_eq!(count(&c, "sync_spine_versions"), 0);
        let mut fork = changes[5].clone();
        fork.prev = Some(Hash32([7; 32]));
        let signed = chain::sign_change(
            &a.key(),
            &a.lib.id,
            &fork.writer,
            fork.seq,
            fork.prev,
            &fork.versions,
        );
        fork.hash = signed.hash;
        let err = take(&c, &a, &v, &floor, &[changes[4].clone(), fork]);
        assert!(matches!(err, Err(ChainError::Link)));
        assert_eq!(held(&c, &a), 0);
        take(&c, &a, &v, &floor, &changes[4..]).unwrap();
        assert_eq!(held(&c, &a), 6);
        assert_eq!(note_of(&c.db, "book"), Some(Some("6".into())));
    }

    #[test]
    fn a_view_lands_whole_or_not_at_all() {
        let mut a = Device::new();
        for n in 0..101 {
            let mut h = highlight("book", Some("note"), 1_760_000_001_000);
            h.id = format!("00000000-0000-4000-8000-{n:012}");
            a.db.save_annotation(&h).unwrap();
        }
        let ch = a.change();
        a.note("note:00000000-0000-4000-8000-000000000000", "latest note");
        let later = a.change();
        let v = view(&mut a, &[ch, later]);
        assert_eq!(v.through.s, 2);
        let c = Device::new();
        assert_eq!(
            take(&c, &a, &v, &Floor::default(), &[]).unwrap().received,
            vec![Received::Asks]
        );
        for table in [
            "sync_versions",
            "sync_changes",
            "sync_spine_versions",
            "sync_seen",
            "sync_undo",
        ] {
            assert_eq!(count(&c, table), 0, "{table}");
        }
        assert!(c.db.annotations("book").unwrap().is_empty());
        let identity: (i64, String) =
            c.db.conn()
                .query_row("SELECT seq, hash FROM sync_question", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
        assert_eq!(identity, (v.through.s as i64, v.through.h.to_string()));
        guard::answer(c.db.conn(), &c.lib, guard::Answer::Apply, now()).unwrap();
        assert!(matches!(
            take(&c, &a, &v, &Floor::default(), &[]).unwrap().received[0],
            Received::Applied { .. }
        ));
        assert_eq!(c.db.annotations("book").unwrap().len(), 101);
        assert_eq!(held(&c, &a), v.through.s);
    }

    #[test]
    fn a_view_failure_rolls_back_and_stop_syncing_undoes_an_unapproved_view() {
        let mut a = Device::new();
        let ch = edit(&mut a, "whole note", 1);
        let v = view(&mut a, &[ch]);
        let c = Device::new();
        c.db.conn()
            .execute_batch(
                "CREATE TEMP TRIGGER fail_view BEFORE INSERT ON sync_versions
             BEGIN SELECT RAISE(ABORT, 'injected view failure'); END;",
            )
            .unwrap();
        assert!(take(&c, &a, &v, &Floor::default(), &[]).is_err());
        for table in [
            "sync_versions",
            "sync_changes",
            "sync_spine_versions",
            "sync_seen",
        ] {
            assert_eq!(count(&c, table), 0, "{table}");
        }
        c.db.conn().execute_batch("DROP TRIGGER fail_view").unwrap();
        take(&c, &a, &v, &Floor::default(), &[]).unwrap();
        assert_eq!(note_of(&c.db, "book"), Some(Some("whole note".into())));
        assert_eq!(
            guard::answer(c.db.conn(), &c.lib, guard::Answer::StopSyncing, now()).unwrap(),
            2
        );
        assert!(c.db.annotations("book").unwrap().is_empty());
    }

    #[test]
    fn a_paused_library_receives_no_view() {
        let mut a = Device::new();
        let ch = edit(&mut a, "one", 1);
        let v = view(&mut a, &[ch]);
        let c = Device::new();
        c.db.conn()
            .execute("UPDATE sync_libraries SET state = 'paused'", [])
            .unwrap();
        assert_eq!(
            take(&c, &a, &v, &Floor::default(), &[]).unwrap().received,
            vec![Received::Paused]
        );
        assert_eq!(held(&c, &a), 0);
        assert_eq!(count(&c, "sync_spine_versions"), 0);
    }

    #[test]
    fn a_view_carries_marks_and_respects_stopping_and_the_clock() {
        let mut a = Device::new();
        let ch = edit(&mut a, "marked", 1);
        let raw = ch
            .versions
            .iter()
            .find(|v| v["key"] == key("note").as_str())
            .unwrap();
        let mark = Entry {
            device: a.lib.device,
            seq: ch.seq,
            hash: record::version_hash(raw),
        };
        a.mark(mark);
        let v = view(&mut a, &[ch]);
        let c = Device::new();
        apply::stop(c.db.conn(), &c.lib, a.lib.device, "test budget", now()).unwrap();
        let stopped = take(&c, &a, &v, &Floor::default(), &[]).unwrap();
        assert_eq!(stopped.received, vec![Received::Stopped]);
        assert_eq!(stopped.marks, vec![mark]);
        assert_eq!(held(&c, &a), 0);
        assert_eq!(count(&c, "sync_spine_versions"), 0);

        // SY10: at an earlier arrival time the records wait for the clock.
        let c = Device::new();
        let bodies = v
            .shards
            .iter()
            .flat_map(|s| &s.records)
            .filter_map(|r| r.version["body"]["hash"].as_str())
            .map(|h| {
                journal::body(a.db.conn(), &Hash32::parse(h).unwrap())
                    .unwrap()
                    .unwrap()
            })
            .collect();
        let got = receive_view(
            c.db.conn(),
            &c.lib,
            &v,
            &a.lib.device,
            &a.key().public(),
            &Floor::default(),
            false,
            bodies,
            vec![],
            0,
        )
        .unwrap();
        assert_eq!(got.marks, vec![mark]);
        assert_eq!(
            got.received,
            vec![Received::Applied {
                applied: 0,
                set_aside: 2
            }]
        );
        assert_eq!(held(&c, &a), 1);
        assert!(c.db.annotations("book").unwrap().is_empty());
        assert_eq!(apply::apply_pending(c.db.conn(), &c.lib, now()).unwrap(), 2);
        assert_eq!(note_of(&c.db, "book"), Some(Some("marked".into())));
    }

    #[test]
    fn a_removed_devices_view_is_refused() {
        let mut a = Device::new();
        let ch = edit(&mut a, "one", 1);
        let v = view(&mut a, &[ch]);
        let c = Device::new();
        assert!(matches!(
            receive_view(
                c.db.conn(),
                &c.lib,
                &v,
                &a.lib.device,
                &a.key().public(),
                &Floor::default(),
                true,
                vec![],
                vec![],
                now()
            ),
            Err(ChainError::Removed)
        ));
        assert_eq!(held(&c, &a), 0);
    }

    #[test]
    fn an_accepted_view_generation_only_rises_after_a_successful_save() {
        struct Refuses;
        impl Keystore for Refuses {
            fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
                Ok(None)
            }
            fn save(&self, _: &[u8]) -> Result<(), StoreError> {
                Err(StoreError::Unreadable)
            }
        }
        let mut c = Device::new();
        let peer = Device::new();
        let before = c.store.clone();
        assert!(device_file::raise_view_generation(
            &Refuses,
            &mut c.store,
            &c.lib.id,
            &peer.lib.device,
            3
        )
        .is_err());
        assert_eq!(c.store, before);
        device_file::raise_view_generation(&c.ks, &mut c.store, &c.lib.id, &peer.lib.device, 3)
            .unwrap();
        device_file::raise_view_generation(&Refuses, &mut c.store, &c.lib.id, &peer.lib.device, 2)
            .unwrap();
        let saved = DeviceStore::load(&c.ks).unwrap();
        assert_eq!(
            saved.library(&c.lib.id).unwrap().floors[&peer.lib.device.to_string()].gen,
            3
        );
    }
}
