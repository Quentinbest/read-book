//! SY12, SY13, SY20, SP11, SP13: descent, heads and what a merge decides.
//!
//! A record's versions form a graph through their `history` (SP11). Its heads are
//! the versions nothing else descends from. One head settles the record. Several
//! heads are concurrent changes made apart: for places, settings and reading state
//! the newest wins (item 105); for highlights, notes and choices the reader decides
//! (SY13, item 100), unless their contents are equal. A deletion against a change
//! made apart is a conflict the same way, and until the reader chooses the thing
//! stays deleted (item 102). The outcome depends only on the set of versions, never
//! on the order they arrived in, so every device reaches the same one.

use super::clock::Stamp;
use super::ids::Hash32;
use super::record::{Content, Entry, Kind, Version};
use std::collections::{BTreeMap, HashSet};

/// A version and the change file it came in: its ID is (device, seq, hash) (SP11).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub version: Version,
    pub seq: u64,
}

impl Placed {
    pub fn id(&self) -> Entry {
        Entry {
            device: self.version.device,
            seq: self.seq,
            hash: self.version.hash,
        }
    }

    pub fn stamp(&self) -> Stamp {
        Stamp {
            clock: self.version.clock,
            device: self.version.device,
        }
    }

    pub fn is_deleted(&self) -> bool {
        matches!(self.version.content, Content::Deleted { .. })
    }

    /// SP11: `self` descends from `other` when it is `other`, or its history entry
    /// for `other`'s device has a sequence at least `other`'s. One device's versions
    /// of a key form a line, since each names its writer's previous one.
    pub fn descends_from(&self, other: &Placed) -> bool {
        if self.version.device == other.version.device {
            return self.seq >= other.seq;
        }
        self.version
            .history
            .iter()
            .any(|e| e.device == other.version.device && e.seq >= other.seq)
    }

    /// What two versions hold, compared: equal contents are not a conflict (SY13).
    fn same_content(&self, other: &Placed) -> bool {
        match (&self.version.content, &other.version.content) {
            (Content::Body { hash: a, .. }, Content::Body { hash: b, .. }) => a == b,
            (Content::Deleted { .. }, Content::Deleted { .. }) => true,
            (Content::Inline(a), Content::Inline(b)) => a == b,
            _ => false,
        }
    }
}

/// What the named device's chain shows at an entry's sequence (SP11).
#[derive(Debug, PartialEq, Eq)]
pub enum Lookup {
    /// That change file, or its spine entry, isn't here yet: the version waits.
    Unknown,
    /// The chain holds no version of this key there.
    Absent,
    /// The chain's version of this key there has this hash.
    Present(Hash32),
}

/// What each device's chain holds, as far as this device has read it.
pub trait Chains {
    fn version_at(&self, device: super::ids::DeviceId, seq: u64, key: &str) -> Lookup;
}

/// SY9, SP11: whether a version's history can be trusted yet.
#[derive(Debug, PartialEq, Eq)]
pub enum HistoryCheck {
    Ok,
    /// An entry names a change this device can't check yet.
    Wait,
    /// An entry the named chain shows never existed: a forged or future ancestor.
    False,
}

/// SP11: every history entry must name, in that device's chain, a version of the
/// same key with that hash. So a device can't claim to descend from an edit it never
/// saw, and drop it.
pub fn check_history(v: &Version, chains: &impl Chains) -> HistoryCheck {
    let mut wait = false;
    for e in &v.history {
        match chains.version_at(e.device, e.seq, v.key.as_str()) {
            Lookup::Present(h) if h == e.hash => {}
            Lookup::Unknown => wait = true,
            _ => return HistoryCheck::False,
        }
    }
    if wait {
        HistoryCheck::Wait
    } else {
        HistoryCheck::Ok
    }
}

/// SP13: whether `v` lies outside a tombstone's causal frontier, so the deletion
/// never saw it. A note added apart to a deleted highlight, or anything changed in a
/// removed book, makes the deletion a conflict (SY13, SY18).
pub fn outside_frontier(tombstone: &Placed, v: &Placed) -> bool {
    let Content::Deleted { covers, .. } = &tombstone.version.content else {
        return false;
    };
    if v.version.device == tombstone.version.device {
        return v.seq > tombstone.seq;
    }
    match covers.iter().find(|e| e.device == v.version.device) {
        Some(e) => v.seq > e.seq,
        None => true,
    }
}

