//! SP30: equivocation proofs, checkable with the device's key alone.
//!
//! Two valid signatures by one device for the same type and sequence that differ
//! in anything they sign are a proof; so is a `rotate` with a `change` signed after
//! it closed the chain. Every device that holds one relays it and quarantines the
//! device. This module reads, writes and checks a proof's signed core, and finds a
//! change proof's cutoff from the change files a relayer attaches (its sides'
//! copies and the `ancestry`); reading those files out of overflow copies and
//! bundles, and a spine where the ancestry is long, are the callers'.
//!
//! PROVISIONAL: where SP30 leaves the bytes open, the reading here is Delegated to
//! Claude, not approved (`docs/decisions.md`, “Equivocation proofs”).

use super::envelope::{self, Place, Signer};
use super::ids::{b64url_decode, b64url_encode, device_id, DeviceId, Hash32, Id16};
use super::json::{self, Numbers};
use super::trust::Mark;
use serde_json::{json, Map, Value};

/// SP30: the types whose signatures have a place in a history (SP6).
pub const TYPES: [&str; 5] = ["change", "view", "device", "rotate", "enroll"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProofError {
    #[error("a proof that isn't SP30's shape")]
    Shape,
    #[error("a type with no place in a history, or a sequence of 0")]
    Type,
    #[error("two signatures over the same thing: no equivocation")]
    Same,
    #[error("a signature that isn't the device's")]
    Signature,
    #[error("a change that isn't after the rotation's `last`")]
    NotAfter,
}

pub type Result<T> = std::result::Result<T, ProofError>;

/// One signed side: the payload hash and, for a change, the `prev` and `vroot` its
/// signature covers, with the signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub hash: Hash32,
    pub prev: Option<Hash32>,
    pub vroot: Option<Hash32>,
    pub sig: [u8; 64],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proof {
    /// Two signatures for one type and sequence that differ in what they sign.
    Twice {
        device: DeviceId,
        type_: String,
        seq: u64,
        a: Side,
        b: Side,
    },
    /// A `rotate` (its own payload bytes and signature), and a change signed after
    /// the chain it closed.
    Closed {
        device: DeviceId,
        rotate: (Vec<u8>, [u8; 64]),
        seq: u64,
        change: Side,
    },
}

fn hash(v: Option<&Value>) -> Result<Hash32> {
    v.and_then(Value::as_str)
        .and_then(Hash32::parse)
        .ok_or(ProofError::Shape)
}

fn opt_hash(v: Option<&Value>) -> Result<Option<Hash32>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        v => hash(v).map(Some),
    }
}

fn sig(v: Option<&Value>) -> Result<[u8; 64]> {
    v.and_then(Value::as_str)
        .and_then(b64url_decode)
        .and_then(|b| b.try_into().ok())
        .ok_or(ProofError::Shape)
}

fn side(o: &Map<String, Value>) -> Result<Side> {
    Ok(Side {
        hash: hash(o.get("hash"))?,
        prev: opt_hash(o.get("prev"))?,
        vroot: opt_hash(o.get("vroot"))?,
        sig: sig(o.get("sig"))?,
    })
}

fn side_json(s: &Side) -> Value {
    let mut o = json!({ "hash": s.hash.to_string(), "sig": b64url_encode(&s.sig) });
    if s.vroot.is_some() {
        o["prev"] = s.prev.map_or(Value::Null, |p| Value::from(p.to_string()));
        o["vroot"] = Value::from(s.vroot.map(|v| v.to_string()));
    }
    o
}

impl Proof {
    pub fn device(&self) -> DeviceId {
        match self {
            Proof::Twice { device, .. } | Proof::Closed { device, .. } => *device,
        }
    }

    /// Reads a proof from a `proofs` entry (SP16). Fields SP30 adds for finding the
    /// cutoff (`copy`, `base`, `ancestry`) are read past here.
    pub fn parse(v: &Value) -> Result<Self> {
        let o = v.as_object().ok_or(ProofError::Shape)?;
        let device = o
            .get("device")
            .and_then(Value::as_str)
            .and_then(Id16::parse)
            .ok_or(ProofError::Shape)?;
        let type_ = o
            .get("type")
            .and_then(Value::as_str)
            .ok_or(ProofError::Shape)?;
        let obj = |k: &str| o.get(k).and_then(Value::as_object).ok_or(ProofError::Shape);
        if type_ == "closed" {
            let r = obj("rotate")?;
            let c = obj("change")?;
            return Ok(Proof::Closed {
                device,
                rotate: (
                    r.get("payload")
                        .and_then(Value::as_str)
                        .and_then(b64url_decode)
                        .ok_or(ProofError::Shape)?,
                    sig(r.get("sig"))?,
                ),
                seq: c
                    .get("seq")
                    .and_then(Value::as_u64)
                    .ok_or(ProofError::Shape)?,
                change: side(c)?,
            });
        }
        Ok(Proof::Twice {
            device,
            type_: type_.to_string(),
            seq: o
                .get("seq")
                .and_then(Value::as_u64)
                .ok_or(ProofError::Shape)?,
            a: side(obj("a")?)?,
            b: side(obj("b")?)?,
        })
    }

