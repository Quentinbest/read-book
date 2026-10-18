//! SY17, SY21, SP35: the week a deletion is kept, measured by time a device can vouch for.
//!
//! No wall-clock times are compared across devices. Each device measures a deletion's
//! age from when it first saw it: by its monotonic clock while running, and by its
//! wall clock across a restart only when that clock isn't marked as running ahead
//! and didn't jump against the monotonic clock. Restore is offered while this
//! device's own age is under the week, whatever other devices report (item 111);
//! content may leave storage only once every enrolled device reports the week.
//!
//! SY21: nothing here deletes. Missing objects, a storage that's gone or a file that
//! can't be read only ever leave things as they are; only a tombstone removes.

use super::ids::DeviceId;
use super::record::Entry;
use std::collections::HashMap;

/// SY17 (prov., item 106): the week.
pub const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// SY17: a wall clock that moved this much against the monotonic clock isn't vouched for.
pub const JUMP_MS: u64 = 5 * 60 * 1000;

/// A device's vouched-for time: milliseconds it can stand behind since it started counting.
#[derive(Debug, Clone)]
pub struct Vouched {
    total_ms: u64,
    last_mono_ms: Option<u64>,
    last_wall_ms: u64,
    /// Marked as running ahead (SY10), or its wall clock jumped: no ages or marks
    /// are published until it is clear.
    pub untrusted_wall: bool,
}

impl Vouched {
    pub fn new(wall_ms: u64) -> Self {
        Vouched {
            total_ms: 0,
            last_mono_ms: None,
            last_wall_ms: wall_ms,
            untrusted_wall: false,
        }
    }

    pub fn now(&self) -> u64 {
        self.total_ms
    }

    /// While running: the monotonic clock is the measure; the wall clock is only
    /// watched for jumps.
    pub fn tick(&mut self, mono_ms: u64, wall_ms: u64) {
        if let Some(last) = self.last_mono_ms {
            let mono = mono_ms.saturating_sub(last);
            let wall = wall_ms as i128 - self.last_wall_ms as i128;
            if (wall - mono as i128).unsigned_abs() > JUMP_MS as u128 {
                self.untrusted_wall = true;
            }
            self.total_ms += mono;
        }
        self.last_mono_ms = Some(mono_ms);
        self.last_wall_ms = wall_ms;
    }

    /// Across a restart: the wall clock counts only if it can be vouched for and
    /// moved forward; otherwise the time away isn't counted.
    pub fn restart(&mut self, mono_ms: u64, wall_ms: u64, clock_ahead: bool) {
        if clock_ahead {
            self.untrusted_wall = true;
        }
        if !self.untrusted_wall && wall_ms > self.last_wall_ms {
            self.total_ms += wall_ms - self.last_wall_ms;
        }
        self.last_mono_ms = Some(mono_ms);
        self.last_wall_ms = wall_ms;
    }

    /// The clock was corrected and is clear again (SY10).
    pub fn clear(&mut self) {
        self.untrusted_wall = false;
    }

    /// SY17: a device so marked publishes no ages or marks.
    pub fn publishes_ages(&self) -> bool {
        !self.untrusted_wall
    }
}

/// Ages this device knows: its own, and those other devices reported (SP35).
#[derive(Debug, Default)]
pub struct Ages {
    /// When this device first saw each retiring version, in vouched-for time.
    first_seen: HashMap<Entry, u64>,
    /// Each device's reported age, and this device's vouched time when it received it.
    reported: HashMap<(Entry, DeviceId), (u64, u64)>,
}

impl Ages {
    pub fn saw(&mut self, retiring: Entry, now: u64) {
        self.first_seen.entry(retiring).or_insert(now);
    }

    pub fn own_age(&self, retiring: &Entry, now: u64) -> Option<u64> {
        self.first_seen.get(retiring).map(|t| now - t)
    }

    /// A device's reported age, from its newest device file (whose counter rose).
    pub fn report(&mut self, retiring: Entry, from: DeviceId, age_ms: u64, now: u64) {
        self.reported.insert((retiring, from), (age_ms, now));
    }

    /// SP35: a reported age plus the time since it arrived, by this device's clock.
    pub fn reported_age(&self, retiring: &Entry, from: DeviceId, now: u64) -> Option<u64> {
        self.reported
            .get(&(*retiring, from))
            .map(|(age, at)| age + (now - at))
    }

