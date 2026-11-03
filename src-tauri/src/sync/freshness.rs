//! SP31 (c), SP32, item 113 (a), #148: pure membership freshness decisions.
//!
//! The caller owns hearing history; persistence, waiting versions and the reader's
//! question belong to later work. All elapsed time is SY17's vouched-for time.

use super::ids::{DeviceId, Hash32};
use super::membership::State;
use super::record::{Content, Kind, Version};
use super::retention::{Vouched, WEEK_MS};
use super::trust::Record;
use std::collections::{BTreeMap, BTreeSet};

/// SP31 (c): trusted enrolled devices whose accepted head doesn't include the removal.
/// "Includes" follows the membership graph's links: predecessors and what a
/// `confirm` names (SP20 (4a)), as membership itself does.
/// The caller includes every enrolled device, with `None` when no file is held.
pub fn not_caught_up(
    records: &BTreeMap<Hash32, Record>,
    removal: &Hash32,
    heads: &BTreeMap<DeviceId, Option<Hash32>>,
    trusted: impl Fn(&DeviceId) -> bool,
) -> Vec<DeviceId> {
    heads
        .iter()
        .filter(|(device, _)| trusted(device))
        .filter(|(_, head)| {
            let mut pending: Vec<_> = head.iter().copied().collect();
            let mut seen = BTreeSet::new();
            while let Some(hash) = pending.pop() {
                if !seen.insert(hash) {
                    continue;
                }
                // SP31: a head not held here proves no inclusion.
                if let Some(record) = records.get(&hash) {
                    if hash == *removal {
                        return false;
                    }
                    // SP20 (4a): a `confirm` naming the removal counts as made
                    // on its own `prev`, even from another branch.
                    pending.extend(super::membership::links(record));
                }
            }
            true
        })
        .map(|(device, _)| *device)
        .collect()
}

/// SP32, SP27: exclude the writer's whole enrollment subtree, across rotations.
pub fn witnesses(state: &State, this: DeviceId, writer: DeviceId) -> BTreeSet<DeviceId> {
    let lineage = state.members.get(&writer).map_or(writer, |m| m.lineage);
    let mut excluded = BTreeSet::from([lineage]);
    loop {
        let before = excluded.len();
        // SP20: closed and removed identities still carry enrollment ancestry.
        for member in state.members.values() {
            if member.enroller.is_some_and(|d| excluded.contains(&d)) {
                excluded.insert(member.lineage);
            }
        }
        if excluded.len() == before {
            break;
        }
    }
    state
        .members
        .iter()
        .filter(|(device, member)| {
            **device != this
                && state.trusted(device).is_some()
                && !excluded.contains(&member.lineage)
        })
        .map(|(device, _)| *device)
        .collect()
}

/// SP32: caller-owned history of accepted rising counters and the reader's Apply.
#[derive(Debug, Default)]
pub struct Heard {
    counters: BTreeMap<DeviceId, (u64, u64)>,
    applied: BTreeMap<DeviceId, u64>,
}

impl Heard {
    /// SP16, SP32: a repeated or lower counter isn't hearing from a device.
    pub fn saw_counter(&mut self, device: DeviceId, counter: u64, clock: &Vouched) {
        let previous = self.counters.get(&device).map_or(0, |(n, _)| *n);
        if counter > previous {
            self.counters.insert(device, (counter, clock.now()));
        }
    }

