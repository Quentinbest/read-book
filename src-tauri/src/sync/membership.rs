//! SP20, SP21: computing membership from the records in `trust/`.
//!
//! A device holds the genesis it joined and ignores every other. From it, it builds
//! the graph of records whose `prev` it holds and judges each against the
//! membership at its predecessor: which devices were trusted there, and which
//! sealed head was newest. The trusted set is then every device enrolled by a
//! record that counts, its rotations included, minus the closure of every removal
//! that counts. Records are judged in the order the graph allows, ties broken by
//! when this device first saw them, so what one device first knew to be the newest
//! sealed head stays its newest (SP20 (2), (4)).
//!
//! Sibling sealed heads (two removals made apart on one head) both count, and
//! their removals unite, on a device that held neither. A device whose device
//! store already held one of them as its newest sealed head treats the other as
//! overtaken: that removal waits for a `confirm` (SP20 (2), (4), (4a)).
//!
//! PROVISIONAL: where SP20 leaves room, the reading here is Delegated to Claude,
//! not approved (`docs/decisions.md`, “Computing membership” (1)–(7) and
//! “Sibling sealed heads” (1)–(6)).
//!
//! `merge` and the suspension of devices enrolled after a fork are 1c.4c-ii's:
//! until then a merge waits (`Wait::Fork`) and counts for nothing.

use super::envelope::Signer;
use super::ids::{DeviceId, Hash32};
use super::trust::{Body, Confirms, Mark, Record};
use std::collections::{BTreeMap, BTreeSet};

/// SP20: a library has at most 12 devices.
pub const MAX_DEVICES: usize = 12;
/// SP20 (prov.): under any device, at most 2 devices join a day.
pub const ENROLLMENTS_A_DAY: usize = 2;
/// SY7: past two rotations of one identity in 30 days, a rotation needs the code.
pub const ROTATIONS_WITHOUT_CODE: usize = 2;
/// The most records one computation judges, oldest first sight first, so a folder
/// filled with well-formed junk records can't exhaust memory or time (its
/// ancestor sets grow with the square of the graph). PROVISIONAL (decisions.md,
/// “Computing membership”, (7)); #122 makes the computation incremental.
pub const MAX_RECORDS: usize = 1024;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const ROTATION_WINDOW_MS: i64 = 30 * DAY_MS;

/// What membership is computed from.
pub struct Inputs<'a> {
    /// The genesis this device joined (SP28).
    pub genesis: Hash32,
    /// The records it holds, opened (`trust::open`), by name.
    pub records: &'a BTreeMap<Hash32, Record>,
    /// When this device first saw each record, by the time it can vouch for (SY17).
    /// A record missing here was seen now.
    pub first_seen: &'a BTreeMap<Hash32, i64>,
    /// Code enrollments that a live enrolled device's device file names a head
    /// including, or that the reader confirmed on this device (SP20 (2)).
    pub confirmed: &'a BTreeSet<Hash32>,
    /// Records in the membership head this device joined at (its checkpoint,
    /// SP24): its enrollments aren't timed again.
    pub checkpoint: &'a BTreeSet<Hash32>,
    /// The payload hash of a device's change `s` in the chain this device holds,
    /// if it holds it, for a rotation's `last` (SP20 (3)).
    pub held: &'a dyn Fn(&DeviceId, u64) -> Option<Hash32>,
    /// SP21: the newest sealed head, removals and rotations this device has
    /// accepted, from its device store (`Membership::accepted`). A graph in which
    /// any of them doesn't count is stale, never a reason to trust anyone again.
    pub accepted: &'a BTreeSet<Hash32>,
    /// The time this device can vouch for now.
    pub now_ms: i64,
}

/// Why a record doesn't count yet; it is judged again as things change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// SP20 (1): its signer's quota, or its signer's enrollment `n − 1`.
    Quota,
    /// SP20 (1), (2): the library would pass 12 devices.
    Cap,
    /// SP20 (2): a code enrollment nobody has confirmed yet.
    Unconfirmed,
    /// SP20 (3): this device doesn't hold the old chain through `last` yet.
    Chain,
    /// SP20 (4): a removal on a head this device knows a later sealed head to have
    /// overtaken. It counts once a `confirm` confirms it.
    Confirm,
    /// A `merge`, or a record after sealed heads no merge has joined (1c.4c-ii).
    Fork,
    /// Not judged: a predecessor this device doesn't hold, or past `MAX_RECORDS`.
    Unjudged,
}

/// Why a record counts for nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Another genesis than the one this device joined.
    OtherGenesis,
    /// Its signer wasn't trusted at its predecessor.
    Signer,
    /// A signature it needs, from a key the membership names, is missing or wrong.
    Signature,
    /// It introduces a key an ancestor record already introduced (SP20).
    KeyAgain,
    /// A code enrollment on a head another sealed head has overtaken (SP20 (2)).
    Overtaken,
    /// A rotation of an identity that has already rotated (SP27).
    Rotated,
    /// A third rotation in 30 days without the code's signature (SY7).
    NeedsCode,
    /// A removal naming a device the membership at its predecessor doesn't hold:
    /// what a remover hadn't seen is suspended, not removed (SP26).
    UnknownTarget,
    /// A rotation whose `last` isn't the old chain this device holds at that
    /// sequence: a conflict 1c.6 makes an equivocation proof of (SP20 (3), SP30).
    LastConflict,
    /// A code record that doesn't build on a sealed head.
    NotSealed,
    /// SP20 (1): a sibling enrollment the cap held back that a `confirm` didn't
    /// admit, when it admitted others.
    NotAdmitted,
    /// SP20 (4a): a `confirm` of a record that isn't a removal, or admitting an
    /// enrollment the cap didn't hold back, or more than fit under 12.
    Confirms,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Counts,
    Waits(Wait),
    Refused(Refusal),
}

/// One identity a counting record enrolled or rotated in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub device: DeviceId,
    pub key: [u8; 32],
    pub label: String,
    /// The record that enrolled it, or rotated it in.
    pub record: Hash32,
    /// The lineage that enrolled it: none for the first device and for the code.
    pub enroller: Option<DeviceId>,
    /// Its rotation lineage: the identity it was first enrolled as (SP27).
    pub lineage: DeviceId,
}

/// The membership a set of counting records makes.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub members: BTreeMap<DeviceId, Member>,
    /// Identities a counting rotation closed, with their successor and `last`.
    pub rotated: BTreeMap<DeviceId, (DeviceId, Option<Mark>)>,
    /// Every identity the closure of the counting removals takes away.
    pub removed: BTreeSet<DeviceId>,
    /// The cutoff each removal named for a device; the later counts (SP20).
    pub cutoffs: BTreeMap<DeviceId, Option<Mark>>,
}

impl State {
    /// A device trusted now: enrolled, not removed, and not rotated away.
    pub fn trusted(&self, d: &DeviceId) -> Option<&Member> {
        self.members
            .get(d)
            .filter(|_| !self.removed.contains(d) && !self.rotated.contains_key(d))
    }

    pub fn trusted_count(&self) -> usize {
        self.members
            .keys()
            .filter(|d| self.trusted(d).is_some())
            .count()
    }
}

/// The membership computed from the records.
#[derive(Clone, Debug)]
pub struct Membership {
    pub verdicts: BTreeMap<Hash32, Verdict>,
    pub state: State,
    /// The newest sealed head: the counting sealed head every other one leads to.
    pub newest_sealed: Option<Hash32>,
    /// Counting records none of whose ancestors still waits: their verdict can't
    /// change as the graph settles.
    settled: BTreeSet<Hash32>,
}

impl Membership {
    pub fn trusted(&self, d: &DeviceId) -> Option<&Member> {
        self.state.trusted(d)
    }

    /// The key an identity's changes are checked with, while trusted or not: a
    /// removed or rotated identity's changes up to its cutoff or `last` still are.
    pub fn key(&self, d: &DeviceId) -> Option<[u8; 32]> {
        self.state.members.get(d).map(|m| m.key)
    }

    pub fn verdict(&self, h: &Hash32) -> Option<Verdict> {
        self.verdicts.get(h).copied()
    }

    /// SP21: what the device store keeps so that a later graph lacking it is
    /// known to be stale: the newest sealed head, and every counting removal and
    /// rotation.
    /// Only settled records: one counting while an ancestor still waits could turn
    /// refused once that ancestor counts, and keeping it would make unchanged
    /// storage look stale.
    pub fn accepted(&self, records: &BTreeMap<Hash32, Record>) -> BTreeSet<Hash32> {
        self.settled
            .iter()
            .copied()
            .filter(|h| {
                matches!(
                    records.get(h).map(|r| &r.body),
                    Some(Body::Remove { .. } | Body::Rotate { .. })
                )
            })
            .chain(self.newest_sealed.filter(|h| self.settled.contains(h)))
            .collect()
    }
}

/// What a record follows in the graph: its predecessors and, for a `confirm`, the
/// removal or enrollments it names, which count from then on as if made on its
/// `prev` (SP20 (4a)).
fn links(r: &Record) -> Vec<Hash32> {
    let mut l = r.prevs();
    match &r.body {
        Body::Confirm {
            confirms: Confirms::Removal(h),
            ..
        } => l.push(*h),
        Body::Confirm {
            confirms: Confirms::Admits(hs),
            ..
        } => l.extend(hs.iter().copied()),
        _ => {}
    }
    l
}

/// The graph of records reachable from the genesis.
struct Graph<'a> {
    inputs: &'a Inputs<'a>,
    /// Topological order, ties by first sight.
    order: Vec<Hash32>,
    /// Each record's ancestors, what it links to among them (`links`).
    ancestors: BTreeMap<Hash32, BTreeSet<Hash32>>,
}

