//! SP19: the claim and the membership records. Each is a signed file named by its
//! payload hash, `trust/‹hash›.json`; the claim is `libraries/‹library›.json`.
//!
//! This module opens each record and checks what the record alone can show: its
//! fields, that a key it introduces is the one its device ID names and signed it,
//! that an enrollment's reveal names its enroller and applicant, and that a
//! sealing record carries its next code key's signature. Which devices and code
//! keys count, and so whether the other signatures do, is the membership's
//! computation (SP20, 1c.4b), which checks them with `signed_by`.
//!
//! PROVISIONAL: where SP19 leaves the bytes open, the choices here are Delegated
//! to Claude, not approved (`docs/decisions.md`, “The membership records”); each
//! place cites its item.

use super::clock::Hlc;
use super::envelope::{self, Envelope, EnvelopeError, Place, Signer};
use super::ids::{b64url_decode, device_id, DeviceId, Hash32, Id16};
use super::json::{self, Numbers, MAX_SAFE_INTEGER};
use serde_json::{Map, Value};

/// SP8: a `trust/` record is at most 64 KB (prov.), a claim at most 4 KB.
pub const MAX_TRUST_FILE: usize = 64 << 10;
pub const MAX_CLAIM_FILE: usize = 4 << 10;

/// SP19: a label is what the reader confirmed for the device, at most 64
/// characters. PROVISIONAL: a longer one is refused, not cut (decisions.md, (3)).
pub const MAX_LABEL_CHARS: usize = 64;

pub fn trust_path(hash: &Hash32) -> String {
    format!("trust/{hash}.json")
}

pub fn claim_path(library: &Id16) -> String {
    format!("libraries/{library}.json")
}

/// The types `trust/` holds, and the claim's.
pub const TRUST_TYPES: [&str; 7] = [
    "genesis", "escrow", "enroll", "rotate", "remove", "confirm", "merge",
];

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    #[error("larger than SP8 allows")]
    TooLarge,
    #[error("not one of SP19's records")]
    Type,
    #[error("not SP19's shape for its type")]
    Shape,
    #[error("a record format this Linen doesn't know")]
    Format,
    #[error("named by another hash than its payload's")]
    Name,
    #[error("a key that isn't the one its device ID names")]
    Key,
    #[error("a signature this record needs is missing or wrong")]
    Signature,
    #[error("an enrollment's reveal doesn't name its enroller and applicant")]
    Reveal,
}

type Result<T> = std::result::Result<T, TrustError>;

/// SP19, SP22: the recovery code that seals a head: its 16-byte `nonce` (the
/// salt's) and the public half of its signing key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seal {
    pub nonce: [u8; 16],
    pub code_key: [u8; 32],
}

/// A change `{ s, h }` of one device's chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub s: u64,
    pub h: Hash32,
}

/// SP19: one device a removal removes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    pub device: DeviceId,
    pub cutoff: Option<Mark>,
    pub view: Option<Hash32>,
    pub cascade: bool,
}

/// SP19: the reader's answer, at a merge, for one device enrolled after the fork.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keep {
    pub device: DeviceId,
    pub keep: bool,
    pub cutoff: Option<Mark>,
    pub view: Option<Hash32>,
}

/// What a `confirm` confirms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirms {
    /// A removal made on a head another sealed head had overtaken (SP20 (4)).
    Removal(Hash32),
    /// Sibling enrollments the cap held back (SP20 (1)).
    Admits(Vec<Hash32>),
}

