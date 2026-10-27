//! SP30, SP16: relaying checked equivocation proofs with exact copies and ancestry
//! bundles in a device file. #122 bounds the proofs and ancestry carried here.
//! No storage or sync pass is wired in; ancestry needing a spine stays pending.

use super::device_file::{
    overflow_path, DeviceFile, Extra, Overflow, MAX_OVERFLOW_FILE, MAX_OVERFLOW_VALUES,
};
use super::ids::{b64url_decode, b64url_encode, identity_hash, DeviceId, Hash32, Id16};
use super::json::{self, Numbers};
use super::proof::{self, Cutoff, Proof};
use super::trust::Mark;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// #122: PROVISIONAL, at most one proof per accused identity and 32 in all.
pub const MAX_PROOFS: usize = 32;
/// SP30: PROVISIONAL, encoded ancestry bytes (including standalone copies).
pub const MAX_ANCESTRY_BYTES: usize = 32 << 20;
/// SP30: PROVISIONAL, at most four ancestry bundles per proof.
pub const MAX_ANCESTRY_BUNDLES: usize = 4;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelayError {
    #[error("more than 32 proofs, or more than one proof for an accused device")]
    Cap,
    #[error("change copies must accompany exactly the change proofs")]
    Shape,
    #[error("a copy exceeds SP8's 8 MB limit")]
    TooLarge,
}

pub type Result<T> = std::result::Result<T, RelayError>;
pub type RelayedParts = (Vec<Value>, Vec<Extra>);

/// SP30: the caller has already checked `proof` with the accused device's key.
#[derive(Clone, Debug)]
pub struct Relayed {
    pub proof: Proof,
    pub change: Option<ChangeFiles>,
}

#[derive(Clone, Debug)]
pub struct ChangeFiles {
    /// Exact bytes of sides `a` and `b`, in that order (not their payload bytes).
    pub copies: [Vec<u8>; 2],
    /// A hint only: readers establish the cutoff from signed links.
    pub base: Option<Mark>,
    pub ancestry: Vec<Vec<u8>>,
}

fn bundle(part: u64, items: &[Value]) -> Vec<u8> {
    json::canonical(&json!({ "field": "bundle", "part": part, "items": items }))
}

fn extra(field: &str, part: u64, bytes: Vec<u8>) -> Extra {
    Extra {
        field: field.into(),
        part,
        bytes,
    }
}

/// SP30, #122: returns list entries and extra parts for
/// `device_file::write_with_overflow`. Oversized ancestry is omitted in its
/// entirety; the two side copies and the untrusted `base` hint remain.
pub fn relayed(proofs: &[Relayed]) -> Result<RelayedParts> {
    relayed_with_limits(
        proofs,
        MAX_ANCESTRY_BYTES,
        MAX_ANCESTRY_BUNDLES,
        MAX_OVERFLOW_FILE,
    )
}

// The same packer takes smaller limits in tests, without changing production
// constants or allocating 32 MB just to exercise the limit (SP30).
fn relayed_with_limits(
    proofs: &[Relayed],
    max_bytes: usize,
    max_bundles: usize,
    max_part: usize,
) -> Result<RelayedParts> {
    let mut devices = BTreeSet::new();
    if proofs.len() > MAX_PROOFS || proofs.iter().any(|p| !devices.insert(p.proof.device())) {
        return Err(RelayError::Cap);
    }
    let mut entries = Vec::new();
    let mut parts = Vec::new();
    let (mut copy_part, mut bundle_part) = (0, 0);
    for p in proofs {
        let mut v = p.proof.to_json();
        let is_change = matches!(&p.proof, Proof::Twice { type_, .. } if type_ == "change");
        if is_change != p.change.is_some() {
            return Err(RelayError::Shape);
        }
        if let Some(c) = &p.change {
            for (side, bytes) in ["a", "b"].into_iter().zip(&c.copies) {
                if bytes.len() > max_part {
                    return Err(RelayError::TooLarge);
                }
                v[side]["copy"] = json!(identity_hash(bytes).to_string());
                parts.push(extra("copy", copy_part, bytes.clone()));
                copy_part += 1;
            }
            v["base"] = c
                .base
                .map_or(Value::Null, |m| json!({ "s": m.s, "h": m.h.to_string() }));
            if let Some(ancestry) = pack_ancestry(
                &c.ancestry,
                copy_part,
                bundle_part,
                max_bytes,
                max_bundles,
                max_part,
            )? {
                v["ancestry"] = json!(ancestry
                    .iter()
                    .map(|p| identity_hash(&p.bytes).to_string())
                    .collect::<Vec<_>>());
                copy_part += ancestry.iter().filter(|p| p.field == "copy").count() as u64;
                bundle_part += ancestry.iter().filter(|p| p.field == "bundle").count() as u64;
                parts.extend(ancestry);
            }
        }
        entries.push(v);
    }
    Ok((entries, parts))
}

