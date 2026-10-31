//! SP17, SP36, #176: a view can add Restore protection, never take it away.
//!
//! Caller-owned library state, like `hold::Ledger`. Claims remain until the
//! caller verifies SP36 lines 2 and 3; only locally proven objects may supply a
//! retired cleanup candidate. Nothing here deletes content or runs a sync pass.

use super::apply::JournalChains;
use super::cleanup::{Name, Week};
use super::compact;
use super::ids::DeviceId;
use super::journal::{self, JournalError, Library};
use super::merge::{Chains, Lookup, Prior};
use super::record::{self, Checked, Entry, Key, Version};
use super::view::View;
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// SP17: retain the union of claims, including omissions in later generations.
/// One instance belongs to one library and its naming key.
#[derive(Debug, Default)]
pub struct KnownWeeks {
    entries: HashMap<Entry, BTreeMap<Key, BTreeMap<Name, bool>>>,
}

fn raw(
    conn: &Connection,
    lib: &Library,
    key: &Key,
    id: Entry,
) -> Result<Option<Version>, JournalError> {
    let Some(raw) = journal::version_raw(conn, lib, key, id)? else {
        return Ok(None);
    };
    let Ok(raw) = serde_json::from_str(&raw) else {
        return Ok(None);
    };
    let Ok(Checked::Version(v)) = record::check_version(raw, id.device) else {
        return Ok(None);
    };
    Ok((v.key == *key && v.hash == id.hash).then_some(v))
}

