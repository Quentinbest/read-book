//! SP33, SP34, SY11: reservations before uploads and holds before naming content.
//!
//! Pure library state, supplied by the caller. Persistence in the database or
//! device store belongs to Stage 2; nothing here fetches, uploads or removes an
//! object. The caller supplies checked chains, resolved frontiers, live names
//! and settled reader choices. All elapsed time comes from this device's
//! `retention::Vouched`, never another device's wall clock.
//!
//! PROVISIONAL: the per-device limits below, using binary byte units like the
//! surrounding sync limits. SP35 ages and SP36 cleanup are outside this module.

use super::chain::Change;
use super::frontier::Held;
use super::ids::{DeviceId, Hash32};
use super::json::MAX_SAFE_INTEGER;
use super::record::{Content, Entry, Key, Version};
use super::retention::{Vouched, WEEK_MS};
use super::trust::Mark;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// SP33: PROVISIONAL open reservations per device, also the per-change list cap.
pub const MAX_RESERVATIONS: usize = 20;
/// SP33: PROVISIONAL reserved bytes per device (5 GB).
pub const MAX_RESERVED_BYTES: u64 = 5 * 1024 * 1024 * 1024;
/// SP34: PROVISIONAL open holds per device, also the per-change list cap.
pub const MAX_HOLDS: usize = 200;
/// SP34: PROVISIONAL held bytes per device (2 GB).
pub const MAX_HELD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Blob,
    Book,
}