    pub fn to_json(&self) -> Value {
        match self {
            Proof::Twice {
                device,
                type_,
                seq,
                a,
                b,
            } => json!({ "device": device.to_string(), "type": type_, "seq": seq,
                         "a": side_json(a), "b": side_json(b) }),
            Proof::Closed {
                device,
                rotate,
                seq,
                change,
            } => {
                let mut c = side_json(change);
                c["seq"] = Value::from(*seq);
                json!({ "device": device.to_string(), "type": "closed",
                        "rotate": { "payload": b64url_encode(&rotate.0), "sig": b64url_encode(&rotate.1) },
                        "change": c })
            }
        }
    }

    /// SP30: checks the proof with the device's key alone. For a `closed` proof,
    /// returns the cutoff the rotation's `last` names (`None` when it closed an empty
    /// chain); for two signatures, `None` (their cutoff is a later part's).
    pub fn check(&self, library: &Id16, key: &[u8; 32]) -> Result<Option<Mark>> {
        let device = self.device();
        if device_id(key) != device {
            return Err(ProofError::Signature);
        }
        // The exact bytes a side's signature covers (SP6).
        let message = |type_: &str, seq: u64, s: &Side| {
            let place = Place {
                seq,
                prev: s.prev,
                vroot: s.vroot,
            };
            envelope::message(type_, library, &Signer::Device(device), &place, &s.hash)
        };
        let signed = |type_: &str, seq: u64, s: &Side| {
            super::crypto::verify(key, &message(type_, seq, s), &s.sig)
        };
        match self {
            Proof::Twice {
                type_, seq, a, b, ..
            } => {
                if !TYPES.contains(&type_.as_str()) || *seq == 0 {
                    return Err(ProofError::Type);
                }
                // Only a change's signature covers `prev` and `vroot` (SP6), and a
                // change's always covers its `vroot`.
                let shaped = |s: &Side| {
                    if type_ == "change" {
                        s.vroot.is_some()
                    } else {
                        s.prev.is_none() && s.vroot.is_none()
                    }
                };
                if !shaped(a) || !shaped(b) {
                    return Err(ProofError::Shape);
                }
                // The sides must differ in what is signed, not in how it is written:
                // a missing `prev` and an all-zero one are signed alike.
                if message(type_, *seq, a) == message(type_, *seq, b) {
                    return Err(ProofError::Same);
                }
                if signed(type_, *seq, a) && signed(type_, *seq, b) {
                    Ok(None)
                } else {
                    Err(ProofError::Signature)
                }
            }
            Proof::Closed {
                rotate,
                seq,
                change,
                ..
            } => {
                // The rotation's own payload, so `last` is read from signed bytes.
                let p = json::parse(&rotate.0, Numbers::Integers).map_err(|_| ProofError::Shape)?;
                let names = |k: &str, want: &Id16| {
                    p.get(k).and_then(Value::as_str).and_then(Id16::parse) == Some(*want)
                };
                if p.get("type").and_then(Value::as_str) != Some("rotate")
                    || !names("library", library)
                    || !names("old", &device)
                {
                    return Err(ProofError::Shape);
                }
                let last = match p.get("last") {
                    Some(Value::Null) => None,
                    Some(l) => Some(Mark {
                        s: l.get("s")
                            .and_then(Value::as_u64)
                            .ok_or(ProofError::Shape)?,
                        h: hash(l.get("h"))?,
                    }),
                    None => return Err(ProofError::Shape),
                };
                let rotate_side = Side {
                    hash: super::crypto::payload_hash(&rotate.0),
                    prev: None,
                    vroot: None,
                    sig: rotate.1,
                };
                if !signed("rotate", last.map_or(1, |m| m.s + 1), &rotate_side) {
                    return Err(ProofError::Signature);
                }
                if *seq <= last.map_or(0, |m| m.s) {
                    return Err(ProofError::NotAfter);
                }
                if change.vroot.is_none() || !signed("change", *seq, change) {
                    return Err(ProofError::Signature);
                }
                Ok(last)
            }
        }
    }
}