impl<'a> Graph<'a> {
    fn new(inputs: &'a Inputs<'a>) -> Self {
        let seen = |h: &Hash32| inputs.first_seen.get(h).copied().unwrap_or(inputs.now_ms);
        let mut order = Vec::new();
        let mut ancestors: BTreeMap<Hash32, BTreeSet<Hash32>> = BTreeMap::new();
        if inputs.records.contains_key(&inputs.genesis) {
            ancestors.insert(inputs.genesis, BTreeSet::new());
            order.push(inputs.genesis);
        }
        // Each record waits on one of its predecessors not yet placed, and moves to
        // the next when that one is placed: one entry a record, however many
        // predecessors it names, and nothing more for records the genesis never
        // reaches.
        let mut watching: BTreeMap<Hash32, Vec<Hash32>> = BTreeMap::new();
        for (h, r) in inputs.records {
            if *h == inputs.genesis {
                continue;
            }
            if let Some(first) = links(r).first() {
                watching.entry(*first).or_default().push(*h);
            }
        }
        // Repeatedly take, among records whose predecessors are all placed, the one
        // first seen (then the smaller hash).
        let mut ready: std::collections::BinaryHeap<std::cmp::Reverse<(i64, Hash32)>> =
            std::collections::BinaryHeap::new();
        let release = |h: &Hash32,
                       placed: &BTreeMap<Hash32, BTreeSet<Hash32>>,
                       watching: &mut BTreeMap<Hash32, Vec<Hash32>>,
                       ready: &mut std::collections::BinaryHeap<
            std::cmp::Reverse<(i64, Hash32)>,
        >| {
            for c in watching.remove(h).unwrap_or_default() {
                match links(&inputs.records[&c])
                    .into_iter()
                    .find(|p| !placed.contains_key(p))
                {
                    Some(next) => watching.entry(next).or_default().push(c),
                    None => ready.push(std::cmp::Reverse((seen(&c), c))),
                }
            }
        };
        if !order.is_empty() {
            release(&inputs.genesis, &ancestors, &mut watching, &mut ready);
        }
        while let Some(std::cmp::Reverse((_, h))) = ready.pop() {
            if order.len() >= MAX_RECORDS {
                break;
            }
            let mut anc = BTreeSet::new();
            for p in links(&inputs.records[&h]) {
                anc.insert(p);
                anc.extend(ancestors[&p].iter().copied());
            }
            ancestors.insert(h, anc);
            order.push(h);
            release(&h, &ancestors, &mut watching, &mut ready);
        }
        Graph {
            inputs,
            order,
            ancestors,
        }
    }

    /// `a` descends from `b` (or is it).
    fn descends(&self, a: &Hash32, b: &Hash32) -> bool {
        a == b || self.ancestors.get(a).is_some_and(|s| s.contains(b))
    }

    /// The membership the counting records among `set` make.
    fn state(&self, verdicts: &BTreeMap<Hash32, Verdict>, set: &BTreeSet<Hash32>) -> State {
        let mut st = State::default();
        let mut removals: Vec<&Body> = Vec::new();
        for h in self.order.iter().filter(|h| set.contains(*h)) {
            if verdicts.get(h) != Some(&Verdict::Counts) {
                continue;
            }
            let r = &self.inputs.records[h];
            match &r.body {
                Body::Genesis {
                    device, key, label, ..
                } => {
                    st.members.insert(
                        *device,
                        Member {
                            device: *device,
                            key: *key,
                            label: label.clone(),
                            record: *h,
                            enroller: None,
                            lineage: *device,
                        },
                    );
                }
                Body::Enroll {
                    device,
                    key,
                    label,
                    by,
                    ..
                } => {
                    let enroller = by.and_then(|b| st.members.get(&b).map(|m| m.lineage));
                    st.members.entry(*device).or_insert(Member {
                        device: *device,
                        key: *key,
                        label: label.clone(),
                        record: *h,
                        enroller,
                        lineage: *device,
                    });
                }
                Body::Rotate {
                    old,
                    new,
                    key,
                    last,
                    ..
                } => {
                    if let Some(o) = st.members.get(old).cloned() {
                        st.members.insert(
                            *new,
                            Member {
                                device: *new,
                                key: *key,
                                label: o.label.clone(),
                                record: *h,
                                enroller: o.enroller,
                                lineage: o.lineage,
                            },
                        );
                        st.rotated.insert(*old, (*new, *last));
                    }
                }
                b @ Body::Remove { .. } => removals.push(b),
                _ => {}
            }
        }
        // The closure of every counting removal: the removed lineages, every lineage
        // they enrolled (directly or through others), and every identity of those.
        let mut gone: BTreeSet<DeviceId> = BTreeSet::new();
        for b in &removals {
            if let Body::Remove { removes, .. } = b {
                for rm in removes {
                    if let Some(m) = st.members.get(&rm.device) {
                        gone.insert(m.lineage);
                    }
                    // SP20: when one device has two cutoffs, the later counts.
                    let c = st.cutoffs.entry(rm.device).or_insert(rm.cutoff);
                    if rm.cutoff.map(|m| m.s) > c.map(|m| m.s) {
                        *c = rm.cutoff;
                    }
                }
            }
        }
        let gone = closure(&st.members, gone);
        st.removed = st
            .members
            .values()
            .filter(|m| gone.contains(&m.lineage))
            .map(|m| m.device)
            .collect();
        st
    }

    /// The counting sealed heads among `set`.
    fn sealed(&self, verdicts: &BTreeMap<Hash32, Verdict>, set: &BTreeSet<Hash32>) -> Vec<Hash32> {
        self.order
            .iter()
            .filter(|h| set.contains(*h))
            .filter(|h| verdicts.get(*h) == Some(&Verdict::Counts))
            .filter(|h| self.inputs.records[*h].seal().is_some())
            .copied()
            .collect()
    }

    /// The newest of `heads`: the one every other leads to, if there is one.
    fn newest(&self, heads: &[Hash32]) -> Option<Hash32> {
        heads
            .iter()
            .find(|a| heads.iter().all(|b| self.descends(a, b)))
            .copied()
    }

    /// A record's predecessors and their ancestors, inclusive.
    fn upto(&self, h: &Hash32) -> BTreeSet<Hash32> {
        let mut s: BTreeSet<Hash32> = self.ancestors[h].clone();
        s.remove(h);
        s
    }

    /// A record and its ancestors: the membership at it.
    fn through(&self, h: &Hash32) -> BTreeSet<Hash32> {
        let mut s = self.ancestors.get(h).cloned().unwrap_or_default();
        s.insert(*h);
        s
    }

    /// The newest sealed head at a record's predecessors.
    fn base(&self, verdicts: &BTreeMap<Hash32, Verdict>, h: &Hash32) -> Option<Hash32> {
        let before = self.upto(h);
        self.newest(&self.sealed(verdicts, &before))
    }
}

/// What a pass of judging learns for the next, until nothing changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Pass {
    /// Code enrollments and confirms that a sealed head judged after them overtakes.
    overtaken: BTreeSet<Hash32>,
    /// SP20: the cap is counted over the whole graph, so removals judged later in
    /// it make room for enrollments judged earlier: the devices a counting removal
    /// anywhere takes away.
    freed: BTreeSet<DeviceId>,
    /// Removals a counting `confirm` confirms (SP20 (4a)).
    confirmed: BTreeSet<Hash32>,
    /// Enrollments a counting `confirm` admits (SP20 (1)).
    admitted: BTreeSet<Hash32>,
    /// SP20 (1): enrollments on sibling branches that together passed the cap.
    contended: BTreeSet<Hash32>,
    /// Whether they now all fit under it, removals having made room.
    contended_fit: bool,
}

impl Pass {
    fn next(&self, g: &Graph<'_>, verdicts: &BTreeMap<Hash32, Verdict>) -> Pass {
        let inputs = g.inputs;
        let all: BTreeSet<Hash32> = g.order.iter().copied().collect();
        let state = g.state(verdicts, &all);
        let sealed = g.sealed(verdicts, &all);
        let counting: Vec<&Hash32> = g
            .order
            .iter()
            .filter(|h| verdicts.get(*h) == Some(&Verdict::Counts))
            .collect();
        let mut next = self.clone();
        next.freed = state.removed.clone();
        // A code enrollment or a confirm on a head that a sealed head judged after
        // it overtook: judged again as refused.
        // Rebuilt each pass from the sealed heads counting now, so a head that
        // counted for a pass and then stopped overtakes nothing.
        next.overtaken = g
            .order
            .iter()
            .copied()
            .filter(|h| {
                let prev = match &inputs.records[h].body {
                    Body::Enroll { by: None, prev, .. } | Body::Confirm { prev, .. } => prev,
                    _ => return false,
                };
                sealed
                    .iter()
                    .any(|s| s != prev && g.descends(s, prev) && !g.descends(s, h))
            })
            .collect();
        next.confirmed.clear();
        next.admitted.clear();
        for h in &counting {
            match &inputs.records[*h].body {
                Body::Confirm {
                    confirms: Confirms::Removal(r),
                    ..
                } => {
                    next.confirmed.insert(*r);
                }
                Body::Confirm {
                    confirms: Confirms::Admits(e),
                    ..
                } => next.admitted.extend(e.iter().copied()),
                _ => {}
            }
        }
        // SP20 (1): an enrollment the cap holds back beside a counting one on a
        // sibling branch: neither came first, so neither counts until removals
        // make room or a `confirm` admits the reader's choice. Siblings are apart
        // in the graph and built on one sealed head or on sibling ones; an
        // enrollment on an older head than the other's isn't a sibling of it, so
        // a device long trusted is never held back by a newcomer.
        let enrollments = |v: Verdict| -> Vec<(Hash32, Option<Hash32>)> {
            g.order
                .iter()
                .filter(|h| matches!(inputs.records[*h].body, Body::Enroll { .. }))
                .filter(|h| verdicts.get(*h) == Some(&v) && !inputs.checkpoint.contains(*h))
                .map(|h| (*h, g.base(verdicts, h)))
                .collect()
        };
        let counted = enrollments(Verdict::Counts);
        for (w, bw) in enrollments(Verdict::Waits(Wait::Cap)) {
            for (e, be) in &counted {
                let apart = !g.descends(&w, e) && !g.descends(e, &w);
                let older = match (bw, be) {
                    (Some(x), Some(y)) => x != *y && (g.descends(&x, y) || g.descends(y, &x)),
                    _ => true,
                };
                if apart && !older {
                    next.contended.insert(w);
                    next.contended.insert(*e);
                }
            }
        }
        let theirs = enrolled(inputs, &next.contended);
        let others = state
            .members
            .keys()
            .filter(|d| state.trusted(d).is_some() && !theirs.contains(*d))
            .count();
        next.contended_fit = others + theirs.len() <= MAX_DEVICES;
        next
    }
}

/// The devices some enrollments enrol.
fn enrolled(inputs: &Inputs<'_>, records: &BTreeSet<Hash32>) -> BTreeSet<DeviceId> {
    records
        .iter()
        .filter_map(|h| match inputs.records.get(h).map(|r| &r.body) {
            Some(Body::Enroll { device, .. }) => Some(*device),
            _ => None,
        })
        .collect()
}