impl Kind {
    fn read(v: &Value) -> Option<Self> {
        match v.as_str()? {
            "blob" => Some(Self::Blob),
            "book" => Some(Self::Book),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Book => "book",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reserve {
    pub kind: Kind,
    pub name: Hash32,
    pub size: u64,
}

impl Reserve {
    /// SP33, SP9: a keyed object name, kind and integer byte size.
    pub fn read(v: &Value) -> Option<Self> {
        Some(Self {
            kind: Kind::read(v.get("kind")?)?,
            name: Hash32::parse(v.get("name")?.as_str()?)?,
            size: v.get("size")?.as_u64().filter(|s| *s <= MAX_SAFE_INTEGER)?,
        })
    }

    pub fn write(&self) -> Value {
        json!({"kind": self.kind.as_str(), "name": self.name.to_string(), "size": self.size})
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub kind: Kind,
    pub name: Hash32,
    pub hash: Hash32,
    pub size: u64,
    pub for_key: Key,
}

impl Hold {
    /// SP34, SY28: a held blob is bounded by the target record's body limit.
    pub fn read(v: &Value) -> Option<Self> {
        let r = Reserve::read(v)?;
        let (for_key, kind) = Key::parse(v.get("for")?.as_str()?)?;
        if r.kind == Kind::Blob && r.size > kind.body_limit() {
            return None;
        }
        Some(Self {
            kind: r.kind,
            name: r.name,
            size: r.size,
            hash: Hash32::parse(v.get("hash")?.as_str()?)?,
            for_key,
        })
    }

    pub fn write(&self) -> Value {
        json!({"kind": self.kind.as_str(), "name": self.name.to_string(),
            "hash": self.hash.to_string(), "size": self.size, "for": self.for_key.as_str()})
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HoldError {
    #[error("not SP33/SP34's item shape")]
    Shape,
    #[error("past SP33's reservation limit")]
    Reservations,
    #[error("past SP34's hold limit")]
    Holds,
    #[error("not the next change in the ledger's chain")]
    Chain,
}

/// SP33, SP34, #122: check lists before applying a change or fetching content.
pub fn read_lists(
    reserve: &[Value],
    hold: &[Value],
) -> Result<(Vec<Reserve>, Vec<Hold>), HoldError> {
    if reserve.len() > MAX_RESERVATIONS {
        return Err(HoldError::Reservations);
    }
    if hold.len() > MAX_HOLDS {
        return Err(HoldError::Holds);
    }
    let reserve = reserve
        .iter()
        .map(Reserve::read)
        .collect::<Option<Vec<_>>>()
        .ok_or(HoldError::Shape)?;
    let hold = hold
        .iter()
        .map(Hold::read)
        .collect::<Option<Vec<_>>>()
        .ok_or(HoldError::Shape)?;
    limits(
        reserve.iter().map(|r| r.size),
        MAX_RESERVATIONS,
        MAX_RESERVED_BYTES,
    )
    .map_err(|_| HoldError::Reservations)?;
    limits(hold.iter().map(|h| h.size), MAX_HOLDS, MAX_HELD_BYTES).map_err(|_| HoldError::Holds)?;
    Ok((reserve, hold))
}

fn limits(sizes: impl Iterator<Item = u64>, count: usize, bytes: u64) -> Result<(), ()> {
    let mut total = 0u64;
    for (i, size) in sizes.enumerate() {
        total = total.checked_add(size).ok_or(())?;
        if i >= count || total > bytes {
            return Err(());
        }
    }
    Ok(())
}

/// SP12: the zero-based entry in its writer's change, for a Stop waiting key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemId {
    pub device: DeviceId,
    pub seq: u64,
    pub index: usize,
}

impl ItemId {
    pub fn choice_key(self, hold: bool) -> String {
        let list = if hold { "hold" } else { "reserve" };
        format!("choice:{list}.{}.{}.{}", self.device, self.seq, self.index)
    }
}

/// A change's payload hash, distinct from a version hash (SP14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeId {
    pub device: DeviceId,
    pub seq: u64,
    pub hash: Hash32,
}

impl From<&Change> for ChangeId {
    fn from(c: &Change) -> Self {
        Self {
            device: c.device,
            seq: c.seq,
            hash: c.hash,
        }
    }
}

/// SP16, SP29: `accepted` is the output of `frontier::accepted_by_all`, made
/// from resolved `frontier::Vouched` entries. Check the target's hash too: a
/// sequence on a different branch is not an acknowledgement of this change.
pub fn included_by_all(
    change: ChangeId,
    accepted: &BTreeMap<DeviceId, Mark>,
    held: &dyn Fn(&DeviceId, u64) -> Option<Held>,
) -> bool {
    accepted.get(&change.device).is_some_and(|m| {
        m.s >= change.seq
            && held(&change.device, m.s).is_some_and(|h| h.h == m.h)
            && held(&change.device, change.seq).is_some_and(|h| h.h == change.hash)
    })
}

/// SP33: mere presence in storage is insufficient without a live reference.
pub fn may_rely_on_stored_object(named_by_live_version: bool) -> bool {
    named_by_live_version
}

/// SP34: Restore, undo and Keep must hold content that has no live reference.
pub fn needs_hold(named_by_live_version: bool) -> bool {
    !may_rely_on_stored_object(named_by_live_version)
}

#[derive(Clone, Debug)]
struct Reservation {
    item: Reserve,
    first_seen: u64,
}

#[derive(Clone, Debug)]
struct Holding {
    item: Hold,
    change: ChangeId,
    accepted_at: Option<u64>,
    naming: Option<(ChangeId, Entry)>,
}

/// A checked version's reference to an object. The caller derives book references
/// from validated bodies and blob names with the library naming key (SP9, SY23).
#[derive(Clone, Debug)]
pub struct Named {
    pub kind: Kind,
    pub name: Hash32,
    pub key: Key,
    pub version: Entry,
}

/// SP33, SP34: open entries are counted separately even when names coincide.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    reservations: BTreeMap<ItemId, Reservation>,
    holds: BTreeMap<ItemId, Holding>,
    last: BTreeMap<DeviceId, Mark>,
}

impl Ledger {
    /// SP17, SP33: this writer's reservations that have not ended or lapsed.
    pub(crate) fn open_reservations(
        &self,
        device: DeviceId,
    ) -> impl Iterator<Item = (ItemId, Reserve)> + '_ {
        self.reservations
            .iter()
            .filter(move |(id, _)| id.device == device)
            .map(|(id, r)| (*id, r.item.clone()))
    }

    /// SP17, SP34: this writer's holds, retaining their choice identities.
    pub(crate) fn open_holds(&self, device: DeviceId) -> impl Iterator<Item = (ItemId, Hold)> + '_ {
        self.holds
            .iter()
            .filter(move |(id, _)| id.device == device)
            .map(|(id, h)| (*id, h.item.clone()))
    }

    /// SP33, SP34: preflight this device's new lists before writing them. Peer
    /// reception uses the same limits. Refusal changes no state.
    pub fn check_additions(
        &self,
        device: DeviceId,
        reserve: &[Reserve],
        hold: &[Hold],
    ) -> Result<(), HoldError> {
        read_lists(
            &reserve.iter().map(Reserve::write).collect::<Vec<_>>(),
            &hold.iter().map(Hold::write).collect::<Vec<_>>(),
        )?;
        limits(
            self.reservations
                .iter()
                .filter(|(id, _)| id.device == device)
                .map(|(_, r)| r.item.size)
                .chain(reserve.iter().map(|r| r.size)),
            MAX_RESERVATIONS,
            MAX_RESERVED_BYTES,
        )
        .map_err(|_| HoldError::Reservations)?;
        limits(
            self.holds
                .iter()
                .filter(|(id, _)| id.device == device)
                .map(|(_, h)| h.item.size)
                .chain(hold.iter().map(|h| h.size)),
            MAX_HOLDS,
            MAX_HELD_BYTES,
        )
        .map_err(|_| HoldError::Holds)
    }

    /// SP33, SP34: receive a checked, consecutive change. Before fetching objects,
    /// preflight its parsed lists with `check_additions`; `named` is supplied once
    /// its references are verified against the versions and bodies. A replay of the
    /// latest change is harmless; gaps and older deliveries must be resolved by
    /// the chain reader first. The first change may follow a compacted checkpoint.
    pub fn receive(
        &mut self,
        change: &Change,
        named: &[Named],
        removed: bool,
        clock: &Vouched,
    ) -> Result<(), HoldError> {
        if let Some(last) = self.last.get(&change.device) {
            if last.s == change.seq && last.h == change.hash {
                return Ok(());
            }
            if change.seq != last.s + 1 || change.prev != Some(last.h) {
                return Err(HoldError::Chain);
            }
        }
        if named.iter().any(|n| {
            n.version.device != change.device
                || n.version.seq != change.seq
                || !change.versions.iter().any(|v| {
                    v.get("key").and_then(Value::as_str) == Some(n.key.as_str())
                        && super::record::version_hash(v) == n.version.hash
                })
        }) {
            return Err(HoldError::Shape);
        }
        let (reserve, hold) = read_lists(&change.reserve, &change.hold)?;
        self.check_additions(change.device, &reserve, &hold)?;
        // SP33: only a later change of this writer names its reservation.
        // Removed writers wait for the reader even if their naming change arrives.
        if !removed {
            self.reservations.retain(|id, r| {
                !(id.device == change.device
                    && id.seq < change.seq
                    && named
                        .iter()
                        .any(|n| n.kind == r.item.kind && n.name == r.item.name))
            });
        }
        for (id, h) in &mut self.holds {
            if id.device == change.device && id.seq < change.seq && h.naming.is_none() {
                if let Some(n) = named.iter().find(|n| {
                    n.kind == h.item.kind && n.name == h.item.name && n.key == h.item.for_key
                }) {
                    h.naming = Some((change.into(), n.version));
                }
            }
        }
        for (index, item) in reserve.into_iter().enumerate() {
            self.reservations.insert(
                ItemId {
                    device: change.device,
                    seq: change.seq,
                    index,
                },
                Reservation {
                    item,
                    first_seen: clock.now(),
                },
            );
        }
        for (index, item) in hold.into_iter().enumerate() {
            self.holds.insert(
                ItemId {
                    device: change.device,
                    seq: change.seq,
                    index,
                },
                Holding {
                    item,
                    change: change.into(),
                    accepted_at: None,
                    naming: None,
                },
            );
        }
        self.last.insert(
            change.device,
            Mark {
                s: change.seq,
                h: change.hash,
            },
        );
        Ok(())
    }

    /// SP34, SY11: publishing waits for the hold's acknowledgement and for the
    /// content to be present again. This check is for the future publishing caller.
    pub fn may_publish_naming(
        &self,
        id: ItemId,
        present_in_storage: bool,
        progress: &Progress<'_>,
    ) -> bool {
        !progress.removed.contains(&id.device)
            && present_in_storage
            && self
                .holds
                .get(&id)
                .is_some_and(|h| included_by_all(h.change, progress.accepted, progress.held))
    }

    /// SP34: own open holds whose target keys still wait for acknowledgements.
    /// The caller additionally withholds keys whose content is absent in storage:
    /// `Progress` describes frontiers, while `may_publish_naming` checks presence.
    /// New holds being published must have their keys withheld on that call too.
    pub fn withheld_keys(&self, device: DeviceId, progress: &Progress<'_>) -> BTreeSet<Key> {
        self.holds
            .iter()
            .filter(|(id, _)| id.device == device && !self.may_publish_naming(**id, true, progress))
            .map(|(_, h)| h.item.for_key.clone())
            .collect()
    }

    /// SP33, SP34: advance local timers and apply the reader's settled choices.
    /// A dangling week starts when this device observes all acknowledgements,
    /// not on receipt of the hold. Seeing its naming change stops that timer.
    pub fn advance(&mut self, clock: &Vouched, p: &Progress<'_>) {
        let chose = |key: &str, action: &str| {
            p.choices.iter().any(|v| {
                v.key.as_str() == key
                    && matches!(&v.content, Content::Inline(body)
                if body.get("choice").and_then(Value::as_str) == Some(action))
            })
        };
        self.reservations.retain(|id, r| {
            if p.removed.contains(&id.device) {
                !p.late_decided.contains(&id.device)
                    && !chose(&id.choice_key(false), "stop-waiting")
            } else {
                !(p.all_listed_read.contains(&id.device)
                    && clock.now().saturating_sub(r.first_seen) >= WEEK_MS)
            }
        });
        self.holds.retain(|id, h| {
            // SP34 overrides SY11's broader wording: only this exact Stop waiting
            // ends a removed writer's hold, never a timer, Keep or Discard.
            if p.removed.contains(&id.device) {
                return !chose(&id.choice_key(true), "stop-waiting");
            }
            if let Some((change, version)) = h.naming {
                let key = format!("choice:{}.{}.{}", version.device, version.seq, version.hash);
                return !included_by_all(change, p.accepted, p.held) && !chose(&key, "discard");
            }
            if included_by_all(h.change, p.accepted, p.held) {
                h.accepted_at.get_or_insert(clock.now());
            }
            !h.accepted_at
                .is_some_and(|t| clock.now().saturating_sub(t) >= WEEK_MS)
        });
    }

    /// SP33, SP34: protects an object while any device has an open entry for it.
    /// A false result is not permission to remove it; SP36 has further gates.
    pub fn protects(&self, kind: Kind, name: Hash32) -> bool {
        self.reservations
            .values()
            .any(|r| r.item.kind == kind && r.item.name == name)
            || self
                .holds
                .values()
                .any(|h| h.item.kind == kind && h.item.name == name)
    }

    pub fn reservation_count(&self, device: DeviceId) -> usize {
        self.reservations
            .keys()
            .filter(|id| id.device == device)
            .count()
    }

    pub fn hold_count(&self, device: DeviceId) -> usize {
        self.holds.keys().filter(|id| id.device == device).count()
    }
}

/// Checked inputs from the caller, not storage or clocks read by the ledger.
pub struct Progress<'a> {
    /// Output of `frontier::accepted_by_all`, not an enrollment checkpoint's floors.
    pub accepted: &'a BTreeMap<DeviceId, Mark>,
    pub held: &'a dyn Fn(&DeviceId, u64) -> Option<Held>,
    pub removed: &'a BTreeSet<DeviceId>,
    pub all_listed_read: &'a BTreeSet<DeviceId>,
    /// SP33: writers whose late changes the reader has finished keeping/discarding.
    pub late_decided: &'a BTreeSet<DeviceId>,
    /// SP12: only checked, applied, conflict-free choices; a conflicted choice
    /// releases nothing. Resolving choices belongs to the record merge caller.
    pub choices: &'a [Version],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::Keypair;
    use crate::sync::ids::Id16;
    use crate::sync::{chain, crypto, frontier, record};

