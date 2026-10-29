//! SP35, SY17, #122: bounded ages and permanent past-the-week marks in device files.
//!
//! Ages and marks stay in memory; persistence belongs to Stage 2. Nothing here
//! is wired into the sync pass or app, and moving marks into SP17's shards is the
//! caller's responsibility.

use super::device_file::DeviceFile;
use super::ids::{DeviceId, Hash32, Id16};
use super::record::Entry;
use super::retention::{Ages, Vouched, WEEK_MS};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

// SP35, #122: PROVISIONAL. An age uses 6 JSON values and a mark uses 4.
// At 10,000 each, 100,000 values leave nearly 150,000 of the device file's
// MAX_OVERFLOW_VALUES (250,000) for wrappers and the other seven lists, including
// relay's capped proofs. device_file still enforces the combined budget.
pub const MAX_AGES: usize = 10_000;
pub const MAX_MARKS: usize = 10_000;

/// SP35: own marks awaiting a view, and the devices that have reported marks.
#[derive(Debug, Default)]
pub struct Marks {
    /// First-seen vouched time, retained even after the content is removed.
    own: HashMap<Entry, u64>,
    /// SP17: prevents a retirement still in `ages` from republishing its mark.
    in_view: HashSet<Entry>,
    reported: HashMap<Entry, HashSet<DeviceId>>,
}

impl Marks {
    /// SP17, SP35: all own marks still to move, including those beyond the cap.
    pub fn pending(&self) -> impl Iterator<Item = &Entry> {
        self.own.keys()
    }

    /// SP35: call once these marks have been moved into a view's shards.
    pub fn moved_into_view(&mut self, versions: &[Entry]) {
        for v in versions {
            self.own.remove(v);
            self.in_view.insert(*v);
        }
    }

    /// SP35: a peer's mark stays known when a later device file omits it.
    pub fn marked_by(&self, version: &Entry, device: &DeviceId) -> bool {
        self.reported
            .get(version)
            .is_some_and(|devices| devices.contains(device))
    }
}

fn entry(v: &Entry) -> Value {
    json!({ "d": v.device.to_string(), "s": v.seq, "h": v.hash.to_string() })
}

fn parse_entry(v: &Value) -> Option<Entry> {
    Some(Entry {
        device: v.get("d")?.as_str().and_then(Id16::parse)?,
        seq: v.get("s")?.as_u64().filter(|s| *s > 0)?,
        hash: v.get("h")?.as_str().and_then(Hash32::parse)?,
    })
}

/// SP35: list items for `Fields::lists`. `retiring` names versions whose retired
/// content is still present; versions with no own age are omitted.
///
/// #122: oldest first (earliest own observation), with version ID breaking ties;
/// only the first MAX_AGES / MAX_MARKS entries are written. Marks beyond the cap
/// remain in memory. At WEEK_MS the week has passed, as in retention's SY17 check.
/// An untrusted clock writes both lists empty, matching SP16's existing shape,
/// and keeps the marks in memory for publication once the clock is clear.
pub fn lists(
    ages: &Ages,
    marks: &mut Marks,
    retiring: &[Entry],
    clock: &Vouched,
) -> BTreeMap<String, Vec<Value>> {
    let mut out = BTreeMap::from([("ages".into(), Vec::new()), ("marks".into(), Vec::new())]);
    let now = clock.now();
    let mut versions: Vec<_> = retiring
        .iter()
        .filter_map(|v| ages.own_age(v, now).map(|age| (*v, now - age)))
        .collect();
    versions.sort_unstable_by_key(|(v, seen)| (*seen, v.device, v.seq, v.hash));
    versions.dedup_by_key(|(v, _)| *v);
    // SP35: record every earned mark before capping either output list, and
    // whether or not the clock lets them be published: vouched-for time keeps
    // counting by the monotonic clock, so an own age stays valid, and a mark
    // earned meanwhile waits in memory even if the content goes before the clock
    // is clear.
    for (v, seen) in &versions {
        if now - seen >= WEEK_MS && !marks.in_view.contains(v) {
            marks.own.entry(*v).or_insert(*seen);
        }
    }
    if !clock.publishes_ages() {
        return out;
    }
    out.insert(
        "ages".into(),
        versions
            .iter()
            .take(MAX_AGES)
            .map(|(v, seen)| json!({ "v": entry(v), "ms": now - seen }))
            .collect(),
    );
    let mut pending: Vec<_> = marks.own.iter().collect();
    pending.sort_unstable_by_key(|(v, seen)| (**seen, v.device, v.seq, v.hash));
    out.insert(
        "marks".into(),
        pending
            .iter()
            .take(MAX_MARKS)
            .map(|(v, _)| entry(v))
            .collect(),
    );
    out
}