/// SP21: storage that lacks, or no longer counts, something this device has
/// accepted: stale (waited for, SY30) or a restored library (SY27).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stale {
    pub missing: Vec<Hash32>,
}

/// SP20: computes the membership, or `Stale` (SP21) when the records lack, or
/// don't count, something this device has accepted.
pub fn compute(inputs: &Inputs<'_>) -> Result<Membership, Stale> {
    let g = Graph::new(inputs);
    // SP20 (2), (4): this device's own view, the newest of the sealed heads its
    // device store holds. A sealed head that isn't it or after it has been
    // overtaken here. With sibling heads held, there is none.
    let held: Vec<Hash32> = inputs
        .accepted
        .iter()
        .filter(|h| g.ancestors.contains_key(*h) && inputs.records[*h].seal().is_some())
        .copied()
        .collect();
    let local = g.newest(&held);
    // Each pass judges with what the previous one found (`Pass`), until nothing
    // changes. Each pass that changes something unlocks or settles a record, so
    // the graph's size bounds the passes; the bound only keeps a pathological
    // graph from looping.
    let mut pass = Pass::default();
    let mut passes = 0;
    loop {
        passes += 1;
        let verdicts = judge(&g, &pass, local);
        let all: BTreeSet<Hash32> = g.order.iter().copied().collect();
        let next = pass.next(&g, &verdicts);
        if next != pass && passes <= 2 * g.order.len() {
            pass = next;
            continue;
        }
        let sealed = g.sealed(&verdicts, &all);
        let state = g.state(&verdicts, &all);
        let newest_sealed = g.newest(&sealed);
        let mut verdicts = verdicts;
        // Another genesis counts for nothing; a record left out of the graph (a
        // predecessor not held, or past the bound) isn't judged yet.
        for (h, r) in inputs.records {
            verdicts.entry(*h).or_insert(if r.prevs().is_empty() {
                Verdict::Refused(Refusal::OtherGenesis)
            } else {
                Verdict::Waits(Wait::Unjudged)
            });
        }
        let missing: Vec<Hash32> = inputs
            .accepted
            .iter()
            .filter(|h| verdicts.get(*h) != Some(&Verdict::Counts))
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(Stale { missing });
        }
        let settled = g
            .order
            .iter()
            .filter(|h| verdicts.get(*h) == Some(&Verdict::Counts))
            .filter(|h| {
                g.ancestors[*h]
                    .iter()
                    .all(|a| !matches!(verdicts.get(a), Some(Verdict::Waits(_))))
            })
            // A rotation is unsettled while another rotation of the same identity
            // waits, placed or not yet (its predecessor missing): that one stands
            // once it counts (one successor, SP27).
            .filter(|h| match &inputs.records[*h].body {
                Body::Rotate { old, .. } => !inputs.records.iter().any(|(o, r)| {
                    o != *h
                        && matches!(verdicts.get(o), Some(Verdict::Waits(_)))
                        && matches!(&r.body, Body::Rotate { old: o2, .. } if o2 == old)
                }),
                _ => true,
            })
            .copied()
            .collect();
        return Ok(Membership {
            verdicts,
            state,
            newest_sealed,
            settled,
        });
    }
}

fn judge(g: &Graph<'_>, pass: &Pass, local: Option<Hash32>) -> BTreeMap<Hash32, Verdict> {
    let inputs = g.inputs;
    let seen = |h: &Hash32| inputs.first_seen.get(h).copied().unwrap_or(inputs.now_ms);
    let recent = |h: &Hash32| !inputs.checkpoint.contains(h) && seen(h) > inputs.now_ms - DAY_MS;
    let mut verdicts: BTreeMap<Hash32, Verdict> = BTreeMap::new();
    for h in &g.order {
        let r = &inputs.records[h];
        if *h == inputs.genesis {
            verdicts.insert(*h, Verdict::Counts);
            continue;
        }
        let before = g.upto(h);
        let st = g.state(&verdicts, &before);
        let sealed_before = g.sealed(&verdicts, &before);
        // All counting records so far, for what is counted over the whole graph.
        let so_far: BTreeSet<Hash32> = verdicts.keys().copied().collect();
        let global = g.state(&verdicts, &so_far);
        let all_sealed = g.sealed(&verdicts, &so_far);
        // SP20: a key any ancestor record introduced, whatever that record's
        // verdict, can't be introduced again.
        let introduced = |key: &[u8; 32], device: &DeviceId| {
            before.iter().any(|a| match &inputs.records[a].body {
                Body::Genesis {
                    device: d, key: k, ..
                }
                | Body::Enroll {
                    device: d, key: k, ..
                }
                | Body::Rotate { new: d, key: k, .. } => k == key || d == device,
                _ => false,
            })
        };
        let v = match &r.body {
            // Another genesis, a claim or an escrow is never in the graph: none has a
            // `prev`.
            Body::Genesis { .. } | Body::Claim { .. } | Body::Escrow { .. } => continue,
            // Merges are 1c.4c-ii's.
            Body::Merge { .. } => Verdict::Waits(Wait::Fork),
            Body::Confirm { prev, confirms, .. } => {
                // SP20 (4a): the membership at its `prev`, the newest sealed head.
                let st = g.state(&verdicts, &g.through(prev));
                let code = inputs.records[prev].seal().map(|c| c.code_key);
                let target = match confirms {
                    // A removal waiting for it (or counting already, as a sibling):
                    // its own signatures held at its own `prev` when it was judged.
                    Confirms::Removal(rm) => match &inputs.records[rm].body {
                        Body::Remove { removes, .. }
                            if matches!(
                                verdicts.get(rm),
                                Some(Verdict::Counts | Verdict::Waits(Wait::Confirm))
                            ) =>
                        {
                            Some(closure(
                                &st.members,
                                removes
                                    .iter()
                                    .map(|x| {
                                        st.members.get(&x.device).map_or(x.device, |m| m.lineage)
                                    })
                                    .collect(),
                            ))
                        }
                        _ => None,
                    },
                    // Enrollments the cap held back, fitting together under 12.
                    Confirms::Admits(list) => {
                        let theirs = enrolled(inputs, &pass.contended);
                        let others = global
                            .members
                            .keys()
                            .filter(|d| {
                                global.trusted(d).is_some()
                                    && !pass.freed.contains(*d)
                                    && !theirs.contains(*d)
                            })
                            .count();
                        let admitted = enrolled(inputs, &list.iter().copied().collect());
                        // The reader's first choice stands: a later confirm can't
                        // admit one it left out.
                        let chosen = verdicts.iter().any(|(x, v)| {
                            *v == Verdict::Counts
                                && matches!(
                                    inputs.records[x].body,
                                    Body::Confirm {
                                        confirms: Confirms::Admits(_),
                                        ..
                                    }
                                )
                        });
                        let fits = !chosen
                            && list.iter().all(|e| pass.contended.contains(e))
                            && others + admitted.len() <= MAX_DEVICES;
                        fits.then(BTreeSet::new)
                    }
                };
                // Its device signer: trusted at `prev`, and removed neither by the
                // removal it confirms nor by any removal that counts.
                let signed = target.as_ref().is_some_and(|gone| {
                    st.members.values().any(|m| {
                        st.trusted(&m.device).is_some()
                            && !gone.contains(&m.lineage)
                            && !global.removed.contains(&m.device)
                            && !pass.freed.contains(&m.device)
                            && r.signed_by(&Signer::Device(m.device), &m.key, 0)
                    })
                });
                if code.is_none() || verdicts.get(prev) != Some(&Verdict::Counts) {
                    Verdict::Refused(Refusal::NotSealed)
                } else if pass.overtaken.contains(h)
                    || all_sealed
                        .iter()
                        .any(|s| s != prev && g.descends(s, prev) && !g.descends(s, h))
                {
                    Verdict::Refused(Refusal::Overtaken)
                } else if !code.is_some_and(|k| r.signed_by(&Signer::Code, &k, 0)) {
                    Verdict::Refused(Refusal::Signature)
                } else if target.is_none() {
                    Verdict::Refused(Refusal::Confirms)
                } else if !signed {
                    Verdict::Refused(Refusal::Signer)
                } else {
                    Verdict::Counts
                }
            }
            Body::Enroll {
                device,
                key,
                by: Some(signer),
                n,
                ..
            } => {
                let n = n.unwrap_or(0);
                match st.trusted(signer) {
                    None => Verdict::Refused(Refusal::Signer),
                    Some(s) if !r.signed_by(&Signer::Device(*signer), &s.key, n) => {
                        Verdict::Refused(Refusal::Signature)
                    }
                    Some(_) if introduced(key, device) => Verdict::Refused(Refusal::KeyAgain),
                    Some(s) => {
                        let lineage = s.lineage;
                        // The lineage's enrollments held here, by their `n`.
                        // Only enrollments that count: a signed-looking record whose
                        // enroller's signature doesn't hold proves no history.
                        let by_n = |k: u64| {
                            // Among the records judged so far, which the graph's
                            // bound limits: never a scan of everything held.
                            verdicts
                                .iter()
                                .filter(|(_, v)| **v == Verdict::Counts)
                                .map(|(x, _)| (x, &inputs.records[x]))
                                .find(|(_, o)| {
                                    matches!(&o.body, Body::Enroll { by: Some(b), n: Some(m), .. }
                                        if *m == k && global.members.get(b).map(|x| x.lineage) == Some(lineage))
                                })
                        };
                        // Its `n` follows the lineage's `n − 1`, which this device must
                        // hold; `n − 2` was first seen a day ago; and its ancestors'
                        // quotas allow it.
                        let waits_for_quota = (n > 1 && by_n(n - 1).is_none())
                            || (!inputs.checkpoint.contains(h)
                                && n > 2
                                && by_n(n - 2).is_some_and(|(k, _)| recent(k)))
                            || !enrollment_quota(g, &verdicts, &global, h, lineage, &recent);
                        if waits_for_quota {
                            Verdict::Waits(Wait::Quota)
                        } else {
                            under_cap(pass, &global, h)
                        }
                    }
                }
            }
            Body::Enroll {
                prev,
                device,
                key,
                by: None,
                ..
            } => match inputs.records.get(prev).and_then(Record::seal) {
                None => Verdict::Refused(Refusal::NotSealed),
                Some(_) if pass.overtaken.contains(h) => Verdict::Refused(Refusal::Overtaken),
                // The code of the sealed head it builds on, which must be the newest.
                Some(seal) if !r.signed_by(&Signer::Code, &seal.code_key, 0) => {
                    Verdict::Refused(Refusal::Signature)
                }
                // A sibling this device treats as overtaken waits for a confirm,
                // so a code enrollment on it is refused here (SP20 (2)).
                Some(_) if verdicts.get(prev) != Some(&Verdict::Counts) => {
                    Verdict::Refused(Refusal::NotSealed)
                }
                Some(_)
                    if all_sealed
                        .iter()
                        .any(|s| s != prev && g.descends(s, prev) && !g.descends(s, h)) =>
                {
                    Verdict::Refused(Refusal::Overtaken)
                }
                Some(_) if introduced(key, device) => Verdict::Refused(Refusal::KeyAgain),
                Some(_) if !inputs.confirmed.contains(h) => Verdict::Waits(Wait::Unconfirmed),
                Some(_) => under_cap(pass, &global, h),
            },
            Body::Rotate {
                old,
                last,
                new,
                key,
                ..
            } => match st.trusted(old) {
                None if st.rotated.contains_key(old) => Verdict::Refused(Refusal::Rotated),
                // SP27: one successor, anywhere in the graph. A second rotation of the
                // same identity is an equivocation, which 1c.6 makes a proof of; here
                // the one first seen stands and the other counts for nothing.
                Some(_) if global.rotated.contains_key(old) => Verdict::Refused(Refusal::Rotated),
                None => Verdict::Refused(Refusal::Signer),
                Some(o) => {
                    let seq = last.map_or(1, |m| m.s + 1);
                    let lineage = o.lineage;
                    // SY7: rotations of this lineage first seen in the last 30 days.
                    let recent_rotations = g
                        .order
                        .iter()
                        .filter(|x| verdicts.get(*x) == Some(&Verdict::Counts))
                        .filter(|x| {
                            matches!(&inputs.records[*x].body, Body::Rotate { old: o2, .. }
                                if global.members.get(o2).map(|m| m.lineage) == Some(lineage))
                        })
                        // Checkpoint rotations count too: the checkpoint's exception
                        // is for enrollment timing only, and a device that just
                        // joined errs towards asking for the code.
                        .filter(|x| seen(x) > inputs.now_ms - ROTATION_WINDOW_MS)
                        .count();
                    let code_key = g
                        .newest(&sealed_before)
                        .and_then(|s| inputs.records[&s].seal().map(|c| c.code_key));
                    if !r.signed_by(&Signer::Device(*old), &o.key, seq) {
                        Verdict::Refused(Refusal::Signature)
                    } else if introduced(key, new) {
                        Verdict::Refused(Refusal::KeyAgain)
                    } else if recent_rotations >= ROTATIONS_WITHOUT_CODE
                        && !code_key.is_some_and(|k| r.signed_by(&Signer::Code, &k, 0))
                    {
                        Verdict::Refused(Refusal::NeedsCode)
                    } else {
                        // The old chain must be held through exactly `last`.
                        match last.map(|m| ((inputs.held)(old, m.s), m.h)) {
                            None => Verdict::Counts,
                            Some((None, _)) => Verdict::Waits(Wait::Chain),
                            Some((Some(held), named)) if held != named => {
                                Verdict::Refused(Refusal::LastConflict)
                            }
                            Some(_) => Verdict::Counts,
                        }
                    }
                }
            },
            Body::Remove { removes, .. } => {
                // The newest sealed head at its predecessor, whose code must sign.
                let newest = g.newest(&sealed_before);
                match newest {
                    None => Verdict::Waits(Wait::Fork),
                    Some(s) => {
                        let code = inputs.records[&s].seal().map(|c| c.code_key);
                        // What it removes, as lineages: removing any identity of a
                        // lineage removes all of them (SP27).
                        // Every device it names must be known here: one enrolled or
                        // rotated in later could otherwise turn out to be its own
                        // signer's lineage.
                        let unknown = removes.iter().any(|x| !st.members.contains_key(&x.device));
                        // And what they enrolled: a signer the cascade would take
                        // is removed by it too.
                        let removed = closure(
                            &st.members,
                            removes
                                .iter()
                                .map(|x| st.members.get(&x.device).map_or(x.device, |m| m.lineage))
                                .collect(),
                        );
                        // One signer trusted at its predecessor and not removed by it.
                        let device_signed = st.members.values().any(|m| {
                            st.trusted(&m.device).is_some()
                                && !removed.contains(&m.lineage)
                                && r.signed_by(&Signer::Device(m.device), &m.key, 0)
                        });
                        if unknown {
                            Verdict::Refused(Refusal::UnknownTarget)
                        } else if !device_signed {
                            Verdict::Refused(Refusal::Signer)
                        } else if !code.is_some_and(|k| r.signed_by(&Signer::Code, &k, 0)) {
                            Verdict::Refused(Refusal::Signature)
                        } else if local.is_some_and(|n| !g.descends(&s, &n))
                            && !inputs.accepted.contains(h)
                            && !pass.confirmed.contains(h)
                        {
                            // SP20 (4): built on a head this device knows a later
                            // sealed head overtook. A sibling seen at once instead
                            // counts, and the removals unite.
                            Verdict::Waits(Wait::Confirm)
                        } else {
                            Verdict::Counts
                        }
                    }
                }
            }
        };
        verdicts.insert(*h, v);
    }
    verdicts
}