    const LIB: Id16 = Id16([1; 16]);
    const WRITER: Id16 = Id16([2; 16]);

    fn reserve(size: u64) -> Reserve {
        Reserve {
            kind: Kind::Book,
            name: Hash32([3; 32]),
            size,
        }
    }

    fn hold(size: u64) -> Hold {
        Hold {
            kind: Kind::Book,
            name: Hash32([3; 32]),
            hash: Hash32([4; 32]),
            size,
            for_key: Key::parse("bookFile:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6")
                .unwrap()
                .0,
        }
    }

    fn clock(ms: u64) -> Vouched {
        let mut clock = Vouched::new(0);
        clock.tick(0, 0);
        clock.tick(ms, ms);
        clock
    }

    fn change(
        key: &Keypair,
        before: Option<&Change>,
        reserve: &[Reserve],
        hold: &[Hold],
    ) -> Change {
        let item = self::hold(1);
        let version = json!({"format": 1, "key": item.for_key.as_str(),
            "device": key.device().to_string(), "clock": [0, 0], "history": [],
            "body": {"hash": item.hash.to_string(), "size": 1}});
        let signed = chain::sign_change_with(
            key,
            &LIB,
            &WRITER,
            before.map_or(1, |c| c.seq + 1),
            before.map(|c| c.hash),
            &[version],
            reserve,
            hold,
        );
        chain::open_change(&signed.bytes, &LIB, &key.device(), &key.public()).unwrap()
    }

