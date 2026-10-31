//! SP36, SY11: decide whether stored content may be removed, from verified facts.
//!
//! SY21's spirit: nothing here deletes, fetches or persists anything; only a
//! decision is returned. Stage 2 supplies a complete snapshot of checked signed
//! files, resolved frontiers, names and reader choices, and advances the ledger
//! before calling. Times are this device's vouched-for time, never remote clocks.
//! Choosing which view generations are kept (SP36's closing sentence) comes with
//! views in #150; their `week` entries are inputs here, empty until views exist.

use super::hold::{ChangeId, Kind, Ledger};
use super::ids::{DeviceId, Hash32};
use super::record::Entry;
use super::retention::{Ages, Vouched, WEEK_MS};
use std::collections::{BTreeMap, HashMap};

/// SP9: the kind is part of an object's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name {
    pub kind: Kind,
    pub name: Hash32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    /// SP17: the version that stopped naming this content. For a view's claim,
    /// construct this candidate only when `KnownWeeks::may_release` proves the
    /// object for that retirement (#176); a claim alone only keeps content.
    Retired(Entry),
    /// SP36 line 5, including expired, never-named reservations (SY11).
    Unnamed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Object {
    pub name: Name,
    pub content: Content,
}

#[derive(Clone, Copy, Debug)]
pub struct Reading {
    pub through_newest_own: bool,
    pub all_listed_after_own: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Retirement {
    /// SP14: the change's hash, distinct from the retiring version's hash.
    pub change: ChangeId,
    /// SP29: checked with `hold::included_by_all` or `frontier::accepted_by_all`,
    /// including own/inherited entries and any SP27 succession, over the devices
    /// SP36 counts (SP25: not keys without a first device file).
    pub included_by_all: bool,
    /// True when there was no hold, or the last hold has ended under SP34.
    pub last_hold_ended: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Week {
    pub name: Name,
    pub retiring: Entry,
}

#[derive(Clone, Copy, Debug)]
pub struct LaterFile {
    /// The caller verified this file was published after the object sighting;
    /// receiving an old file late is insufficient (SP36 line 5).
    pub published_after_sighting: bool,
    pub first_seen: u64,
    /// Read through this file's own, finding no reservation, hold or version
    /// naming the object. This is stronger than the global reading gate.
    pub read_through_own_without_name: bool,
}

#[derive(Clone, Debug)]
pub struct Unnamed {
    pub first_seen: u64,
    pub later_files: BTreeMap<DeviceId, LaterFile>,
}

/// Caller-verified facts, all for the same library and membership snapshot.
/// Missing per-device evidence blocks removal; floors alone are not reading or
/// acknowledgement. Names include every live version, not just merge winners.
pub struct Facts<'a> {
    pub this: DeviceId,
    /// The complete current membership, including `this`.
    pub enrolled: &'a [DeviceId],
    /// SP25: enrolled keys that haven't published their first device file (an
    /// applicant that cancelled or vanished). Until it does, such a key counts in
    /// none of SP36's lines.
    pub without_device_file: &'a [DeviceId],
    pub reading: &'a BTreeMap<DeviceId, Reading>,
    pub all_fetchable_late_read: bool,
    /// SP26/SP12: true until the unfetched over-cap tail has a final discard-all
    /// accepted by all. Merely offering/applying a choice does not release it.
    pub undiscarded_unfetched_late_over_cap: bool,
    pub live_names: &'a [Name],
    pub waiting_late_names: &'a [Name],
    pub weeks: &'a [Week],
    /// Already advanced with current time, frontiers and settled choices.
    pub ledger: &'a Ledger,
    pub ages: &'a Ages,
    pub clock: &'a Vouched,
    /// Evidence for the candidate object's retirements, including its `week`
    /// entries. `last_hold_ended` concerns this object, not other content that
    /// the same version retired.
    pub retirements: &'a HashMap<Entry, Retirement>,
    /// SP17, SP36: every version this device knows to have retired each object.
    /// Content is addressed by its hash, so two records can share it and both
    /// retire it; it may go only once every one of those retirements allows it.
    pub retired_by: &'a BTreeMap<Name, Vec<Entry>>,
    /// SY11: silent 30 days or writing 7 days without taking in others' changes.
    pub holding_back: &'a [DeviceId],
    pub unnamed: &'a BTreeMap<Name, Unnamed>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    Line1(Line1),
    Line2(Line2),
    Line3(Line3),
    Line4(DeviceId),
    Line5(Line5),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line1 {
    UnreadOwn(DeviceId),
    UnreadListedChanges(DeviceId),
    UnreadLateChanges,
    UnfetchedLateOverCap,
    NamedByLiveVersion,
    NamedByWaitingLateChange,
    NamedByWeek(Entry),
    OpenHoldOrReservation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line2 {
    /// SY17 also forbids removing unnamed objects while this clock is untrusted.
    UntrustedClock,
    WeekNotPassed(Entry),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line3 {
    UnknownRetirement(Entry),
    RetirementNotIncluded(ChangeId),
    LastHoldOpen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line5 {
    UnknownSighting,
    WeekNotPassed,
    MissingLaterFile(DeviceId),
    FileNotPublishedLater(DeviceId),
    FileSeenTooSoon(DeviceId),
    FileNotReadWithoutName(DeviceId),
}

/// SP25, SP36: the enrolled devices SP36's lines count.
fn counted(facts: &Facts<'_>) -> Vec<DeviceId> {
    facts
        .enrolled
        .iter()
        .filter(|d| **d == facts.this || !facts.without_device_file.contains(d))
        .copied()
        .collect()
}

fn week_passed(retiring: &Entry, facts: &Facts<'_>) -> bool {
    facts
        .ages
        .week_passed_everywhere(retiring, facts.this, &counted(facts), facts.clock.now())
}

fn retirement_finished(retiring: &Entry, facts: &Facts<'_>) -> Result<(), Line3> {
    let retirement = facts
        .retirements
        .get(retiring)
        .ok_or(Line3::UnknownRetirement(*retiring))?;
    if !retirement.included_by_all {
        return Err(Line3::RetirementNotIncluded(retirement.change));
    }
    if !retirement.last_hold_ended {
        return Err(Line3::LastHoldOpen);
    }
    Ok(())
}

/// SP36: the first failing line explains why the object must stay. `Ok(())`
/// authorizes nothing beyond this snapshot; the caller performs no deletion here.
pub fn may_remove(object: &Object, facts: &Facts<'_>) -> Result<(), Blocked> {
    // SP36 line 1, SP18: read own and every listed tail, including removed peers.
    // Every enrolled identity's listed change files are read, even one with no
    // device file yet (SP25 exempts it from the lines that need one, its `own`
    // among them): a key that published a change, a hold say, and stopped before
    // its first device file can still name content. A vanished applicant has none
    // listed, so it holds nothing back.
    let counting = counted(facts);
    for device in facts.enrolled {
        let reading = facts.reading.get(device);
        if counting.contains(device) && !reading.is_some_and(|r| r.through_newest_own) {
            return Err(Blocked::Line1(Line1::UnreadOwn(*device)));
        }
        if !reading.is_some_and(|r| r.all_listed_after_own) {
            return Err(Blocked::Line1(Line1::UnreadListedChanges(*device)));
        }
    }
    if !facts.all_fetchable_late_read {
        return Err(Blocked::Line1(Line1::UnreadLateChanges));
    }
    // SP36 line 1: an unknown over-cap tail can name ANY object.
    if facts.undiscarded_unfetched_late_over_cap {
        return Err(Blocked::Line1(Line1::UnfetchedLateOverCap));
    }
    if facts.live_names.contains(&object.name) {
        return Err(Blocked::Line1(Line1::NamedByLiveVersion));
    }
    if facts.waiting_late_names.contains(&object.name) {
        return Err(Blocked::Line1(Line1::NamedByWaitingLateChange));
    }
    // SP36 line 1, SP17: each week stops counting only for its own retirement,
    // once BOTH lines 2 and 3 hold, even in a view kept for good.
    for week in facts.weeks.iter().filter(|w| w.name == object.name) {
        if !facts.clock.publishes_ages()
            || !week_passed(&week.retiring, facts)
            || retirement_finished(&week.retiring, facts).is_err()
        {
            return Err(Blocked::Line1(Line1::NamedByWeek(week.retiring)));
        }
    }
    // SP36 line 1, SP33/SP34: the advanced ledger excludes ended/lapsed entries.
    if facts.ledger.protects(object.name.kind, object.name.name) {
        return Err(Blocked::Line1(Line1::OpenHoldOrReservation));
    }
    // SP36 line 2, SY17: no removal while this device's clock is untrusted.
    if !facts.clock.publishes_ages() {
        return Err(Blocked::Line2(Line2::UntrustedClock));
    }
    if let Content::Retired(retiring) = object.content {
        // SP17, SP36 lines 2 and 3: every retirement of this content, not only
        // the one asked about, since another record's deletion may still need it
        // for its Restore week.
        let mut all = vec![retiring];
        all.extend(
            facts
                .retired_by
                .get(&object.name)
                .into_iter()
                .flatten()
                .filter(|r| **r != retiring),
        );
        for retiring in all {
            if !week_passed(&retiring, facts) {
                return Err(Blocked::Line2(Line2::WeekNotPassed(retiring)));
            }
            // SP36 line 3: acknowledgement of the retirement and its last hold ended.
            retirement_finished(&retiring, facts).map_err(Blocked::Line3)?;
        }
    }
    // SP36 line 4, SY11: removed devices no longer have a say here.
    if let Some(device) = counted(facts)
        .into_iter()
        .find(|d| facts.holding_back.contains(d))
    {
        return Err(Blocked::Line4(device));
    }
    // SP36 line 5: receipt time alone cannot prove a file was published later.
    if object.content == Content::Unnamed {
        let sighting = facts
            .unnamed
            .get(&object.name)
            .ok_or(Blocked::Line5(Line5::UnknownSighting))?;
        let now = facts.clock.now();
        if now.saturating_sub(sighting.first_seen) < WEEK_MS {
            return Err(Blocked::Line5(Line5::WeekNotPassed));
        }
        for device in &counted(facts) {
            let file = sighting
                .later_files
                .get(device)
                .ok_or(Blocked::Line5(Line5::MissingLaterFile(*device)))?;
            if !file.published_after_sighting {
                return Err(Blocked::Line5(Line5::FileNotPublishedLater(*device)));
            }
            if file.first_seen > now
                || file.first_seen.saturating_sub(sighting.first_seen) < WEEK_MS
            {
                return Err(Blocked::Line5(Line5::FileSeenTooSoon(*device)));
            }
            if !file.read_through_own_without_name {
                return Err(Blocked::Line5(Line5::FileNotReadWithoutName(*device)));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::Keypair;
    use crate::sync::frontier::Held;
    use crate::sync::hold::{Hold, ItemId, Named, Progress, Reserve};
    use crate::sync::ids::Id16;
    use crate::sync::record::{Key, Version};
    use crate::sync::trust::Mark;
    use crate::sync::{chain, frontier, hold, record};
    use serde_json::json;
    use std::collections::BTreeSet;

    const A: DeviceId = Id16([1; 16]);
    const B: DeviceId = Id16([2; 16]);
    const C: DeviceId = Id16([3; 16]);
    const NAME: Name = Name {
        kind: Kind::Blob,
        name: Hash32([4; 32]),
    };
    const RETIRING: Entry = Entry {
        device: A,
        seq: 2,
        hash: Hash32([5; 32]),
    };
    const RETIRED: Object = Object {
        name: NAME,
        content: Content::Retired(RETIRING),
    };
    const UNNAMED: Object = Object {
        name: NAME,
        content: Content::Unnamed,
    };

    fn clock(ms: u64) -> Vouched {
        let mut clock = Vouched::new(0);
        clock.tick(0, 0);
        clock.tick(ms, ms);
        clock
    }

    struct World {
        enrolled: Vec<DeviceId>,
        without_device_file: Vec<DeviceId>,
        reading: BTreeMap<DeviceId, Reading>,
        late_read: bool,
        over_cap: bool,
        live: Vec<Name>,
        late_names: Vec<Name>,
        weeks: Vec<Week>,
        ledger: Ledger,
        ages: Ages,
        clock: Vouched,
        retirements: HashMap<Entry, Retirement>,
        retired_by: BTreeMap<Name, Vec<Entry>>,
        holding_back: Vec<DeviceId>,
        unnamed: BTreeMap<Name, Unnamed>,
    }

    impl World {
        fn ready() -> Self {
            let mut ages = Ages::default();
            ages.saw(RETIRING, 0);
            for d in [B, C] {
                ages.report(RETIRING, d, WEEK_MS, WEEK_MS);
            }
            Self {
                enrolled: vec![A, B, C],
                without_device_file: vec![],
                reading: [A, B, C]
                    .map(|d| {
                        (
                            d,
                            Reading {
                                through_newest_own: true,
                                all_listed_after_own: true,
                            },
                        )
                    })
                    .into(),
                late_read: true,
                over_cap: false,
                live: vec![],
                late_names: vec![],
                weeks: vec![],
                ledger: Ledger::default(),
                ages,
                clock: clock(WEEK_MS),
                retirements: HashMap::from([(
                    RETIRING,
                    Retirement {
                        change: ChangeId {
                            device: A,
                            seq: 2,
                            hash: Hash32([6; 32]),
                        },
                        included_by_all: true,
                        last_hold_ended: true,
                    },
                )]),
                retired_by: BTreeMap::from([(NAME, vec![RETIRING])]),
                holding_back: vec![],
                unnamed: BTreeMap::from([(
                    NAME,
                    Unnamed {
                        first_seen: 0,
                        later_files: [A, B, C]
                            .map(|d| {
                                (
                                    d,
                                    LaterFile {
                                        published_after_sighting: true,
                                        first_seen: WEEK_MS,
                                        read_through_own_without_name: true,
                                    },
                                )
                            })
                            .into(),
                    },
                )]),
            }
        }

        fn check(&self, object: &Object) -> Result<(), Blocked> {
            may_remove(
                object,
                &Facts {
                    this: A,
                    enrolled: &self.enrolled,
                    without_device_file: &self.without_device_file,
                    reading: &self.reading,
                    all_fetchable_late_read: self.late_read,
                    undiscarded_unfetched_late_over_cap: self.over_cap,
                    live_names: &self.live,
                    waiting_late_names: &self.late_names,
                    weeks: &self.weeks,
                    ledger: &self.ledger,
                    ages: &self.ages,
                    clock: &self.clock,
                    retirements: &self.retirements,
                    retired_by: &self.retired_by,
                    holding_back: &self.holding_back,
                    unnamed: &self.unnamed,
                },
            )
        }
    }

    #[test]
    fn every_cleanup_line_holds() {
        let w = World::ready();
        assert_eq!(w.check(&RETIRED), Ok(()));
        assert_eq!(w.check(&UNNAMED), Ok(()));
    }

    #[test]
    fn shared_content_waits_for_every_retirement_of_it() {
        // SP17, SP36: two records name one blob, and both retire it. The one asked
        // about has passed its week; the other hasn't, so the blob stays.
        let other = Entry {
            device: B,
            seq: 7,
            hash: Hash32([8; 32]),
        };
        let mut w = World::ready();
        w.retired_by.get_mut(&NAME).unwrap().push(other);
        w.ages.saw(other, WEEK_MS - 1);
        w.retirements.insert(
            other,
            Retirement {
                change: ChangeId {
                    device: B,
                    seq: 7,
                    hash: Hash32([9; 32]),
                },
                included_by_all: false,
                last_hold_ended: true,
            },
        );
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line2(Line2::WeekNotPassed(other)))
        );
        // Its week passed everywhere, but its change isn't in every frontier yet.
        let mut ages = Ages::default();
        for r in [RETIRING, other] {
            ages.saw(r, 0);
            for d in [B, C] {
                ages.report(r, d, WEEK_MS, WEEK_MS);
            }
        }
        w.ages = ages;
        assert!(matches!(
            w.check(&RETIRED),
            Err(Blocked::Line3(Line3::RetirementNotIncluded(_)))
        ));
        w.retirements.get_mut(&other).unwrap().included_by_all = true;
        assert_eq!(w.check(&RETIRED), Ok(()));
    }

    #[test]
    fn an_enrolled_key_without_a_device_file_counts_in_no_line() {
        // SP25: D enrolled, but its applicant vanished before its first device
        // file. D has no reading, age, frontier or later file to give, and no line
        // waits for it. Once it publishes one, it counts like any other.
        const D: DeviceId = Id16([4; 16]);
        let mut w = World::ready();
        w.enrolled.push(D);
        w.holding_back.push(D);
        // A vanished applicant has no change files listed: all of them are read.
        w.reading.insert(
            D,
            Reading {
                through_newest_own: false,
                all_listed_after_own: true,
            },
        );
        assert!(
            w.check(&RETIRED).is_err(),
            "counted until it's known to have no file"
        );
        w.without_device_file.push(D);
        assert_eq!(w.check(&RETIRED), Ok(()));
        assert_eq!(w.check(&UNNAMED), Ok(()));
        w.without_device_file.clear();
        assert_eq!(w.check(&RETIRED), Err(Blocked::Line1(Line1::UnreadOwn(D))));
    }

    #[test]
    fn a_key_without_a_device_file_still_has_its_listed_changes_read() {
        // SP36 line 1: D published a change (a hold, say) and stopped before its
        // first device file. SP25 exempts D from the lines that need a device
        // file, not from reading what it listed: until that is read, nothing goes.
        const D: DeviceId = Id16([4; 16]);
        let mut w = World::ready();
        w.enrolled.push(D);
        w.without_device_file.push(D);
        w.reading.insert(
            D,
            Reading {
                through_newest_own: false,
                all_listed_after_own: false,
            },
        );
        for object in [&RETIRED, &UNNAMED] {
            assert_eq!(
                w.check(object),
                Err(Blocked::Line1(Line1::UnreadListedChanges(D)))
            );
        }
        w.reading.get_mut(&D).unwrap().all_listed_after_own = true;
        assert_eq!(w.check(&RETIRED), Ok(()));
    }

    #[test]
    fn line_1_waits_for_own_listed_and_fetchable_late_changes() {
        let mut w = World::ready();
        w.reading.remove(&C);
        assert_eq!(w.check(&RETIRED), Err(Blocked::Line1(Line1::UnreadOwn(C))));
        w.reading.insert(
            C,
            Reading {
                through_newest_own: false,
                all_listed_after_own: true,
            },
        );
        assert_eq!(w.check(&RETIRED), Err(Blocked::Line1(Line1::UnreadOwn(C))));
        w.reading.get_mut(&C).unwrap().through_newest_own = true;
        w.reading.get_mut(&C).unwrap().all_listed_after_own = false;
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line1(Line1::UnreadListedChanges(C)))
        );
        w.reading.get_mut(&C).unwrap().all_listed_after_own = true;
        w.late_read = false;
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line1(Line1::UnreadLateChanges))
        );
        w.late_read = true;
        w.late_names.push(NAME);
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line1(Line1::NamedByWaitingLateChange))
        );
        w.late_names.clear();
        assert_eq!(w.check(&RETIRED), Ok(()));
    }