/// What a record's versions add up to.
#[derive(Debug, PartialEq)]
pub enum Outcome<'a> {
    /// One version stands; it may be a tombstone.
    Settled(&'a Placed),
    /// SY13, item 102: the reader decides between `all`. Until then `shown` stands in
    /// the library: a tombstone if one is among them, else the one changed last.
    Conflict {
        shown: &'a Placed,
        all: Vec<&'a Placed>,
    },
}

/// Whether concurrent versions of a kind go to the reader (SY13) or the newest wins
/// (SY12, SY14, SY15, SY16; item 105).
pub fn reader_decides(kind: Kind) -> bool {
    matches!(kind, Kind::Annotation | Kind::Note | Kind::Choice)
}

/// One record's versions, gathered from every device in any order.
#[derive(Debug, Default)]
pub struct Record {
    versions: BTreeMap<(super::ids::DeviceId, u64), Placed>,
}

impl Record {
    /// Adds a version once; a repeat delivery changes nothing.
    pub fn add(&mut self, v: Placed) -> bool {
        let at = (v.version.device, v.seq);
        if self.versions.contains_key(&at) {
            return false;
        }
        self.versions.insert(at, v);
        true
    }

    /// SP11: the versions no other descends from, leaving out those a choice marked
    /// wrong-clock or kept (SY10), in a fixed order.
    pub fn heads(&self, ignored: &HashSet<Entry>) -> Vec<&Placed> {
        let live: Vec<&Placed> = self
            .versions
            .values()
            .filter(|v| !ignored.contains(&v.id()))
            .collect();
        let mut heads: Vec<&Placed> = live
            .iter()
            .copied()
            .filter(|v| !live.iter().any(|o| o.id() != v.id() && o.descends_from(v)))
            .collect();
        heads.sort_by_key(|v| v.stamp());
        heads
    }

    /// What the record comes to (SY12, SY13; items 102, 105).
    pub fn outcome(&self, ignored: &HashSet<Entry>) -> Option<Outcome<'_>> {
        let heads = self.heads(ignored);
        let newest = *heads.last()?;
        if heads.len() == 1 || heads.iter().all(|h| h.same_content(newest)) {
            return Some(Outcome::Settled(newest));
        }
        if !reader_decides(newest.version.kind) {
            return Some(Outcome::Settled(newest));
        }
        let shown = heads
            .iter()
            .rev()
            .find(|h| h.is_deleted())
            .copied()
            .unwrap_or(newest);
        Some(Outcome::Conflict { shown, all: heads })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::ids::{Hash32, Id16};
    use crate::sync::record::{check_version, Checked};
    use serde_json::{json, Value};

    const NOTE: &str = "note:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
    const PLACE: &str = "place:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";

    fn dev(n: u8) -> Id16 {
        Id16([n; 16])
    }

    fn body(n: u8) -> Value {
        json!({ "hash": Hash32([n; 32]).to_string(), "size": 10 })
    }

    /// A version of `key` by device `d` at `seq`, descending from `parents`.
    fn make(key: &str, d: u8, seq: u64, ms: u64, parents: &[&Placed], content: Value) -> Placed {
        let mut history: BTreeMap<Id16, (u64, Hash32)> = BTreeMap::new();
        for p in parents {
            for e in &p.version.history {
                let cur = history.entry(e.device).or_insert((0, e.hash));
                if e.seq > cur.0 {
                    *cur = (e.seq, e.hash);
                }
            }
            let cur = history
                .entry(p.version.device)
                .or_insert((0, p.version.hash));
            if p.seq > cur.0 {
                *cur = (p.seq, p.version.hash);
            }
        }
        let history: Vec<Value> = history
            .iter()
            .map(|(dv, (s, h))| json!({ "d": dv.to_string(), "s": s, "h": h.to_string() }))
            .collect();
        let mut v = json!({
            "format": 1, "key": key, "device": dev(d).to_string(),
            "clock": [ms, 0], "history": history,
        });
        let (k, x) = content.as_object().unwrap().iter().next().unwrap();
        v[k] = x.clone();
        let Checked::Version(version) = check_version(v, dev(d)).unwrap() else {
            panic!()
        };
        Placed { version, seq }
    }

    fn edit(key: &str, d: u8, seq: u64, ms: u64, parents: &[&Placed], b: u8) -> Placed {
        make(key, d, seq, ms, parents, json!({ "body": body(b) }))
    }