impl KnownWeeks {
    /// SP17, #176: take claims only after the authenticated view lands. Missing
    /// raw predecessors or book-file bodies leave objects unproven; later calls
    /// retry all retained claims, even those this view omits. Proof never lapses.
    pub fn receive_view(
        &mut self,
        conn: &Connection,
        lib: &Library,
        naming: &[u8; 32],
        device: DeviceId,
        view: &View,
    ) -> Result<(), JournalError> {
        for record in view.shards.iter().flat_map(|s| &s.records) {
            for week in &record.week {
                // SP17: view::open checks the entry shape, but does not refuse
                // a week's retiring version belonging to another writer.
                if week.v.device != device {
                    continue;
                }
                let objects = self
                    .entries
                    .entry(week.v)
                    .or_default()
                    .entry(record.key.clone())
                    .or_default();
                for name in &week.objects {
                    objects.entry(*name).or_insert(false);
                }
            }
        }
        let chains = JournalChains { conn, lib };
        for (id, keys) in &mut self.entries {
            for (key, objects) in keys {
                if chains.version_at(id.device, id.seq, key.as_str()) != Lookup::Present(id.hash) {
                    continue;
                }
                // SP17: the retirement must itself stop naming the object, so
                // its own raw version is needed; the spine's hash alone can't
                // show which of several versions naming it was the last.
                let Some(retiring) = raw(conn, lib, key, *id)? else {
                    continue;
                };
                let Ok(still) = compact::names(conn, naming, &retiring) else {
                    continue;
                };
                let mut parents = Vec::new();
                // SP11: even when only the spine holds the retirement, it
                // identifies the writer's exact previous occurrence of this key.
                if let Prior::At(seq, hash) = chains.prior_version(id.device, key.as_str(), id.seq)
                {
                    parents.push(Entry {
                        device: id.device,
                        seq,
                        hash,
                    });
                }
                // SP11: a first own version can also replace another writer's
                // version, explicitly named by its checked raw history.
                parents.extend(retiring.history.into_iter().filter(|p| {
                    chains.version_at(p.device, p.seq, key.as_str()) == Lookup::Present(p.hash)
                }));
                for parent in parents {
                    if let Some(v) = raw(conn, lib, key, parent)? {
                        // SP9: missing book-file bodies cannot prove object names.
                        if let Ok(names) = compact::names(conn, naming, &v) {
                            for (name, proven) in objects.iter_mut() {
                                *proven |= names.contains(name) && !still.contains(name);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// SP36: the caller verified both lines 2 and 3 from signed evidence.
    pub fn release(&mut self, verified: &dyn Fn(&Entry) -> bool) {
        self.entries.retain(|id, _| !verified(id));
    }

    /// SP17, SP36: all known objects protect content, including unproven claims.
    pub fn weeks(&self) -> Vec<Week> {
        self.retired_by()
            .into_iter()
            .flat_map(|(name, ids)| ids.into_iter().map(move |retiring| Week { name, retiring }))
            .collect()
    }

    /// SP36: every retirement must permit removal, including unproven claims.
    pub fn retired_by(&self) -> BTreeMap<Name, Vec<Entry>> {
        let mut out: BTreeMap<Name, Vec<Entry>> = BTreeMap::new();
        for (id, keys) in &self.entries {
            let names: BTreeSet<_> = keys.values().flat_map(|v| v.keys().copied()).collect();
            for name in names {
                out.entry(name).or_default().push(*id);
            }
        }
        for ids in out.values_mut() {
            ids.sort_by_key(|id| (id.device, id.seq, id.hash));
        }
        out
    }

    /// SP17, #176: only a proven object may form `cleanup::Content::Retired`.
    /// This is provenance, not permission to remove: SP36 still applies.
    pub fn may_release(&self, name: Name, retiring: &Entry) -> bool {
        self.entries.get(retiring).is_some_and(|keys| {
            keys.values()
                .any(|objects| objects.get(&name) == Some(&true))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::compact::tests::Device;
    use crate::sync::device_store::Floor;
    use crate::sync::hold::Ledger;
    use crate::sync::ids::Hash32;
    use crate::sync::retention::{Ages, Vouched};
    use crate::sync::{chain, cleanup, read};
    use std::collections::HashMap;

    const NAMING: [u8; 32] = [42; 32];
    const NOTE: &str = "note:00000000-0000-4000-8000-000000000001";

    fn fixture() -> (Device, Vec<chain::Change>, View, Name, Entry) {
        let mut a = Device::new();
        a.note(NOTE, "before");
        let first = a.change();
        a.note(NOTE, "after");
        let second = a.change();
        let changes = vec![first, second];
        let built = a.build(None, &changes, &|_| false);
        let written = a.save(&built);
        let view = a.open(&written);
        let week = &view
            .shards
            .iter()
            .flat_map(|s| &s.records)
            .find(|r| r.key.as_str() == NOTE)
            .unwrap()
            .week[0];
        let (name, id) = (week.objects[0], week.v);
        (a, changes, view, name, id)
    }

    fn send(c: &Device, a: &Device, change: &chain::Change) {
        let signed = chain::sign_change(
            &a.key(),
            &a.lib.id,
            &change.writer,
            change.seq,
            change.prev,
            &change.versions,
        );
        let bodies = change
            .versions
            .iter()
            .filter_map(|v| v["body"]["hash"].as_str())
            .map(|h| {
                journal::body(a.db.conn(), &Hash32::parse(h).unwrap())
                    .unwrap()
                    .unwrap()
            })
            .collect();
        chain::receive_change(
            c.db.conn(),
            &c.lib,
            &signed.bytes,
            &a.lib.device,
            &a.key().public(),
            &[],
            bodies,
            crate::store::now_ms(),
        )
        .unwrap();
    }

    fn take(c: &Device, a: &Device, view: &View) {
        let bodies = view
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
        read::receive_view(
            c.db.conn(),
            &c.lib,
            view,
            &a.lib.device,
            &a.key().public(),
            &Floor::default(),
            false,
            bodies,
            vec![],
            crate::store::now_ms(),
        )
        .unwrap();
    }

    fn receive(known: &mut KnownWeeks, c: &Device, a: &Device, view: &View) {
        known
            .receive_view(c.db.conn(), &c.lib, &NAMING, a.lib.device, view)
            .unwrap();
    }

    fn clear(view: &mut View) {
        for r in view.shards.iter_mut().flat_map(|s| &mut s.records) {
            r.week.clear();
        }
    }

    #[test]
    fn a_week_entry_a_later_view_leaves_out_stays_until_released() {
        let (a, _, mut view, name, id) = fixture();
        let c = Device::new();
        let mut known = KnownWeeks::default();
        receive(&mut known, &c, &a, &view);
        let extra = Name {
            name: Hash32([9; 32]),
            ..name
        };
        view.shards
            .iter_mut()
            .flat_map(|s| &mut s.records)
            .find(|r| r.key.as_str() == NOTE)
            .unwrap()
            .week[0]
            .objects = vec![extra];
        receive(&mut known, &c, &a, &view);
        assert_eq!(known.weeks().len(), 2, "objects are unioned");
        clear(&mut view);
        receive(&mut known, &c, &a, &view);
        assert_eq!(known.retired_by()[&name], vec![id]);
        known.release(&|_| false);
        assert_eq!(known.weeks().len(), 2);
        known.release(&|v| *v == id);
        assert!(known.weeks().is_empty());
    }

    #[test]
    fn a_version_still_naming_the_object_did_not_retire_it() {
        // v1 and v2 both name X; only v3 stops naming it. A claim that v2
        // retired X must not be proven, or cleanup would count v2's older age.
        let mut a = Device::new();
        let c = Device::new();
        let v1 = a.note(NOTE, "same");
        let mut changes = vec![a.change()];
        let v2 = a.note(NOTE, "same");
        changes.push(a.change());
        a.note(NOTE, "other");
        changes.push(a.change());
        let name = *compact::names(a.db.conn(), &NAMING, &v1)
            .unwrap()
            .first()
            .unwrap();
        assert!(compact::names(a.db.conn(), &NAMING, &v2)
            .unwrap()
            .contains(&name));
        let built = a.build(None, &changes, &|_| false);
        let written = a.save(&built);
        let mut view = a.open(&written);
        for ch in &changes {
            send(&c, &a, ch);
        }
        let honest = view
            .shards
            .iter()
            .flat_map(|s| &s.records)
            .find(|r| r.key.as_str() == NOTE)
            .unwrap()
            .week[0]
            .v;
        let claimed = Entry {
            device: a.lib.device,
            seq: changes[1].seq,
            hash: v2.hash,
        };
        let mut known = KnownWeeks::default();
        receive(&mut known, &c, &a, &view);
        assert!(known.may_release(name, &honest));
        let week = &mut view
            .shards
            .iter_mut()
            .flat_map(|s| &mut s.records)
            .find(|r| r.key.as_str() == NOTE)
            .unwrap()
            .week;
        week.clear();
        week.push(crate::sync::view::Retirement {
            v: claimed,
            objects: vec![name],
        });
        receive(&mut known, &c, &a, &view);
        assert!(!known.may_release(name, &claimed));
        assert!(known.retired_by()[&name].contains(&claimed));
    }

    #[test]
    fn an_old_retirement_claiming_recent_content_cannot_release_it() {
        let (mut a, changes, mut view, original, id) = fixture();
        let recent = a.note(NOTE, "recent content");
        let name = *compact::names(a.db.conn(), &NAMING, &recent)
            .unwrap()
            .first()
            .unwrap();
        a.change();
        view.shards
            .iter_mut()
            .flat_map(|s| &mut s.records)
            .find(|r| r.key.as_str() == NOTE)
            .unwrap()
            .week[0]
            .objects
            .push(name);
        let c = Device::new();
        send(&c, &a, &changes[0]);
        take(&c, &a, &view);
        let mut known = KnownWeeks::default();
        receive(&mut known, &c, &a, &view);
        assert!(known
            .weeks()
            .iter()
            .any(|w| w.name == name && w.retiring == id));
        assert_eq!(known.retired_by()[&name], vec![id]);
        assert!(!known.may_release(name, &id));
        assert!(
            known.may_release(original, &id),
            "the retirement itself resolves"
        );
    }

    #[test]
    fn a_retirement_outside_the_writers_history_only_keeps() {
        let (a, changes, mut view, name, _) = fixture();
        let c = Device::new();
        send(&c, &a, &changes[0]);
        take(&c, &a, &view);
        let week = &mut view
            .shards
            .iter_mut()
            .flat_map(|s| &mut s.records)
            .find(|r| r.key.as_str() == NOTE)
            .unwrap()
            .week[0];
        week.v.hash = Hash32([9; 32]);
        let false_id = week.v;
        let mut known = KnownWeeks::default();
        receive(&mut known, &c, &a, &view);
        assert!(!known.may_release(name, &false_id));
        assert_eq!(known.retired_by()[&name], vec![false_id]);
        view.shards
            .iter_mut()
            .flat_map(|s| &mut s.records)
            .find(|r| r.key.as_str() == NOTE)
            .unwrap()
            .week[0]
            .v
            .device = c.lib.device;
        receive(&mut known, &c, &a, &view);
        assert_eq!(known.weeks().len(), 1, "foreign writer ignored");
    }

    #[test]
    fn a_proven_retirement_may_release() {
        let (a, changes, mut view, name, id) = fixture();
        let c = Device::new();
        let mut known = KnownWeeks::default();
        take(&c, &a, &view);
        receive(&mut known, &c, &a, &view);
        assert!(
            !known.may_release(name, &id),
            "spine alone has no predecessor body"
        );
        // Retry an omitted claim after this reader gains the predecessor.
        let c = Device::new();
        send(&c, &a, &changes[0]);
        take(&c, &a, &view);
        let repeated = view.clone();
        clear(&mut view);
        receive(&mut known, &c, &a, &view);
        assert!(known.may_release(name, &id));
        view = repeated;
        receive(&mut known, &c, &a, &view);
        assert!(known.may_release(name, &id), "a repeat cannot erase proof");
        clear(&mut view);
        receive(&mut known, &Device::new(), &a, &view);
        assert!(
            known.may_release(name, &id),
            "missing evidence never loses proof"
        );
    }

    #[test]
    fn cleanup_never_removes_content_only_a_claim_retired() {
        let (a, _, view, name, id) = fixture();
        let c = Device::new();
        let mut known = KnownWeeks::default();
        receive(&mut known, &c, &a, &view);
        assert!(!known.may_release(name, &id));
        let mut clock = Vouched::new(0);
        clock.tick(0, 0);
        let reading = BTreeMap::from([(
            c.lib.device,
            cleanup::Reading {
                through_newest_own: true,
                all_listed_after_own: true,
            },
        )]);
        let facts = cleanup::Facts {
            this: c.lib.device,
            enrolled: &[c.lib.device],
            without_device_file: &[],
            reading: &reading,
            all_fetchable_late_read: true,
            undiscarded_unfetched_late_over_cap: false,
            live_names: &[],
            waiting_late_names: &[],
            weeks: &known.weeks(),
            ledger: &Ledger::default(),
            ages: &Ages::default(),
            clock: &clock,
            retirements: &HashMap::new(),
            retired_by: &known.retired_by(),
            holding_back: &[],
            unnamed: &BTreeMap::new(),
        };
        assert_eq!(
            cleanup::may_remove(
                &cleanup::Object {
                    name,
                    content: cleanup::Content::Unnamed
                },
                &facts
            ),
            Err(cleanup::Blocked::Line1(cleanup::Line1::NamedByWeek(id)))
        );
    }
}