    #[test]
    fn line_2_waits_for_every_devices_week_including_this_one() {
        let mut w = World::ready();
        w.ages = Ages::default();
        let blocked = Err(Blocked::Line2(Line2::WeekNotPassed(RETIRING)));
        assert_eq!(w.check(&RETIRED), blocked);
        w.ages.saw(RETIRING, 1);
        for d in [B, C] {
            w.ages.report(RETIRING, d, 10 * WEEK_MS, WEEK_MS);
        }
        assert_eq!(w.check(&RETIRED), blocked, "others cannot shorten our week");
        w.clock = clock(WEEK_MS + 1);
        assert_eq!(w.check(&RETIRED), Ok(()));
        w.ages.report(RETIRING, C, WEEK_MS - 1, w.clock.now());
        assert_eq!(w.check(&RETIRED), blocked, "lagging peer");
        w.clock = clock(WEEK_MS + 2);
        assert_eq!(
            w.check(&RETIRED),
            Ok(()),
            "reported age grows by local time"
        );
        w.ages = Ages::default();
        w.ages.saw(RETIRING, 0);
        w.ages.report(RETIRING, B, WEEK_MS, w.clock.now());
        assert_eq!(w.check(&RETIRED), blocked, "missing C's report");
    }

    #[test]
    fn line_3_waits_for_the_retiring_change_and_last_hold() {
        let mut w = World::ready();
        let retirement = w.retirements[&RETIRING];
        w.retirements.remove(&RETIRING);
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line3(Line3::UnknownRetirement(RETIRING)))
        );
        w.retirements.insert(
            RETIRING,
            Retirement {
                included_by_all: false,
                ..retirement
            },
        );
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line3(Line3::RetirementNotIncluded(
                retirement.change
            )))
        );
        w.retirements.insert(
            RETIRING,
            Retirement {
                last_hold_ended: false,
                ..retirement
            },
        );
        assert_eq!(w.check(&RETIRED), Err(Blocked::Line3(Line3::LastHoldOpen)));
        w.retirements.insert(RETIRING, retirement);
        assert_eq!(w.check(&RETIRED), Ok(()));
    }

    #[test]
    fn line_4_waits_only_for_enrolled_devices_holding_cleanup_back() {
        let mut w = World::ready();
        w.holding_back = vec![C];
        assert_eq!(w.check(&RETIRED), Err(Blocked::Line4(C)));
        assert_eq!(w.check(&UNNAMED), Err(Blocked::Line4(C)));
        w.enrolled.retain(|d| *d != C);
        assert_eq!(w.check(&RETIRED), Ok(()));
        assert_eq!(w.check(&UNNAMED), Ok(()));
    }

    #[test]
    fn line_5_requires_a_week_and_later_read_device_files_from_every_device() {
        let mut w = World::ready();
        let ready = w.unnamed[&NAME].clone();
        w.unnamed.clear();
        assert_eq!(
            w.check(&UNNAMED),
            Err(Blocked::Line5(Line5::UnknownSighting))
        );
        w.unnamed.insert(
            NAME,
            Unnamed {
                first_seen: 1,
                ..ready.clone()
            },
        );
        assert_eq!(w.check(&UNNAMED), Err(Blocked::Line5(Line5::WeekNotPassed)));
        w.unnamed.insert(NAME, ready.clone());
        // The local device also needs evidence; missing data never passes vacuously.
        for d in [A, B, C] {
            let files = &mut w.unnamed.get_mut(&NAME).unwrap().later_files;
            files.remove(&d);
            assert_eq!(
                w.check(&UNNAMED),
                Err(Blocked::Line5(Line5::MissingLaterFile(d)))
            );
            w.unnamed.insert(NAME, ready.clone());
            w.unnamed
                .get_mut(&NAME)
                .unwrap()
                .later_files
                .get_mut(&d)
                .unwrap()
                .published_after_sighting = false;
            assert_eq!(
                w.check(&UNNAMED),
                Err(Blocked::Line5(Line5::FileNotPublishedLater(d)))
            );
            w.unnamed.insert(NAME, ready.clone());
            for at in [WEEK_MS - 1, WEEK_MS + 1] {
                w.unnamed
                    .get_mut(&NAME)
                    .unwrap()
                    .later_files
                    .get_mut(&d)
                    .unwrap()
                    .first_seen = at;
                assert_eq!(
                    w.check(&UNNAMED),
                    Err(Blocked::Line5(Line5::FileSeenTooSoon(d)))
                );
            }
            w.unnamed.insert(NAME, ready.clone());
            w.unnamed
                .get_mut(&NAME)
                .unwrap()
                .later_files
                .get_mut(&d)
                .unwrap()
                .read_through_own_without_name = false;
            assert_eq!(
                w.check(&UNNAMED),
                Err(Blocked::Line5(Line5::FileNotReadWithoutName(d)))
            );
            w.unnamed.insert(NAME, ready.clone());
        }
        assert_eq!(w.check(&UNNAMED), Ok(()));
    }

    #[test]
    fn an_untrusted_clock_blocks_retired_and_unnamed_content() {
        let mut w = World::ready();
        w.clock.tick(WEEK_MS + 1, 20 * WEEK_MS);
        for object in [RETIRED, UNNAMED] {
            assert_eq!(w.check(&object), Err(Blocked::Line2(Line2::UntrustedClock)));
        }
        w.clock.clear();
        assert_eq!(w.check(&RETIRED), Ok(()));
        assert_eq!(w.check(&UNNAMED), Ok(()));
    }

    #[test]
    fn an_unfetched_over_cap_late_tail_blocks_every_object() {
        let mut w = World::ready();
        w.over_cap = true;
        for kind in [Kind::Blob, Kind::Book] {
            for content in [Content::Retired(RETIRING), Content::Unnamed] {
                for name in [Hash32([4; 32]), Hash32([9; 32])] {
                    assert_eq!(
                        w.check(&Object {
                            name: Name { kind, name },
                            content
                        }),
                        Err(Blocked::Line1(Line1::UnfetchedLateOverCap))
                    );
                }
            }
        }
        w.over_cap = false;
        assert_eq!(w.check(&RETIRED), Ok(()));
    }

    #[test]
    fn a_week_entry_stops_counting_only_when_its_lines_2_and_3_hold() {
        let mut w = World::ready();
        // A view may name the same object for a different retirement.
        let other = Entry { seq: 3, ..RETIRING };
        w.weeks.push(Week {
            name: NAME,
            retiring: other,
        });
        let blocked = Err(Blocked::Line1(Line1::NamedByWeek(other)));
        assert_eq!(w.check(&RETIRED), blocked);
        w.ages.saw(other, 0);
        for d in [B, C] {
            w.ages.report(other, d, WEEK_MS, WEEK_MS);
        }
        let retirement = Retirement {
            change: ChangeId {
                seq: 3,
                ..w.retirements[&RETIRING].change
            },
            included_by_all: false,
            last_hold_ended: true,
        };
        w.retirements.insert(other, retirement);
        assert_eq!(w.check(&RETIRED), blocked);
        w.retirements.get_mut(&other).unwrap().included_by_all = true;
        w.retirements.get_mut(&other).unwrap().last_hold_ended = false;
        assert_eq!(w.check(&RETIRED), blocked);
        w.retirements.get_mut(&other).unwrap().last_hold_ended = true;
        assert_eq!(w.check(&RETIRED), Ok(()));
        w.clock.untrusted_wall = true;
        assert_eq!(w.check(&RETIRED), blocked);
        w.clock.clear();
        w.ages.report(other, C, WEEK_MS - 1, WEEK_MS);
        assert_eq!(w.check(&RETIRED), blocked);
        // A name in another storage kind does not protect this blob.
        w.weeks[0].name.kind = Kind::Book;
        assert_eq!(w.check(&RETIRED), Ok(()));
    }

    #[test]
    fn cleanup_reports_the_first_failing_line() {
        let mut w = World::ready();
        w.live.push(NAME);
        w.ages = Ages::default();
        w.retirements.get_mut(&RETIRING).unwrap().last_hold_ended = false;
        w.holding_back.push(C);
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line1(Line1::NamedByLiveVersion))
        );
        w.live.clear();
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line2(Line2::WeekNotPassed(RETIRING)))
        );
        w.ages = World::ready().ages;
        assert_eq!(w.check(&RETIRED), Err(Blocked::Line3(Line3::LastHoldOpen)));
        w.retirements.get_mut(&RETIRING).unwrap().last_hold_ended = true;
        assert_eq!(w.check(&RETIRED), Err(Blocked::Line4(C)));
        w.unnamed.clear();
        assert_eq!(w.check(&UNNAMED), Err(Blocked::Line4(C)));
        w.holding_back.clear();
        assert_eq!(
            w.check(&UNNAMED),
            Err(Blocked::Line5(Line5::UnknownSighting))
        );
    }

    fn held_item() -> Hold {
        Hold {
            kind: NAME.kind,
            name: NAME.name,
            hash: Hash32([7; 32]),
            size: 1,
            for_key: Key::parse("note:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6")
                .unwrap()
                .0,
        }
    }

    fn change(key: &Keypair, reserve: &[Reserve], holds: &[Hold]) -> chain::Change {
        let signed = chain::sign_change_with(key, &A, &B, 1, None, &[], reserve, holds);
        chain::open_change(&signed.bytes, &A, &key.device(), &key.public()).unwrap()
    }

    /// Exercise Ledger's own acknowledgement and expiry logic, not a mock hold.
    fn advance(
        ledger: &mut Ledger,
        c: &chain::Change,
        at: u64,
        removed: bool,
        decided: bool,
        choices: &[Version],
    ) {
        let devices = BTreeSet::from([c.device]);
        let empty = BTreeSet::new();
        ledger.advance(
            &clock(at),
            &Progress {
                accepted: &BTreeMap::from([(
                    c.device,
                    Mark {
                        s: c.seq,
                        h: c.hash,
                    },
                )]),
                held: &|d, s| {
                    (*d == c.device && s == c.seq).then_some(Held {
                        h: c.hash,
                        prev: c.prev,
                        vroot: c.vroot,
                    })
                },
                removed: if removed { &devices } else { &empty },
                all_listed_read: &devices,
                late_decided: if decided { &devices } else { &empty },
                choices,
            },
        );
    }

    fn choice(key: &str, action: &str) -> Version {
        let raw = json!({"format": 1, "key": key, "device": A.to_string(),
            "clock": [0, 0], "history": [], "inline": {"choice": action}});
        let record::Checked::Version(v) = record::check_version(raw, A).unwrap() else {
            panic!()
        };
        v
    }

    #[test]
    fn a_lapsed_reservation_and_an_ended_hold_do_not_block() {
        let key = Keypair::from_seed(&[8; 32]);
        for is_hold in [false, true] {
            let reserve = Reserve {
                kind: NAME.kind,
                name: NAME.name,
                size: 1,
            };
            let c = if is_hold {
                change(&key, &[], &[held_item()])
            } else {
                change(&key, &[reserve], &[])
            };
            let mut w = World::ready();
            w.ledger.receive(&c, &[], false, &clock(0)).unwrap();
            advance(&mut w.ledger, &c, 0, false, false, &[]);
            advance(&mut w.ledger, &c, WEEK_MS - 1, false, false, &[]);
            assert_eq!(
                w.check(&UNNAMED),
                Err(Blocked::Line1(Line1::OpenHoldOrReservation))
            );
            advance(&mut w.ledger, &c, WEEK_MS, false, false, &[]);
            assert_eq!(w.check(&UNNAMED), Ok(()));
        }
    }

    #[test]
    fn a_blob_uploaded_before_its_change_and_seen_in_between_is_not_removed() {
        let key = Keypair::from_seed(&[8; 32]);
        let c = change(
            &key,
            &[Reserve {
                kind: NAME.kind,
                name: NAME.name,
                size: 1,
            }],
            &[],
        );
        let mut w = World::ready();
        w.ledger.receive(&c, &[], false, &clock(0)).unwrap();
        assert_eq!(
            w.check(&UNNAMED),
            Err(Blocked::Line1(Line1::OpenHoldOrReservation))
        );
        // Even when the reservation has not arrived, the fresh object waits.
        w.ledger = Ledger::default();
        w.unnamed.get_mut(&NAME).unwrap().first_seen = WEEK_MS;
        assert_eq!(w.check(&UNNAMED), Err(Blocked::Line5(Line5::WeekNotPassed)));
    }

    #[test]
    fn a_note_unchanged_for_a_month_keeps_its_blob() {
        let mut w = World::ready();
        w.clock = clock(30 * 24 * 60 * 60 * 1000);
        w.live.push(NAME);
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line1(Line1::NamedByLiveVersion))
        );
        w.live[0].kind = Kind::Book;
        assert_eq!(w.check(&RETIRED), Ok(()));
    }

    /// #149 words this as "lasts until its late change is kept or discarded".
    /// SP34 is stricter: a removed writer's hold ends only with its exact
    /// `choice:hold.` Stop waiting, even after Keep or Discard.
    #[test]
    fn a_removed_devices_hold_keeps_its_content_until_the_reader_stops_waiting() {
        let key = Keypair::from_seed(&[8; 32]);
        let c = change(&key, &[], &[held_item()]);
        let mut w = World::ready();
        w.ledger.receive(&c, &[], true, &clock(0)).unwrap();
        advance(&mut w.ledger, &c, 100 * WEEK_MS, true, false, &[]);
        let held = Err(Blocked::Line1(Line1::OpenHoldOrReservation));
        assert_eq!(w.check(&RETIRED), held);
        // Stage 2 supplies the completed Keep's replacement live reference.
        advance(&mut w.ledger, &c, 100 * WEEK_MS, true, true, &[]);
        w.live.push(NAME);
        assert!(w.ledger.protects(NAME.kind, NAME.name));
        assert_eq!(
            w.check(&RETIRED),
            Err(Blocked::Line1(Line1::NamedByLiveVersion))
        );
        w.live.clear();
        assert_eq!(w.check(&RETIRED), held, "Keep alone did not end the hold");
        let id = ItemId {
            device: c.device,
            seq: c.seq,
            index: 0,
        };
        // Discard, a reservation's Stop waiting and another hold's choice do not end it.
        for v in [
            choice(
                &format!("choice:{}.{}.{}", c.device, c.seq, RETIRING.hash),
                "discard",
            ),
            choice(&id.choice_key(false), "stop-waiting"),
            choice(&ItemId { index: 1, ..id }.choice_key(true), "stop-waiting"),
        ] {
            advance(&mut w.ledger, &c, 100 * WEEK_MS, true, true, &[v]);
            assert_eq!(w.check(&RETIRED), held);
        }
        advance(
            &mut w.ledger,
            &c,
            100 * WEEK_MS,
            true,
            true,
            &[choice(&id.choice_key(true), "stop-waiting")],
        );
        assert!(!w.ledger.protects(NAME.kind, NAME.name));
        assert_eq!(
            w.check(&RETIRED),
            Ok(()),
            "nothing names it and the other lines hold"
        );
    }

    /// #149, SY23: no-conflict-copy behavior and repair belong to storage, outside
    /// this decision function. Here either writer's live reference protects the
    /// single shared content-addressed object, for both books and blobs.
    #[test]
    fn two_devices_uploading_the_same_object_leave_it_while_either_names_it() {
        for kind in [Kind::Blob, Kind::Book] {
            let mut w = World::ready();
            let object = Object {
                name: Name { kind, ..NAME },
                ..RETIRED
            };
            let writers = [Keypair::from_seed(&[8; 32]), Keypair::from_seed(&[9; 32])];
            let references: Vec<_> = writers
                .iter()
                .map(|key| Named {
                    kind,
                    name: object.name.name,
                    key: held_item().for_key,
                    version: Entry {
                        device: key.device(),
                        seq: 2,
                        hash: RETIRING.hash,
                    },
                })
                .collect();
            for kept in [vec![0, 1], vec![0], vec![1]] {
                w.live = kept
                    .into_iter()
                    .map(|i| Name {
                        kind: references[i].kind,
                        name: references[i].name,
                    })
                    .collect();
                assert_eq!(
                    w.check(&object),
                    Err(Blocked::Line1(Line1::NamedByLiveVersion))
                );
            }
            w.live.clear();
            assert_eq!(w.check(&object), Ok(()));
        }
    }

    #[test]
    fn a_restore_minutes_before_the_week_ends_keeps_its_content_whatever_order() {
        let key = Keypair::from_seed(&[8; 32]);
        let c = change(&key, &[], &[held_item()]);
        let before_week = WEEK_MS - 2 * 60 * 1000;
        for hold_first in [true, false] {
            let mut w = World::ready();
            // Three enrolled devices: A cleans, the hold's writer restores,
            // and C lags. A and the writer have aged the retirement.
            let writer = key.device();
            w.enrolled = vec![A, writer, C];
            let reading = w.reading.remove(&B).unwrap();
            w.reading.insert(writer, reading);
            w.ages.report(RETIRING, writer, WEEK_MS, WEEK_MS);
            w.ages.report(RETIRING, C, 2 * 60 * 1000, WEEK_MS);
            // Exercise the existing frontier helpers with three devices, one lagging.
            let retirement = w.retirements[&RETIRING].change;
            let held = |d: &DeviceId, s: u64| {
                (*d == retirement.device && s == retirement.seq).then_some(Held {
                    h: retirement.hash,
                    prev: None,
                    vroot: Hash32([0; 32]),
                })
            };
            let accepted = frontier::accepted_by_all(
                &BTreeSet::from([A, writer, C]),
                &A,
                &BTreeMap::new(),
                &BTreeMap::from([
                    (writer, BTreeMap::from([(A, 2)])),
                    (C, BTreeMap::from([(A, 1)])),
                ]),
                &BTreeMap::from([(A, 2)]),
                &held,
                &BTreeMap::new(),
            );
            w.retirements.get_mut(&RETIRING).unwrap().included_by_all =
                hold::included_by_all(retirement, &accepted, &held);
            if hold_first {
                w.ledger
                    .receive(&c, &[], false, &clock(before_week))
                    .unwrap();
                assert_eq!(
                    w.check(&RETIRED),
                    Err(Blocked::Line1(Line1::OpenHoldOrReservation))
                );
                w.ages.report(RETIRING, C, WEEK_MS, WEEK_MS);
                assert_eq!(
                    w.check(&RETIRED),
                    Err(Blocked::Line1(Line1::OpenHoldOrReservation))
                );
            } else {
                assert_eq!(
                    w.check(&RETIRED),
                    Err(Blocked::Line2(Line2::WeekNotPassed(RETIRING)))
                );
                w.ages.report(RETIRING, C, WEEK_MS, WEEK_MS);
                assert_eq!(
                    w.check(&RETIRED),
                    Err(Blocked::Line3(Line3::RetirementNotIncluded(retirement)))
                );
                w.ledger
                    .receive(&c, &[], false, &clock(before_week))
                    .unwrap();
                assert_eq!(
                    w.check(&RETIRED),
                    Err(Blocked::Line1(Line1::OpenHoldOrReservation))
                );
            }
            // The naming change is sent only after the hold is acknowledged.
            w.live.push(NAME);
            assert_eq!(
                w.check(&RETIRED),
                Err(Blocked::Line1(Line1::NamedByLiveVersion))
            );
        }
    }
}