/// SP35: read an opened peer file and its complete `DeviceFile::lists` result.
/// `now` is the receiver's vouched-for time when the file arrived. The caller
/// accepts only a newer counter (SP29), so replay never resets the receipt time.
/// #122: inspect at most the cap in list order; malformed entries consume a slot
/// but vouch for nothing, like malformed frontier entries (SP16).
pub fn read(
    file: &DeviceFile,
    lists: &BTreeMap<String, Vec<Value>>,
    ages: &mut Ages,
    marks: &mut Marks,
    now: u64,
) {
    // #122: a peer's report is kept only for a retiring version this device
    // knows itself (it has its own age), so what a peer can make it hold is bounded
    // by this device's own state, however many files the peer sends. A report for
    // a version learned later comes again: a peer republishes its ages in every
    // file, and a mark that has moved on is in that peer's view (SP17).
    let known = |v: &Entry| ages.own_age(v, now).is_some();
    let mut reports = Vec::new();
    if let Some(items) = lists.get("ages") {
        for item in items.iter().take(MAX_AGES) {
            if let Some((v, ms)) = item
                .get("v")
                .and_then(parse_entry)
                .zip(item.get("ms").and_then(Value::as_u64))
                .filter(|(v, _)| known(v))
            {
                reports.push((v, ms));
            }
        }
    }
    let marked: Vec<Entry> = lists
        .get("marks")
        .into_iter()
        .flat_map(|items| items.iter().take(MAX_MARKS).filter_map(parse_entry))
        .filter(|v| known(v))
        .collect();
    for (v, ms) in reports {
        ages.report(v, file.device, ms, now);
    }
    for v in marked {
        marks.reported.entry(v).or_default().insert(file.device);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::Keypair;
    use crate::sync::device_file::{self, Fields};
    use crate::sync::json as sync_json;
    use crate::sync::retention::JUMP_MS;

    const LIB: Id16 = Id16([7; 16]);

    fn version(seq: u64) -> Entry {
        Entry {
            device: Id16([9; 16]),
            seq,
            hash: Hash32([4; 32]),
        }
    }

    fn clock(now: u64) -> Vouched {
        let mut clock = Vouched::new(0);
        clock.tick(0, 0);
        clock.tick(now, now);
        clock
    }

    fn opened(lists: BTreeMap<String, Vec<Value>>) -> (DeviceFile, BTreeMap<String, Vec<Value>>) {
        let key = Keypair::from_seed(&[1; 32]);
        let fields = Fields {
            lists,
            ..Fields::default()
        };
        let written = device_file::write(&key, &LIB, &Id16([2; 16]), 1, &fields).unwrap();
        let file = device_file::open(&written.bytes, &LIB, &key.device(), &key.public()).unwrap();
        let lists = file
            .lists(&|path| {
                written
                    .parts
                    .iter()
                    .find(|(p, _)| p == path)
                    .map(|(_, b)| b.clone())
            })
            .unwrap();
        (file, lists)
    }

    #[test]
    fn ages_and_marks_round_trip_through_a_device_file() {
        let (old, recent, unknown) = (version(1), version(2), version(3));
        let mut own = Ages::default();
        own.saw(old, 0);
        own.saw(recent, WEEK_MS);
        let expected = lists(
            &own,
            &mut Marks::default(),
            &[recent, unknown, old, old],
            &clock(WEEK_MS + 10),
        );
        assert_eq!(
            expected["ages"],
            vec![
                json!({"v": entry(&old), "ms": WEEK_MS + 10}),
                json!({"v": entry(&recent), "ms": 10})
            ]
        );
        assert_eq!(expected["marks"], vec![entry(&old)]);
        let (file, received) = opened(expected.clone());
        assert_eq!(received["ages"], expected["ages"]);
        assert_eq!(received["marks"], expected["marks"]);
        // #122: the reader keeps reports only for retiring versions it knows.
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        ages.saw(old, 90);
        ages.saw(recent, 90);
        read(&file, &received, &mut ages, &mut marks, 100);
        assert_eq!(
            ages.reported_age(&old, file.device, 100),
            Some(WEEK_MS + 10)
        );
        assert_eq!(ages.reported_age(&recent, file.device, 100), Some(10));
        assert_eq!(ages.reported_age(&unknown, file.device, 100), None);
        assert!(marks.marked_by(&old, &file.device));
        assert!(!marks.marked_by(&recent, &file.device));
        // Reading a report never changes this device's own age.
        assert_eq!(ages.own_age(&old, 100), Some(10));
    }

    #[test]
    fn a_reported_age_grows_by_the_time_since_the_file_arrived() {
        let v = version(1);
        let (file, received) = opened(BTreeMap::from([(
            "ages".into(),
            vec![json!({"v": entry(&v), "ms": 25})],
        )]));
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        let mut time = clock(300);
        ages.saw(v, 0);
        read(&file, &received, &mut ages, &mut marks, time.now());
        assert_eq!(ages.reported_age(&v, file.device, time.now()), Some(25));
        time.tick(350, 350);
        assert_eq!(ages.reported_age(&v, file.device, time.now()), Some(75));
    }

    #[test]
    fn a_version_is_marked_once_its_own_age_passes_a_week_and_stays_marked() {
        let v = version(1);
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        ages.saw(v, 0);
        ages.report(v, Id16([3; 16]), 2 * WEEK_MS, 0);
        assert!(lists(&ages, &mut marks, &[v], &clock(WEEK_MS - 1))["marks"].is_empty());
        assert_eq!(
            lists(&ages, &mut marks, &[v], &clock(WEEK_MS))["marks"],
            vec![entry(&v)]
        );
        assert_eq!(
            lists(&ages, &mut marks, &[v, v], &clock(WEEK_MS + 1))["marks"],
            vec![entry(&v)]
        );
        // SP35: removing the retired content stops ages, but never retracts marks.
        let after = lists(&Ages::default(), &mut marks, &[], &clock(2 * WEEK_MS));
        assert!(after["ages"].is_empty());
        assert_eq!(after["marks"], vec![entry(&v)]);
        let (file, received) = opened(after);
        let (mut peer_ages, mut peer_marks) = (Ages::default(), Marks::default());
        peer_ages.saw(v, 0);
        read(&file, &received, &mut peer_ages, &mut peer_marks, 0);
        read(&file, &BTreeMap::new(), &mut peer_ages, &mut peer_marks, 1);
        assert!(peer_marks.marked_by(&v, &file.device));
        assert!(!peer_marks.marked_by(&v, &Id16([3; 16])));
    }

    #[test]
    fn a_marked_version_moved_into_a_view_leaves_the_device_files_marks() {
        let (a, b) = (version(1), version(2));
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        ages.saw(a, 0);
        ages.saw(b, 1);
        lists(&ages, &mut marks, &[a, b], &clock(WEEK_MS + 1));
        assert_eq!(marks.pending().count(), 2);
        marks.moved_into_view(&[a]);
        marks.moved_into_view(&[a]);
        let after = lists(&ages, &mut marks, &[a, b], &clock(2 * WEEK_MS));
        assert_eq!(after["ages"].len(), 2);
        assert_eq!(after["marks"], vec![entry(&b)]);
        assert_eq!(marks.pending().copied().collect::<Vec<_>>(), vec![b]);
    }

    #[test]
    fn no_ages_or_marks_are_published_while_the_clock_is_ahead_or_has_jumped() {
        for jumped in [false, true] {
            let v = version(1);
            let (mut ages, mut marks) = (Ages::default(), Marks::default());
            ages.saw(v, 0);
            let mut time = clock(WEEK_MS);
            assert_eq!(
                lists(&ages, &mut marks, &[v], &time)["marks"],
                vec![entry(&v)]
            );
            if jumped {
                time.tick(WEEK_MS + 1, WEEK_MS + JUMP_MS + 2);
            } else {
                time.untrusted_wall = true;
            }
            let hidden = lists(&ages, &mut marks, &[v], &time);
            let (_, received) = opened(hidden);
            assert!(received["ages"].is_empty());
            assert!(received["marks"].is_empty());
            assert_eq!(marks.pending().count(), 1);
            time.clear();
            let (_, received) = opened(lists(&ages, &mut marks, &[v], &time));
            assert_eq!(
                received["ages"],
                vec![json!({"v": entry(&v), "ms": time.now()})]
            );
            assert_eq!(received["marks"], vec![entry(&v)]);
        }
    }

    #[test]
    fn a_mark_earned_while_the_clock_is_untrusted_survives_the_content_going() {
        // SP35: the week passes while the clock is marked ahead; the mark is earned
        // and waits, and is published once the clock is clear, though the retired
        // content went in between.
        let v = version(1);
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        ages.saw(v, 0);
        let mut time = clock(WEEK_MS);
        time.untrusted_wall = true;
        let hidden = lists(&ages, &mut marks, &[v], &time);
        assert!(hidden["marks"].is_empty());
        assert_eq!(marks.pending().collect::<Vec<_>>(), vec![&v]);
        time.clear();
        let after = lists(&ages, &mut marks, &[], &time);
        assert!(after["ages"].is_empty());
        assert_eq!(after["marks"], vec![entry(&v)]);
    }

    #[test]
    fn ages_and_marks_past_their_caps_are_cut_when_written_and_ignored_when_read() {
        let count = MAX_AGES.max(MAX_MARKS) + 2;
        let versions: Vec<_> = (1..=count as u64).map(version).collect();
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        // SP35: the largest sequence was seen first; input/ID order isn't age order.
        for v in &versions {
            ages.saw(*v, count as u64 - v.seq);
        }
        let time = clock(WEEK_MS + count as u64);
        let capped = lists(&ages, &mut marks, &versions, &time);
        assert_eq!(capped["ages"].len(), MAX_AGES);
        assert_eq!(capped["marks"].len(), MAX_MARKS);
        for (i, item) in capped["ages"].iter().enumerate() {
            assert_eq!(item["v"], entry(&versions[count - 1 - i]));
        }
        for (i, item) in capped["marks"].iter().enumerate() {
            assert_eq!(*item, entry(&versions[count - 1 - i]));
        }
        assert_eq!(marks.pending().count(), count);
        let budget: u64 = capped
            .values()
            .map(|items| sync_json::count_values(&Value::Array(items.clone())))
            .sum();
        assert_eq!(budget, (6 * MAX_AGES + 4 * MAX_MARKS) as u64);
        assert!(budget < device_file::MAX_OVERFLOW_VALUES / 2);
        let (_, received) = opened(capped.clone());
        assert_eq!(received["ages"], capped["ages"]);
        assert_eq!(received["marks"], capped["marks"]);
        let moved: Vec<_> = versions.iter().rev().take(MAX_MARKS).copied().collect();
        marks.moved_into_view(&moved);
        assert_eq!(
            lists(&ages, &mut marks, &versions, &time)["marks"],
            vec![entry(&version(2)), entry(&version(1))]
        );

        // #122: malformed values count toward the read cap; the tail is ignored.
        let mut incoming = BTreeMap::from([
            (
                "ages".into(),
                versions
                    .iter()
                    .map(|v| json!({"v": entry(v), "ms": v.seq}))
                    .collect::<Vec<_>>(),
            ),
            (
                "marks".into(),
                versions.iter().map(entry).collect::<Vec<_>>(),
            ),
        ]);
        incoming.get_mut("ages").unwrap()[0] = Value::Null;
        incoming.get_mut("marks").unwrap()[0] = Value::Null;
        let (file, received) = opened(incoming);
        let (mut peer_ages, mut peer_marks) = (Ages::default(), Marks::default());
        for v in &versions {
            peer_ages.saw(*v, 100);
        }
        read(&file, &received, &mut peer_ages, &mut peer_marks, 100);
        for (i, v) in versions.iter().enumerate() {
            assert_eq!(
                peer_ages.reported_age(v, file.device, 100),
                (i > 0 && i < MAX_AGES).then_some(v.seq)
            );
            assert_eq!(
                peer_marks.marked_by(v, &file.device),
                i > 0 && i < MAX_MARKS
            );
        }
    }

    #[test]
    fn a_peers_reports_of_versions_this_device_doesnt_know_arent_kept() {
        // #122: a peer sending file after file of made-up version IDs can't grow
        // what this device holds; only versions it knows itself are kept.
        let known = version(1);
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        ages.saw(known, 0);
        for round in 0..3u64 {
            let made_up: Vec<Entry> = (0..50).map(|i| version(1_000 + round * 50 + i)).collect();
            let (file, received) = opened(BTreeMap::from([
                (
                    "ages".into(),
                    made_up
                        .iter()
                        .chain([&known])
                        .map(|v| json!({"v": entry(v), "ms": 5}))
                        .collect(),
                ),
                (
                    "marks".into(),
                    made_up.iter().chain([&known]).map(entry).collect(),
                ),
            ]));
            read(&file, &received, &mut ages, &mut marks, 10);
            assert_eq!(ages.reported_age(&known, file.device, 10), Some(5));
            assert!(made_up
                .iter()
                .all(|v| ages.reported_age(v, file.device, 10).is_none()));
            assert_eq!(marks.reported.len(), 1);
        }
    }

    #[test]
    fn a_malformed_age_or_mark_is_left_out() {
        let good = version(1);
        let mut bad_ids = vec![
            Value::Null,
            json!("not an ID"),
            json!({}),
            entry(&version(0)),
        ];
        for (field, value) in [
            ("d", json!("bad")),
            ("h", json!("bad")),
            ("s", json!(-1)),
            ("s", json!("1")),
        ] {
            let mut id = entry(&version(2));
            id[field] = value;
            bad_ids.push(id);
        }
        let mut bad_ages: Vec<_> = bad_ids.iter().map(|v| json!({"v": v, "ms": 10})).collect();
        bad_ages.extend([
            Value::Null,
            json!({"v": entry(&version(3))}),
            json!({"ms": 10}),
        ]);
        for ms in [json!(-1), json!("10"), Value::Null] {
            bad_ages.push(json!({"v": entry(&version(3)), "ms": ms}));
        }
        bad_ages.push(json!({"v": entry(&good), "ms": 0}));
        bad_ids.push(entry(&good));
        let (file, received) = opened(BTreeMap::from([
            ("ages".into(), bad_ages),
            ("marks".into(), bad_ids),
        ]));
        let (mut ages, mut marks) = (Ages::default(), Marks::default());
        for seq in [0, 1, 2, 3] {
            ages.saw(version(seq), 50);
        }
        read(&file, &received, &mut ages, &mut marks, 50);
        assert_eq!(ages.reported_age(&good, file.device, 50), Some(0));
        assert!(marks.marked_by(&good, &file.device));
        for seq in [0, 2, 3] {
            assert_eq!(ages.reported_age(&version(seq), file.device, 50), None);
            assert!(!marks.marked_by(&version(seq), &file.device));
        }
        assert_eq!(marks.reported.len(), 1);
    }
}