    fn named(c: &Change, item: &Hold) -> Named {
        Named {
            kind: item.kind,
            name: item.name,
            key: item.for_key.clone(),
            version: Entry {
                device: c.device,
                seq: c.seq,
                hash: record::version_hash(&c.versions[0]),
            },
        }
    }

    fn id(c: &Change) -> ItemId {
        ItemId {
            device: c.device,
            seq: c.seq,
            index: 0,
        }
    }

    fn choice(key: &str, action: &str) -> Version {
        let device = Id16([8; 16]);
        let raw = json!({"format": 1, "key": key, "device": device.to_string(),
            "clock": [0, 0], "history": [], "inline": {"choice": action}});
        let record::Checked::Version(v) = record::check_version(raw, device).unwrap() else {
            panic!()
        };
        v
    }

    #[derive(Default)]
    struct World {
        chains: BTreeMap<(DeviceId, u64), Held>,
        accepted: BTreeMap<DeviceId, Mark>,
        removed: BTreeSet<DeviceId>,
        read: BTreeSet<DeviceId>,
        decided: BTreeSet<DeviceId>,
        choices: Vec<Version>,
    }

    impl World {
        fn saw(&mut self, c: &Change) {
            self.chains.insert(
                (c.device, c.seq),
                Held {
                    h: c.hash,
                    prev: c.prev,
                    vroot: c.vroot,
                },
            );
        }