fn pack_ancestry(
    files: &[Vec<u8>],
    mut copy_part: u64,
    mut bundle_part: u64,
    max_bytes: usize,
    max_bundles: usize,
    max_part: usize,
) -> Result<Option<Vec<Extra>>> {
    let mut parts = Vec::new();
    let mut items = Vec::new();
    let mut size = bundle(bundle_part, &[]).len();
    let (mut total, mut bundles) = (0, 0);
    for bytes in files {
        if bytes.len() > max_part {
            return Err(RelayError::TooLarge);
        }
        let encoded = b64url_encode(bytes);
        let n = encoded.len() + 2; // The JSON string's quotes; base64url needs no escaping.
        if !items.is_empty() && size + 1 + n > max_part {
            let b = bundle(bundle_part, &items);
            total += b.len();
            parts.push(extra("bundle", bundle_part, b));
            bundles += 1;
            bundle_part += 1;
            items.clear();
            size = bundle(bundle_part, &[]).len();
        }
        // Recheck the wrapper after advancing `part`: 9 -> 10 adds a byte.
        if items.is_empty() && size + n > max_part {
            total += bytes.len();
            parts.push(extra("copy", copy_part, bytes.clone()));
            copy_part += 1;
        } else {
            size += n + usize::from(!items.is_empty());
            items.push(Value::String(encoded));
        }
        if total + if items.is_empty() { 0 } else { size } > max_bytes
            || bundles + usize::from(!items.is_empty()) > max_bundles
        {
            return Ok(None);
        }
    }
    if !items.is_empty() {
        parts.push(extra("bundle", bundle_part, bundle(bundle_part, &items)));
    }
    Ok(Some(parts))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Quarantine {
    At(Option<Mark>),
    /// SP30: use the accepted-by-all mark until the ancestry can be resolved.
    Pending,
    /// Non-change `Twice` proofs: the caller resolves the cutoff from frontiers
    /// (and, for rotations, the signed `last` values). No cutoff is inferred here.
    ByFrontier,
    /// This entry quarantines nobody.
    Invalid(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub device: DeviceId,
    pub cutoff: Quarantine,
}

type Fetch<'a> = &'a dyn Fn(&str, u64) -> Option<Vec<u8>>;

// SP16, SP8: supply the size bound to storage, then check it and the hash before
// any JSON or base64 decoding. Missing descriptors/files are retryable.
fn fetch_part(
    file: &DeviceFile,
    o: &Overflow,
    fetch: Fetch<'_>,
) -> std::result::Result<Vec<u8>, Quarantine> {
    if o.size > MAX_OVERFLOW_FILE as u64 {
        return Err(Quarantine::Invalid("overflow size exceeds SP8".into()));
    }
    let b = fetch(&overflow_path(&file.device, &o.hash), o.size).ok_or(Quarantine::Pending)?;
    if b.len() as u64 != o.size || identity_hash(&b) != o.hash {
        return Err(Quarantine::Invalid("overflow hash or size mismatch".into()));
    }
    Ok(b)
}

fn part_items(bytes: &[u8], o: &Overflow, left: &mut u64) -> Option<Vec<Value>> {
    let (v, rest) = json::parse_within(bytes, Numbers::Integers, *left).ok()?;
    *left = rest;
    if v.get("field").and_then(Value::as_str) != Some(o.field.as_str())
        || v.get("part").and_then(Value::as_u64) != Some(o.part)
    {
        return None;
    }
    v.get("items")?.as_array().cloned()
}

// Read only the proofs list, in SP16 order. An absent or damaged list part stops
// this pass at the known prefix; retry on the next pass. It cannot supply an
// accused identity until its bytes have been verified. Other lists aren't read.
fn entries(file: &DeviceFile, fetch: Fetch<'_>) -> Vec<Value> {
    let mut out: Vec<_> = file
        .fields
        .lists
        .get("proofs")
        .into_iter()
        .flatten()
        .take(MAX_PROOFS)
        .cloned()
        .collect();
    let mut parts: Vec<_> = file
        .overflow
        .iter()
        .filter(|o| o.field == "proofs")
        .collect();
    parts.sort_by_key(|o| o.part);
    let mut left = MAX_OVERFLOW_VALUES;
    for (i, o) in parts.into_iter().enumerate() {
        if out.len() == MAX_PROOFS || o.part != i as u64 {
            break;
        }
        let Ok(bytes) = fetch_part(file, o, fetch) else {
            break;
        };
        let Some(items) = part_items(&bytes, o, &mut left) else {
            break;
        };
        out.extend(items.into_iter().take(MAX_PROOFS - out.len()));
    }
    out
}

/// SP30, #122: checks only the first 32 list entries and the first entry for each
/// accused identity, in list order. Malformed entries without an identity are
/// ignored but consume a slot. A missing/damaged proofs-list part ends the known
/// prefix for this pass. `fetch(path, size)` must read at most `size` bytes.
/// `keys` supplies the accused's key, never the relayer's. Only `At` is a resolved
/// cutoff; `Pending` and `ByFrontier` require the caller's SP30 policy.
pub fn read(
    file: &DeviceFile,
    fetch: Fetch<'_>,
    library: &Id16,
    keys: &dyn Fn(&DeviceId) -> Option<[u8; 32]>,
    held: &dyn Fn(&DeviceId, u64) -> Option<Hash32>,
) -> Vec<Read> {
    let mut seen = BTreeSet::new();
    entries(file, fetch)
        .iter()
        .filter_map(|v| {
            let device = v
                .get("device")
                .and_then(Value::as_str)
                .and_then(Id16::parse)?;
            if !seen.insert(device) {
                return None;
            }
            let cutoff = read_one(v, file, fetch, library, keys, held).unwrap_or_else(|q| q);
            Some(Read { device, cutoff })
        })
        .collect()
}

fn read_one(
    v: &Value,
    file: &DeviceFile,
    fetch: Fetch<'_>,
    library: &Id16,
    keys: &dyn Fn(&DeviceId) -> Option<[u8; 32]>,
    held: &dyn Fn(&DeviceId, u64) -> Option<Hash32>,
) -> std::result::Result<Quarantine, Quarantine> {
    let invalid = |e: proof::ProofError| Quarantine::Invalid(e.to_string());
    let p = Proof::parse(v).map_err(invalid)?;
    let device = p.device();
    let key = keys(&device).ok_or_else(|| Quarantine::Invalid("unknown accused key".into()))?;
    let cutoff = p.check(library, &key).map_err(invalid)?;
    match &p {
        Proof::Closed { .. } => return Ok(Quarantine::At(cutoff)),
        Proof::Twice { type_, .. } if type_ != "change" => return Ok(Quarantine::ByFrontier),
        _ => {}
    }
    // SP30: the two checked signatures quarantine the device. What its copies and
    // ancestry can't settle, missing or damaged, only leaves the cutoff pending.
    Ok(change(&p, v, file, fetch, library, &key, held).unwrap_or(Quarantine::Pending))
}

/// SP30: a checked change proof's cutoff from the copies and ancestry it names.
fn change(
    p: &Proof,
    v: &Value,
    file: &DeviceFile,
    fetch: Fetch<'_>,
    library: &Id16,
    key: &[u8; 32],
    held: &dyn Fn(&DeviceId, u64) -> Option<Hash32>,
) -> std::result::Result<Quarantine, Quarantine> {
    let device = p.device();
    let invalid = |e: proof::ProofError| Quarantine::Invalid(e.to_string());
    let descriptor = |value: Option<&Value>, copy: bool| {
        let hash = value
            .and_then(Value::as_str)
            .and_then(Hash32::parse)
            .ok_or_else(|| Quarantine::Invalid("missing or malformed copy/ancestry hash".into()))?;
        let o = file
            .overflow
            .iter()
            .find(|o| o.hash == hash)
            .ok_or(Quarantine::Pending)?;
        if o.field != "copy" && (copy || o.field != "bundle") {
            return Err(Quarantine::Invalid(
                "proof references the wrong overflow field".into(),
            ));
        }
        Ok(o)
    };
    let mut files = Vec::new();
    for side in ["a", "b"] {
        let o = descriptor(v.get(side).and_then(|s| s.get("copy")), true)?;
        files.push(fetch_part(file, o, fetch)?);
    }
    let ancestry = match v.get("ancestry") {
        None => &[][..],
        Some(Value::Array(a)) => a.as_slice(),
        _ => return Err(Quarantine::Invalid("ancestry is not a list".into())),
    };
    let mut named = Vec::new();
    let (mut bytes, mut bundles) = (0u64, 0);
    for h in ancestry {
        let o = descriptor(Some(h), false)?;
        bytes = bytes.saturating_add(o.size);
        bundles += usize::from(o.field == "bundle");
        if bytes > MAX_ANCESTRY_BYTES as u64 || bundles > MAX_ANCESTRY_BUNDLES {
            return Ok(Quarantine::Pending);
        }
        named.push(o);
    }
    let mut left = MAX_OVERFLOW_VALUES;
    for o in named {
        let b = fetch_part(file, o, fetch)?;
        if o.field == "copy" {
            files.push(b);
        } else {
            let items = part_items(&b, o, &mut left)
                .ok_or_else(|| Quarantine::Invalid("malformed ancestry bundle".into()))?;
            for item in items {
                let b = item
                    .as_str()
                    .and_then(b64url_decode)
                    .ok_or_else(|| Quarantine::Invalid("invalid bundle base64url".into()))?;
                files.push(b);
            }
        }
    }
    // SP30: base is deliberately ignored; only checked links determine a cutoff.
    Ok(
        match proof::change_cutoff(p, library, key, &files, &|s| held(&device, s))
            .map_err(invalid)?
        {
            Cutoff::Found(m) => Quarantine::At(m),
            Cutoff::Pending => Quarantine::Pending,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{
        chain,
        crypto::{self, Keypair},
        device_file::{self, Fields},
        envelope::{self, Place, Signer},
        proof::Side,
        trust,
    };
    use std::{cell::Cell, collections::BTreeMap};

    const LIB: Id16 = Id16([7; 16]);
    const W: Id16 = Id16([8; 16]);
    type Storage = BTreeMap<String, Vec<u8>>;

    fn side(c: &chain::Signed, prev: Option<Hash32>) -> Side {
        Side {
            hash: c.hash,
            prev,
            vroot: Some(envelope::vroot(&[])),
            sig: envelope::open(&c.bytes, "change", &LIB).unwrap().sigs[0].sig,
        }
    }

    // SP30: the same signed-fork construction as proof.rs, with both branches'
    // complete ancestry so a reader with no held chain can find the cutoff.
    fn fork() -> (Keypair, Relayed, Mark) {
        let key = Keypair::from_seed(&[1; 32]);
        let c1 = chain::sign_change(&key, &LIB, &W, 1, None, &[]);
        let a2 = chain::sign_change(&key, &LIB, &W, 2, Some(c1.hash), &[]);
        let b2 = chain::sign_change(&key, &LIB, &Id16([9; 16]), 2, Some(c1.hash), &[]);
        let a3 = chain::sign_change(&key, &LIB, &W, 3, Some(a2.hash), &[]);
        let b3 = chain::sign_change(&key, &LIB, &W, 3, Some(b2.hash), &[]);
        let mark = Mark { s: 1, h: c1.hash };
        let p = Proof::Twice {
            device: key.device(),
            type_: "change".into(),
            seq: 3,
            a: side(&a3, Some(a2.hash)),
            b: side(&b3, Some(b2.hash)),
        };
        p.check(&LIB, &key.public()).unwrap();
        (
            key,
            Relayed {
                proof: p,
                change: Some(ChangeFiles {
                    copies: [a3.bytes, b3.bytes],
                    base: Some(mark),
                    ancestry: vec![c1.bytes, a2.bytes, b2.bytes],
                }),
            },
            mark,
        )
    }

    fn closed(key: &Keypair) -> Relayed {
        let last = Hash32([4; 32]);
        let (bytes, _) = trust::sign(
            "rotate",
            &LIB,
            json!({ "old": key.device().to_string(), "last": { "s": 2, "h": last.to_string() } }),
            &[(key, Signer::Device(key.device()), 3)],
        );
        let e = envelope::open(&bytes, "rotate", &LIB).unwrap();
        let c = chain::sign_change(key, &LIB, &W, 3, Some(last), &[]);
        Relayed {
            proof: Proof::Closed {
                device: key.device(),
                rotate: (e.payload, e.sigs[0].sig),
                seq: 3,
                change: side(&c, Some(last)),
            },
            change: None,
        }
    }

    fn device_file(entries: Vec<Value>, parts: &[Extra]) -> (DeviceFile, Storage) {
        let key = Keypair::from_seed(&[99; 32]);
        let mut f = Fields::default();
        f.lists.insert("proofs".into(), entries);
        let written = device_file::write_with_overflow(&key, &LIB, &W, 1, &f, parts).unwrap();
        let opened = device_file::open(&written.bytes, &LIB, &key.device(), &key.public()).unwrap();
        (opened, written.parts.into_iter().collect())
    }

    fn round_trip(p: &Relayed) -> (DeviceFile, Storage) {
        let (entries, parts) = relayed(std::slice::from_ref(p)).unwrap();
        device_file(entries, &parts)
    }

    fn read_for(f: &DeviceFile, storage: &Storage, key: &Keypair) -> Vec<Read> {
        read(
            f,
            &|path, size| {
                assert!(size <= MAX_OVERFLOW_FILE as u64);
                storage.get(path).cloned()
            },
            &LIB,
            &|d| (*d == key.device()).then(|| key.public()),
            &|_, _| None,
        )
    }

    #[test]
    fn a_change_proof_round_trips_through_a_device_file() {
        let (key, mut p, mark) = fork();
        let c = p.change.as_mut().unwrap();
        // A relayer's false base must not replace the checked common predecessor.
        c.base = Some(Mark {
            s: 200,
            h: Hash32([5; 32]),
        });
        let files: Vec<_> = c.copies.iter().chain(&c.ancestry).cloned().collect();
        let direct =
            proof::change_cutoff(&p.proof, &LIB, &key.public(), &files, &|_| None).unwrap();
        assert_eq!(direct, Cutoff::Found(Some(mark)));
        let (f, storage) = round_trip(&p);
        assert_eq!(f.overflow.iter().filter(|o| o.field == "copy").count(), 2);
        assert_eq!(f.overflow.iter().filter(|o| o.field == "bundle").count(), 1);
        assert_eq!(
            read_for(&f, &storage, &key),
            vec![Read {
                device: key.device(),
                cutoff: Quarantine::At(Some(mark))
            }]
        );
        // SP16 list readers still join lists, and never fetch a proof's copies
        // or bundles: only `read` does.
        let attachments: Vec<String> = f
            .overflow
            .iter()
            .filter(|o| matches!(o.field.as_str(), "copy" | "bundle"))
            .map(|o| overflow_path(&f.device, &o.hash))
            .collect();
        assert_eq!(
            f.lists(&|path| {
                assert!(!attachments.iter().any(|a| a == path), "fetched {path}");
                storage.get(path).cloned()
            })
            .unwrap(),
            f.fields.lists
        );
        // The same signed ancestry travels as exact copies when its encoded
        // files don't fit a bundle. Use a small local part limit to exercise it.
        let c = p.change.as_ref().unwrap();
        let max_part = c
            .copies
            .iter()
            .chain(&c.ancestry)
            .map(Vec::len)
            .max()
            .unwrap();
        let (entries, parts) =
            relayed_with_limits(std::slice::from_ref(&p), MAX_ANCESTRY_BYTES, 4, max_part).unwrap();
        assert!(parts.iter().all(|p| p.field == "copy"));
        let (f, storage) = device_file(entries, &parts);
        assert_eq!(
            read_for(&f, &storage, &key)[0].cutoff,
            Quarantine::At(Some(mark))
        );

        // The caller's held chain is queried for the accused, not the relayer.
        p.change.as_mut().unwrap().ancestry.remove(0);
        let (f, storage) = round_trip(&p);
        assert_eq!(read_for(&f, &storage, &key)[0].cutoff, Quarantine::Pending);
        let got = read(
            &f,
            &|path, _| storage.get(path).cloned(),
            &LIB,
            &|_| Some(key.public()),
            &|d, s| {
                assert_eq!(*d, key.device());
                (s == 1).then_some(mark.h)
            },
        );
        assert_eq!(got[0].cutoff, Quarantine::At(Some(mark)));
    }

    #[test]
    fn a_closed_proof_is_relayed_and_read() {
        let key = Keypair::from_seed(&[1; 32]);
        let p = closed(&key);
        let cutoff = p.proof.check(&LIB, &key.public()).unwrap();
        assert_eq!(
            cutoff,
            Some(Mark {
                s: 2,
                h: Hash32([4; 32])
            })
        );
        let (f, storage) = round_trip(&p);
        assert!(storage.is_empty());
        assert_eq!(
            read_for(&f, &storage, &key)[0].cutoff,
            Quarantine::At(cutoff)
        );
    }

    #[test]
    fn a_proof_that_doesnt_check_quarantines_nobody() {
        let (key, p, _) = fork();
        let (mut entries, parts) = relayed(&[p]).unwrap();
        entries[0]["a"]["sig"] = json!(b64url_encode(&[0; 64]));
        let (f, _) = device_file(entries, &parts);
        let got = read(
            &f,
            &|_, _| panic!("invalid proof must not fetch attachments"),
            &LIB,
            &|_| Some(key.public()),
            &|_, _| None,
        );
        assert!(matches!(got[0].cutoff, Quarantine::Invalid(_)));
    }

    #[test]
    fn a_missing_bundle_leaves_the_proof_pending() {
        let (key, p, _) = fork();
        let (f, mut storage) = round_trip(&p);
        let o = f.overflow.iter().find(|o| o.field == "bundle").unwrap();
        storage.remove(&overflow_path(&f.device, &o.hash));
        assert_eq!(read_for(&f, &storage, &key)[0].cutoff, Quarantine::Pending);
    }

    #[test]
    fn a_damaged_attachment_is_refused_but_the_proof_still_quarantines() {
        // SP30: the checked signatures quarantine the device; a bundle or copy
        // that doesn't match its hash is never used, and leaves the cutoff pending.
        for field in ["bundle", "copy"] {
            let (key, p, _) = fork();
            let (f, mut storage) = round_trip(&p);
            let o = f.overflow.iter().find(|o| o.field == field).unwrap();
            let b = storage.get_mut(&overflow_path(&f.device, &o.hash)).unwrap();
            b[0] ^= 1;
            assert_eq!(
                read_for(&f, &storage, &key),
                vec![Read {
                    device: key.device(),
                    cutoff: Quarantine::Pending
                }]
            );
        }
    }

    #[test]
    fn the_proofs_list_is_capped() {
        let keys: Vec<_> = (1..=40).map(|i| Keypair::from_seed(&[i; 32])).collect();
        let proofs: Vec<_> = keys.iter().map(closed).collect();
        assert_eq!(relayed(&proofs[..33]), Err(RelayError::Cap));
        assert!(relayed(&proofs[..32]).is_ok());
        assert_eq!(
            relayed(&[proofs[0].clone(), proofs[0].clone()]),
            Err(RelayError::Cap)
        );
        let entries = proofs.iter().map(|p| p.proof.to_json()).collect();
        let (f, _) = device_file(entries, &[]);
        let calls = Cell::new(0);
        let read_keys = |d: &DeviceId| {
            calls.set(calls.get() + 1);
            keys.iter().find(|k| k.device() == *d).map(Keypair::public)
        };
        let got = read(&f, &|_, _| None, &LIB, &read_keys, &|_, _| None);
        assert_eq!(got.len(), 32);
        assert_eq!(calls.get(), 32);
        assert_eq!(
            got.iter().map(|r| r.device).collect::<Vec<_>>(),
            keys[..32].iter().map(Keypair::device).collect::<Vec<_>>()
        );
        assert!(got
            .iter()
            .all(|r| matches!(r.cutoff, Quarantine::At(Some(_)))));
        let (f, _) = device_file(vec![proofs[0].proof.to_json(); 2], &[]);
        assert_eq!(read_for(&f, &Storage::new(), &keys[0]).len(), 1);
        // Duplicates consume slots; entry 33 cannot bypass the first-32 cap.
        let mut entries = vec![proofs[0].proof.to_json(); 32];
        entries.push(proofs[1].proof.to_json());
        let (f, _) = device_file(entries, &[]);
        assert_eq!(
            read(&f, &|_, _| None, &LIB, &read_keys, &|_, _| None).len(),
            1
        );
    }

    #[test]
    fn an_ancestry_past_the_limit_is_relayed_without_it_and_reads_pending() {
        let (key, p, _) = fork();
        // Exercise each production limit through the same packer, with small
        // limits local to this test. No 32 MB fixture or cfg-altered constants.
        for (bytes, bundles) in [(1, MAX_ANCESTRY_BUNDLES), (MAX_ANCESTRY_BYTES, 0)] {
            let (entries, parts) =
                relayed_with_limits(std::slice::from_ref(&p), bytes, bundles, MAX_OVERFLOW_FILE)
                    .unwrap();
            assert!(entries[0].get("ancestry").is_none());
            assert_eq!(parts.len(), 2);
            assert!(parts.iter().all(|p| p.field == "copy"));
            let (f, storage) = device_file(entries, &parts);
            assert_eq!(read_for(&f, &storage, &key)[0].cutoff, Quarantine::Pending);
        }
    }

    #[test]
    fn bundles_split_and_large_encoded_files_use_exact_copies() {
        // Small limits exercise encoded size rather than just raw input size.
        let files = vec![vec![1; 100], vec![2; 100], vec![3; 200]];
        let parts = pack_ancestry(&files, 2, 0, 1000, 4, 250).unwrap().unwrap();
        assert_eq!(parts.iter().filter(|p| p.field == "bundle").count(), 2);
        assert_eq!(
            parts.iter().find(|p| p.field == "copy").unwrap().bytes,
            files[2]
        );
        assert!(parts.iter().all(|p| p.bytes.len() <= 250));
        for p in parts.iter().filter(|p| p.field == "bundle") {
            let v = json::parse(&p.bytes, Numbers::Integers).unwrap();
            assert_eq!(v["part"], p.part);
            let decoded = b64url_decode(v["items"][0].as_str().unwrap()).unwrap();
            assert!(files[..2].contains(&decoded));
        }
        assert_eq!(
            pack_ancestry(&[vec![0; 251]], 0, 0, 1000, 4, 250),
            Err(RelayError::TooLarge)
        );
        assert!(pack_ancestry(&files, 2, 0, 1000, 1, 250).unwrap().is_none());
        let boundary = bundle(9, &[json!(b64url_encode(&files[0]))]).len();
        let parts = pack_ancestry(&files[..2], 0, 9, 1000, 4, boundary)
            .unwrap()
            .unwrap();
        assert!(parts.iter().all(|p| p.bytes.len() <= boundary));
        assert_eq!(parts[0].field, "bundle");
        assert_eq!(parts[1].field, "copy");
    }

    #[test]
    fn an_overflowed_proofs_list_is_read_in_order_and_stops_at_the_cap() {
        let key = Keypair::from_seed(&[1; 32]);
        let mut entries = vec![closed(&key).proof.to_json(); 40];
        for v in &mut entries {
            v["padding"] = json!("x".repeat(8000));
        }
        let (f, storage) = device_file(entries, &[]);
        assert!(f.fields.lists["proofs"].is_empty());
        assert!(f.overflow.iter().any(|o| o.field == "proofs"));
        assert_eq!(read_for(&f, &storage, &key).len(), 1);
        // Inline entries precede all overflow parts; nothing past 32 is fetched.
        let mut f = f;
        f.fields
            .lists
            .insert("proofs".into(), vec![closed(&key).proof.to_json(); 32]);
        assert_eq!(
            read(
                &f,
                &|_, _| panic!("past cap"),
                &LIB,
                &|_| Some(key.public()),
                &|_, _| None
            )
            .len(),
            1
        );
    }

    #[test]
    fn non_change_twice_proofs_leave_the_cutoff_to_the_caller() {
        let key = Keypair::from_seed(&[1; 32]);
        for type_ in ["view", "device", "enroll", "rotate"] {
            let signed = |bytes: &[u8]| Side {
                hash: crypto::payload_hash(bytes),
                prev: None,
                vroot: None,
                sig: envelope::sign(
                    &key,
                    Signer::Device(key.device()),
                    type_,
                    &LIB,
                    &Place {
                        seq: 1,
                        ..Default::default()
                    },
                    bytes,
                )
                .sig,
            };
            let p = Relayed {
                proof: Proof::Twice {
                    device: key.device(),
                    type_: type_.into(),
                    seq: 1,
                    a: signed(b"a"),
                    b: signed(b"b"),
                },
                change: None,
            };
            let (f, storage) = round_trip(&p);
            assert_eq!(
                read_for(&f, &storage, &key)[0].cutoff,
                Quarantine::ByFrontier
            );
        }
    }

    #[test]
    fn copy_sizes_and_bundle_shapes_are_checked_before_use() {
        // SP30: a refused attachment leaves the checked proof's cutoff pending.
        let (key, p, _) = fork();
        let (mut f, storage) = round_trip(&p);
        let copy = f.overflow.iter_mut().find(|o| o.field == "copy").unwrap();
        copy.size += 1;
        assert_eq!(read_for(&f, &storage, &key)[0].cutoff, Quarantine::Pending);
        copy_size_over_limit(&key, &p);
        for bad in [
            json!({ "field": "bundle", "part": 1, "items": [] }),
            json!({ "field": "bundle", "part": 0, "items": ["!"] }),
        ] {
            let (mut f, mut storage) = round_trip(&p);
            let o = f.overflow.iter_mut().find(|o| o.field == "bundle").unwrap();
            let bytes = json::canonical(&bad);
            o.hash = identity_hash(&bytes);
            o.size = bytes.len() as u64;
            f.fields.lists.get_mut("proofs").unwrap()[0]["ancestry"] = json!([o.hash.to_string()]);
            storage.insert(overflow_path(&f.device, &o.hash), bytes);
            assert_eq!(read_for(&f, &storage, &key)[0].cutoff, Quarantine::Pending);
        }
    }

    fn copy_size_over_limit(key: &Keypair, p: &Relayed) {
        let (mut f, _) = round_trip(p);
        f.overflow
            .iter_mut()
            .find(|o| o.field == "copy")
            .unwrap()
            .size = MAX_OVERFLOW_FILE as u64 + 1;
        let got = read(
            &f,
            &|_, _| panic!("oversized descriptor must not be fetched"),
            &LIB,
            &|_| Some(key.public()),
            &|_, _| None,
        );
        // Refused unread; the checked proof still quarantines (SP30).
        assert_eq!(got[0].cutoff, Quarantine::Pending);
    }

    #[test]
    fn missing_copies_and_unknown_keys_do_not_produce_a_cutoff() {
        let (key, p, _) = fork();
        let (mut f, mut storage) = round_trip(&p);
        let copy = f.overflow.iter().find(|o| o.field == "copy").unwrap();
        storage.remove(&overflow_path(&f.device, &copy.hash));
        assert_eq!(read_for(&f, &storage, &key)[0].cutoff, Quarantine::Pending);
        f.overflow.retain(|o| o.field != "copy");
        assert_eq!(read_for(&f, &storage, &key)[0].cutoff, Quarantine::Pending);
        assert!(matches!(
            read(
                &f,
                &|_, _| panic!("unknown key"),
                &LIB,
                &|_| None,
                &|_, _| None
            )[0]
            .cutoff,
            Quarantine::Invalid(_)
        ));
    }

    #[test]
    fn excessive_ancestry_is_not_fetched() {
        let (key, p, _) = fork();
        for field in ["bundle", "copy"] {
            let (mut f, storage) = round_trip(&p);
            let hash = Hash32([55; 32]);
            f.overflow.push(Overflow {
                field: field.into(),
                part: 50,
                hash,
                size: if field == "bundle" {
                    1
                } else {
                    MAX_OVERFLOW_FILE as u64
                },
            });
            f.fields.lists.get_mut("proofs").unwrap()[0]["ancestry"] =
                json!(vec![hash.to_string(); 5]);
            let got = read(
                &f,
                &|path, _| {
                    assert_ne!(path, overflow_path(&f.device, &hash));
                    storage.get(path).cloned()
                },
                &LIB,
                &|_| Some(key.public()),
                &|_, _| None,
            );
            assert_eq!(got[0].cutoff, Quarantine::Pending);
        }
    }

    #[test]
    fn writing_without_extras_preserves_existing_device_bytes() {
        let key = Keypair::from_seed(&[1; 32]);
        let f = Fields::default();
        assert_eq!(
            device_file::write(&key, &LIB, &W, 1, &f).unwrap(),
            device_file::write_with_overflow(&key, &LIB, &W, 1, &f, &[]).unwrap()
        );
        let bad = extra("frontier", 0, b"{}".to_vec());
        assert!(device_file::write_with_overflow(&key, &LIB, &W, 1, &f, &[bad]).is_err());
    }
}