/// SP19's records, by type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Claim {
        device: DeviceId,
        writer: Id16,
        key: [u8; 32],
        genesis: Hash32,
    },
    Genesis {
        device: DeviceId,
        key: [u8; 32],
        label: String,
        clock: Hlc,
        code: Seal,
    },
    Escrow {
        head: Hash32,
        nonce: [u8; 24],
        ct: Vec<u8>,
    },
    Enroll {
        prev: Hash32,
        device: DeviceId,
        key: [u8; 32],
        label: String,
        clock: Hlc,
        /// The enrolling device, or `None` for the recovery code.
        by: Option<DeviceId>,
        /// For a device signer, how many devices it has enrolled, this one included.
        n: Option<u64>,
        sas: Option<Hash32>,
        /// For a device signer: the payload of the new device's `enroll-reveal`
        /// (SP24), exact bytes, and the new device's own signature on it. A code
        /// enrollment has neither: the new key signs the record itself.
        /// PROVISIONAL (decisions.md, (2)).
        reveal: Option<(Vec<u8>, [u8; 64])>,
    },
    Rotate {
        prev: Hash32,
        old: DeviceId,
        last: Option<Mark>,
        new: DeviceId,
        key: [u8; 32],
        clock: Hlc,
    },
    Remove {
        prev: Hash32,
        removes: Vec<Removal>,
        clock: Hlc,
        code: Seal,
    },
    Confirm {
        prev: Hash32,
        confirms: Confirms,
        code: Seal,
    },
    Merge {
        prevs: Vec<Hash32>,
        clock: Hlc,
        keeps: Vec<Keep>,
        code: Seal,
    },
}

/// A record opened and checked for what it alone can show.
#[derive(Clone, Debug)]
pub struct Record {
    pub hash: Hash32,
    pub body: Body,
    envelope: Envelope,
    library: Id16,
}

// Field readers: each refuses what isn't its form.

fn obj<'a>(b: &'a Map<String, Value>, k: &str) -> Result<&'a Map<String, Value>> {
    b.get(k).and_then(Value::as_object).ok_or(TrustError::Shape)
}

fn id(v: Option<&Value>) -> Result<Id16> {
    v.and_then(Value::as_str)
        .and_then(Id16::parse)
        .ok_or(TrustError::Shape)
}

fn hash(v: Option<&Value>) -> Result<Hash32> {
    v.and_then(Value::as_str)
        .and_then(Hash32::parse)
        .ok_or(TrustError::Shape)
}

fn opt_hash(v: Option<&Value>) -> Result<Option<Hash32>> {
    match v {
        Some(Value::Null) => Ok(None),
        other => hash(other).map(Some),
    }
}

fn bytes<const N: usize>(v: Option<&Value>) -> Result<[u8; N]> {
    v.and_then(Value::as_str)
        .and_then(b64url_decode)
        .and_then(|b| b.try_into().ok())
        .ok_or(TrustError::Shape)
}

fn label(v: Option<&Value>) -> Result<String> {
    v.and_then(Value::as_str)
        .filter(|l| l.chars().count() <= MAX_LABEL_CHARS)
        .map(str::to_owned)
        .ok_or(TrustError::Shape)
}

/// SP10: a clock reading `[ms, n]`, 48-bit milliseconds. PROVISIONAL (decisions.md,
/// (1)).
fn clock(v: Option<&Value>) -> Result<Hlc> {
    v.and_then(Value::as_array)
        .filter(|a| a.len() == 2)
        .and_then(|a| Some(Hlc(a[0].as_u64()?, u16::try_from(a[1].as_u64()?).ok()?)))
        .filter(|c| c.is_valid())
        .ok_or(TrustError::Shape)
}

fn seq(v: Option<&Value>) -> Result<u64> {
    v.and_then(Value::as_u64)
        .filter(|s| (1..=MAX_SAFE_INTEGER).contains(s))
        .ok_or(TrustError::Shape)
}

fn mark(v: Option<&Value>) -> Result<Option<Mark>> {
    match v {
        Some(Value::Null) => Ok(None),
        Some(Value::Object(o)) => Ok(Some(Mark {
            s: seq(o.get("s"))?,
            h: hash(o.get("h"))?,
        })),
        _ => Err(TrustError::Shape),
    }
}

fn seal(b: &Map<String, Value>) -> Result<Seal> {
    let c = obj(b, "code")?;
    Ok(Seal {
        nonce: bytes(c.get("nonce"))?,
        code_key: bytes(c.get("codeKey"))?,
    })
}

fn array<'a>(b: &'a Map<String, Value>, k: &str) -> Result<&'a Vec<Value>> {
    b.get(k).and_then(Value::as_array).ok_or(TrustError::Shape)
}

