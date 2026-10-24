//! SP29: frontiers, and what every enrolled device has accepted.
//!
//! A device file's `frontier` lists, for each other device whose changes it has
//! applied, the latest one with that device's own signature, so the entry proves
//! itself; `inherited` lists the accepted-by-all marks it holds (SP16). An entry
//! counts only once it resolves to a change in the named chain this device holds,
//! with the hash it names (and, for a frontier entry, a signature that is the named
//! device's). From what each enrolled device vouches for, a device keeps for every
//! device the newest change every enrolled device has included: its accepted-by-all
//! mark, which never moves back.
//!
//! PROVISIONAL: where SP29 leaves room, the reading here is Delegated to Claude,
//! not approved (`docs/decisions.md`, “Frontiers and accepted by all”).

use super::device_store::{LibraryState, StoreError};
use super::envelope::{self, Place, Signer};
use super::ids::{b64url_decode, b64url_encode, DeviceId, Hash32, Id16};
use super::trust::Mark;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One `frontier` entry (with its signer's `sig`) or `inherited` entry (without).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub d: DeviceId,
    pub s: u64,
    pub h: Hash32,
    pub sig: Option<[u8; 64]>,
}

/// A change this device holds in a device's chain: its payload hash, and the
/// `prev` and `vroot` its signature covers (SP6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub h: Hash32,
    pub prev: Option<Hash32>,
    pub vroot: Hash32,
}

/// Where an entry stands against the chain this device holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// It names a change this device holds, with its hash and, for a frontier
    /// entry, the named device's signature.
    Resolved,
    /// This device doesn't hold that change yet, or the named device's key.
    Pending,
    /// The chain shows it false: another hash at that sequence, or a signature
    /// that isn't the named device's. Refused, and its writer named (SP29).
    False,
}

/// Reads a `frontier` (`signed`) or `inherited` list. An entry that isn't SP16's
/// shape vouches for nothing and is left out.
pub fn entries(list: &[Value], signed: bool) -> Vec<Entry> {
    list.iter()
        .filter_map(|v| {
            let o = v.as_object()?;
            let sig = if signed {
                Some(
                    o.get("sig")
                        .and_then(Value::as_str)
                        .and_then(b64url_decode)
                        .and_then(|b| b.try_into().ok())?,
                )
            } else {
                None
            };
            Some(Entry {
                d: o.get("d").and_then(Value::as_str).and_then(Id16::parse)?,
                s: o.get("s").and_then(Value::as_u64).filter(|s| *s > 0)?,
                h: o.get("h").and_then(Value::as_str).and_then(Hash32::parse)?,
                sig,
            })
        })
        .collect()
}

/// SP29: an entry resolves to a change in the named chain this device holds, whose
/// hash matches and, for a frontier entry, whose signature is the named device's.
pub fn resolve(
    e: &Entry,
    library: &Id16,
    key: Option<&[u8; 32]>,
    held: &dyn Fn(&DeviceId, u64) -> Option<Held>,
) -> Resolution {
    let Some(c) = held(&e.d, e.s) else {
        return Resolution::Pending;
    };
    if c.h != e.h {
        return Resolution::False;
    }
    let Some(sig) = e.sig else {
        return Resolution::Resolved;
    };
    let Some(key) = key else {
        return Resolution::Pending;
    };
    let place = Place {
        seq: e.s,
        prev: c.prev,
        vroot: Some(c.vroot),
    };
    let message = envelope::message("change", library, &Signer::Device(e.d), &place, &c.h);
    if super::ids::device_id(key) == e.d && super::crypto::verify(key, &message, &sig) {
        Resolution::Resolved
    } else {
        Resolution::False
    }
}

/// What one device file vouches for: for each device, the highest sequence among
/// its resolved `frontier` and `inherited` entries (SP16: its frontier counts as at
/// least its own inherited entry), and the entries the chain shows false.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vouched {
    pub seen: BTreeMap<DeviceId, u64>,
    pub false_entries: Vec<Entry>,
}