        // SP29: use the real frontier calculation, including the writer's own
        // inclusion and peers' resolved frontier/inherited high-water marks.
        fn acknowledge(&mut self, c: &Change, peers: &[(DeviceId, u64)]) {
            self.saw(c);
            let enrolled = peers.iter().map(|(d, _)| *d).chain([c.device]).collect();
            let peers = peers
                .iter()
                .map(|(d, s)| (*d, BTreeMap::from([(c.device, *s)])))
                .collect();
            self.accepted = frontier::accepted_by_all(
                &enrolled,
                &c.device,
                &BTreeMap::new(),
                &peers,
                &BTreeMap::from([(c.device, c.seq)]),
                &|d, s| self.chains.get(&(*d, s)).copied(),
                &self.accepted,
            );
        }

        fn using<T>(&self, f: impl FnOnce(&Progress<'_>) -> T) -> T {
            f(&Progress {
                accepted: &self.accepted,
                held: &|d, s| self.chains.get(&(*d, s)).copied(),
                removed: &self.removed,
                all_listed_read: &self.read,
                late_decided: &self.decided,
                choices: &self.choices,
            })
        }

        fn advance(&self, ledger: &mut Ledger, ms: u64) {
            self.using(|p| ledger.advance(&clock(ms), p));
        }
    }

    #[test]
    fn a_reservation_past_twenty_or_five_gb_is_refused() {
        let key = Keypair::from_seed(&[1; 32]);
        for items in [
            vec![reserve(1); MAX_RESERVATIONS],
            vec![reserve(MAX_RESERVED_BYTES)],
        ] {
            let mut ledger = Ledger::default();
            let first = change(&key, None, &items, &[]);
            ledger.receive(&first, &[], false, &clock(0)).unwrap();
            let next = change(&key, Some(&first), &[reserve(1)], &[]);
            assert_eq!(
                ledger.receive(&next, &[], false, &clock(1)),
                Err(HoldError::Reservations)
            );
            assert_eq!(ledger.reservation_count(key.device()), items.len());
            assert_eq!(ledger.last[&key.device()].s, 1, "refusal is atomic");
        }
    }