    /// SY17, item 111: Restore is offered while this device's own age is under the
    /// week. Other devices' reports never shorten it.
    pub fn restore_offered(&self, retiring: &Entry, now: u64) -> bool {
        self.own_age(retiring, now).is_some_and(|a| a < WEEK_MS)
    }

    /// The oldest age reported, for labelling a Restore on a device back late
    /// (“Deleted on ‹device› about 10 days ago”).
    pub fn oldest_reported(&self, retiring: &Entry, now: u64) -> Option<u64> {
        self.reported
            .iter()
            .filter(|((r, _), _)| r == retiring)
            .map(|(_, (age, at))| age + (now - at))
            .max()
    }

    /// SY17, SP36 line 2: every enrolled device, this one included, reports at
    /// least the week. One clock moved forward can't end it.
    pub fn week_passed_everywhere(
        &self,
        retiring: &Entry,
        this: DeviceId,
        enrolled: &[DeviceId],
        now: u64,
    ) -> bool {
        self.own_age(retiring, now).is_some_and(|a| a >= WEEK_MS)
            && enrolled.iter().filter(|d| **d != this).all(|d| {
                self.reported_age(retiring, *d, now)
                    .is_some_and(|a| a >= WEEK_MS)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::ids::{Hash32, Id16};

    const DAY: u64 = 24 * 60 * 60 * 1000;

    fn del() -> Entry {
        Entry {
            device: Id16([9; 16]),
            seq: 4,
            hash: Hash32([4; 32]),
        }
    }

    #[test]
    fn running_time_is_monotonic_and_restarts_count_a_trusted_wall_clock() {
        let mut v = Vouched::new(1_000_000);
        v.tick(0, 1_000_000);
        v.tick(DAY, 1_000_000 + DAY);
        assert_eq!(v.now(), DAY);
        v.restart(5, 1_000_000 + 3 * DAY, false);
        assert_eq!(v.now(), 3 * DAY, "two days off, counted by the wall clock");
        // A wall clock set back across a restart adds nothing.
        v.restart(5, 1_000_000, false);
        assert_eq!(v.now(), 3 * DAY);
    }

    #[test]
    fn a_jumped_or_ahead_clock_isnt_vouched_for() {
        let mut v = Vouched::new(0);
        v.tick(0, 0);
        v.tick(1_000, 8 * DAY); // wall jumped 8 days in one second of running
        assert!(!v.publishes_ages());
        assert_eq!(v.now(), 1_000, "only the monotonic second counts");
        v.restart(0, 20 * DAY, false);
        assert_eq!(
            v.now(),
            1_000,
            "time away isn't counted while the wall clock is untrusted"
        );
        v.clear();
        let mut w = Vouched::new(0);
        w.restart(0, DAY, true);
        assert!(!w.publishes_ages() && w.now() == 0);
    }

    #[test]
    fn expiry_under_skew() {
        // Stage 1 check: one clock 8 days ahead removes nothing.
        let me = Id16([1; 16]);
        let other = Id16([2; 16]);
        let mut ages = Ages::default();
        ages.saw(del(), 0);
        ages.report(del(), other, 8 * DAY, 0); // forged or ahead: says 8 days at once
        assert!(!ages.week_passed_everywhere(&del(), me, &[me, other], DAY));
        assert!(
            ages.restore_offered(&del(), DAY),
            "others' reports never shorten Restore"
        );
        assert!(ages.week_passed_everywhere(&del(), me, &[me, other], WEEK_MS));
        assert!(!ages.restore_offered(&del(), WEEK_MS));
    }

    #[test]
    fn a_device_back_late_offers_its_own_week_labelled_with_the_reported_age() {
        // Item 111 (a): back on day 10, a deletion first seen elsewhere on day 0.
        let other = Id16([2; 16]);
        let mut ages = Ages::default();
        let now = 10 * DAY;
        ages.saw(del(), now);
        ages.report(del(), other, 10 * DAY, now);
        assert!(ages.restore_offered(&del(), now));
        assert_eq!(ages.oldest_reported(&del(), now + DAY), Some(11 * DAY));
        assert!(!ages.restore_offered(&del(), now + WEEK_MS));
    }

    #[test]
    fn a_silent_device_holds_the_week_open() {
        let me = Id16([1; 16]);
        let silent = Id16([3; 16]);
        let mut ages = Ages::default();
        ages.saw(del(), 0);
        assert!(!ages.week_passed_everywhere(&del(), me, &[me, silent], 30 * DAY));
    }
}