pub fn vouched(
    frontier: &[Entry],
    inherited: &[Entry],
    library: &Id16,
    key_of: &dyn Fn(&DeviceId) -> Option<[u8; 32]>,
    held: &dyn Fn(&DeviceId, u64) -> Option<Held>,
) -> Vouched {
    let mut v = Vouched::default();
    for e in frontier.iter().chain(inherited) {
        match resolve(e, library, key_of(&e.d).as_ref(), held) {
            Resolution::Resolved => {
                let s = v.seen.entry(e.d).or_insert(0);
                *s = (*s).max(e.s);
            }
            Resolution::Pending => {}
            Resolution::False => v.false_entries.push(e.clone()),
        }
    }
    v
}

/// SP29: the accepted-by-all marks, from what each enrolled device vouches for.
///
/// For a device `d`, every enrolled device but `d` must include the change: this
/// device through what it has applied (`applied`), each other one through its
/// device file (`peers`); `d` itself has always seen its own changes. So in a
/// library with one enrolled device, nobody else is waited for, and its mark is its
/// latest change (`own`). A mark never moves back, and keeps its place when the
/// change it would move to isn't held here. Devices no longer enrolled keep the
/// mark they had: removing one drops only what it alone vouched for.
pub fn accepted_by_all(
    enrolled: &BTreeSet<DeviceId>,
    me: &DeviceId,
    applied: &BTreeMap<DeviceId, u64>,
    peers: &BTreeMap<DeviceId, BTreeMap<DeviceId, u64>>,
    own: &BTreeMap<DeviceId, u64>,
    held: &dyn Fn(&DeviceId, u64) -> Option<Held>,
    previous: &BTreeMap<DeviceId, Mark>,
) -> BTreeMap<DeviceId, Mark> {
    let mut out = previous.clone();
    for d in enrolled {
        let included = enrolled
            .iter()
            .filter(|e| *e != d)
            .map(|e| {
                if e == me {
                    applied.get(d).copied().unwrap_or(0)
                } else {
                    peers.get(e).and_then(|p| p.get(d)).copied().unwrap_or(0)
                }
            })
            .min()
            .unwrap_or_else(|| own.get(d).copied().unwrap_or(0));
        if included == 0 || out.get(d).is_some_and(|m| m.s >= included) {
            continue;
        }
        if let Some(c) = held(d, included) {
            out.insert(
                *d,
                Mark {
                    s: included,
                    h: c.h,
                },
            );
        }
    }
    out
}

/// SP16: this device's own `frontier` list: for each other device whose changes it
/// has applied, the latest one with that device's signature on it.
pub fn frontier_list(applied: &BTreeMap<DeviceId, (Mark, [u8; 64])>) -> Vec<Value> {
    applied
        .iter()
        .map(|(d, (m, sig))| {
            json!({ "d": d.to_string(), "s": m.s, "h": m.h.to_string(), "sig": b64url_encode(sig) })
        })
        .collect()
}

/// SP16: this device's `inherited` list: the accepted-by-all marks it holds.
pub fn inherited_list(marks: &BTreeMap<DeviceId, Mark>) -> Vec<Value> {
    marks
        .iter()
        .map(|(d, m)| json!({ "d": d.to_string(), "s": m.s, "h": m.h.to_string() }))
        .collect()
}

/// SP28: the accepted-by-all marks the device store keeps, outside the library, so
/// restoring the library can't move them back: `acceptedByAll` as
/// `{ ‹device›: { "s", "h" } }`. PROVISIONAL (decisions.md, “Frontiers and
/// accepted by all”, (4)).
pub fn stored_marks(state: &LibraryState) -> Result<BTreeMap<DeviceId, Mark>, StoreError> {
    let Some(v) = state.rest.get("acceptedByAll") else {
        return Ok(BTreeMap::new());
    };
    let o = v.as_object().ok_or(StoreError::Unreadable)?;
    o.iter()
        .map(|(d, m)| {
            let d = Id16::parse(d).ok_or(StoreError::Unreadable)?;
            let s = m
                .get("s")
                .and_then(Value::as_u64)
                .ok_or(StoreError::Unreadable)?;
            let h = m
                .get("h")
                .and_then(Value::as_str)
                .and_then(Hash32::parse)
                .ok_or(StoreError::Unreadable)?;
            Ok((d, Mark { s, h }))
        })
        .collect()
}