fn body(type_: &str, b: &Map<String, Value>) -> Result<Body> {
    Ok(match type_ {
        "claim" => Body::Claim {
            device: id(b.get("device"))?,
            writer: id(b.get("writer"))?,
            key: bytes(b.get("key"))?,
            genesis: hash(b.get("genesis"))?,
        },
        "genesis" => Body::Genesis {
            device: id(b.get("device"))?,
            key: bytes(b.get("key"))?,
            label: label(b.get("label"))?,
            clock: clock(b.get("clock"))?,
            code: seal(b)?,
        },
        "escrow" => Body::Escrow {
            head: hash(b.get("head"))?,
            nonce: bytes(b.get("nonce"))?,
            ct: b
                .get("ct")
                .and_then(Value::as_str)
                .and_then(b64url_decode)
                .ok_or(TrustError::Shape)?,
        },
        "enroll" => {
            let by = match b.get("by").and_then(Value::as_str) {
                Some("code") => None,
                Some(d) => Some(Id16::parse(d).ok_or(TrustError::Shape)?),
                None => return Err(TrustError::Shape),
            };
            // SP19: `n` for a device signer, absent for the code.
            let n = match (by, b.get("n")) {
                (Some(_), v) => Some(seq(v)?),
                (None, None) => None,
                (None, Some(_)) => return Err(TrustError::Shape),
            };
            Body::Enroll {
                prev: hash(b.get("prev"))?,
                device: id(b.get("device"))?,
                key: bytes(b.get("key"))?,
                label: label(b.get("label"))?,
                clock: clock(b.get("clock"))?,
                by,
                n,
                // SP24: a device enrollment binds the transcript its readers
                // confirmed (`sas` = `t`); a code enrollment has none.
                sas: match (by, opt_hash(b.get("sas"))?) {
                    (Some(_), Some(t)) => Some(t),
                    (None, None) => None,
                    _ => return Err(TrustError::Shape),
                },
                reveal: match (by, b.get("reveal"), b.get("pop")) {
                    (Some(_), r, pop) => Some((
                        r.and_then(Value::as_str)
                            .and_then(b64url_decode)
                            .ok_or(TrustError::Shape)?,
                        bytes(pop)?,
                    )),
                    (None, None, None) => None,
                    _ => return Err(TrustError::Shape),
                },
            }
        }
        "rotate" => Body::Rotate {
            prev: hash(b.get("prev"))?,
            old: id(b.get("old"))?,
            last: mark(b.get("last"))?,
            new: id(b.get("new"))?,
            key: bytes(b.get("key"))?,
            clock: clock(b.get("clock"))?,
        },
        "remove" => Body::Remove {
            prev: hash(b.get("prev"))?,
            removes: array(b, "removes")?
                .iter()
                .map(|r| {
                    let r = r.as_object().ok_or(TrustError::Shape)?;
                    Ok(Removal {
                        device: id(r.get("device"))?,
                        cutoff: mark(r.get("cutoff"))?,
                        view: opt_hash(r.get("view"))?,
                        cascade: r
                            .get("cascade")
                            .and_then(Value::as_bool)
                            .ok_or(TrustError::Shape)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            clock: clock(b.get("clock"))?,
            code: seal(b)?,
        },
        "confirm" => Body::Confirm {
            prev: hash(b.get("prev"))?,
            // One or the other, never both. PROVISIONAL (decisions.md, (4)).
            confirms: match (b.get("confirms"), b.get("admits")) {
                (Some(c), None) => Confirms::Removal(hash(Some(c))?),
                (None, Some(Value::Array(a))) if !a.is_empty() => Confirms::Admits(
                    a.iter()
                        .map(|h| hash(Some(h)))
                        .collect::<Result<Vec<_>>>()?,
                ),
                _ => return Err(TrustError::Shape),
            },
            code: seal(b)?,
        },
        "merge" => Body::Merge {
            prevs: {
                let p = array(b, "prevs")?
                    .iter()
                    .map(|h| hash(Some(h)))
                    .collect::<Result<Vec<_>>>()?;
                // SP19: two or more sibling heads, each once.
                let distinct: std::collections::BTreeSet<&Hash32> = p.iter().collect();
                if p.len() < 2 || distinct.len() != p.len() {
                    return Err(TrustError::Shape);
                }
                p
            },
            clock: clock(b.get("clock"))?,
            keeps: array(b, "keeps")?
                .iter()
                .map(|k| {
                    let k = k.as_object().ok_or(TrustError::Shape)?;
                    Ok(Keep {
                        device: id(k.get("device"))?,
                        keep: k
                            .get("keep")
                            .and_then(Value::as_bool)
                            .ok_or(TrustError::Shape)?,
                        cutoff: mark(k.get("cutoff"))?,
                        view: opt_hash(k.get("view"))?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            code: seal(b)?,
        },
        _ => return Err(TrustError::Type),
    })
}

/// Writes and signs a record of `type_` from its `fields` (SP19's, beyond `type`,
/// `format` and `library`), in canonical form, by each signer for its `seq`.
/// Returns the file's bytes and its name, the payload hash.
pub fn sign(
    type_: &str,
    library: &Id16,
    fields: Value,
    signers: &[(&super::crypto::Keypair, Signer, u64)],
) -> (Vec<u8>, Hash32) {
    let mut p = fields;
    p["type"] = Value::from(type_);
    p["format"] = Value::from(1);
    p["library"] = Value::from(library.to_string());
    let payload = json::canonical(&p);
    let sigs: Vec<envelope::Sig> = signers
        .iter()
        .map(|(key, by, seq)| {
            envelope::sign(
                key,
                *by,
                type_,
                library,
                &Place {
                    seq: *seq,
                    ..Place::default()
                },
                &payload,
            )
        })
        .collect();
    (
        envelope::write(type_, &payload, &sigs),
        super::crypto::payload_hash(&payload),
    )
}

/// The envelope's `type`, read before the envelope is opened for it.
fn peek_type(bytes: &[u8]) -> Result<String> {
    let v = json::parse(bytes, Numbers::Integers)
        .map_err(|e| TrustError::Envelope(EnvelopeError::Json(e)))?;
    v.get("type")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(TrustError::Type)
}

fn open_as(bytes: &[u8], library: &Id16, type_: &str) -> Result<Record> {
    let e = envelope::open(bytes, type_, library)?;
    if e.body.get("format").and_then(Value::as_u64) != Some(1) {
        return Err(if e.body.contains_key("format") {
            TrustError::Format
        } else {
            TrustError::Shape
        });
    }
    let record = Record {
        hash: e.hash,
        body: body(type_, &e.body)?,
        envelope: e,
        library: *library,
    };
    record.self_check()?;
    Ok(record)
}

/// SP19: opens `trust/‹name›.json`: one of SP19's records, for this library, whose
/// payload hash is its name, and which passes what it alone can show.
pub fn open(bytes: &[u8], library: &Id16, name: &Hash32) -> Result<Record> {
    if bytes.len() > MAX_TRUST_FILE {
        return Err(TrustError::TooLarge);
    }
    let type_ = peek_type(bytes)?;
    if !TRUST_TYPES.contains(&type_.as_str()) {
        return Err(TrustError::Type);
    }
    let r = open_as(bytes, library, &type_)?;
    if r.hash != *name {
        return Err(TrustError::Name);
    }
    Ok(r)
}

/// SP19: opens `libraries/‹library›.json`, a library's claim.
pub fn open_claim(bytes: &[u8], library: &Id16) -> Result<Record> {
    if bytes.len() > MAX_CLAIM_FILE {
        return Err(TrustError::TooLarge);
    }
    open_as(bytes, library, "claim")
}

impl Record {
    /// SP6: whether `by` signed this record for `seq`, with `key`. Membership
    /// records sign with no `prev` and no `vroot`.
    pub fn signed_by(&self, by: &Signer, key: &[u8; 32], seq: u64) -> bool {
        self.envelope.verified(
            by,
            key,
            &self.library,
            &Place {
                seq,
                ..Place::default()
            },
        )
    }

    /// The code key this record seals its head with, if it is a sealed head
    /// (SP19: genesis, remove, confirm, merge).
    pub fn seal(&self) -> Option<&Seal> {
        match &self.body {
            Body::Genesis { code, .. }
            | Body::Remove { code, .. }
            | Body::Confirm { code, .. }
            | Body::Merge { code, .. } => Some(code),
            _ => None,
        }
    }

    /// The heads this record builds on.
    pub fn prevs(&self) -> Vec<Hash32> {
        match &self.body {
            Body::Enroll { prev, .. }
            | Body::Rotate { prev, .. }
            | Body::Remove { prev, .. }
            | Body::Confirm { prev, .. } => vec![*prev],
            Body::Merge { prevs, .. } => prevs.clone(),
            _ => Vec::new(),
        }
    }

    /// What the record alone can show (SP19, SP20).
    fn self_check(&self) -> Result<()> {
        let sig = |by: Signer, key: &[u8; 32], seq: u64| {
            if self.signed_by(&by, key, seq) {
                Ok(())
            } else {
                Err(TrustError::Signature)
            }
        };
        match &self.body {
            // A record that introduces a key is refused unless its device ID is that
            // key's and the key signed it.
            Body::Claim { device, key, .. } => {
                if device_id(key) != *device {
                    return Err(TrustError::Key);
                }
                sig(Signer::Device(*device), key, 0)
            }
            Body::Genesis {
                device, key, code, ..
            } => {
                if device_id(key) != *device {
                    return Err(TrustError::Key);
                }
                sig(Signer::Device(*device), key, 0)?;
                // The first code's key seals the genesis.
                sig(Signer::Code, &code.code_key, 0)
            }
            Body::Escrow { .. } => Ok(()),
            Body::Enroll {
                device,
                key,
                by,
                reveal,
                ..
            } => {
                if device_id(key) != *device {
                    return Err(TrustError::Key);
                }
                match (by, reveal) {
                    // A device enrollment proves the key by `pop`.
                    (Some(enroller), Some((reveal, pop))) => {
                        self.check_reveal(*device, key, *enroller, reveal, pop)
                    }
                    // A code enrollment is signed by the new device's key (seq 0).
                    (None, None) => sig(Signer::Device(*device), key, 0),
                    _ => Err(TrustError::Shape),
                }
            }
            Body::Rotate { new, key, old, .. } => {
                // PROVISIONAL: no rotation to itself (decisions.md, (5)).
                if device_id(key) != *new || new == old {
                    return Err(TrustError::Key);
                }
                // The new key signs with seq 0.
                sig(Signer::Device(*new), key, 0)
            }
            // The next code's key signs the record that makes it the code.
            Body::Remove { code, .. } | Body::Confirm { code, .. } | Body::Merge { code, .. } => {
                sig(Signer::Code, &code.code_key, 0)
            }
        }
    }

    /// SP19, SP24: a device enrollment's `reveal` is the new device's
    /// `enroll-reveal` payload, for this library, naming the record's key as
    /// `applicant` and the enrolling device as `enroller`; `pop` is the new key's
    /// own signature on it, so a `pop` can't be reused by another enroller.
    fn check_reveal(
        &self,
        device: DeviceId,
        key: &[u8; 32],
        enroller: DeviceId,
        reveal: &[u8],
        pop: &[u8; 64],
    ) -> Result<()> {
        let r = json::parse(reveal, Numbers::Integers).map_err(|_| TrustError::Reveal)?;
        let names = |k: &str, want: &Id16| {
            r.get(k).and_then(Value::as_str).and_then(Id16::parse) == Some(*want)
        };
        if r.get("type").and_then(Value::as_str) != Some("enroll-reveal")
            || !names("library", &self.library)
            || !names("applicant", &device)
            || !names("enroller", &enroller)
        {
            return Err(TrustError::Reveal);
        }
        let message = envelope::message(
            "enroll-reveal",
            &self.library,
            &Signer::Device(device),
            &Place::default(),
            &super::crypto::payload_hash(reveal),
        );
        if super::crypto::verify(key, &message, pop) {
            Ok(())
        } else {
            Err(TrustError::Reveal)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::Keypair;
    use crate::sync::ids::b64url_encode;
    use serde_json::json;

    const LIB: Id16 = Id16([7; 16]);

    fn k(n: u8) -> Keypair {
        Keypair::from_seed(&[n; 32])
    }

    fn code_of(key: &Keypair) -> Value {
        json!({ "nonce": b64url_encode(&[5; 16]), "codeKey": b64url_encode(&key.public()) })
    }

    fn genesis(device: &Keypair, code: &Keypair) -> Value {
        json!({
            "device": device.device().to_string(),
            "key": b64url_encode(&device.public()),
            "label": "Quentin's MacBook",
            "clock": [1_760_000_000_000u64, 0],
            "code": code_of(code),
        })
    }

    fn open_signed(t: &str, fields: Value, signers: &[(&Keypair, Signer, u64)]) -> Result<Record> {
        let (bytes, name) = sign(t, &LIB, fields, signers);
        open(&bytes, &LIB, &name)
    }

    #[test]
    fn a_genesis_needs_its_device_and_its_first_codes_key() {
        let (d, c) = (k(1), k(2));
        let both = [(&d, Signer::Device(d.device()), 0), (&c, Signer::Code, 0)];
        let r = open_signed("genesis", genesis(&d, &c), &both).unwrap();
        assert_eq!(r.seal().unwrap().code_key, c.public());
        assert!(r.prevs().is_empty());
        // Without the code's signature, or the device's.
        assert!(matches!(
            open_signed("genesis", genesis(&d, &c), &both[..1]),
            Err(TrustError::Signature)
        ));
        assert!(matches!(
            open_signed("genesis", genesis(&d, &c), &both[1..]),
            Err(TrustError::Signature)
        ));
        // A key that isn't the one the device ID names.
        let mut wrong = genesis(&d, &c);
        wrong["device"] = json!(k(9).device().to_string());
        assert!(matches!(
            open_signed("genesis", wrong, &both),
            Err(TrustError::Key)
        ));
        // Named by another hash, or read for another library.
        let (bytes, _) = sign("genesis", &LIB, genesis(&d, &c), &both);
        assert!(matches!(
            open(&bytes, &LIB, &Hash32([0; 32])),
            Err(TrustError::Name)
        ));
        assert!(open(&bytes, &Id16([8; 16]), &Hash32([0; 32])).is_err());
    }

    /// A reveal by `applicant`, naming `enroller`, and the applicant's `pop` on it.
    fn reveal(applicant: &Keypair, enroller: &DeviceId) -> (Vec<u8>, [u8; 64]) {
        let payload = json::canonical(&json!({
            "type": "enroll-reveal", "library": LIB.to_string(),
            "applicant": applicant.device().to_string(), "enroller": enroller.to_string(),
            "request": "aa".repeat(32), "answer": "bb".repeat(32), "na": b64url_encode(&[3; 32]),
            "device": applicant.device().to_string(), "writer": Id16([4; 16]).to_string(),
        }));
        let pop = applicant.sign(&envelope::message(
            "enroll-reveal",
            &LIB,
            &Signer::Device(applicant.device()),
            &Place::default(),
            &crate::sync::crypto::payload_hash(&payload),
        ));
        (payload, pop)
    }

    fn enroll_by(
        enroller: &Keypair,
        new: &Keypair,
        n: u64,
        revealed: (Vec<u8>, [u8; 64]),
    ) -> Value {
        json!({
            "prev": "cc".repeat(32), "device": new.device().to_string(),
            "key": b64url_encode(&new.public()), "label": "iPad",
            "clock": [1_760_000_000_000u64, 1], "by": enroller.device().to_string(), "n": n,
            "sas": "dd".repeat(32), "reveal": b64url_encode(&revealed.0),
            "pop": b64url_encode(&revealed.1),
        })
    }

    #[test]
    fn a_device_enrollment_carries_the_new_keys_reveal_for_its_enroller() {
        let (e, new) = (k(1), k(3));
        let by = [(&e, Signer::Device(e.device()), 2)];
        let r = open_signed(
            "enroll",
            enroll_by(&e, &new, 2, reveal(&new, &e.device())),
            &by,
        )
        .unwrap();
        // The enroller's signature is for its `n`: membership checks it with its key.
        assert!(r.signed_by(&Signer::Device(e.device()), &e.public(), 2));
        assert!(!r.signed_by(&Signer::Device(e.device()), &e.public(), 3));
        // A reveal naming another enroller, so a pop can't be reused.
        assert!(matches!(
            open_signed(
                "enroll",
                enroll_by(&e, &new, 2, reveal(&new, &k(8).device())),
                &by
            ),
            Err(TrustError::Reveal)
        ));
        // A pop by another key than the one enrolled.
        let (payload, _) = reveal(&new, &e.device());
        let (_, other_pop) = reveal(&k(8), &e.device());
        assert!(matches!(
            open_signed("enroll", enroll_by(&e, &new, 2, (payload, other_pop)), &by),
            Err(TrustError::Reveal)
        ));
        // A device enrollment without its transcript (`sas`).
        let mut no_sas = enroll_by(&e, &new, 2, reveal(&new, &e.device()));
        no_sas["sas"] = json!(null);
        assert!(matches!(
            open_signed("enroll", no_sas, &by),
            Err(TrustError::Shape)
        ));
        // A device enrollment without its `n`.
        let mut no_n = enroll_by(&e, &new, 2, reveal(&new, &e.device()));
        no_n.as_object_mut().unwrap().remove("n");
        assert!(matches!(
            open_signed("enroll", no_n, &by),
            Err(TrustError::Shape)
        ));
    }

    #[test]
    fn a_code_enrollment_is_signed_by_the_new_key_and_has_no_n_or_reveal() {
        let (new, code) = (k(3), k(2));
        let fields = json!({
            "prev": "cc".repeat(32), "device": new.device().to_string(),
            "key": b64url_encode(&new.public()), "label": "iPad",
            "clock": [1_760_000_000_000u64, 1], "by": "code", "sas": null,
        });
        let signers = [
            (&code, Signer::Code, 0),
            (&new, Signer::Device(new.device()), 0),
        ];
        let r = open_signed("enroll", fields.clone(), &signers).unwrap();
        assert!(r.signed_by(&Signer::Code, &code.public(), 0));
        // Without the new key's own signature.
        assert!(matches!(
            open_signed("enroll", fields.clone(), &signers[..1]),
            Err(TrustError::Signature)
        ));
        // With an `n`, a reveal or a transcript: not a code enrollment's.
        let mut with_sas = fields.clone();
        with_sas["sas"] = json!("dd".repeat(32));
        assert!(matches!(
            open_signed("enroll", with_sas, &signers),
            Err(TrustError::Shape)
        ));
        let mut with_n = fields.clone();
        with_n["n"] = json!(1);
        assert!(matches!(
            open_signed("enroll", with_n, &signers),
            Err(TrustError::Shape)
        ));
        let mut with_reveal = fields;
        with_reveal["reveal"] = json!(b64url_encode(b"{}"));
        assert!(matches!(
            open_signed("enroll", with_reveal, &signers),
            Err(TrustError::Shape)
        ));
    }

    #[test]
    fn a_rotation_is_signed_by_its_new_key_and_its_old_one_for_its_place() {
        let (old, new) = (k(1), k(4));
        let fields = json!({
            "prev": "cc".repeat(32), "old": old.device().to_string(),
            "last": { "s": 7, "h": "ee".repeat(32) }, "new": new.device().to_string(),
            "key": b64url_encode(&new.public()), "clock": [1_760_000_000_000u64, 2],
        });
        let signers = [
            (&old, Signer::Device(old.device()), 8),
            (&new, Signer::Device(new.device()), 0),
        ];
        let r = open_signed("rotate", fields.clone(), &signers).unwrap();
        // SP19: the old key signs with `last.s + 1`.
        assert!(r.signed_by(&Signer::Device(old.device()), &old.public(), 8));
        assert!(!r.signed_by(&Signer::Device(old.device()), &old.public(), 7));
        assert!(matches!(
            open_signed("rotate", fields, &signers[..1]),
            Err(TrustError::Signature)
        ));
    }

    #[test]
    fn a_removal_confirm_and_merge_carry_the_next_codes_signature() {
        let (d, current, next) = (k(1), k(2), k(5));
        let removal = json!({
            "prev": "cc".repeat(32),
            "removes": [{ "device": k(3).device().to_string(), "cutoff": { "s": 4, "h": "ee".repeat(32) }, "view": null, "cascade": true }],
            "clock": [1_760_000_000_000u64, 3], "code": code_of(&next),
        });
        let all = [
            (&d, Signer::Device(d.device()), 0),
            (&current, Signer::Code, 0),
            (&next, Signer::Code, 0),
        ];
        let r = open_signed("remove", removal.clone(), &all).unwrap();
        let Body::Remove { removes, .. } = &r.body else {
            unreachable!()
        };
        assert_eq!(removes[0].cutoff.unwrap().s, 4);
        assert!(r.signed_by(&Signer::Code, &current.public(), 0));
        assert!(r.signed_by(&Signer::Device(d.device()), &d.public(), 0));
        // Without the next code's signature: refused.
        assert!(matches!(
            open_signed("remove", removal, &all[..2]),
            Err(TrustError::Signature)
        ));
        // A confirm names a removal or admits enrollments, never both or neither.
        let confirm = |extra: Value| {
            let mut f = json!({ "prev": "cc".repeat(32), "code": code_of(&next) });
            for (k, v) in extra.as_object().unwrap() {
                f[k] = v.clone();
            }
            open_signed("confirm", f, &all)
        };
        assert!(confirm(json!({ "confirms": "ab".repeat(32) })).is_ok());
        assert!(confirm(json!({ "admits": ["ab".repeat(32)] })).is_ok());
        assert!(matches!(
            confirm(json!({ "confirms": "ab".repeat(32), "admits": ["ab".repeat(32)] })),
            Err(TrustError::Shape)
        ));
        assert!(matches!(confirm(json!({})), Err(TrustError::Shape)));
        // A merge of fewer than two heads isn't one.
        let merge = |prevs: Vec<String>| {
            open_signed(
                "merge",
                json!({ "prevs": prevs, "clock": [1_760_000_000_000u64, 4], "keeps": [], "code": code_of(&next) }),
                &all,
            )
        };
        assert_eq!(
            merge(vec!["aa".repeat(32), "bb".repeat(32)])
                .unwrap()
                .prevs()
                .len(),
            2
        );
        assert!(matches!(
            merge(vec!["aa".repeat(32)]),
            Err(TrustError::Shape)
        ));
        // Nor is one naming the same head twice.
        assert!(matches!(
            merge(vec!["aa".repeat(32), "aa".repeat(32)]),
            Err(TrustError::Shape)
        ));
    }

    #[test]
    fn a_claim_and_records_of_other_kinds_or_formats() {
        let d = k(1);
        let claim = json!({
            "device": d.device().to_string(), "writer": Id16([4; 16]).to_string(),
            "key": b64url_encode(&d.public()), "genesis": "ab".repeat(32),
        });
        let (bytes, _) = sign("claim", &LIB, claim, &[(&d, Signer::Device(d.device()), 0)]);
        assert!(open_claim(&bytes, &LIB).is_ok());
        // A claim isn't a `trust/` record.
        assert!(matches!(
            open(&bytes, &LIB, &Hash32([0; 32])),
            Err(TrustError::Type)
        ));
        // A device file isn't one either; nor is a record of a later format.
        let (other, name) = sign("device", &LIB, json!({}), &[]);
        assert!(matches!(open(&other, &LIB, &name), Err(TrustError::Type)));
        let mut later = genesis(&d, &k(2));
        later["format"] = json!(2);
        let signers = [
            (&d, Signer::Device(d.device()), 0),
            (&k(2), Signer::Code, 0),
        ];
        let p = {
            let mut p = later.clone();
            p["type"] = json!("genesis");
            p["library"] = json!(LIB.to_string());
            json::canonical(&p)
        };
        let sigs: Vec<envelope::Sig> = signers
            .iter()
            .map(|(key, by, seq)| {
                envelope::sign(
                    key,
                    *by,
                    "genesis",
                    &LIB,
                    &Place {
                        seq: *seq,
                        ..Place::default()
                    },
                    &p,
                )
            })
            .collect();
        let bytes = envelope::write("genesis", &p, &sigs);
        assert!(matches!(
            open(&bytes, &LIB, &crate::sync::crypto::payload_hash(&p)),
            Err(TrustError::Format)
        ));
        // Larger than SP8 allows: refused before it is read.
        assert!(matches!(
            open(&vec![b' '; MAX_TRUST_FILE + 1], &LIB, &name),
            Err(TrustError::TooLarge)
        ));
        assert!(matches!(
            open_claim(&vec![b' '; MAX_CLAIM_FILE + 1], &LIB),
            Err(TrustError::TooLarge)
        ));
    }
}