    fn delete(key: &str, d: u8, seq: u64, ms: u64, parents: &[&Placed]) -> Placed {
        // SP13: one entry per device, the latest change of it the deleter had seen.
        let mut latest: BTreeMap<Id16, u64> = BTreeMap::new();
        for p in parents {
            let s = latest.entry(p.version.device).or_insert(0);
            *s = (*s).max(p.seq);
        }
        let covers: Vec<Value> = latest
            .iter()
            .map(|(d, s)| json!({ "d": d.to_string(), "s": s, "h": Hash32([0; 32]).to_string() }))
            .collect();
        make(
            key,
            d,
            seq,
            ms,
            parents,
            json!({ "deleted": { "covers": covers } }),
        )
    }

    fn outcome_of(vs: &[&Placed]) -> Option<String> {
        let mut r = Record::default();
        for v in vs {
            r.add((*v).clone());
        }
        r.outcome(&HashSet::new()).map(|o| match o {
            Outcome::Settled(p) => format!("settled {}:{}", p.version.device, p.seq),
            Outcome::Conflict { shown, all } => format!(
                "conflict shown {}:{} of {}",
                shown.version.device,
                shown.seq,
                all.len()
            ),
        })
    }

    #[test]
    fn a_descendant_replaces_its_ancestor() {
        let a = edit(NOTE, 1, 1, 100, &[], 1);
        let b = edit(NOTE, 2, 1, 200, &[&a], 2);
        assert!(b.descends_from(&a) && !a.descends_from(&b));
        assert_eq!(outcome_of(&[&a, &b]), outcome_of(&[&b]));
    }

    #[test]
    fn notes_changed_apart_go_to_the_reader() {
        let a = edit(NOTE, 1, 1, 100, &[], 1);
        let b = edit(NOTE, 2, 1, 300, &[&a], 2);
        let c = edit(NOTE, 3, 1, 200, &[&a], 3);
        let o = outcome_of(&[&a, &b, &c]).unwrap();
        // SY13: the one changed last shows until the reader chooses.
        assert_eq!(o, format!("conflict shown {}:1 of 2", dev(2)));
        // The reader's choice descends from both, so it settles on every device.
        let choice = edit(NOTE, 1, 2, 400, &[&b, &c], 3);
        assert_eq!(
            outcome_of(&[&a, &b, &c, &choice]).unwrap(),
            format!("settled {}:2", dev(1))
        );
    }

    #[test]
    fn equal_contents_are_not_a_conflict() {
        let a = edit(NOTE, 1, 1, 100, &[], 1);
        let b = edit(NOTE, 2, 1, 300, &[&a], 7);
        let c = edit(NOTE, 3, 1, 200, &[&a], 7);
        assert!(outcome_of(&[&a, &b, &c]).unwrap().starts_with("settled"));
    }

    #[test]
    fn the_newest_place_wins_even_earlier_in_the_book() {
        // SY12, item 105: places don't go to the reader.
        let a = edit(PLACE, 1, 1, 100, &[], 1);
        let b = edit(PLACE, 2, 1, 300, &[&a], 2);
        let c = edit(PLACE, 3, 1, 200, &[&a], 3);
        assert_eq!(
            outcome_of(&[&c, &a, &b]).unwrap(),
            format!("settled {}:1", dev(2))
        );
    }

    #[test]
    fn a_change_and_a_deletion_made_apart_stay_deleted_until_the_reader_chooses() {
        // Item 102: a conflict only when the change came after the last shared sync.
        let a = edit(NOTE, 1, 1, 100, &[], 1);
        let del = delete(NOTE, 1, 2, 200, &[&a]);
        let changed = edit(NOTE, 2, 1, 300, &[&a], 2);
        assert_eq!(
            outcome_of(&[&a, &del, &changed]).unwrap(),
            format!("conflict shown {}:2 of 2", dev(1))
        );
        // A device away for a week that didn't change the note brings nothing back (SY20).
        assert_eq!(
            outcome_of(&[&a, &del]).unwrap(),
            format!("settled {}:2", dev(1))
        );
    }

    #[test]
    fn a_note_added_apart_to_a_deleted_highlight_is_outside_its_frontier() {
        // SY13: the deleting device never saw this note, even its first.
        let hl = edit(
            "annotation:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6",
            1,
            1,
            100,
            &[],
            1,
        );
        let del = delete(
            "annotation:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6",
            1,
            2,
            200,
            &[&hl],
        );
        let seen = edit(NOTE, 2, 1, 150, &[], 2);
        let del2 = delete(
            "annotation:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6",
            1,
            2,
            200,
            &[&hl, &seen],
        );
        let unseen = edit(NOTE, 2, 2, 250, &[&seen], 3);
        assert!(outside_frontier(&del, &seen), "no entry for device 2");
        assert!(!outside_frontier(&del2, &seen));
        assert!(outside_frontier(&del2, &unseen));
    }

