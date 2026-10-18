//! SY10, SP10: the hybrid logical clock, and what “newest” means.
//!
//! A reading is `[ms, n]`: milliseconds since the Unix epoch and a counter. A change
//! made after a device has seen another version always sorts after it, whatever
//! either system clock says. Between concurrent versions the higher reading wins,
//! ties broken by device ID, so every device picks the same one (SY10).

use super::ids::DeviceId;
use serde::{Deserialize, Serialize};

/// SY10 (prov., item 106): a remote reading this far ahead names its device.
pub const AHEAD_MS: u64 = 5 * 60 * 1000;
/// SY10 (prov., item 106): a remote reading this far ahead is set aside.
pub const SET_ASIDE_MS: u64 = 24 * 60 * 60 * 1000;
/// SP10: `ms` is 48 bits.
pub const MAX_MS: u64 = (1 << 48) - 1;

/// A clock reading, written `[ms, n]` (SP10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hlc(pub u64, pub u16);

impl Hlc {
    pub fn ms(self) -> u64 {
        self.0
    }
    pub fn n(self) -> u16 {
        self.1
    }
    pub fn is_valid(self) -> bool {
        self.0 <= MAX_MS
    }
}

/// A version's place in SY10's order: its reading, then its writer's ID as bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    pub clock: Hlc,
    pub device: DeviceId,
}

/// What a remote reading means on arrival (SY10).
#[derive(Debug, PartialEq, Eq)]
pub enum Arrival {
    /// Applied, and the clock moved to it if it was later.
    Applied,
    /// Applied, and its device is named as running ahead.
    Ahead,
    /// Set aside: applied only once this clock catches up; the clock didn't move.
    SetAside,
}

/// A device's clock.
#[derive(Debug)]
pub struct Clock {
    last: Hlc,
}

impl Clock {
    pub fn new(last: Hlc) -> Self {
        Clock { last }
    }

    pub fn last(&self) -> Hlc {
        self.last
    }

    /// SP10: the reading for a local write at system time `wall_ms`. At `n` =
    /// 65535 the clock moves `ms` on by one rather than waiting, so a remote reading
    /// saturated at 65535 can never stop this device from writing.
    pub fn tick(&mut self, wall_ms: u64) -> Hlc {
        let ms = wall_ms.min(MAX_MS).max(self.last.0);
        self.last = if ms > self.last.0 {
            Hlc(ms, 0)
        } else if self.last.1 == u16::MAX {
            Hlc(ms + 1, 0)
        } else {
            Hlc(ms, self.last.1 + 1)
        };
        self.last
    }

    /// SY10: a remote reading arriving at system time `wall_ms`. A set-aside
    /// reading never moves the clock, so no device's clock follows a wrong one.
    pub fn observe(&mut self, remote: Hlc, wall_ms: u64) -> Arrival {
        let ahead = remote.0.saturating_sub(wall_ms);
        if ahead > SET_ASIDE_MS {
            return Arrival::SetAside;
        }
        if remote > self.last {
            self.last = remote;
        }
        if ahead > AHEAD_MS {
            Arrival::Ahead
        } else {
            Arrival::Applied
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::ids::Id16;

    #[test]
    fn local_writes_always_move_forward() {
        let mut c = Clock::new(Hlc(1_000, 0));
        assert_eq!(c.tick(2_000), Hlc(2_000, 0));
        // The system clock went back: the reading still rises.
        assert_eq!(c.tick(1_500), Hlc(2_000, 1));
        assert_eq!(c.tick(2_000), Hlc(2_000, 2));
        assert_eq!(c.tick(2_001), Hlc(2_001, 0));
    }

    #[test]
    fn a_saturated_counter_moves_ms_on() {
        // SP10: a remote reading at n = 65535 can't stall this clock.
        let mut c = Clock::new(Hlc(1_000, 0));
        let wall = 1_000;
        assert_eq!(
            c.observe(Hlc(wall + 60_000, u16::MAX), wall),
            Arrival::Applied
        );
        assert_eq!(c.tick(wall), Hlc(wall + 60_001, 0));
        assert_eq!(c.tick(wall), Hlc(wall + 60_001, 1));
    }

    #[test]
    fn arrivals_ahead_are_named_or_set_aside() {
        let wall = 10_000_000_000;
        let mut c = Clock::new(Hlc(wall, 0));
        assert_eq!(c.observe(Hlc(wall + 1_000, 0), wall), Arrival::Applied);
        assert_eq!(
            c.last(),
            Hlc(wall + 1_000, 0),
            "a later reading moves the clock"
        );
        assert_eq!(c.observe(Hlc(wall + AHEAD_MS + 1, 0), wall), Arrival::Ahead);
        let before = c.last();
        assert_eq!(
            c.observe(Hlc(wall + SET_ASIDE_MS + 1, 0), wall),
            Arrival::SetAside
        );
        assert_eq!(
            c.last(),
            before,
            "a set-aside reading never moves the clock"
        );
        // An earlier remote reading leaves the clock where it is.
        assert_eq!(c.observe(Hlc(wall - 5, 0), wall), Arrival::Applied);
        assert_eq!(c.last(), before);
    }

    #[test]
    fn ties_break_by_device_id() {
        let a = Stamp {
            clock: Hlc(5, 1),
            device: Id16([1; 16]),
        };
        let b = Stamp {
            clock: Hlc(5, 1),
            device: Id16([2; 16]),
        };
        let c = Stamp {
            clock: Hlc(5, 2),
            device: Id16([0; 16]),
        };
        assert!(a < b && b < c);
    }

    #[test]
    fn readings_are_written_as_pairs() {
        assert_eq!(
            serde_json::to_string(&Hlc(1760702400123, 7)).unwrap(),
            "[1760702400123,7]"
        );
        assert!(!Hlc(MAX_MS + 1, 0).is_valid());
    }
}