    /// SP32: Apply is the reader's own assurance, for this writer only.
    pub fn applied(&mut self, writer: DeviceId, clock: &Vouched) {
        self.applied.insert(writer, clock.now());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    InTouch,
    NoWitness,
    OutOfTouch { witness: DeviceId, since_ms: u64 },
}

/// SP32: a week (PROVISIONAL) without a witness or this writer's Apply waits.
pub fn out_of_touch(
    state: &State,
    this: DeviceId,
    writer: DeviceId,
    heard: &Heard,
    clock: &Vouched,
) -> Touch {
    let witnesses = witnesses(state, this, writer);
    let Some(mut witness) = witnesses.first().copied() else {
        return Touch::NoWitness;
    };
    let mut latest = 0;
    // SP32: never heard means time zero. PROVISIONAL: ties name the smallest ID.
    for device in witnesses {
        let at = heard.counters.get(&device).map_or(0, |(_, at)| *at);
        if at > latest {
            witness = device;
            latest = at;
        }
    }
    latest = latest.max(heard.applied.get(&writer).copied().unwrap_or(0));
    let since_ms = clock.now().saturating_sub(latest);
    if since_ms >= WEEK_MS {
        Touch::OutOfTouch { witness, since_ms }
    } else {
        Touch::InTouch
    }
}

/// SP32: deletions of every kind wait; only these existing bodies' edits wait.
pub fn waits_when_out_of_touch(version: &Version, replaces_existing: bool) -> bool {
    if matches!(version.content, Content::Deleted { .. }) {
        return true;
    }
    match version.kind {
        // SP32: book-file replacements and edits of highlights and notes.
        Kind::BookFile | Kind::Annotation | Kind::Note => replaces_existing,
        // SP32: places, settings, per-book settings and reading states proceed.
        // Book metadata and protocol choices aren't the held body edits either.
        Kind::Place
        | Kind::Setting
        | Kind::BookSetting
        | Kind::BookOpened
        | Kind::BookFinished
        | Kind::Book
        | Kind::Choice => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::clock::Hlc;
    use crate::sync::crypto::Keypair;
    use crate::sync::envelope::Signer;
    use crate::sync::ids::{b64url_encode, Id16};
    use crate::sync::membership::Member;
    use crate::sync::record::Key;
    use crate::sync::trust;
    use serde_json::{json, Value};

    fn d(n: u8) -> DeviceId {
        Id16([n; 16])
    }

    fn h(n: u8) -> Hash32 {
        Hash32([n; 32])
    }

    fn member(state: &mut State, device: u8, enroller: Option<u8>, lineage: u8) {
        state.members.insert(
            d(device),
            Member {
                device: d(device),
                key: [device; 32],
                label: device.to_string(),
                record: h(device),
                enroller: enroller.map(d),
                lineage: d(lineage),
            },
        );
    }

    fn family() -> State {
        let mut state = State::default();
        member(&mut state, 1, None, 1); // A
        member(&mut state, 2, Some(1), 2); // A enrolls B
        member(&mut state, 3, Some(2), 3); // B enrolls C
        member(&mut state, 4, Some(1), 4); // A enrolls D
        state
    }

    fn at(ms: u64) -> Vouched {
        let mut clock = Vouched::new(0);
        clock.tick(0, 0);
        clock.tick(ms, ms);
        clock
    }

    fn add(records: &mut BTreeMap<Hash32, Record>, kind: &str, mut fields: Value) -> Hash32 {
        let device = Keypair::from_seed(&[1; 32]);
        let code = Keypair::from_seed(&[2; 32]);
        fields["code"] = json!({
            "nonce": b64url_encode(&[0; 16]), "codeKey": b64url_encode(&code.public())
        });
        if kind == "genesis" {
            fields["device"] = json!(device.device().to_string());
            fields["key"] = json!(b64url_encode(&device.public()));
            fields["label"] = json!("A");
        }
        let library = Id16([9; 16]);
        let (bytes, hash) = trust::sign(
            kind,
            &library,
            fields,
            &[
                (&device, Signer::Device(device.device()), 0),
                (&code, Signer::Code, 0),
            ],
        );
        records.insert(hash, trust::open(&bytes, &library, &hash).unwrap());
        hash
    }

    #[test]
    fn a_device_whose_head_doesnt_include_the_removal_hasnt_caught_up() {
        let mut records = BTreeMap::new();
        let old = add(&mut records, "genesis", json!({ "clock": [0, 0] }));
        let removal = add(
            &mut records,
            "remove",
            json!({ "prev": old.to_string(), "clock": [1, 0], "removes": [{
                "device": d(9).to_string(), "cutoff": null, "view": null, "cascade": false
            }] }),
        );
        let descendant = add(
            &mut records,
            "confirm",
            json!({ "prev": removal.to_string(), "confirms": removal.to_string() }),
        );
        let merge = add(
            &mut records,
            "merge",
            json!({ "prevs": [old.to_string(), removal.to_string()], "clock": [2, 0], "keeps": [] }),
        );
        let heads = BTreeMap::from([
            (d(1), Some(removal)),
            (d(2), Some(descendant)),
            (d(3), Some(old)),
            (d(4), Some(merge)),
            (d(5), None),
            (d(6), Some(h(99))), // Unknown head.
            (d(9), None),        // Removed device.
        ]);
        assert_eq!(
            not_caught_up(&records, &removal, &heads, |device| *device != d(9)),
            vec![d(3), d(5), d(6)]
        );
        // SP31: even a head equal to the removal must be held here.
        records.remove(&removal);
        assert_eq!(
            not_caught_up(&records, &removal, &heads, |device| *device != d(9)),
            vec![d(1), d(2), d(3), d(4), d(5), d(6)]
        );
    }

    #[test]
    fn a_confirm_on_another_branch_includes_the_removal() {
        // SP20 (4a): the removal is on one branch; a `confirm` on another names
        // it, and a head descending from that confirm has caught up.
        let mut records = BTreeMap::new();
        let old = add(&mut records, "genesis", json!({ "clock": [0, 0] }));
        let removal = add(
            &mut records,
            "remove",
            json!({ "prev": old.to_string(), "clock": [1, 0], "removes": [{
                "device": d(9).to_string(), "cutoff": null, "view": null, "cascade": false
            }] }),
        );
        // Another branch from the same head: a removal of a different device.
        let other = add(
            &mut records,
            "remove",
            json!({ "prev": old.to_string(), "clock": [1, 1], "removes": [{
                "device": d(8).to_string(), "cutoff": null, "view": null, "cascade": false
            }] }),
        );
        let confirm = add(
            &mut records,
            "confirm",
            json!({ "prev": other.to_string(), "confirms": removal.to_string() }),
        );
        let heads = BTreeMap::from([(d(1), Some(confirm)), (d(2), Some(other))]);
        assert_eq!(
            not_caught_up(&records, &removal, &heads, |_| true),
            vec![d(2)]
        );
    }

    #[test]
    fn witnesses_leave_out_the_writer_and_everything_it_enrolled() {
        let mut state = family();
        assert_eq!(witnesses(&state, d(1), d(2)), BTreeSet::from([d(4)]));
        member(&mut state, 5, Some(2), 3); // C rotates.
        state.rotated.insert(d(3), (d(5), None));
        member(&mut state, 6, Some(3), 6); // C's lineage enrolls another.
        member(&mut state, 7, Some(1), 2); // B rotates.
        state.rotated.insert(d(2), (d(7), None));
        member(&mut state, 8, Some(2), 8); // B's lineage enrolls another.
        assert_eq!(witnesses(&state, d(1), d(7)), BTreeSet::from([d(4)]));
        assert_eq!(witnesses(&state, d(1), d(2)), BTreeSet::from([d(4)]));
        state.removed.insert(d(4));
        assert!(witnesses(&state, d(1), d(7)).is_empty());
    }

    #[test]
    fn a_great_grandchild_listed_first_is_still_left_out() {
        // SP32: everything the writer enrolled "directly or through others",
        // whatever order the members happen to be listed in. Device 10 enrolls
        // 11, 11 enrolls 12, 12 enrolls 13; the deeper ones sort first.
        let mut state = State::default();
        member(&mut state, 20, None, 20); // this device
        member(&mut state, 21, Some(20), 21); // an independent witness
        member(&mut state, 10, Some(20), 10); // the writer
        member(&mut state, 3, Some(10), 3);
        member(&mut state, 2, Some(3), 2);
        member(&mut state, 1, Some(2), 1);
        assert_eq!(witnesses(&state, d(20), d(10)), BTreeSet::from([d(21)]));
    }

    #[test]
    fn two_devices_have_no_witness() {
        let mut state = State::default();
        member(&mut state, 1, None, 1);
        member(&mut state, 2, Some(1), 2);
        assert!(witnesses(&state, d(1), d(2)).is_empty());
        assert_eq!(
            out_of_touch(&state, d(1), d(2), &Heard::default(), &at(WEEK_MS)),
            Touch::NoWitness
        );
    }

    #[test]
    fn a_writer_that_enrolled_every_other_device_has_no_witness() {
        let state = family();
        assert!(witnesses(&state, d(4), d(1)).is_empty());
        assert_eq!(
            out_of_touch(&state, d(4), d(1), &Heard::default(), &at(WEEK_MS)),
            Touch::NoWitness
        );
    }

    #[test]
    fn a_week_without_a_rising_counter_from_a_witness_is_out_of_touch() {
        let mut state = family();
        let mut heard = Heard::default();
        let touch =
            |state: &State, heard: &Heard, ms| out_of_touch(state, d(1), d(2), heard, &at(ms));
        assert_eq!(touch(&state, &heard, WEEK_MS - 1), Touch::InTouch);
        assert_eq!(
            touch(&state, &heard, WEEK_MS),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS
            }
        );
        heard.saw_counter(d(4), 2, &at(100));
        heard.saw_counter(d(4), 2, &at(WEEK_MS));
        heard.saw_counter(d(4), 1, &at(WEEK_MS));
        heard.saw_counter(d(2), 50, &at(WEEK_MS + 100)); // Writer isn't a witness.
        assert_eq!(touch(&state, &heard, WEEK_MS + 99), Touch::InTouch);
        assert_eq!(
            touch(&state, &heard, WEEK_MS + 100),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS
            }
        );
        heard.applied(d(3), &at(WEEK_MS + 100)); // Another writer's Apply doesn't count.
        assert_eq!(
            touch(&state, &heard, WEEK_MS + 123),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS + 23
            }
        );
        heard.applied(d(2), &at(WEEK_MS + 123));
        assert_eq!(touch(&state, &heard, 2 * WEEK_MS + 122), Touch::InTouch);
        assert_eq!(
            touch(&state, &heard, 2 * WEEK_MS + 123),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS
            }
        );
        heard.saw_counter(d(4), 3, &at(2 * WEEK_MS + 123));
        assert_eq!(touch(&state, &heard, 2 * WEEK_MS + 123), Touch::InTouch);
        member(&mut state, 5, Some(1), 5); // A second independent witness.
        heard.saw_counter(d(5), 1, &at(2 * WEEK_MS + 200));
        assert_eq!(
            touch(&state, &heard, 3 * WEEK_MS + 200),
            Touch::OutOfTouch {
                witness: d(5),
                since_ms: WEEK_MS
            }
        );
    }

    #[test]
    fn what_waits_when_out_of_touch() {
        let id = "12345678-1234-4123-8123-123456789abc";
        for (key, waits) in [
            (format!("place:{id}"), false),
            (format!("annotation:{id}"), true),
            (format!("note:{id}"), true),
            ("setting:font".into(), false),
            (format!("bookSetting:{id}:font"), false),
            (format!("book:{id}"), false),
            (format!("bookFile:{id}"), true),
            (format!("bookRead:{id}:opened"), false),
            (format!("bookRead:{id}:finished"), false),
            (format!("choice:obj.{}", h(1)), false),
        ] {
            let (key, kind) = Key::parse(&key).unwrap();
            let mut version = Version {
                key,
                kind,
                device: d(1),
                clock: Hlc(0, 0),
                history: Vec::new(),
                content: if kind == Kind::Choice {
                    Content::Inline(json!({ "choice": "try-again", "clears": [] }))
                } else {
                    Content::Body {
                        hash: h(1),
                        size: 1,
                    }
                },
                hash: h(2),
                raw: Value::Null,
            };
            assert!(
                !waits_when_out_of_touch(&version, false),
                "{kind:?} addition"
            );
            assert_eq!(
                waits_when_out_of_touch(&version, true),
                waits,
                "{kind:?} edit"
            );
            version.content = Content::Deleted {
                covers: Vec::new(),
                identity: None,
            };
            for replaces_existing in [false, true] {
                assert!(
                    waits_when_out_of_touch(&version, replaces_existing),
                    "{kind:?} deletion"
                );
            }
        }
    }
}