/// Stores the marks, never moving one back: a mark lower than the one stored keeps
/// the stored one.
pub fn store_marks(
    state: &mut LibraryState,
    marks: &BTreeMap<DeviceId, Mark>,
) -> Result<(), StoreError> {
    let mut all = stored_marks(state)?;
    for (d, m) in marks {
        if all.get(d).map_or(true, |o| m.s > o.s) {
            all.insert(*d, *m);
        }
    }
    let mut o = Map::new();
    for (d, m) in &all {
        o.insert(d.to_string(), json!({ "s": m.s, "h": m.h.to_string() }));
    }
    state.rest.insert("acceptedByAll".into(), Value::Object(o));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::chain;
    use crate::sync::crypto::Keypair;

    const LIB: Id16 = Id16([7; 16]);
    const W: Id16 = Id16([8; 16]);

    fn k(n: u8) -> Keypair {
        Keypair::from_seed(&[n; 32])
    }

    /// A device's chain of `n` empty changes: what this device holds of it, and
    /// each change's signature.
    fn chain_of(key: &Keypair, n: u64) -> (BTreeMap<u64, Held>, BTreeMap<u64, [u8; 64]>) {
        let mut held = BTreeMap::new();
        let mut sigs = BTreeMap::new();
        let mut prev = None;
        for s in 1..=n {
            let c = chain::sign_change(key, &LIB, &W, s, prev, &[]);
            let sig = envelope::open(&c.bytes, "change", &LIB).unwrap().sigs[0].sig;
            held.insert(
                s,
                Held {
                    h: c.hash,
                    prev,
                    vroot: envelope::vroot(&[]),
                },
            );
            sigs.insert(s, sig);
            prev = Some(c.hash);
        }
        (held, sigs)
    }

    #[test]
    fn a_frontier_entry_counts_once_it_resolves_with_its_signers_signature() {
        let a = k(1);
        let (held, sigs) = chain_of(&a, 3);
        let get =
            |d: &DeviceId, s: u64| (*d == a.device()).then(|| held.get(&s).copied()).flatten();
        let key = a.public();
        let entry = |s: u64, h: Hash32, sig: [u8; 64]| Entry {
            d: a.device(),
            s,
            h,
            sig: Some(sig),
        };
        assert_eq!(
            resolve(&entry(2, held[&2].h, sigs[&2]), &LIB, Some(&key), &get),
            Resolution::Resolved
        );
        // Not held yet.
        assert_eq!(
            resolve(&entry(4, held[&2].h, sigs[&2]), &LIB, Some(&key), &get),
            Resolution::Pending
        );
        // Another hash at that sequence, or another change's signature: false.
        assert_eq!(
            resolve(&entry(2, held[&3].h, sigs[&2]), &LIB, Some(&key), &get),
            Resolution::False
        );
        assert_eq!(
            resolve(&entry(2, held[&2].h, sigs[&3]), &LIB, Some(&key), &get),
            Resolution::False
        );
        // An inherited entry has no signature: its hash is enough.
        let inherited = Entry {
            sig: None,
            ..entry(2, held[&2].h, sigs[&2])
        };
        assert_eq!(resolve(&inherited, &LIB, None, &get), Resolution::Resolved);
    }

    #[test]
    fn a_device_file_vouches_for_the_higher_of_its_frontier_and_inherited_entries() {
        let a = k(1);
        let (held, sigs) = chain_of(&a, 5);
        let get =
            |d: &DeviceId, s: u64| (*d == a.device()).then(|| held.get(&s).copied()).flatten();
        let key_of = |d: &DeviceId| (*d == a.device()).then(|| a.public());
        let frontier = entries(
            &[
                json!({ "d": a.device().to_string(), "s": 2, "h": held[&2].h.to_string(), "sig": b64url_encode(&sigs[&2]) }),
            ],
            true,
        );
        let inherited = entries(
            &[
                json!({ "d": a.device().to_string(), "s": 4, "h": held[&4].h.to_string() }),
                // False: another hash at 5.
                json!({ "d": a.device().to_string(), "s": 5, "h": held[&1].h.to_string() }),
                // Not SP16's shape: left out.
                json!({ "d": a.device().to_string(), "s": 0, "h": held[&1].h.to_string() }),
            ],
            false,
        );
        assert_eq!(inherited.len(), 2);
        let v = vouched(&frontier, &inherited, &LIB, &key_of, &get);
        assert_eq!(v.seen[&a.device()], 4);
        assert_eq!(v.false_entries.len(), 1);
    }

    #[test]
    fn a_mark_is_what_every_enrolled_device_included_and_never_moves_back() {
        let (a, b, c) = (k(1), k(2), k(3));
        let (ha, _) = chain_of(&a, 6);
        let held = |d: &DeviceId, s: u64| (*d == a.device()).then(|| ha.get(&s).copied()).flatten();
        let enrolled: BTreeSet<DeviceId> =
            [a.device(), b.device(), c.device()].into_iter().collect();
        let me = b.device();
        // This device (B) applied A through 5; C's file vouches for A through 3.
        let applied: BTreeMap<DeviceId, u64> = [(a.device(), 5)].into_iter().collect();
        let mut peers: BTreeMap<DeviceId, BTreeMap<DeviceId, u64>> = BTreeMap::new();
        peers.insert(c.device(), [(a.device(), 3)].into_iter().collect());
        let own: BTreeMap<DeviceId, u64> = [(a.device(), 6)].into_iter().collect();
        let marks = accepted_by_all(
            &enrolled,
            &me,
            &applied,
            &peers,
            &own,
            &held,
            &BTreeMap::new(),
        );
        assert_eq!(marks[&a.device()], Mark { s: 3, h: ha[&3].h });
        // C then vouches for less: the mark stays.
        peers.insert(c.device(), [(a.device(), 1)].into_iter().collect());
        let again = accepted_by_all(&enrolled, &me, &applied, &peers, &own, &held, &marks);
        assert_eq!(again[&a.device()].s, 3);
        // With C removed, only B waits on A: the mark rises to what B applied.
        let two: BTreeSet<DeviceId> = [a.device(), b.device()].into_iter().collect();
        let after = accepted_by_all(&two, &me, &applied, &peers, &own, &held, &again);
        assert_eq!(after[&a.device()].s, 5);
        // Alone, a device waits on nobody: its mark is its latest change.
        let alone: BTreeSet<DeviceId> = [a.device()].into_iter().collect();
        let solo = accepted_by_all(
            &alone,
            &a.device(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &own,
            &held,
            &BTreeMap::new(),
        );
        assert_eq!(solo[&a.device()].s, 6);
    }

    #[test]
    fn the_device_store_keeps_marks_and_never_moves_one_back() {
        let mut state = LibraryState::new().unwrap();
        let a = k(1).device();
        let m = |s: u64| Mark {
            s,
            h: Hash32([s as u8; 32]),
        };
        store_marks(&mut state, &[(a, m(4))].into_iter().collect()).unwrap();
        store_marks(&mut state, &[(a, m(2))].into_iter().collect()).unwrap();
        assert_eq!(stored_marks(&state).unwrap()[&a], m(4));
        store_marks(&mut state, &[(a, m(7))].into_iter().collect()).unwrap();
        assert_eq!(stored_marks(&state).unwrap()[&a], m(7));
        // The lists this device writes in its file.
        let sig = [9u8; 64];
        let frontier = frontier_list(&[(a, (m(7), sig))].into_iter().collect());
        assert_eq!(entries(&frontier, true)[0].sig, Some(sig));
        let inherited = inherited_list(&stored_marks(&state).unwrap());
        assert_eq!(entries(&inherited, false)[0].s, 7);
    }
}