    #[test]
    fn a_choice_can_set_a_wrong_clock_version_aside() {
        // SY10: a version marked wrong-clock is left out when heads are ordered.
        let a = edit(PLACE, 1, 1, 100, &[], 1);
        let wrong = edit(PLACE, 2, 1, 9_999_999, &[&a], 2);
        let right = edit(PLACE, 3, 1, 200, &[&a], 3);
        let mut r = Record::default();
        for v in [&a, &wrong, &right] {
            r.add(v.clone());
        }
        let ignored: HashSet<Entry> = [wrong.id()].into();
        assert!(
            matches!(r.outcome(&ignored), Some(Outcome::Settled(p)) if p.version.device == dev(3))
        );
    }

    struct Fake(Vec<Placed>);
    impl Chains for Fake {
        fn version_at(&self, d: Id16, s: u64, key: &str) -> Lookup {
            match self.0.iter().find(|p| p.version.device == d && p.seq == s) {
                Some(p) if p.version.key.as_str() == key => Lookup::Present(p.version.hash),
                Some(_) => Lookup::Absent,
                None if s > 5 => Lookup::Unknown,
                None => Lookup::Absent,
            }
        }
    }

    #[test]
    fn history_entries_are_checked_against_the_named_chain() {
        let a = edit(NOTE, 1, 1, 100, &[], 1);
        let b = edit(NOTE, 2, 1, 200, &[&a], 2);
        assert_eq!(
            check_history(&b.version, &Fake(vec![a.clone()])),
            HistoryCheck::Ok
        );
        // SY9: an ancestor the named chain shows never existed is refused.
        assert_eq!(
            check_history(&b.version, &Fake(vec![])),
            HistoryCheck::False
        );
        let other_key = edit(PLACE, 1, 1, 100, &[], 1);
        assert_eq!(
            check_history(&b.version, &Fake(vec![other_key])),
            HistoryCheck::False
        );
        // One it can't check yet waits.
        let far = edit(NOTE, 1, 9, 100, &[], 1);
        let c = edit(NOTE, 2, 1, 200, &[&far], 2);
        assert_eq!(check_history(&c.version, &Fake(vec![])), HistoryCheck::Wait);
    }

    /// A small seeded generator, so the order-independence check is repeatable.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    #[test]
    fn the_outcome_is_the_same_in_any_order_and_repeats_change_nothing() {
        // Stage 1's merge property: commutative, idempotent, the same in any order.
        let mut rng = Rng(0x5eed_1a2b_3c4d);
        for round in 0..200 {
            let key = if round % 2 == 0 { NOTE } else { PLACE };
            let mut all: Vec<Placed> = Vec::new();
            let mut seq = [0u64; 3];
            for step in 0..8u64 {
                let d = rng.below(3) as u8;
                seq[d as usize] += 1;
                // Each device descends from a random subset of what exists.
                let parents: Vec<&Placed> = all.iter().filter(|_| rng.below(2) == 0).collect();
                let own: Vec<&Placed> = all
                    .iter()
                    .filter(|p| p.version.device == dev(d + 1))
                    .collect();
                let mut ps = parents;
                if let Some(last) = own.last() {
                    ps.push(last);
                }
                let v = if rng.below(5) == 0 {
                    delete(key, d + 1, seq[d as usize], 100 + step * 10, &ps)
                } else {
                    edit(
                        key,
                        d + 1,
                        seq[d as usize],
                        100 + rng.below(50),
                        &ps,
                        rng.below(4) as u8,
                    )
                };
                all.push(v);
            }
            let refs: Vec<&Placed> = all.iter().collect();
            let expected = outcome_of(&refs);
            for _ in 0..5 {
                let mut shuffled = refs.clone();
                for i in (1..shuffled.len()).rev() {
                    shuffled.swap(i, rng.below(i as u64 + 1) as usize);
                }
                let mut twice = shuffled.clone();
                twice.extend(shuffled.iter().take(3));
                assert_eq!(outcome_of(&shuffled), expected, "round {round}");
                assert_eq!(outcome_of(&twice), expected, "round {round}, repeated");
            }
        }
    }
}