/// Where a change proof's cutoff stands (SP30).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cutoff {
    /// The latest change both branches share, or `None` when they share none (a
    /// fork at the device's first change: the cutoff is before any of them).
    Found(Option<Mark>),
    /// A change file a branch needs isn't among those given, so the branch can't
    /// yet be followed back to a change both share or to the chain held here.
    /// Until it resolves, the accepted-by-all mark is the cutoff (SP30).
    Pending,
}

/// SP30: a change proof's cutoff, the two branches' latest common predecessor.
/// Each branch is followed back by `prev` through the change files given, each
/// opened and checked against the device's key, until it reaches the chain this
/// device holds (`held`, the payload hash at each sequence) or its first change;
/// below that it is the held chain. The cutoff is fixed by the two signed sides,
/// so every device reaches the same one; the relayer's `base` is only a hint, and
/// isn't trusted. A file that doesn't open as the device's change is no evidence.
pub fn change_cutoff(
    proof: &Proof,
    library: &Id16,
    key: &[u8; 32],
    files: &[Vec<u8>],
    held: &dyn Fn(u64) -> Option<Hash32>,
) -> Result<Cutoff> {
    let Proof::Twice {
        device,
        type_,
        seq,
        a,
        b,
    } = proof
    else {
        return Err(ProofError::Type);
    };
    if type_ != "change" {
        return Err(ProofError::Type);
    }
    proof.check(library, key)?;
    let by_hash: std::collections::BTreeMap<Hash32, super::chain::Change> = files
        .iter()
        .filter_map(|f| super::chain::open_change(f, library, device, key).ok())
        .map(|c| (c.hash, c))
        .collect();
    // One branch, by sequence, from the side down to where it meets the held chain
    // (`joins`), or to its first change (`joins` = 0).
    struct Branch {
        at: std::collections::BTreeMap<u64, Hash32>,
        joins: u64,
    }
    let follow = |side: &Side| -> Option<Branch> {
        let mut at = std::collections::BTreeMap::new();
        at.insert(*seq, side.hash);
        let (mut s, mut prev) = (*seq - 1, side.prev);
        loop {
            let Some(p) = prev else {
                return (s == 0).then_some(Branch { at, joins: 0 });
            };
            if s == 0 {
                return None;
            }
            if held(s) == Some(p) {
                return Some(Branch { at, joins: s });
            }
            let c = by_hash.get(&p).filter(|c| c.seq == s)?;
            at.insert(s, p);
            prev = c.prev;
            s -= 1;
        }
    };
    let (Some(x), Some(y)) = (follow(a), follow(b)) else {
        return Ok(Cutoff::Pending);
    };
    let get = |br: &Branch, s: u64| {
        br.at
            .get(&s)
            .copied()
            .or_else(|| (s <= br.joins).then(|| held(s)).flatten())
    };
    for s in (1..*seq).rev() {
        match (get(&x, s), get(&y, s)) {
            (Some(h), Some(k)) if h == k => return Ok(Cutoff::Found(Some(Mark { s, h }))),
            _ => {}
        }
    }
    Ok(Cutoff::Found(None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::chain;
    use crate::sync::crypto::Keypair;
    use crate::sync::trust;

    const LIB: Id16 = Id16([7; 16]);
    const W: Id16 = Id16([8; 16]);

    /// A change's signed side, from the file `chain::sign_change` writes.
    fn change_side(c: &chain::Signed, prev: Option<Hash32>) -> Side {
        Side {
            hash: c.hash,
            prev,
            vroot: Some(envelope::vroot(&[])),
            sig: envelope::open(&c.bytes, "change", &LIB).unwrap().sigs[0].sig,
        }
    }

    #[test]
    fn two_changes_at_one_sequence_are_a_proof() {
        let a = Keypair::from_seed(&[1; 32]);
        let one = chain::sign_change(&a, &LIB, &W, 1, None, &[]);
        // Change 2 twice: on change 1, and on a made-up predecessor.
        let x = chain::sign_change(&a, &LIB, &W, 2, Some(one.hash), &[]);
        let y = chain::sign_change(&a, &LIB, &W, 2, Some(Hash32([3; 32])), &[]);
        let proof = Proof::Twice {
            device: a.device(),
            type_: "change".into(),
            seq: 2,
            a: change_side(&x, Some(one.hash)),
            b: change_side(&y, Some(Hash32([3; 32]))),
        };
        assert_eq!(proof.check(&LIB, &a.public()), Ok(None));
        // It reads back as written.
        assert_eq!(Proof::parse(&proof.to_json()).unwrap(), proof);
        // The same signature twice proves nothing; another key's signature, or
        // a type with no history, isn't a proof.
        let same = Proof::Twice {
            device: a.device(),
            type_: "change".into(),
            seq: 2,
            a: change_side(&x, Some(one.hash)),
            b: change_side(&x, Some(one.hash)),
        };
        assert_eq!(same.check(&LIB, &a.public()), Err(ProofError::Same));
        // Nor does one signature copied with its missing `prev` written as zeros:
        // both are signed alike.
        let zeros = Proof::Twice {
            device: a.device(),
            type_: "change".into(),
            seq: 1,
            a: change_side(&one, None),
            b: change_side(&one, Some(Hash32([0; 32]))),
        };
        assert_eq!(zeros.check(&LIB, &a.public()), Err(ProofError::Same));
        let b = Keypair::from_seed(&[2; 32]);
        assert_eq!(proof.check(&LIB, &b.public()), Err(ProofError::Signature));
        let Proof::Twice { a: sa, b: sb, .. } = proof.clone() else {
            unreachable!()
        };
        let claim = Proof::Twice {
            device: a.device(),
            type_: "claim".into(),
            seq: 2,
            a: sa,
            b: sb,
        };
        assert_eq!(claim.check(&LIB, &a.public()), Err(ProofError::Type));
    }

    #[test]
    fn two_device_files_at_one_counter_are_a_proof() {
        let a = Keypair::from_seed(&[1; 32]);
        let side_of = |name: &str| {
            let f = crate::sync::device_file::Fields {
                name: name.into(),
                ..Default::default()
            };
            let w = crate::sync::device_file::write(&a, &LIB, &W, 4, &f).unwrap();
            Side {
                hash: w.hash,
                prev: None,
                vroot: None,
                sig: envelope::open(&w.bytes, "device", &LIB).unwrap().sigs[0].sig,
            }
        };
        let proof = Proof::Twice {
            device: a.device(),
            type_: "device".into(),
            seq: 4,
            a: side_of("Mac"),
            b: side_of("Other"),
        };
        assert_eq!(proof.check(&LIB, &a.public()), Ok(None));
        // A device file's signature covers no `prev`: a side naming one isn't
        // SP30's shape, so one signature can't be made to look like two.
        let Proof::Twice { a: sa, .. } = proof.clone() else {
            unreachable!()
        };
        let with_prev = Proof::Twice {
            device: a.device(),
            type_: "device".into(),
            seq: 4,
            a: sa.clone(),
            b: Side {
                prev: Some(Hash32([0; 32])),
                ..sa
            },
        };
        assert_eq!(with_prev.check(&LIB, &a.public()), Err(ProofError::Shape));
        // A sequence of 0 has no place in a history.
        let Proof::Twice { a: sa, b: sb, .. } = proof else {
            unreachable!()
        };
        let zero = Proof::Twice {
            device: a.device(),
            type_: "device".into(),
            seq: 0,
            a: sa,
            b: sb,
        };
        assert_eq!(zero.check(&LIB, &a.public()), Err(ProofError::Type));
    }

    #[test]
    fn a_change_signed_after_a_rotation_closed_the_chain_is_a_proof() {
        let (a, new) = (Keypair::from_seed(&[1; 32]), Keypair::from_seed(&[2; 32]));
        let two = Hash32([4; 32]);
        // A rotates to `new` with `last` = change 2, signing with sequence 3.
        let (bytes, name) = trust::sign(
            "rotate",
            &LIB,
            json!({ "prev": "ab".repeat(32), "old": a.device().to_string(),
                    "last": { "s": 2, "h": two.to_string() }, "new": new.device().to_string(),
                    "key": b64url_encode(&new.public()), "clock": [1_760_000_000_000u64, 3] }),
            &[
                (&a, Signer::Device(a.device()), 3),
                (&new, Signer::Device(new.device()), 0),
            ],
        );
        let e = envelope::open(&bytes, "rotate", &LIB).unwrap();
        let rsig = e
            .sigs
            .iter()
            .find(|s| s.by == Signer::Device(a.device()))
            .unwrap()
            .sig;
        assert_eq!(e.hash, name);
        let closed = |seq: u64| {
            let c = chain::sign_change(&a, &LIB, &W, seq, Some(two), &[]);
            Proof::Closed {
                device: a.device(),
                rotate: (e.payload.clone(), rsig),
                seq,
                change: change_side(&c, Some(two)),
            }
        };
        // Change 3, signed after the chain closed at 2: a proof, with cutoff 2.
        let p = closed(3);
        assert_eq!(p.check(&LIB, &a.public()), Ok(Some(Mark { s: 2, h: two })));
        assert_eq!(Proof::parse(&p.to_json()).unwrap(), p);
        // Change 2 is within the closed chain: no proof.
        assert_eq!(
            closed(2).check(&LIB, &a.public()),
            Err(ProofError::NotAfter)
        );
    }

    #[test]
    fn a_change_proofs_cutoff_is_the_latest_change_both_branches_share() {
        let a = Keypair::from_seed(&[1; 32]);
        // The chain held here: changes 1–3.
        let c1 = chain::sign_change(&a, &LIB, &W, 1, None, &[]);
        let c2 = chain::sign_change(&a, &LIB, &W, 2, Some(c1.hash), &[]);
        let c3 = chain::sign_change(&a, &LIB, &W, 3, Some(c2.hash), &[]);
        let held = |s: u64| [c1.hash, c2.hash, c3.hash].get(s as usize - 1).copied();
        // Branch A: change 4 on change 3. Branch B forked at 3: another change 3,
        // then a change 4 on it.
        let a4 = chain::sign_change(&a, &LIB, &W, 4, Some(c3.hash), &[]);
        let b3 = chain::sign_change(&a, &LIB, &Id16([9; 16]), 3, Some(c2.hash), &[]);
        let b4 = chain::sign_change(&a, &LIB, &W, 4, Some(b3.hash), &[]);
        let proof = Proof::Twice {
            device: a.device(),
            type_: "change".into(),
            seq: 4,
            a: change_side(&a4, Some(c3.hash)),
            b: change_side(&b4, Some(b3.hash)),
        };
        let files = vec![a4.bytes.clone(), b4.bytes.clone(), b3.bytes.clone()];
        assert_eq!(
            change_cutoff(&proof, &LIB, &a.public(), &files, &held),
            Ok(Cutoff::Found(Some(Mark { s: 2, h: c2.hash })))
        );
        // Without B's change 3, B can't be followed back yet.
        let short = vec![a4.bytes.clone(), b4.bytes.clone()];
        assert_eq!(
            change_cutoff(&proof, &LIB, &a.public(), &short, &held),
            Ok(Cutoff::Pending)
        );
        // Another device's file in its place is no evidence.
        let other = Keypair::from_seed(&[2; 32]);
        let forged = chain::sign_change(&other, &LIB, &W, 3, Some(c2.hash), &[]);
        let wrong = vec![a4.bytes.clone(), b4.bytes.clone(), forged.bytes];
        assert_eq!(
            change_cutoff(&proof, &LIB, &a.public(), &wrong, &held),
            Ok(Cutoff::Pending)
        );
        // A change 4 whose `prev` names change 2: the file it names isn't the
        // change before it, so that branch can't be followed.
        let skip = chain::sign_change(&a, &LIB, &Id16([9; 16]), 4, Some(c2.hash), &[]);
        let skipping = Proof::Twice {
            device: a.device(),
            type_: "change".into(),
            seq: 4,
            a: change_side(&a4, Some(c3.hash)),
            b: change_side(&skip, Some(c2.hash)),
        };
        let held_two = |s: u64| [c1.hash, c2.hash].get(s as usize - 1).copied();
        assert_eq!(
            change_cutoff(
                &skipping,
                &LIB,
                &a.public(),
                &[c2.bytes.clone(), c3.bytes.clone()],
                &held_two
            ),
            Ok(Cutoff::Pending)
        );
    }

    #[test]
    fn a_fork_at_the_first_change_cuts_off_before_any() {
        let a = Keypair::from_seed(&[1; 32]);
        let x = chain::sign_change(&a, &LIB, &W, 1, None, &[]);
        let y = chain::sign_change(&a, &LIB, &Id16([9; 16]), 1, None, &[]);
        let proof = Proof::Twice {
            device: a.device(),
            type_: "change".into(),
            seq: 1,
            a: change_side(&x, None),
            b: change_side(&y, None),
        };
        assert_eq!(
            change_cutoff(&proof, &LIB, &a.public(), &[], &|_| None),
            Ok(Cutoff::Found(None))
        );
    }
}