    #[test]
    fn a_reservation_ends_when_named() {
        let key = Keypair::from_seed(&[1; 32]);
        let c = change(&key, None, &[reserve(1)], &[]);
        let mut ledger = Ledger::default();
        ledger
            .receive(&c, &[named(&c, &hold(1))], false, &clock(0))
            .unwrap();
        assert_eq!(
            ledger.reservation_count(c.device),
            1,
            "same-change naming is too early"
        );
        let next = change(&key, Some(&c), &[], &[]);
        let mut wrong = named(&next, &hold(1));
        wrong.kind = Kind::Blob;
        let mut other = ledger.clone();
        other.receive(&next, &[wrong], false, &clock(1)).unwrap();
        assert!(other.protects(Kind::Book, reserve(1).name));
        ledger
            .receive(&next, &[named(&next, &hold(1))], false, &clock(1))
            .unwrap();
        assert_eq!(ledger.reservation_count(c.device), 0);
        ledger.receive(&next, &[], false, &clock(2)).unwrap();
        assert_eq!(
            ledger.reservation_count(c.device),
            0,
            "replay cannot reopen it"
        );
    }

    #[test]
    fn a_reservation_lapses_after_seven_days_only_once_its_writers_listed_changes_are_read() {
        let key = Keypair::from_seed(&[1; 32]);
        let c = change(&key, None, &[reserve(1)], &[]);
        let mut ledger = Ledger::default();
        ledger.receive(&c, &[], false, &clock(100)).unwrap();
        let mut world = World::default();
        world.advance(&mut ledger, 100 + WEEK_MS);
        assert_eq!(ledger.reservation_count(c.device), 1);
        world.read.insert(c.device);
        world.advance(&mut ledger, 100 + WEEK_MS - 1);
        assert_eq!(ledger.reservation_count(c.device), 1);
        world.advance(&mut ledger, 100 + WEEK_MS);
        assert_eq!(ledger.reservation_count(c.device), 0);
    }

    #[test]
    fn a_removed_writers_reservation_lasts_until_the_reader_decides() {
        let key = Keypair::from_seed(&[1; 32]);
        let c = change(&key, None, &[reserve(1)], &[]);
        let mut ledger = Ledger::default();
        ledger.receive(&c, &[], true, &clock(0)).unwrap();
        let next = change(&key, Some(&c), &[], &[]);
        ledger
            .receive(&next, &[named(&next, &hold(1))], true, &clock(1))
            .unwrap();
        let mut world = World::default();
        world.removed.insert(c.device);
        world.read.insert(c.device);
        world.advance(&mut ledger, 10 * WEEK_MS);
        assert_eq!(ledger.reservation_count(c.device), 1);
        let mut stopped = ledger.clone();
        world
            .choices
            .push(choice(&id(&c).choice_key(false), "stop-waiting"));
        world.advance(&mut stopped, 10 * WEEK_MS);
        assert_eq!(stopped.reservation_count(c.device), 0);
        world.choices.clear();
        // SP33: the late-change caller reports completed Keep or Discard alike.
        world.decided.insert(c.device);
        world.advance(&mut ledger, 10 * WEEK_MS);
        assert_eq!(ledger.reservation_count(c.device), 0);
    }

    #[test]
    fn an_object_in_storage_named_by_no_live_version_needs_a_hold() {
        assert!(needs_hold(false));
        assert!(!may_rely_on_stored_object(false));
        assert!(!needs_hold(true));
        assert!(may_rely_on_stored_object(true));
    }

    #[test]
    fn the_naming_change_waits_until_every_frontier_includes_the_hold() {
        let key = Keypair::from_seed(&[1; 32]);
        let c = change(&key, None, &[], &[hold(1)]);
        let mut ledger = Ledger::default();
        ledger.receive(&c, &[], false, &clock(0)).unwrap();
        let mut world = World::default();
        let (b, d) = (Id16([7; 16]), Id16([8; 16]));
        world.acknowledge(&c, &[(b, 1), (d, 0)]);
        assert!(!world.using(|p| ledger.may_publish_naming(id(&c), true, p)));
        world.acknowledge(&c, &[(b, 1), (d, 1)]);
        assert!(world.using(|p| ledger.may_publish_naming(id(&c), true, p)));
        assert!(!world.using(|p| ledger.may_publish_naming(id(&c), false, p)));
        world.chains.get_mut(&(c.device, c.seq)).unwrap().h = Hash32([9; 32]);
        assert!(
            !world.using(|p| ledger.may_publish_naming(id(&c), true, p)),
            "wrong branch"
        );
        world.accepted.clear();
        world.acknowledge(&c, &[]);
        assert!(
            world.using(|p| ledger.may_publish_naming(id(&c), true, p)),
            "one device waits for nobody"
        );
    }