/// SP20: the closure of removing `lineages`: them, and every lineage they enrolled,
/// directly or through others.
fn closure(
    members: &BTreeMap<DeviceId, Member>,
    mut gone: BTreeSet<DeviceId>,
) -> BTreeSet<DeviceId> {
    loop {
        let more: Vec<DeviceId> = members
            .values()
            .filter(|m| !gone.contains(&m.lineage))
            .filter(|m| m.enroller.is_some_and(|e| gone.contains(&e)))
            .map(|m| m.lineage)
            .collect();
        if more.is_empty() {
            return gone;
        }
        gone.extend(more);
    }
}

/// SP20 (1), (2): an enrollment that passes everything else, against the cap. One
/// of contending siblings counts once admitted, or once they all fit; when a
/// `confirm` admitted others, it doesn't. PROVISIONAL (decisions.md, “Sibling
/// sealed heads”, (4)): every contending enrollment is in one pool.
fn under_cap(pass: &Pass, global: &State, h: &Hash32) -> Verdict {
    if pass.admitted.contains(h) {
        Verdict::Counts
    } else if pass.contended.contains(h) {
        if !pass.admitted.is_empty() {
            Verdict::Refused(Refusal::NotAdmitted)
        } else if pass.contended_fit {
            Verdict::Counts
        } else {
            Verdict::Waits(Wait::Cap)
        }
    } else if cap_count(global, &pass.freed) >= MAX_DEVICES {
        Verdict::Waits(Wait::Cap)
    } else {
        Verdict::Counts
    }
}

/// The devices the cap counts: those trusted so far, less those a counting removal
/// anywhere in the graph takes away (`freed`, from the previous pass).
fn cap_count(global: &State, freed: &BTreeSet<DeviceId>) -> usize {
    global
        .members
        .keys()
        .filter(|d| global.trusted(d).is_some() && !freed.contains(*d))
        .count()
}

