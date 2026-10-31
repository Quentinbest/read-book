//! SY11, SP17, SP36, #111: superseded payloads become permanent spine entries.
//! Pure library code; the caller supplies the acknowledgement and age decision.

use super::journal::{self, Library, Result};
use super::merge::Placed;
use super::record::{Content, Entry, Key};
use rusqlite::{params, Connection};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    pub versions: usize,
    /// Exact UTF-8 payload bytes removed, excluding bodies and SQLite overhead.
    pub raw_bytes: usize,
}

/// SY11, SP36: drop superseded payloads only when `may_drop(key, superseded,
/// newer)` attests that every enrolled device's frontier includes `newer`, and
/// seven days have passed by this device's vouched clock. SP26, SP30: a version
/// accepted by everyone is never reverted, so its ancestor cannot revive.
/// Each pair names distinct, numbered, applied versions of the same key, with
/// `newer` descending from `superseded`. Nothing is wired into a sync pass.
pub fn prune(
    conn: &Connection,
    lib: &Library,
    may_drop: &dyn Fn(&Key, &Entry, &Entry) -> bool,
) -> Result<Pruned> {
    let tx = conn.unchecked_transaction()?;
    let keys: Vec<String> = tx.prepare("SELECT DISTINCT key FROM sync_versions WHERE library = ?1 AND applied = 2 AND seq IS NOT NULL")?
        .query_map([lib.id.to_string()], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    let mut pruned = Pruned::default();
    for key in keys {
        let rows: Vec<(String, i64, String, i64)> = tx.prepare(
            "SELECT device, seq, raw, applied FROM sync_versions
             WHERE library = ?1 AND key = ?2 AND applied IN (1, 2) AND seq IS NOT NULL ORDER BY seq DESC",
        )?.query_map(params![lib.id.to_string(), key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let versions = rows
            .iter()
            .map(|(device, seq, raw, state)| {
                let device = super::ids::Id16::parse(device)
                    .ok_or(super::record::RecordError::Field("device"))?;
                Ok((
                    Placed {
                        version: journal::check(raw, device)?,
                        seq: *seq as u64,
                    },
                    *state,
                    raw.len(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for (candidate, state, bytes) in &versions {
            if *state != 2
                || !versions.iter().any(|(newer, _, _)| {
                    newer.id() != candidate.id()
                        && newer.descends_from(candidate)
                        && may_drop(&candidate.version.key, &candidate.id(), &newer.id())
                })
            {
                continue;
            }
            let id = candidate.id();
            tx.execute("INSERT OR IGNORE INTO sync_spine_versions (library, device, seq, key_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![lib.id.to_string(), id.device.to_string(), id.seq as i64,
                    &super::envelope::vlist_entry(&key, &id.hash)[..16], id.hash.to_string()])?;
            tx.execute("DELETE FROM sync_versions WHERE library = ?1 AND key = ?2 AND device = ?3 AND seq = ?4 AND hash = ?5",
                params![lib.id.to_string(), key, id.device.to_string(), id.seq as i64, id.hash.to_string()])?;
            // SY11, SP36: another version's body stays. SY26: sync_undo has its
            // own copies; leave those alone.
            if let Content::Body { hash, .. } = &candidate.version.content {
                journal::drop_body_if_unnamed(&tx, hash)?;
            }
            pruned.versions += 1;
            pruned.raw_bytes += bytes;
        }
    }
    tx.commit()?;
    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::apply::{self, JournalChains, Received};
    use crate::sync::compact::tests::Device;
    use crate::sync::ids::{identity_hash, Hash32};
    use crate::sync::merge::{Chains, Lookup, Prior};
    use crate::sync::read::tests::{branch, edit, is_conflict, key, send};
    use crate::sync::record;
    use serde_json::{json, Value};

    fn note(change: &crate::sync::chain::Change) -> &Value {
        change
            .versions
            .iter()
            .find(|v| v["key"] == key("note").as_str())
            .unwrap()
    }
    fn raw_bytes(d: &Device, writer: &Device) -> i64 {
        d.db.conn().query_row("SELECT COALESCE(SUM(length(CAST(raw AS BLOB))), 0) FROM sync_versions WHERE library = ?1 AND device = ?2",
            params![d.lib.id.to_string(), writer.lib.device.to_string()], |r| r.get(0)).unwrap()
    }
    fn receive(c: &Device, b: &mut Device, text: &str, n: i64) -> crate::sync::chain::Change {
        let ch = edit(b, text, n);
        assert!(matches!(
            send(c, b, &ch),
            Received::Applied { set_aside: 0, .. }
        ));
        ch
    }

    #[test]
    fn many_edits_with_large_unknown_fields_stay_bounded() {
        let mut b = Device::new();
        let c = Device::new();
        let seed = receive(&c, &mut b, "seed", 0);
        let mut previous = record::version_hash(note(&seed));
        let mut freed = 0;
        for seq in 2..=101 {
            let body = json!({"text": format!("edit {seq}")})
                .to_string()
                .into_bytes();
            let value = json!({
                "format": 1, "key": key("note").as_str(), "device": b.lib.device.to_string(),
                "clock": [1_760_000_001_000u64 + seq, 0],
                "history": [{"d": b.lib.device.to_string(), "s": seq - 1, "h": previous.to_string()}],
                "body": {"hash": identity_hash(&body).to_string(), "size": body.len()},
                "unknown": "界".repeat(3400)
            });
            previous = record::version_hash(&value);
            let batch = apply::Batch {
                device: b.lib.device,
                seq,
                hash: identity_hash(value.to_string().as_bytes()),
                versions: vec![value],
                bodies: vec![body],
            };
            let now = crate::store::now_ms();
            let mut got = apply::receive(c.db.conn(), &c.lib, &batch, now).unwrap();
            if got == Received::Asks {
                // SY26: approve the real guard prompt; keep receiving valid edits.
                crate::sync::guard::answer(
                    c.db.conn(),
                    &c.lib,
                    crate::sync::guard::Answer::Apply,
                    now,
                )
                .unwrap();
                got = apply::receive(c.db.conn(), &c.lib, &batch, now).unwrap();
            }
            assert!(
                matches!(got, Received::Applied { set_aside: 0, .. }),
                "sequence {seq}: {got:?}"
            );
            let before = raw_bytes(&c, &b);
            let result = prune(c.db.conn(), &c.lib, &|_, _, _| true).unwrap();
            assert_eq!(before - raw_bytes(&c, &b), result.raw_bytes as i64);
            freed += result.raw_bytes;
        }
        assert!(freed >= 99 * 10_000);
        // #111, SY9: at most about two 10 KB versions, despite 100 edits.
        assert!(raw_bytes(&c, &b) <= 24_000);
        let chains = JournalChains {
            conn: c.db.conn(),
            lib: &c.lib,
        };
        for seq in 1..101 {
            assert!(matches!(
                chains.version_at(b.lib.device, seq, key("note").as_str()),
                Lookup::Present(_)
            ));
        }
        assert_eq!(
            c.db.conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_spine_versions WHERE device = ?1 AND key_hash = ?2",
                    params![
                        b.lib.device.to_string(),
                        &crate::sync::envelope::vlist_entry(key("note").as_str(), &previous)[..16]
                    ],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            100
        );
    }

    #[test]
    fn a_branch_from_a_pruned_version_is_still_verified() {
        let mut b = Device::new();
        let first = edit(&mut b, "one", 1);
        let second = edit(&mut b, "two", 2);
        for valid in [true, false] {
            let c = Device::new();
            send(&c, &b, &first);
            send(&c, &b, &second);
            assert!(
                prune(c.db.conn(), &c.lib, &|_, _, _| true)
                    .unwrap()
                    .versions
                    > 0
            );
            let hash = if valid {
                record::version_hash(note(&first))
            } else {
                Hash32([9; 32])
            };
            assert!(
                matches!(branch(&c, &Device::new(), note(&first), first.seq, hash), Received::Applied { set_aside, .. } if set_aside == usize::from(!valid))
            );
            assert_eq!(is_conflict(&c), valid);
        }
    }

    #[test]
    fn nothing_is_dropped_until_the_caller_says_it_may_be() {
        let mut b = Device::new();
        let c = Device::new();
        let mut edits = Vec::new();
        for n in 1..=11 {
            edits.push(receive(&c, &mut b, &format!("edit {n}"), n));
        }
        let before = raw_bytes(&c, &b);
        assert_eq!(
            prune(c.db.conn(), &c.lib, &|_, _, _| false).unwrap(),
            Pruned::default()
        );
        assert_eq!(raw_bytes(&c, &b), before);
        for (index, state) in [0, -1, -2, -3, -4, -5, -6].into_iter().enumerate() {
            c.db.conn().execute("UPDATE sync_versions SET applied = ?1 WHERE device = ?2 AND seq = ?3 AND key = ?4", params![state, b.lib.device.to_string(), edits[index].seq as i64, key("note").as_str()]).unwrap();
        }
        c.note(key("note").as_str(), "unsent");
        // An unnumbered row is protected even if marked superseded.
        c.db.conn()
            .execute("UPDATE sync_versions SET applied = 2 WHERE seq IS NULL", [])
            .unwrap();
        let protected: Vec<(i64, Option<i64>, String)> =
            c.db.conn()
                .prepare(
                    "SELECT applied, seq, raw FROM sync_versions WHERE applied != 2 OR seq IS NULL",
                )
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
        let result = prune(c.db.conn(), &c.lib, &|k, old, newer| {
            assert_eq!(k, &key("note"));
            assert_ne!(old, newer);
            newer.device == b.lib.device && newer.seq == 11
        })
        .unwrap();
        assert!(result.versions > 0);
        for (state, seq, raw) in protected {
            assert_eq!(c.db.conn().query_row("SELECT COUNT(*) FROM sync_versions WHERE applied = ?1 AND seq IS ?2 AND raw = ?3", params![state, seq, raw], |r| r.get::<_, i64>(0)).unwrap(), 1);
        }
        assert_eq!(
            prune(c.db.conn(), &c.lib, &|_, _, _| true).unwrap(),
            Pruned::default()
        );
    }

    #[test]
    fn a_body_another_version_names_stays() {
        let mut b = Device::new();
        let c = Device::new();
        for (n, text) in ["same", "different", "same"].iter().enumerate() {
            receive(&c, &mut b, text, n as i64);
        }
        let shared = identity_hash(br#"{"text":"same"}"#);
        let unshared = identity_hash(br#"{"text":"different"}"#);
        assert!(journal::body(c.db.conn(), &unshared).unwrap().is_some());
        prune(c.db.conn(), &c.lib, &|_, _, _| true).unwrap();
        assert!(journal::body(c.db.conn(), &shared).unwrap().is_some());
        assert!(journal::body(c.db.conn(), &unshared).unwrap().is_none());
    }

    #[test]
    fn a_pruned_version_is_still_its_writers_prior_version() {
        let mut b = Device::new();
        let c = Device::new();
        let first = receive(&c, &mut b, "one", 1);
        let second = receive(&c, &mut b, "two", 2);
        let mut d = Device::new();
        send(&d, &b, &first);
        send(&d, &b, &second);
        receive(&c, &mut d, "other writer", 3);
        prune(c.db.conn(), &c.lib, &|_, _, _| true).unwrap();
        let chains = JournalChains {
            conn: c.db.conn(),
            lib: &c.lib,
        };
        let hash = record::version_hash(note(&first));
        assert_eq!(
            chains.version_at(b.lib.device, 1, key("note").as_str()),
            Lookup::Present(hash)
        );
        assert_eq!(
            chains.prior_version(b.lib.device, key("note").as_str(), 2),
            Prior::At(1, hash)
        );
        assert_eq!(
            chains.prior_version(b.lib.device, key("note").as_str(), 3),
            Prior::At(2, record::version_hash(note(&second)))
        );
        receive(&c, &mut b, "three", 3);
        assert!(is_conflict(&c)); // SY13: B's new edit is concurrent with D's.
    }

    #[test]
    fn compaction_after_pruning_a_branch_base_still_builds() {
        let mut b = Device::new();
        let mut c = Device::new();
        let p = receive(&c, &mut b, "P", 1);
        let v = edit(&mut c, "V", 2);
        // N descends from P, concurrent with V; receiving it keeps V's row.
        receive(&c, &mut b, "N", 3);
        let id = Entry {
            device: b.lib.device,
            seq: p.seq,
            hash: record::version_hash(note(&p)),
        };
        prune(c.db.conn(), &c.lib, &|_, old, newer| {
            old.device == b.lib.device && newer.device == b.lib.device
        })
        .unwrap();
        assert!(journal::version_raw(c.db.conn(), &c.lib, &key("note"), id)
            .unwrap()
            .is_none());
        assert!(journal::spine_contains(c.db.conn(), &c.lib, &key("note"), id).unwrap());
        c.build(None, std::slice::from_ref(&v), &|_| false);

        // Own changes supplied to build also survive pruning their journal rows.
        let mut own = Device::new();
        let first = edit(&mut own, "own one", 1);
        let second = edit(&mut own, "own two", 2);
        let old = Entry {
            device: own.lib.device,
            seq: first.seq,
            hash: record::version_hash(note(&first)),
        };
        own.mark(old);
        let mut peer = Device::new();
        send(&peer, &own, &first);
        send(&peer, &own, &second);
        receive(&own, &mut peer, "peer descendant", 3);
        prune(own.db.conn(), &own.lib, &|_, _, _| true).unwrap();
        assert!(
            journal::version_raw(own.db.conn(), &own.lib, &key("note"), old)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            JournalChains {
                conn: own.db.conn(),
                lib: &own.lib
            }
            .version_at(old.device, old.seq, key("note").as_str()),
            Lookup::Present(old.hash)
        );
        let built = own.build(None, &[first, second], &|_| false);
        let written = own.save(&built);
        assert!(own
            .open(&written)
            .shards
            .iter()
            .any(|s| s.marks.contains(&old)));
    }

    #[test]
    fn reverting_a_later_edit_accepts_a_pruned_ancestor() {
        use crate::sync::{late, trust::Mark};
        use std::collections::BTreeMap;
        let mut b = Device::new();
        let c = Device::new();
        receive(&c, &mut b, "one", 1);
        let accepted = receive(&c, &mut b, "two", 2);
        prune(c.db.conn(), &c.lib, &|_, _, _| true).unwrap();
        let later = receive(&c, &mut b, "three", 3);
        let cutoff = Mark {
            s: accepted.seq,
            h: accepted.hash,
        };
        let reverted = late::revert_past(
            c.db.conn(),
            &c.lib,
            &BTreeMap::from([(b.lib.device, Some(cutoff))]),
            &BTreeMap::from([(b.lib.device, cutoff)]),
            "removed",
            crate::store::now_ms(),
        )
        .unwrap();
        assert!(reverted.reverted > 0);
        assert!(
            late::reapply_upto(
                c.db.conn(),
                &c.lib,
                b.lib.device,
                Mark {
                    s: later.seq,
                    h: later.hash
                },
                crate::store::now_ms()
            )
            .unwrap()
                > 0
        );
    }

    #[test]
    fn a_keep_finishes_after_its_published_replacement_is_pruned() {
        use crate::sync::{late, trust::Mark};
        use std::collections::BTreeMap;
        let mut b = Device::new();
        let mut c = Device::new();
        let first = receive(&c, &mut b, "one", 1);
        let later = receive(&c, &mut b, "late", 2);
        let now = crate::store::now_ms();
        late::revert_past(
            c.db.conn(),
            &c.lib,
            &BTreeMap::from([(
                b.lib.device,
                Some(Mark {
                    s: first.seq,
                    h: first.hash,
                }),
            )]),
            &BTreeMap::new(),
            "removed",
            now,
        )
        .unwrap();
        late::keep(
            c.db.conn(),
            &c.lib,
            b.lib.device,
            &key("note"),
            later.seq,
            now,
        )
        .unwrap();
        let replacement = c.change();
        let id = Entry {
            device: c.lib.device,
            seq: replacement.seq,
            hash: record::version_hash(note(&replacement)),
        };
        let mut d = Device::new();
        send(&d, &b, &first);
        send(&d, &b, &later);
        send(&d, &c, &replacement);
        receive(&c, &mut d, "after Keep", 3);
        prune(c.db.conn(), &c.lib, &|_, _, _| true).unwrap();
        assert!(journal::version_raw(c.db.conn(), &c.lib, &key("note"), id)
            .unwrap()
            .is_none());
        assert_eq!(late::finish_keeps(c.db.conn(), &c.lib).unwrap(), 1);
    }

    #[test]
    fn a_peer_edit_naming_a_pruned_own_version_still_applies() {
        // SP11, #111: once this device's newest own payload is pruned, its spine
        // still says how far its own chain is held, so ancestry naming it resolves.
        let waiting = |d: &Device, writer: &Device| -> i64 {
            d.db.conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_versions
                     WHERE library = ?1 AND device = ?2 AND applied = -1",
                    params![d.lib.id.to_string(), writer.lib.device.to_string()],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let mut a = Device::new();
        let mut b = Device::new();
        let mut d = Device::new();
        // A's second change edits only the note, so its highest own sequence
        // goes when pruned while its first change's highlight row stays.
        for (n, text) in [(1, "mine"), (2, "mine again")] {
            let own = edit(&mut a, text, n);
            send(&b, &a, &own);
            send(&d, &a, &own);
        }
        let theirs = receive(&a, &mut b, "theirs", 3);
        send(&d, &b, &theirs);
        prune(a.db.conn(), &a.lib, &|_, _, _| true).unwrap();
        let highest: Option<i64> =
            a.db.conn()
                .query_row(
                    "SELECT MAX(seq) FROM sync_versions WHERE library = ?1 AND device = ?2",
                    params![a.lib.id.to_string(), a.lib.device.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(
            highest,
            Some(1),
            "A's own change 2 is pruned, change 1 stays"
        );
        let later = edit(&mut d, "later", 4);
        assert!(note(&later)["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["d"] == a.lib.device.to_string()));
        assert!(matches!(
            send(&a, &d, &later),
            Received::Applied { set_aside: 0, .. }
        ));
        assert_eq!(waiting(&a, &d), 0, "it doesn't wait for A's own chain");
    }

    #[test]
    fn compaction_after_pruning_an_own_book_file_version_still_builds() {
        // #111: a book file's names come from its body; once a superseded own
        // version and its unique body are pruned, compaction must not need them.
        let mut d = Device::new();
        let key = Key::parse("bookFile:00000000-0000-4000-8000-000000000001")
            .unwrap()
            .0;
        let file = |n| {
            crate::sync::journal::LocalContent::Body(
                json!({"hash": Hash32([n; 32]).to_string(), "size": 10,
                "title": "Book", "authors": [], "language": null, "identifier": null,
                "layout": "reflowable"})
                .to_string()
                .into_bytes(),
            )
        };
        let old = d.local(key.as_str(), file(1));
        let one = d.change();
        d.local(key.as_str(), file(2));
        let two = d.change();
        let id = Entry {
            device: d.lib.device,
            seq: one.seq,
            hash: old.hash,
        };
        d.db.conn()
            .execute(
                "UPDATE sync_versions SET applied = 2 WHERE library = ?1 AND seq = ?2",
                params![d.lib.id.to_string(), one.seq as i64],
            )
            .unwrap();
        assert_eq!(
            prune(d.db.conn(), &d.lib, &|_, _, _| true)
                .unwrap()
                .versions,
            1
        );
        let Content::Body { hash, .. } = old.content else {
            panic!("a book file version has a body")
        };
        assert!(journal::body(d.db.conn(), &hash).unwrap().is_none());
        assert!(journal::spine_contains(d.db.conn(), &d.lib, &key, id).unwrap());
        d.build(None, &[one, two], &|_| false);
    }
}