    fn named_hold() -> (Ledger, Change, Change, World) {
        let key = Keypair::from_seed(&[1; 32]);
        let c = change(&key, None, &[], &[hold(1)]);
        let next = change(&key, Some(&c), &[], &[]);
        let mut ledger = Ledger::default();
        ledger.receive(&c, &[], false, &clock(0)).unwrap();
        ledger
            .receive(&next, &[named(&next, &hold(1))], false, &clock(1))
            .unwrap();
        let mut world = World::default();
        world.acknowledge(&c, &[]);
        world.saw(&next);
        (ledger, c, next, world)
    }

    #[test]
    fn a_hold_ends_when_every_frontier_includes_its_naming_change() {
        let (mut ledger, c, next, mut world) = named_hold();
        world.advance(&mut ledger, 10 * WEEK_MS);
        assert_eq!(
            ledger.hold_count(c.device),
            1,
            "naming arrived; no dangling timeout"
        );
        world.acknowledge(&next, &[(Id16([7; 16]), 1)]);
        world.advance(&mut ledger, 10 * WEEK_MS);
        assert_eq!(ledger.hold_count(c.device), 1);
        world.acknowledge(&next, &[(Id16([7; 16]), 2)]);
        world.advance(&mut ledger, 10 * WEEK_MS);
        assert_eq!(ledger.hold_count(c.device), 0);
    }

    #[test]
    fn a_hold_ends_when_the_reader_discards_it() {
        let (mut ledger, c, next, mut world) = named_hold();
        let version = named(&next, &hold(1)).version;
        world.choices.push(choice(
            &format!(
                "choice:{}.{}.{}",
                version.device,
                version.seq + 1,
                version.hash
            ),
            "discard",
        ));
        world.advance(&mut ledger, 1);
        assert_eq!(ledger.hold_count(c.device), 1);
        world.choices.push(choice(
            &format!("choice:{}.{}.{}", version.device, version.seq, version.hash),
            "discard",
        ));
        world.advance(&mut ledger, 1);
        assert_eq!(ledger.hold_count(c.device), 0);
    }

    #[test]
    fn a_hold_ends_after_seven_dangling_days() {
        let key = Keypair::from_seed(&[1; 32]);
        let c = change(&key, None, &[], &[hold(1)]);
        let mut ledger = Ledger::default();
        ledger.receive(&c, &[], false, &clock(0)).unwrap();
        let mut world = World::default();
        world.advance(&mut ledger, 3 * WEEK_MS);
        assert_eq!(
            ledger.hold_count(c.device),
            1,
            "receipt doesn't start the week"
        );
        world.acknowledge(&c, &[]);
        world.advance(&mut ledger, 3 * WEEK_MS);
        world.advance(&mut ledger, 4 * WEEK_MS - 1);
        assert_eq!(ledger.hold_count(c.device), 1);
        world.advance(&mut ledger, 4 * WEEK_MS);
        assert_eq!(ledger.hold_count(c.device), 0);
    }

    #[test]
    fn a_removed_writers_hold_ends_only_with_stop_waiting() {
        let (mut ledger, c, next, mut world) = named_hold();
        world.removed.insert(c.device);
        world.decided.insert(c.device);
        world.acknowledge(&next, &[]);
        let version = named(&next, &hold(1)).version;
        world.choices.push(choice(
            &format!("choice:{}.{}.{}", version.device, version.seq, version.hash),
            "discard",
        ));
        world
            .choices
            .push(choice(&id(&c).choice_key(false), "stop-waiting"));
        world.advance(&mut ledger, 100 * WEEK_MS);
        assert_eq!(ledger.hold_count(c.device), 1);
        world
            .choices
            .push(choice(&id(&c).choice_key(true), "stop-waiting"));
        world.advance(&mut ledger, 100 * WEEK_MS);
        assert_eq!(ledger.hold_count(c.device), 0);
        // A removed writer with no naming change cannot time out either.
        let key = Keypair::from_seed(&[1; 32]);
        let dangling = change(&key, None, &[], &[hold(1)]);
        ledger = Ledger::default();
        ledger.receive(&dangling, &[], true, &clock(0)).unwrap();
        world.choices.clear();
        world.advance(&mut ledger, 0);
        world.advance(&mut ledger, 100 * WEEK_MS);
        assert_eq!(ledger.hold_count(c.device), 1);
    }