/// SP20 (1): an enrollment by `lineage` also counts against the quota of each of
/// its ancestors, from its enroller up to and including the first whose own
/// enrollment this device first saw more than 24 hours ago: under each, at most
/// `ENROLLMENTS_A_DAY` devices whose enrollment is recent, this one included.
fn enrollment_quota(
    g: &Graph<'_>,
    verdicts: &BTreeMap<Hash32, Verdict>,
    global: &State,
    record: &Hash32,
    lineage: DeviceId,
    recent: &dyn Fn(&Hash32) -> bool,
) -> bool {
    if !recent(record) {
        return true;
    }
    // Under `top`: lineages enrolled, directly or through others, by recent counting
    // enrollments, this one included.
    // SP20: two records enrolling one key on different branches are one
    // enrollment, so devices are counted, not records.
    let this = match &g.inputs.records[record].body {
        Body::Enroll { device, .. } => Some(*device),
        _ => None,
    };
    let under = |top: DeviceId| -> usize {
        let mut devices: BTreeSet<DeviceId> = this.into_iter().collect();
        for h in g
            .order
            .iter()
            .filter(|h| verdicts.get(*h) == Some(&Verdict::Counts))
        {
            if let Body::Enroll { device, .. } = &g.inputs.records[h].body {
                if !recent(h) {
                    continue;
                }
                let mut at = global.members.get(device).and_then(|m| m.enroller);
                while let Some(e) = at {
                    if e == top {
                        devices.insert(*device);
                        break;
                    }
                    at = global.members.get(&e).and_then(|m| m.enroller);
                }
            }
        }
        devices.len()
    };
    let mut top = Some(lineage);
    while let Some(t) = top {
        if under(t) > ENROLLMENTS_A_DAY {
            return false;
        }
        // Up to and including the first whose own enrollment isn't recent.
        let m = global.members.get(&t);
        if m.map_or(true, |m| !recent(&m.record)) {
            break;
        }
        top = m.and_then(|m| m.enroller);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::Keypair;
    use crate::sync::envelope::{self, Place};
    use crate::sync::ids::{b64url_encode, Id16};
    use crate::sync::json;
    use crate::sync::trust;
    use serde_json::{json, Value};

    const LIB: Id16 = Id16([7; 16]);
    const NOW: i64 = 1_760_000_000_000;
    const OLD: i64 = NOW - 40 * DAY_MS;

    fn k(n: u8) -> Keypair {
        Keypair::from_seed(&[n; 32])
    }

    trait CloneKey {
        fn clone_key(&self) -> Keypair;
    }

    impl CloneKey for Keypair {
        fn clone_key(&self) -> Keypair {
            Keypair::from_seed(&self.seed())
        }
    }

    fn code(key: &Keypair) -> Value {
        json!({ "nonce": b64url_encode(&[5; 16]), "codeKey": b64url_encode(&key.public()) })
    }

    /// The records of one test library, with when each was first seen.
    #[derive(Default)]
    struct Lib {
        records: BTreeMap<Hash32, Record>,
        seen: BTreeMap<Hash32, i64>,
        confirmed: BTreeSet<Hash32>,
        held: BTreeMap<(DeviceId, u64), Hash32>,
    }

    impl Lib {
        fn add(
            &mut self,
            t: &str,
            fields: Value,
            signers: &[(&Keypair, Signer, u64)],
            seen: i64,
        ) -> Hash32 {
            let (bytes, name) = trust::sign(t, &LIB, fields, signers);
            let r = trust::open(&bytes, &LIB, &name).unwrap();
            self.records.insert(name, r);
            self.seen.insert(name, seen);
            name
        }

        fn genesis(&mut self, d: &Keypair, c: &Keypair) -> Hash32 {
            self.add(
                "genesis",
                json!({ "device": d.device().to_string(), "key": b64url_encode(&d.public()),
                        "label": "first", "clock": [NOW as u64, 0], "code": code(c) }),
                &[(d, Signer::Device(d.device()), 0), (c, Signer::Code, 0)],
                OLD,
            )
        }

        fn enroll(
            &mut self,
            prev: Hash32,
            by: &Keypair,
            new: &Keypair,
            n: u64,
            seen: i64,
        ) -> Hash32 {
            self.enroll_labelled(prev, by, new, n, seen, "new")
        }

        /// `enroll` with a label of its own: another record for the same key.
        #[allow(clippy::too_many_arguments)]
        fn enroll_labelled(
            &mut self,
            prev: Hash32,
            by: &Keypair,
            new: &Keypair,
            n: u64,
            seen: i64,
            label: &str,
        ) -> Hash32 {
            let reveal = json::canonical(&json!({
                "type": "enroll-reveal", "library": LIB.to_string(),
                "applicant": new.device().to_string(), "enroller": by.device().to_string(),
            }));
            let pop = new.sign(&envelope::message(
                "enroll-reveal",
                &LIB,
                &Signer::Device(new.device()),
                &Place::default(),
                &crate::sync::crypto::payload_hash(&reveal),
            ));
            self.add(
                "enroll",
                json!({ "prev": prev.to_string(), "device": new.device().to_string(),
                        "key": b64url_encode(&new.public()), "label": label, "clock": [NOW as u64, 1],
                        "by": by.device().to_string(), "n": n, "sas": "dd".repeat(32),
                        "reveal": b64url_encode(&reveal), "pop": b64url_encode(&pop) }),
                &[(by, Signer::Device(by.device()), n)],
                seen,
            )
        }

        /// A `confirm` on `prev`: `what` is `{ "confirms": … }` or `{ "admits": […] }`.
        fn confirm(
            &mut self,
            prev: Hash32,
            signer: &Keypair,
            current: &Keypair,
            next: &Keypair,
            what: Value,
            seen: i64,
        ) -> Hash32 {
            let mut fields = json!({ "prev": prev.to_string(), "code": code(next) });
            fields
                .as_object_mut()
                .unwrap()
                .extend(what.as_object().unwrap().clone());
            self.add(
                "confirm",
                fields,
                &[
                    (signer, Signer::Device(signer.device()), 0),
                    (current, Signer::Code, 0),
                    (next, Signer::Code, 0),
                ],
                seen,
            )
        }

        fn code_enroll(&mut self, prev: Hash32, c: &Keypair, new: &Keypair, seen: i64) -> Hash32 {
            self.add(
                "enroll",
                json!({ "prev": prev.to_string(), "device": new.device().to_string(),
                        "key": b64url_encode(&new.public()), "label": "by code", "clock": [NOW as u64, 1],
                        "by": "code", "sas": null }),
                &[(c, Signer::Code, 0), (new, Signer::Device(new.device()), 0)],
                seen,
            )
        }

        #[allow(clippy::too_many_arguments)]
        fn remove(
            &mut self,
            prev: Hash32,
            signer: &Keypair,
            current: &Keypair,
            next: &Keypair,
            gone: &[&Keypair],
            seen: i64,
        ) -> Hash32 {
            let removes: Vec<Value> = gone
                .iter()
                .map(|d| json!({ "device": d.device().to_string(), "cutoff": null, "view": null, "cascade": true }))
                .collect();
            self.add(
                "remove",
                json!({ "prev": prev.to_string(), "removes": removes, "clock": [NOW as u64, 2], "code": code(next) }),
                &[(signer, Signer::Device(signer.device()), 0), (current, Signer::Code, 0), (next, Signer::Code, 0)],
                seen,
            )
        }

        fn rotate(
            &mut self,
            prev: Hash32,
            old: &Keypair,
            new: &Keypair,
            last: Option<u64>,
            seen: i64,
        ) -> Hash32 {
            let lastv = last.map_or(Value::Null, |s| json!({ "s": s, "h": "ee".repeat(32) }));
            self.add(
                "rotate",
                json!({ "prev": prev.to_string(), "old": old.device().to_string(), "last": lastv,
                        "new": new.device().to_string(), "key": b64url_encode(&new.public()),
                        "clock": [NOW as u64, 3] }),
                &[
                    (old, Signer::Device(old.device()), last.map_or(1, |s| s + 1)),
                    (new, Signer::Device(new.device()), 0),
                ],
                seen,
            )
        }

        fn compute(&self, genesis: Hash32) -> Membership {
            self.compute_after(genesis, &BTreeSet::new()).unwrap()
        }

        fn compute_with_checkpoint(
            &self,
            genesis: Hash32,
            checkpoint: &BTreeSet<Hash32>,
        ) -> Membership {
            let held = |d: &DeviceId, s: u64| self.held.get(&(*d, s)).copied();
            compute(&Inputs {
                genesis,
                records: &self.records,
                first_seen: &self.seen,
                confirmed: &self.confirmed,
                checkpoint,
                held: &held,
                accepted: &BTreeSet::new(),
                now_ms: NOW,
            })
            .unwrap()
        }

        fn compute_after(
            &self,
            genesis: Hash32,
            accepted: &BTreeSet<Hash32>,
        ) -> Result<Membership, Stale> {
            let held = |d: &DeviceId, s: u64| self.held.get(&(*d, s)).copied();
            compute(&Inputs {
                genesis,
                records: &self.records,
                first_seen: &self.seen,
                confirmed: &self.confirmed,
                checkpoint: &BTreeSet::new(),
                held: &held,
                accepted,
                now_ms: NOW,
            })
        }
    }

    #[test]
    fn the_genesis_this_device_joined_counts_and_another_doesnt() {
        let mut l = Lib::default();
        let g = l.genesis(&k(1), &k(100));
        let other = l.genesis(&k(2), &k(101));
        let m = l.compute(g);
        assert!(m.trusted(&k(1).device()).is_some());
        assert!(m.trusted(&k(2).device()).is_none());
        assert_eq!(
            m.verdict(&other),
            Some(Verdict::Refused(Refusal::OtherGenesis))
        );
        assert_eq!(m.newest_sealed, Some(g));
    }

    #[test]
    fn a_device_enrols_two_a_day_and_its_ancestors_quotas_count() {
        let mut l = Lib::default();
        let (a, b, c, d) = (k(1), k(2), k(3), k(4));
        let g = l.genesis(&a, &k(100));
        let e1 = l.enroll(g, &a, &b, 1, NOW - 1000);
        let e2 = l.enroll(e1, &a, &c, 2, NOW - 900);
        let e3 = l.enroll(e2, &a, &d, 3, NOW - 800);
        let m = l.compute(g);
        assert_eq!(m.verdict(&e1), Some(Verdict::Counts));
        assert_eq!(m.verdict(&e2), Some(Verdict::Counts));
        // A third today: waits for the quota, never refused.
        assert_eq!(m.verdict(&e3), Some(Verdict::Waits(Wait::Quota)));
        // Once its `n − 2` was first seen a day ago, it counts.
        l.seen.insert(e1, NOW - 2 * DAY_MS);
        let m = l.compute(g);
        assert_eq!(m.verdict(&e3), Some(Verdict::Counts));
        // Devices enrolled today can't each enrol two more: B, new today, enrols E,
        // which counts against A's quota too (A has B and C today).
        let mut l = Lib::default();
        let g = l.genesis(&a, &k(100));
        let e1 = l.enroll(g, &a, &b, 1, NOW - 1000);
        let e2 = l.enroll(e1, &a, &c, 2, NOW - 900);
        let e5 = l.enroll(e2, &b, &k(5), 1, NOW - 800);
        let m = l.compute(g);
        assert_eq!(m.verdict(&e5), Some(Verdict::Waits(Wait::Quota)));
    }

    #[test]
    fn one_key_enrolled_on_two_branches_counts_once_against_the_quota() {
        let mut l = Lib::default();
        let (a, b, c) = (k(1), k(2), k(3));
        let g = l.genesis(&a, &k(100));
        // A enrols B twice, on two branches: one enrollment. Then C: the second
        // device today, not the third.
        let e1 = l.enroll(g, &a, &b, 1, NOW - 1000);
        let e1b = l.enroll_labelled(g, &a, &b, 1, NOW - 900, "B again");
        assert_ne!(e1, e1b);
        let e2 = l.enroll(e1, &a, &c, 2, NOW - 800);
        let m = l.compute(g);
        assert_eq!(m.verdict(&e1), Some(Verdict::Counts));
        assert_eq!(m.verdict(&e1b), Some(Verdict::Counts));
        assert_eq!(m.verdict(&e2), Some(Verdict::Counts));
    }

    #[test]
    fn an_enrollment_needs_a_trusted_signer_its_signature_and_a_new_key() {
        let mut l = Lib::default();
        let (a, b) = (k(1), k(2));
        let g = l.genesis(&a, &k(100));
        // Signed by a key the membership doesn't trust.
        let stranger = l.enroll(g, &k(9), &b, 1, OLD);
        // An `n` that the signature doesn't cover: the signature is for its own `n`
        // only, so a record claiming 1 signed for 2 fails.
        let e1 = l.enroll(g, &a, &b, 1, OLD);
        let again = l.enroll(e1, &a, &b, 2, OLD);
        let m = l.compute(g);
        assert_eq!(
            m.verdict(&stranger),
            Some(Verdict::Refused(Refusal::Signer))
        );
        assert_eq!(m.verdict(&e1), Some(Verdict::Counts));
        assert_eq!(m.verdict(&again), Some(Verdict::Refused(Refusal::KeyAgain)));
    }

    #[test]
    fn a_key_a_refused_ancestor_introduced_cant_be_introduced_again() {
        let mut l = Lib::default();
        let (a, b, c0) = (k(1), k(2), k(100));
        let g = l.genesis(&a, &c0);
        // A stranger's enrollment of B is refused; A's enrollment of B built on it
        // reuses the key that ancestor introduced.
        let refused = l.enroll(g, &k(9), &b, 1, OLD);
        let again = l.enroll(refused, &a, &b, 1, OLD + 1);
        let m = l.compute(g);
        assert_eq!(m.verdict(&refused), Some(Verdict::Refused(Refusal::Signer)));
        assert_eq!(m.verdict(&again), Some(Verdict::Refused(Refusal::KeyAgain)));
        // Built beside it, not on it, the same key is a fresh enrollment.
        let beside = l.enroll(g, &a, &b, 1, OLD + 2);
        assert_eq!(l.compute(g).verdict(&beside), Some(Verdict::Counts));
    }

    #[test]
    fn the_library_holds_at_most_twelve_devices() {
        let mut l = Lib::default();
        let a = k(1);
        let g = l.genesis(&a, &k(100));
        let mut prev = g;
        let mut last = g;
        for i in 0..12u8 {
            last = l.enroll(prev, &a, &k(10 + i), u64::from(i) + 1, OLD + i64::from(i));
            prev = last;
        }
        let m = l.compute(g);
        assert_eq!(m.state.trusted_count(), MAX_DEVICES);
        assert_eq!(m.verdict(&last), Some(Verdict::Waits(Wait::Cap)));
    }

    #[test]
    fn a_code_enrollment_counts_on_the_newest_sealed_head_once_confirmed() {
        let mut l = Lib::default();
        let (a, c0) = (k(1), k(100));
        let g = l.genesis(&a, &c0);
        let by_code = l.code_enroll(g, &c0, &k(2), OLD);
        // With another code's key: refused.
        let forged = l.code_enroll(g, &k(101), &k(3), OLD);
        let m = l.compute(g);
        assert_eq!(m.verdict(&by_code), Some(Verdict::Waits(Wait::Unconfirmed)));
        assert_eq!(
            m.verdict(&forged),
            Some(Verdict::Refused(Refusal::Signature))
        );
        l.confirmed.insert(by_code);
        let m = l.compute(g);
        assert_eq!(m.verdict(&by_code), Some(Verdict::Counts));
        assert!(m.trusted(&k(2).device()).is_some());
        // A removal sealed after the same head, even judged after it, overtakes it:
        // the old code can't enrol anyone once a removal has rotated it.
        let e = l.enroll(g, &a, &k(4), 1, OLD + 1);
        let rm = l.remove(e, &a, &c0, &k(102), &[&k(4)], OLD + 2);
        let m = l.compute(g);
        assert_eq!(m.verdict(&rm), Some(Verdict::Counts));
        assert_eq!(
            m.verdict(&by_code),
            Some(Verdict::Refused(Refusal::Overtaken))
        );
        assert!(m.trusted(&k(2).device()).is_none());
    }

    #[test]
    fn a_removal_takes_its_lineage_and_what_it_enrolled() {
        let mut l = Lib::default();
        let (a, b, c, c0, c1) = (k(1), k(2), k(3), k(100), k(101));
        let g = l.genesis(&a, &c0);
        let e1 = l.enroll(g, &a, &b, 1, OLD);
        let e2 = l.enroll(e1, &b, &c, 1, OLD + 1);
        let rm = l.remove(e2, &a, &c0, &c1, &[&b], OLD + 2);
        let m = l.compute(g);
        assert_eq!(m.verdict(&rm), Some(Verdict::Counts));
        // B and C, whom B enrolled, are removed; A stays; the removal is the head.
        assert!(m.trusted(&b.device()).is_none());
        assert!(m.trusted(&c.device()).is_none());
        assert!(m.trusted(&a.device()).is_some());
        assert_eq!(m.newest_sealed, Some(rm));
        // Their keys still check what they signed before.
        assert_eq!(m.key(&b.device()), Some(b.public()));
        // Signed only by a device it removes, or without the current code: refused.
        let mut l2 = Lib::default();
        let g = l2.genesis(&a, &c0);
        let e1 = l2.enroll(g, &a, &b, 1, OLD);
        let by_removed = l2.remove(e1, &b, &c0, &c1, &[&b], OLD + 1);
        let m = l2.compute(g);
        assert_eq!(
            m.verdict(&by_removed),
            Some(Verdict::Refused(Refusal::Signer))
        );
        let mut l3 = Lib::default();
        let g = l3.genesis(&a, &c0);
        let e1 = l3.enroll(g, &a, &b, 1, OLD);
        let no_code = l3.remove(e1, &a, &k(109), &c1, &[&b], OLD + 1);
        let m = l3.compute(g);
        assert_eq!(
            m.verdict(&no_code),
            Some(Verdict::Refused(Refusal::Signature))
        );
    }

    #[test]
    fn a_rotation_hands_the_lineage_on_once() {
        let mut l = Lib::default();
        let (a, a2, a3, c0) = (k(1), k(21), k(22), k(100));
        let g = l.genesis(&a, &c0);
        // The rotations below name `last = { 4, ee… }`: this device holds it.
        l.held
            .insert((a.device(), 4), Hash32::parse(&"ee".repeat(32)).unwrap());
        let r1 = l.rotate(g, &a, &a2, Some(4), OLD);
        let twice = l.rotate(g, &a, &a3, Some(4), OLD + 1);
        let m = l.compute(g);
        assert_eq!(m.verdict(&r1), Some(Verdict::Counts));
        assert!(m.trusted(&a2.device()).is_some());
        assert!(m.trusted(&a.device()).is_none());
        assert_eq!(m.key(&a.device()), Some(a.public()));
        // An identity has one successor; a later rotation of it is refused.
        assert_eq!(m.verdict(&twice), Some(Verdict::Refused(Refusal::Rotated)));
        // A rotation closing a chain this device doesn't hold through `last` waits.
        let mut l = Lib::default();
        let g = l.genesis(&a, &c0);
        let ahead = l.rotate(g, &a, &a2, Some(9), OLD);
        let m = l.compute(g);
        assert_eq!(m.verdict(&ahead), Some(Verdict::Waits(Wait::Chain)));
        // One naming a `last` this device holds with another hash: a conflict.
        let mut l = Lib::default();
        let g = l.genesis(&a, &c0);
        l.held.insert((a.device(), 4), Hash32([1; 32]));
        let conflict = l.rotate(g, &a, &a2, Some(4), OLD);
        let m = l.compute(g);
        assert_eq!(
            m.verdict(&conflict),
            Some(Verdict::Refused(Refusal::LastConflict))
        );
    }

    #[test]
    fn a_third_rotation_in_thirty_days_needs_the_code() {
        let mut l = Lib::default();
        let (a, c0) = (k(1), k(100));
        let g = l.genesis(&a, &c0);
        let (a2, a3, a4) = (k(21), k(22), k(23));
        let r1 = l.rotate(g, &a, &a2, None, NOW - DAY_MS);
        let r2 = l.rotate(r1, &a2, &a3, None, NOW - 1000);
        let r3 = l.rotate(r2, &a3, &a4, None, NOW - 500);
        let m = l.compute(g);
        assert_eq!(m.verdict(&r2), Some(Verdict::Counts));
        assert_eq!(m.verdict(&r3), Some(Verdict::Refused(Refusal::NeedsCode)));
        // The same for a device that joined with the first two in its checkpoint.
        let checkpoint: BTreeSet<Hash32> = [g, r1, r2].into_iter().collect();
        let m = l.compute_with_checkpoint(g, &checkpoint);
        assert_eq!(m.verdict(&r3), Some(Verdict::Refused(Refusal::NeedsCode)));
    }

    #[test]
    fn a_rotated_lineage_cant_sign_its_own_removal() {
        let mut l = Lib::default();
        let (a, a2, a3, b, c0) = (k(1), k(21), k(22), k(2), k(100));
        let g = l.genesis(&b, &c0);
        let e = l.enroll(g, &b, &a, 1, OLD);
        let r1 = l.rotate(e, &a, &a2, None, OLD + 1);
        let r2 = l.rotate(r1, &a2, &a3, None, OLD + 2);
        // A3 signs the removal of A2, an earlier identity of its own lineage.
        let rm = l.remove(r2, &a3, &c0, &k(101), &[&a2], OLD + 3);
        let m = l.compute(g);
        assert_eq!(m.verdict(&rm), Some(Verdict::Refused(Refusal::Signer)));
        assert!(m.trusted(&a3.device()).is_some());
    }

    #[test]
    fn a_device_cant_sign_a_removal_whose_cascade_takes_it() {
        let mut l = Lib::default();
        let (a, b, c, c0) = (k(1), k(2), k(3), k(100));
        let g = l.genesis(&a, &c0);
        let e1 = l.enroll(g, &a, &b, 1, OLD);
        let e2 = l.enroll(e1, &b, &c, 1, OLD + 1);
        // C signs the removal of B, who enrolled it: the cascade would take C too.
        let rm = l.remove(e2, &c, &c0, &k(101), &[&b], OLD + 2);
        let m = l.compute(g);
        assert_eq!(m.verdict(&rm), Some(Verdict::Refused(Refusal::Signer)));
        assert!(m.trusted(&c.device()).is_some());
    }

    #[test]
    fn a_removal_naming_a_device_not_yet_known_is_refused() {
        let mut l = Lib::default();
        let (a, a2, c0) = (k(1), k(21), k(100));
        let g = l.genesis(&a, &c0);
        // A signs the removal of A2, unknown so far; then A rotates to A2.
        let rm = l.remove(g, &a, &c0, &k(101), &[&a2], OLD + 1);
        let r = l.rotate(rm, &a, &a2, None, OLD + 2);
        let m = l.compute(g);
        assert_eq!(
            m.verdict(&rm),
            Some(Verdict::Refused(Refusal::UnknownTarget))
        );
        assert_eq!(m.verdict(&r), Some(Verdict::Counts));
        assert!(m.trusted(&a2.device()).is_some());
    }

    #[test]
    fn unlocks_that_take_many_passes_still_settle() {
        // Twelve devices, then eight enrollments waiting on the cap, each followed
        // by a removal signed by the device enrolled just before it: each pass
        // unlocks one enrollment and the removal after it.
        let mut l = Lib::default();
        let (a, c0) = (k(1), k(100));
        let g = l.genesis(&a, &c0);
        let mut prev = g;
        for i in 0..11u8 {
            prev = l.enroll(prev, &a, &k(10 + i), u64::from(i) + 1, OLD + i64::from(i));
        }
        let mut waiting = Vec::new();
        let mut signer = a.clone_key();
        let mut code_now = c0.clone_key();
        for i in 0..8u8 {
            let new = k(60 + i);
            let e = l.enroll(
                prev,
                &a,
                &new,
                12 + u64::from(i),
                OLD + 100 + 10 * i64::from(i),
            );
            waiting.push(e);
            let next_code = k(110 + i);
            let rm = l.remove(
                e,
                &signer,
                &code_now,
                &next_code,
                &[&k(10 + i)],
                OLD + 105 + 10 * i64::from(i),
            );
            prev = rm;
            signer = new;
            code_now = next_code;
        }
        let m = l.compute(g);
        for e in &waiting {
            assert_eq!(m.verdict(e), Some(Verdict::Counts), "{e:?}");
        }
    }

    #[test]
    fn a_record_counting_over_a_waiting_ancestor_isnt_kept_as_accepted() {
        let mut l = Lib::default();
        let (a, a2, a3, c0) = (k(1), k(21), k(22), k(100));
        let g = l.genesis(&a, &c0);
        // A → A2 waits for the old chain; A → A3, built on it, counts meanwhile.
        let r1 = l.rotate(g, &a, &a2, Some(4), OLD);
        let r2 = l.rotate(r1, &a, &a3, None, OLD + 1);
        let m = l.compute(g);
        assert_eq!(m.verdict(&r1), Some(Verdict::Waits(Wait::Chain)));
        assert_eq!(m.verdict(&r2), Some(Verdict::Counts));
        let accepted = m.accepted(&l.records);
        assert!(!accepted.contains(&r2));
        // The chain arrives: A2 is A's successor and A3's rotation is refused, and
        // the same storage isn't stale.
        l.held
            .insert((a.device(), 4), Hash32::parse(&"ee".repeat(32)).unwrap());
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&r1), Some(Verdict::Counts));
        assert_eq!(m.verdict(&r2), Some(Verdict::Refused(Refusal::Rotated)));
    }

    #[test]
    fn a_rotation_with_an_unjudged_sibling_isnt_kept_as_accepted() {
        let mut l = Lib::default();
        let (a, a2, a3, b, c0) = (k(1), k(21), k(22), k(2), k(100));
        let g = l.genesis(&a, &c0);
        // A → A2 was seen first but its predecessor is missing for now; A → A3
        // counts meanwhile.
        let p = l.enroll(g, &a, &b, 1, OLD);
        let r1 = l.rotate(p, &a, &a2, None, OLD + 1);
        let r2 = l.rotate(g, &a, &a3, None, OLD + 2);
        let held = l.records.remove(&p).unwrap();
        let m = l.compute(g);
        assert_eq!(m.verdict(&r1), Some(Verdict::Waits(Wait::Unjudged)));
        assert_eq!(m.verdict(&r2), Some(Verdict::Counts));
        let accepted = m.accepted(&l.records);
        assert!(!accepted.contains(&r2));
        // The predecessor comes back: A2 is A's successor, and the storage isn't stale.
        l.records.insert(p, held);
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&r1), Some(Verdict::Counts));
        assert_eq!(m.verdict(&r2), Some(Verdict::Refused(Refusal::Rotated)));
    }

    #[test]
    fn a_rotation_with_a_waiting_sibling_isnt_kept_as_accepted() {
        let mut l = Lib::default();
        let (a, a2, a3, c0) = (k(1), k(21), k(22), k(100));
        let g = l.genesis(&a, &c0);
        // Two rotations of A beside each other: the first seen waits for its chain,
        // the second counts meanwhile.
        let r1 = l.rotate(g, &a, &a2, Some(4), OLD);
        let r2 = l.rotate(g, &a, &a3, None, OLD + 1);
        let m = l.compute(g);
        assert_eq!(m.verdict(&r2), Some(Verdict::Counts));
        let accepted = m.accepted(&l.records);
        assert!(!accepted.contains(&r2));
        l.held
            .insert((a.device(), 4), Hash32::parse(&"ee".repeat(32)).unwrap());
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&r1), Some(Verdict::Counts));
        assert_eq!(m.verdict(&r2), Some(Verdict::Refused(Refusal::Rotated)));
    }

    #[test]
    fn storage_missing_what_this_device_accepted_is_stale() {
        let mut l = Lib::default();
        let (a, b, c0) = (k(1), k(2), k(100));
        let g = l.genesis(&a, &c0);
        let e1 = l.enroll(g, &a, &b, 1, OLD);
        let rm = l.remove(e1, &a, &c0, &k(101), &[&b], OLD + 1);
        let m = l.compute(g);
        let accepted = m.accepted(&l.records);
        assert_eq!(accepted, [rm].into_iter().collect());
        // The same storage again: still current.
        assert!(l.compute_after(g, &accepted).is_ok());
        // Storage that lost the removal would trust B again: stale instead.
        l.records.remove(&rm);
        assert_eq!(
            l.compute_after(g, &accepted).unwrap_err(),
            Stale { missing: vec![rm] }
        );
    }

    #[test]
    fn a_removal_later_in_the_graph_makes_room_under_the_cap() {
        let mut l = Lib::default();
        let (a, c0) = (k(1), k(100));
        let g = l.genesis(&a, &c0);
        let mut prev = g;
        for i in 0..11u8 {
            prev = l.enroll(prev, &a, &k(10 + i), u64::from(i) + 1, OLD + i64::from(i));
        }
        // The thirteenth waits; then a removal built on it removes one of the twelve.
        let waiting = l.enroll(prev, &a, &k(50), 12, OLD + 20);
        let rm = l.remove(waiting, &a, &c0, &k(101), &[&k(15)], OLD + 21);
        let m = l.compute(g);
        assert_eq!(m.verdict(&rm), Some(Verdict::Counts));
        assert_eq!(m.verdict(&waiting), Some(Verdict::Counts));
        assert_eq!(m.state.trusted_count(), MAX_DEVICES);
        // Two waiting for one slot: the one judged first takes it.
        let mut l = Lib::default();
        let g = l.genesis(&a, &c0);
        let mut prev = g;
        for i in 0..11u8 {
            prev = l.enroll(prev, &a, &k(10 + i), u64::from(i) + 1, OLD + i64::from(i));
        }
        let first = l.enroll(prev, &a, &k(50), 12, OLD + 20);
        let second = l.enroll(first, &a, &k(51), 13, OLD + 21);
        let rm = l.remove(second, &a, &c0, &k(101), &[&k(15)], OLD + 22);
        let m = l.compute(g);
        assert_eq!(m.verdict(&rm), Some(Verdict::Counts));
        assert_eq!(m.verdict(&first), Some(Verdict::Counts));
        assert_eq!(m.verdict(&second), Some(Verdict::Waits(Wait::Cap)));
        assert_eq!(m.state.trusted_count(), MAX_DEVICES);
    }

    #[test]
    fn a_forged_enrollment_proves_no_quota_history() {
        let mut l = Lib::default();
        let (a, c0) = (k(1), k(100));
        let g = l.genesis(&a, &c0);
        // A record claiming to be A's enrollment 1, signed by another key.
        let new = k(2);
        let reveal = json::canonical(&json!({
            "type": "enroll-reveal", "library": LIB.to_string(),
            "applicant": new.device().to_string(), "enroller": a.device().to_string(),
        }));
        let pop = new.sign(&envelope::message(
            "enroll-reveal",
            &LIB,
            &Signer::Device(new.device()),
            &Place::default(),
            &crate::sync::crypto::payload_hash(&reveal),
        ));
        let forger = k(99);
        let forged = l.add(
            "enroll",
            json!({ "prev": g.to_string(), "device": new.device().to_string(),
                    "key": b64url_encode(&new.public()), "label": "new", "clock": [NOW as u64, 1],
                    "by": a.device().to_string(), "n": 1, "sas": "dd".repeat(32),
                    "reveal": b64url_encode(&reveal), "pop": b64url_encode(&pop) }),
            &[(&forger, Signer::Device(a.device()), 1)],
            OLD,
        );
        // A's real enrollment 2 can't lean on it.
        let second = l.enroll(forged, &a, &k(3), 2, OLD + 1);
        let m = l.compute(g);
        assert_eq!(
            m.verdict(&forged),
            Some(Verdict::Refused(Refusal::Signature))
        );
        assert_eq!(m.verdict(&second), Some(Verdict::Waits(Wait::Quota)));
    }

    #[test]
    fn a_folder_of_junk_records_is_judged_only_up_to_the_bound() {
        let mut l = Lib::default();
        let (a, c0) = (k(1), k(100));
        let g = l.genesis(&a, &c0);
        // Well-formed enrollments by a stranger, chained, each its own new key.
        let stranger = k(99);
        let mut prev = g;
        for i in 0..MAX_RECORDS as u64 + 10 {
            let new = Keypair::from_seed(&{
                let mut s = [7u8; 32];
                s[..8].copy_from_slice(&i.to_be_bytes());
                s
            });
            prev = l.enroll(prev, &stranger, &new, 1, OLD + i as i64);
        }
        let m = l.compute(g);
        let judged = m
            .verdicts
            .values()
            .filter(|v| **v != Verdict::Waits(Wait::Unjudged))
            .count();
        assert_eq!(judged, MAX_RECORDS);
        assert_eq!(m.verdict(&prev), Some(Verdict::Waits(Wait::Unjudged)));
        assert!(m.trusted(&a.device()).is_some());
        // A record whose predecessor isn't held isn't judged either.
        let mut l = Lib::default();
        let g = l.genesis(&a, &c0);
        let orphan = l.enroll(Hash32([3; 32]), &a, &k(2), 1, OLD);
        assert_eq!(
            l.compute(g).verdict(&orphan),
            Some(Verdict::Waits(Wait::Unjudged))
        );
    }

    /// A, B and C; then B removes C and, apart, C removes B, both on one head.
    fn removed_each_other() -> (Lib, Hash32, Hash32, Hash32) {
        let mut l = Lib::default();
        let (a, b, c, c0) = (k(1), k(2), k(3), k(100));
        let g = l.genesis(&a, &c0);
        let e1 = l.enroll(g, &a, &b, 1, OLD);
        let e2 = l.enroll(e1, &a, &c, 2, OLD + 1);
        let r1 = l.remove(e2, &b, &c0, &k(101), &[&c], OLD + 2);
        let r2 = l.remove(e2, &c, &c0, &k(102), &[&b], OLD + 3);
        (l, g, r1, r2)
    }

    #[test]
    fn removals_made_apart_both_count_and_unite() {
        let (mut l, g, r1, r2) = removed_each_other();
        let m = l.compute(g);
        assert_eq!(m.verdict(&r1), Some(Verdict::Counts));
        assert_eq!(m.verdict(&r2), Some(Verdict::Counts));
        assert!(m.trusted(&k(2).device()).is_none());
        assert!(m.trusted(&k(3).device()).is_none());
        assert!(m.trusted(&k(1).device()).is_some());
        // Sibling sealed heads: none is the newest.
        assert_eq!(m.newest_sealed, None);
        // The reader rejoins with either branch's code.
        let back = l.code_enroll(r2, &k(102), &k(4), OLD + 4);
        l.confirmed.insert(back);
        assert_eq!(l.compute(g).verdict(&back), Some(Verdict::Counts));
    }

    #[test]
    fn a_device_that_held_one_sibling_waits_for_a_confirm_of_the_other() {
        let (mut l, g, r1, r2) = removed_each_other();
        // This device saw B's removal first and kept it as its newest sealed head.
        let held = l.records.remove(&r2).unwrap();
        let accepted = l.compute(g).accepted(&l.records);
        assert!(accepted.contains(&r1));
        l.records.insert(r2, held);
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&r2), Some(Verdict::Waits(Wait::Confirm)));
        assert!(m.trusted(&k(2).device()).is_some());
        assert_eq!(m.newest_sealed, Some(r1));
        // A code enrollment on the other branch is refused here.
        let back = l.code_enroll(r2, &k(102), &k(4), OLD + 4);
        l.confirmed.insert(back);
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&back), Some(Verdict::Refused(Refusal::NotSealed)));
        // The reader confirms C's removal with the code of B's: it counts as if made
        // there, and the confirm is the newest sealed head.
        let cf = l.confirm(
            r1,
            &k(1),
            &k(101),
            &k(103),
            json!({ "confirms": r2.to_string() }),
            OLD + 5,
        );
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&cf), Some(Verdict::Counts));
        assert_eq!(m.verdict(&r2), Some(Verdict::Counts));
        assert!(m.trusted(&k(2).device()).is_none());
        assert_eq!(m.newest_sealed, Some(cf));
        // The confirmed removal's own code never becomes a code.
        assert_eq!(m.verdict(&back), Some(Verdict::Refused(Refusal::Overtaken)));
    }

    #[test]
    fn a_confirm_that_stops_counting_overtakes_nothing() {
        let mut l = Lib::default();
        let (a, b, c, d, c0) = (k(1), k(2), k(3), k(4), k(100));
        let g = l.genesis(&a, &c0);
        let e1 = l.enroll(g, &a, &b, 1, OLD);
        let e2 = l.enroll(e1, &a, &c, 2, OLD + 1);
        let e3 = l.enroll(e2, &a, &d, 3, OLD + 2);
        let r1 = l.remove(e3, &b, &c0, &k(101), &[&c], OLD + 3);
        let r2 = l.remove(e3, &c, &c0, &k(102), &[&b], OLD + 4);
        let held = l.records.remove(&r2).unwrap();
        let accepted = l.compute(g).accepted(&l.records);
        l.records.insert(r2, held);
        // A code enrollment on B's removal; then D confirms C's, which would
        // overtake it; then A removes D after the enrollment, which overtakes the
        // confirm and removes its signer: the confirm stops counting.
        let by_code = l.code_enroll(r1, &k(101), &k(5), OLD + 5);
        l.confirmed.insert(by_code);
        let cf = l.confirm(
            r1,
            &d,
            &k(101),
            &k(103),
            json!({ "confirms": r2.to_string() }),
            OLD + 6,
        );
        let rm = l.remove(by_code, &a, &k(101), &k(104), &[&d], OLD + 7);
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&rm), Some(Verdict::Counts));
        assert_eq!(m.verdict(&cf), Some(Verdict::Refused(Refusal::Overtaken)));
        assert_eq!(m.verdict(&by_code), Some(Verdict::Counts));
    }

    #[test]
    fn a_confirm_needs_the_newest_head_its_code_a_removal_and_a_signer_it_spares() {
        let (mut l, g, r1, r2) = removed_each_other();
        let held = l.records.remove(&r2).unwrap();
        let accepted = l.compute(g).accepted(&l.records);
        l.records.insert(r2, held);
        let what = json!({ "confirms": r2.to_string() });
        // Signed by B, whom the confirmed removal takes away.
        let by_b = l.confirm(r1, &k(2), &k(101), &k(103), what.clone(), OLD + 5);
        // Without the newest head's code.
        let no_code = l.confirm(r1, &k(1), &k(100), &k(104), what.clone(), OLD + 5);
        // On a head a later one overtook.
        let old = l.confirm(g, &k(1), &k(100), &k(105), what, OLD + 5);
        // Of a record that isn't a removal.
        let e1 = l
            .records
            .iter()
            .find(|(_, r)| matches!(r.body, Body::Enroll { .. }))
            .map(|(h, _)| *h)
            .unwrap();
        let not_rm = l.confirm(
            r1,
            &k(1),
            &k(101),
            &k(106),
            json!({ "confirms": e1.to_string() }),
            OLD + 5,
        );
        let m = l.compute_after(g, &accepted).unwrap();
        assert_eq!(m.verdict(&by_b), Some(Verdict::Refused(Refusal::Signer)));
        assert_eq!(
            m.verdict(&no_code),
            Some(Verdict::Refused(Refusal::Signature))
        );
        assert_eq!(m.verdict(&old), Some(Verdict::Refused(Refusal::Overtaken)));
        assert_eq!(
            m.verdict(&not_rm),
            Some(Verdict::Refused(Refusal::Confirms))
        );
        assert_eq!(m.verdict(&r2), Some(Verdict::Waits(Wait::Confirm)));
    }

    /// Eleven devices in a line: A and ten it enrolled.
    fn eleven() -> (Lib, Hash32, Hash32) {
        let mut l = Lib::default();
        let a = k(1);
        let g = l.genesis(&a, &k(100));
        let mut prev = g;
        for i in 0..10u8 {
            prev = l.enroll(prev, &a, &k(10 + i), u64::from(i) + 1, OLD + i64::from(i));
        }
        (l, g, prev)
    }

    #[test]
    fn sibling_enrollments_past_the_cap_all_wait_until_removals_make_room() {
        let (mut l, g, h) = eleven();
        // Two devices enrol the twelfth device apart, on one head: neither came first.
        let e = l.enroll(h, &k(10), &k(40), 1, OLD + 20);
        let w = l.enroll(h, &k(11), &k(41), 1, OLD + 21);
        let m = l.compute(g);
        assert_eq!(m.verdict(&e), Some(Verdict::Waits(Wait::Cap)));
        assert_eq!(m.verdict(&w), Some(Verdict::Waits(Wait::Cap)));
        assert_eq!(m.state.trusted_count(), 11);
        // A removal makes room for both.
        l.remove(h, &k(1), &k(100), &k(101), &[&k(15)], OLD + 22);
        let m = l.compute(g);
        assert_eq!(m.verdict(&e), Some(Verdict::Counts));
        assert_eq!(m.verdict(&w), Some(Verdict::Counts));
        assert_eq!(m.state.trusted_count(), MAX_DEVICES);
    }

    #[test]
    fn a_confirm_admits_the_readers_choice_of_sibling_enrollments() {
        let (mut l, g, h) = eleven();
        let e = l.enroll(h, &k(10), &k(40), 1, OLD + 20);
        let w = l.enroll(h, &k(11), &k(41), 1, OLD + 21);
        // Admitting both would pass the cap.
        let both = l.confirm(
            g,
            &k(1),
            &k(100),
            &k(101),
            json!({ "admits": [e.to_string(), w.to_string()] }),
            OLD + 22,
        );
        let m = l.compute(g);
        assert_eq!(m.verdict(&both), Some(Verdict::Refused(Refusal::Confirms)));
        let one = l.confirm(
            g,
            &k(1),
            &k(100),
            &k(102),
            json!({ "admits": [e.to_string()] }),
            OLD + 23,
        );
        let m = l.compute(g);
        assert_eq!(m.verdict(&one), Some(Verdict::Counts));
        assert_eq!(m.verdict(&e), Some(Verdict::Counts));
        assert_eq!(m.verdict(&w), Some(Verdict::Refused(Refusal::NotAdmitted)));
        assert_eq!(m.state.trusted_count(), MAX_DEVICES);
        assert_eq!(m.newest_sealed, Some(one));
        // The choice stands: a later confirm can't admit the one left out.
        let again = l.confirm(
            one,
            &k(1),
            &k(102),
            &k(103),
            json!({ "admits": [w.to_string()] }),
            OLD + 24,
        );
        let m = l.compute(g);
        assert_eq!(m.verdict(&again), Some(Verdict::Refused(Refusal::Confirms)));
        assert_eq!(m.verdict(&w), Some(Verdict::Refused(Refusal::NotAdmitted)));
        assert_eq!(m.state.trusted_count(), MAX_DEVICES);
    }

    #[test]
    fn an_enrollment_on_an_older_head_isnt_held_back_beside_a_newer_one() {
        let (mut l, g, h) = eleven();
        let e3 = l
            .records
            .iter()
            .find(|(_, r)| matches!(&r.body, Body::Enroll { n: Some(3), .. }))
            .map(|(h, _)| *h)
            .unwrap();
        // A removal, then an enrollment after it: eleven again.
        let rm = l.remove(h, &k(1), &k(100), &k(101), &[&k(19)], OLD + 20);
        let after = l.enroll(rm, &k(10), &k(30), 1, OLD + 21);
        // One enrollment on an early record, seen first, and one after the removal.
        let early = l.enroll(e3, &k(11), &k(40), 1, OLD + 22);
        let late = l.enroll(after, &k(12), &k(41), 1, OLD + 23);
        let m = l.compute(g);
        assert_eq!(m.verdict(&after), Some(Verdict::Counts));
        assert_eq!(m.verdict(&early), Some(Verdict::Counts));
        assert_eq!(m.verdict(&late), Some(Verdict::Waits(Wait::Cap)));
    }
}