    #[test]
    fn holds_past_two_hundred_or_two_gb_are_refused() {
        let key = Keypair::from_seed(&[1; 32]);
        for items in [vec![hold(1); MAX_HOLDS], vec![hold(MAX_HELD_BYTES)]] {
            let mut ledger = Ledger::default();
            let c = change(&key, None, &[], &items);
            ledger.receive(&c, &[], false, &clock(0)).unwrap();
            assert_eq!(
                ledger.check_additions(c.device, &[], &[hold(1)]),
                Err(HoldError::Holds),
                "local preflight"
            );
            let next = change(&key, Some(&c), &[], &[hold(1)]);
            assert_eq!(
                ledger.receive(&next, &[], false, &clock(0)),
                Err(HoldError::Holds),
                "peer reception"
            );
            assert_eq!(ledger.hold_count(c.device), items.len());
            assert_eq!(ledger.last[&c.device].s, 1);
        }
    }

    #[test]
    fn untrusted_wall_time_cannot_expire_reservations_or_holds() {
        let key = Keypair::from_seed(&[1; 32]);
        let c = change(&key, None, &[reserve(1)], &[hold(1)]);
        let mut ledger = Ledger::default();
        let mut time = clock(0);
        ledger.receive(&c, &[], false, &time).unwrap();
        let mut world = World::default();
        world.read.insert(c.device);
        world.acknowledge(&c, &[]);
        world.using(|p| ledger.advance(&time, p));
        time.tick(1, 8 * WEEK_MS);
        time.restart(0, 20 * WEEK_MS, false, false);
        assert!(time.untrusted_wall);
        world.using(|p| ledger.advance(&time, p));
        assert_eq!(ledger.reservation_count(c.device), 1);
        assert_eq!(ledger.hold_count(c.device), 1);
    }

    /// #149, SP33, SY11: both devices derive one content-addressed object name,
    /// with independent reservations protecting it until both name it. Actual
    /// provider conflict-copy repair and SP36 removal are not exercised: this
    /// library neither writes storage objects nor implements cleanup.
    #[test]
    fn two_devices_reserve_and_name_one_shared_object() {
        for kind in [Kind::Book, Kind::Blob] {
            let a = Keypair::from_seed(&[1; 32]);
            let b = Keypair::from_seed(&[2; 32]);
            let hash = crypto::payload_hash(b"same content");
            let name = |_: &Keypair| {
                Hash32::parse(&match kind {
                    Kind::Book => crypto::book_name(&[3; 32], &hash),
                    Kind::Blob => crypto::blob_name(&[3; 32], &hash),
                })
                .unwrap()
            };
            assert_eq!(name(&a), name(&b));
            let r = Reserve {
                kind,
                name: name(&a),
                size: 12,
            };
            let mut ledger = Ledger::default();
            let first_a = change(&a, None, std::slice::from_ref(&r), &[]);
            let first_b = change(&b, None, std::slice::from_ref(&r), &[]);
            for c in [&first_a, &first_b] {
                ledger.receive(c, &[], false, &clock(0)).unwrap();
            }
            assert_eq!(ledger.reservation_count(a.device()), 1);
            assert_eq!(ledger.reservation_count(b.device()), 1);
            let item = Hold {
                kind,
                name: r.name,
                hash,
                ..hold(12)
            };
            let next_a = change(&a, Some(&first_a), &[], &[]);
            ledger
                .receive(&next_a, &[named(&next_a, &item)], false, &clock(1))
                .unwrap();
            assert_eq!(ledger.reservation_count(a.device()), 0);
            assert!(
                ledger.protects(kind, r.name),
                "B's reservation still protects the shared object"
            );
            let next_b = change(&b, Some(&first_b), &[], &[]);
            ledger
                .receive(&next_b, &[named(&next_b, &item)], false, &clock(1))
                .unwrap();
            assert_eq!(ledger.reservation_count(b.device()), 0);
            assert!(!ledger.protects(kind, r.name));
            assert_eq!(named(&next_a, &item).name, named(&next_b, &item).name);
        }
    }
}
