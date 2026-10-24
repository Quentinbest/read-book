//! SP24: enrollment by a short code.
//!
//! The applicant commits first (request), an enrolled device answers before the
//! reveal, and the reveal pins the answer's bytes, so neither side can choose the
//! code after seeing the other's input. Each side keeps the exact bytes it used and
//! works only from them; both compute the transcript `t` from those bytes and show
//! the same six digits only when nobody swapped or answered a file in between.
//!
//! Each file is a single writer's signed envelope: its payload names SP5's `device`
//! (the signer) and that device's `writer` token, and is signed with sequence 0,
//! outside the signer's chain. PROVISIONAL (decisions.md, “Enrollment by a short
//! code”).

use super::clock::Hlc;
use super::crypto::{self, CryptoError, Keypair, Transcript};
use super::device_store;
use super::envelope::{self, EnvelopeError, Place, Signer};
use super::ids::{b64url_decode, b64url_encode, device_id, DeviceId, Hash32, Id16};
use super::membership::Membership;
use super::trust::{self, Body, Mark, Record};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// SP24 (SY34): a request, answer or reveal is at most 16 KB.
pub const MAX_FILE: usize = 16 * 1024;
/// The most values one of these files' payloads may build (SP8): a request has a
/// dozen. PROVISIONAL (decisions.md, “Enrollment by a short code”, (3)).
pub const MAX_VALUES: u64 = 64;
/// An `enroll-ok` is at most 64 KB and its payload at most 1,024 values: its
/// checkpoint names a floor for each of up to 11 other devices. PROVISIONAL
/// (decisions.md, “Enrollment by a short code”, (3)).
pub const MAX_OK_FILE: usize = 64 * 1024;
pub const MAX_OK_VALUES: u64 = 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnrollError {
    #[error("an enrollment file past 16 KB")]
    TooLarge,
    #[error("{0}")]
    Envelope(#[from] EnvelopeError),
    #[error("an enrollment file that isn't SP24's shape")]
    Shape,
    #[error("a key its device ID doesn't name, or a file its own device didn't sign")]
    Signature,
    #[error("a file naming another request, answer, applicant or enroller")]
    Mismatch,
    #[error("a reveal whose `na` doesn't open the request's commitment")]
    Commit,
    #[error("an enrollment the membership doesn't trust, or that isn't this device's")]
    NotTrusted,
    #[error("a naming key that doesn't open with this device's `kx`")]
    NamingKey,
    #[error("{0}")]
    Crypto(#[from] CryptoError),
}

pub type Result<T> = std::result::Result<T, EnrollError>;

/// SP24: where each step's file goes in the folder.
pub fn request_path(applicant: &DeviceId) -> String {
    format!("enroll/{applicant}.json")
}
pub fn answer_path(applicant: &DeviceId, device: &DeviceId) -> String {
    format!("enroll/{applicant}.{device}.json")
}
pub fn reveal_path(applicant: &DeviceId) -> String {
    format!("enroll/{applicant}.reveal.json")
}
pub fn ok_path(applicant: &DeviceId, device: &DeviceId) -> String {
    format!("enroll/{applicant}.{device}.ok.json")
}

/// Signs a single writer's file with sequence 0 (`trust::sign` adds `type`,
/// `format` and `library`).
fn write(type_: &str, library: &Id16, mut fields: Value, key: &Keypair, writer: &Id16) -> Vec<u8> {
    fields["device"] = Value::from(key.device().to_string());
    fields["writer"] = Value::from(writer.to_string());
    trust::sign(
        type_,
        library,
        fields,
        &[(key, Signer::Device(key.device()), 0)],
    )
    .0
}

/// Opens a single writer's file, checked for its size, type, library and format,
/// and for its signer: the `device` it names, with `key`.
fn open(bytes: &[u8], type_: &str, library: &Id16, key: &[u8; 32]) -> Result<envelope::Envelope> {
    let (max, values) = if type_ == "enroll-ok" {
        (MAX_OK_FILE, MAX_OK_VALUES)
    } else {
        (MAX_FILE, MAX_VALUES)
    };
    if bytes.len() > max {
        return Err(EnrollError::TooLarge);
    }
    let e = envelope::open_within(bytes, type_, library, values)?;
    if e.body.get("format").and_then(Value::as_u64) != Some(1)
        || e.body
            .get("writer")
            .and_then(Value::as_str)
            .and_then(Id16::parse)
            .is_none()
    {
        return Err(EnrollError::Shape);
    }
    let device = id(&e.body, "device")?;
    // A single writer's file carries its writer's signature and nothing else, so
    // the one signature is the one checked (and a reveal's is the `pop`).
    if e.sigs.len() != 1 || !e.verified(&Signer::Device(device), key, library, &Place::default()) {
        return Err(EnrollError::Signature);
    }
    Ok(e)
}

fn id(b: &Map<String, Value>, k: &str) -> Result<DeviceId> {
    b.get(k)
        .and_then(Value::as_str)
        .and_then(Id16::parse)
        .ok_or(EnrollError::Shape)
}

fn hash(b: &Map<String, Value>, k: &str) -> Result<Hash32> {
    b.get(k)
        .and_then(Value::as_str)
        .and_then(Hash32::parse)
        .ok_or(EnrollError::Shape)
}

fn bytes32(b: &Map<String, Value>, k: &str) -> Result<[u8; 32]> {
    b.get(k)
        .and_then(Value::as_str)
        .and_then(b64url_decode)
        .and_then(|v| v.try_into().ok())
        .ok_or(EnrollError::Shape)
}

/// SP24 step 1, read by an enrolled device.
#[derive(Clone, Debug)]
pub struct Request {
    pub applicant: DeviceId,
    pub key: [u8; 32],
    pub kx: [u8; 32],
    pub name: String,
    pub commit: Hash32,
    /// Its payload hash, which the answer and reveal name.
    pub hash: Hash32,
}

/// What the applicant keeps from its own request: its new key, the secret behind
/// `kx`, and `na`, revealed only after an answer.
pub struct Applicant {
    pub key: Keypair,
    pub kx_secret: [u8; 32],
    pub na: [u8; 32],
    pub request: Request,
}

/// SP24 step 1: a new key, a fresh X25519 key and `na`, and the request committing
/// to them. The request is signed by the new key, which names the applicant.
pub fn request(library: &Id16, writer: &Id16, name: &str) -> Result<(Applicant, Vec<u8>)> {
    // The name becomes the enrollment's label once the reader confirms it: cut to
    // SP19's 64 characters, so what the reader sees is what is enrolled.
    let name: String = name.chars().take(trust::MAX_LABEL_CHARS).collect();
    let key = Keypair::generate()?;
    let kx_secret: [u8; 32] = crypto::random()?;
    let na: [u8; 32] = crypto::random()?;
    let applicant = key.device();
    let kx = crypto::x25519_public(&kx_secret);
    let commit = crypto::commit(library, &applicant, &key.public(), &kx, &na);
    let file = write(
        "enroll-request",
        library,
        json!({
            "applicant": applicant.to_string(),
            "key": b64url_encode(&key.public()),
            "kx": b64url_encode(&kx),
            "name": name,
            "commit": commit.to_string(),
        }),
        &key,
        writer,
    );
    let request = open_request(&file, library)?;
    Ok((
        Applicant {
            key,
            kx_secret,
            na,
            request,
        },
        file,
    ))
}

/// SP24 step 1, opened: signed by the key it names, whose ID is its applicant.
pub fn open_request(bytes: &[u8], library: &Id16) -> Result<Request> {
    // The key it is checked with is the one it names, read first.
    if bytes.len() > MAX_FILE {
        return Err(EnrollError::TooLarge);
    }
    let e = envelope::open_within(bytes, "enroll-request", library, MAX_VALUES)?;
    let key = bytes32(&e.body, "key")?;
    let e = open(bytes, "enroll-request", library, &key)?;
    let b = &e.body;
    let applicant = id(b, "applicant")?;
    if applicant != id(b, "device")? || device_id(&key) != applicant {
        return Err(EnrollError::Signature);
    }
    Ok(Request {
        applicant,
        key,
        kx: bytes32(b, "kx")?,
        name: b
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| n.chars().count() <= trust::MAX_LABEL_CHARS)
            .ok_or(EnrollError::Shape)?
            .to_string(),
        commit: hash(b, "commit")?,
        hash: e.hash,
    })
}

/// SP24 step 2, read by the applicant.
#[derive(Clone, Debug)]
pub struct Answer {
    pub applicant: DeviceId,
    pub request: Hash32,
    pub device: DeviceId,
    pub ne: [u8; 32],
    pub hash: Hash32,
}

/// What the enrolled device keeps from its answer: the request it answered and its
/// answer.
pub struct Answering {
    pub request: Request,
    pub answer: Answer,
}

/// SP24 step 2: an enrolled device answers a request it kept, with a fresh `ne`.
pub fn answer(
    library: &Id16,
    request: &Request,
    device: &Keypair,
    writer: &Id16,
) -> Result<(Answering, Vec<u8>)> {
    let ne: [u8; 32] = crypto::random()?;
    let file = write(
        "enroll-answer",
        library,
        json!({
            "applicant": request.applicant.to_string(),
            "request": request.hash.to_string(),
            "ne": b64url_encode(&ne),
        }),
        device,
        writer,
    );
    let answer = open_answer(&file, library, &device.public())?;
    Ok((
        Answering {
            request: request.clone(),
            answer,
        },
        file,
    ))
}

/// SP24 step 2, opened with the answering device's key (from the membership).
pub fn open_answer(bytes: &[u8], library: &Id16, device_key: &[u8; 32]) -> Result<Answer> {
    let e = open(bytes, "enroll-answer", library, device_key)?;
    let b = &e.body;
    Ok(Answer {
        applicant: id(b, "applicant")?,
        request: hash(b, "request")?,
        device: id(b, "device")?,
        ne: bytes32(b, "ne")?,
        hash: e.hash,
    })
}

/// SP24 step 3, read by the enrolled device: what the `enroll` record carries is
/// its exact payload and the applicant's signature on it (`pop`).
#[derive(Clone, Debug)]
pub struct Reveal {
    pub applicant: DeviceId,
    pub request: Hash32,
    pub answer: Hash32,
    pub enroller: DeviceId,
    pub na: [u8; 32],
    pub payload: Vec<u8>,
    pub pop: [u8; 64],
}

/// SP24 step 3: the applicant, having read one answer to its own request, reveals
/// `na` once, pinning that answer's bytes.
pub fn reveal(
    library: &Id16,
    applicant: &Applicant,
    answer: &Answer,
    writer: &Id16,
) -> Result<Vec<u8>> {
    if answer.applicant != applicant.request.applicant || answer.request != applicant.request.hash {
        return Err(EnrollError::Mismatch);
    }
    Ok(write(
        "enroll-reveal",
        library,
        json!({
            "applicant": applicant.request.applicant.to_string(),
            "request": applicant.request.hash.to_string(),
            "answer": answer.hash.to_string(),
            "enroller": answer.device.to_string(),
            "na": b64url_encode(&applicant.na),
        }),
        &applicant.key,
        writer,
    ))
}

/// SP24 steps 3–4, on the enrolled device: a reveal naming its own answer to the
/// request it kept, by that request's applicant, whose `na` opens the request's
/// commitment.
pub fn open_reveal(bytes: &[u8], library: &Id16, answering: &Answering) -> Result<Reveal> {
    let request = &answering.request;
    let e = open(bytes, "enroll-reveal", library, &request.key)?;
    let b = &e.body;
    let r = Reveal {
        applicant: id(b, "applicant")?,
        request: hash(b, "request")?,
        answer: hash(b, "answer")?,
        enroller: id(b, "enroller")?,
        na: bytes32(b, "na")?,
        payload: e.payload.clone(),
        pop: e.sigs.first().map(|s| s.sig).ok_or(EnrollError::Shape)?,
    };
    if r.applicant != request.applicant
        || r.request != request.hash
        || r.answer != answering.answer.hash
        || r.enroller != answering.answer.device
    {
        return Err(EnrollError::Mismatch);
    }
    if crypto::commit(
        library,
        &request.applicant,
        &request.key,
        &request.kx,
        &r.na,
    ) != request.commit
    {
        return Err(EnrollError::Commit);
    }
    Ok(r)
}

/// SP24 step 4: the transcript `t` and the six-digit code, from the bytes both
/// sides kept.
fn transcript(
    library: &Id16,
    request: &Request,
    answer: &Answer,
    na: &[u8; 32],
    device_key: &[u8; 32],
) -> (Hash32, u32) {
    let t = Transcript {
        library,
        request: &request.hash,
        answer: &answer.hash,
        applicant: &request.applicant,
        key: &request.key,
        kx: &request.kx,
        na,
        device: &answer.device,
        device_key,
        ne: &answer.ne,
    }
    .hash();
    (t, crypto::sas(&t))
}

/// SP24 step 4 on the enrolled device, from its kept request and answer and the
/// reveal it read.
pub fn code_enrolled(
    library: &Id16,
    answering: &Answering,
    reveal: &Reveal,
    device_key: &[u8; 32],
) -> (Hash32, u32) {
    transcript(
        library,
        &answering.request,
        &answering.answer,
        &reveal.na,
        device_key,
    )
}

/// SP24 step 4 on the applicant, from its kept request and `na` and the answer it
/// revealed to.
pub fn code_applicant(
    library: &Id16,
    applicant: &Applicant,
    answer: &Answer,
    device_key: &[u8; 32],
) -> (Hash32, u32) {
    transcript(
        library,
        &applicant.request,
        answer,
        &applicant.na,
        device_key,
    )
}

/// SP24 step 6, after the reader's Allow: the `enroll` record on `prev`, signed by
/// the enrolling device with `seq` = `n`, carrying `sas` = `t` and the reveal's
/// payload and `pop`. The label is the request's name, as the reader confirmed it.
#[allow(clippy::too_many_arguments)]
pub fn enroll(
    library: &Id16,
    prev: &Hash32,
    answering: &Answering,
    reveal: &Reveal,
    t: &Hash32,
    enroller: &Keypair,
    n: u64,
    clock: Hlc,
) -> (Vec<u8>, Hash32) {
    let request = &answering.request;
    trust::sign(
        "enroll",
        library,
        json!({
            "prev": prev.to_string(),
            "device": request.applicant.to_string(),
            "key": b64url_encode(&request.key),
            "label": request.name,
            "clock": [clock.ms(), clock.n()],
            "by": enroller.device().to_string(),
            "n": n,
            "sas": t.to_string(),
            "reveal": b64url_encode(&reveal.payload),
            "pop": b64url_encode(&reveal.pop),
        }),
        &[(enroller, Signer::Device(enroller.device()), n)],
    )
}

fn bytes64(v: Option<&Value>) -> Result<[u8; 64]> {
    v.and_then(Value::as_str)
        .and_then(b64url_decode)
        .and_then(|v| v.try_into().ok())
        .ok_or(EnrollError::Shape)
}

fn hash_v(v: Option<&Value>) -> Result<Hash32> {
    v.and_then(Value::as_str)
        .and_then(Hash32::parse)
        .ok_or(EnrollError::Shape)
}

fn u64_v(v: Option<&Value>) -> Result<u64> {
    v.and_then(Value::as_u64).ok_or(EnrollError::Shape)
}

/// SP24 step 6: a change of the enrolling device's own chain, signed, with the
/// `prev` and `vroot` its signature covers (SP6), so the new device checks it
/// without reading the change. PROVISIONAL (decisions.md, “Enrollment by a short
/// code”, (5)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedChange {
    pub s: u64,
    pub h: Hash32,
    pub prev: Option<Hash32>,
    pub vroot: Hash32,
    pub sig: [u8; 64],
}

/// SP24 step 6: the latest device file the enrolling device accepted from another
/// device, named with that device's own signature on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedFile {
    pub device: DeviceId,
    pub counter: u64,
    pub hash: Hash32,
    pub sig: [u8; 64],
}

/// SP24 step 6: what the new device must never go below. The accepted views SP24
/// also names are SP17's, and join it when views are built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    /// The enrolling device's membership head, and the removals it accepted.
    pub head: Hash32,
    pub removals: Vec<Hash32>,
    /// Its own latest change, or none before its first.
    pub own: Option<SignedChange>,
    /// Its newest view's index hash and generation, or none before its first.
    pub view: Option<(Hash32, u64)>,
    /// Its device-file counter.
    pub counter: u64,
    /// Every other device's floor.
    pub floors: Vec<SignedFile>,
    /// SP29: its frontier, the latest change it applied of each other device, with
    /// that device's signature and the `prev` and `vroot` it covers, so the new
    /// device checks each without reading the change (decisions.md, (5)).
    pub frontier: Vec<(DeviceId, SignedChange)>,
    /// SP29: its accepted-by-all marks, which the new device takes only as floors,
    /// never as accepted by all: it finds those itself from signed device files.
    pub marks: Vec<(DeviceId, Mark)>,
}

fn change_json(c: &SignedChange) -> Value {
    json!({
        "s": c.s, "h": c.h.to_string(), "prev": c.prev.map(|p| p.to_string()),
        "vroot": c.vroot.to_string(), "sig": b64url_encode(&c.sig),
    })
}

fn change_from(c: &Value) -> Result<SignedChange> {
    Ok(SignedChange {
        s: u64_v(c.get("s"))?,
        h: hash_v(c.get("h"))?,
        prev: match c.get("prev") {
            Some(Value::Null) => None,
            p => Some(hash_v(p)?),
        },
        vroot: hash_v(c.get("vroot"))?,
        sig: bytes64(c.get("sig"))?,
    })
}

fn device_v(v: Option<&Value>) -> Result<DeviceId> {
    v.and_then(Value::as_str)
        .and_then(Id16::parse)
        .ok_or(EnrollError::Shape)
}

impl Checkpoint {
    fn to_json(&self) -> Value {
        json!({
            "head": self.head.to_string(),
            "removals": self.removals.iter().map(|h| h.to_string()).collect::<Vec<_>>(),
            "own": self.own.as_ref().map(change_json),
            "view": self.view.map(|(h, gen)| json!({ "h": h.to_string(), "gen": gen })),
            "counter": self.counter,
            "floors": self.floors.iter().map(|f| json!({
                "device": f.device.to_string(), "counter": f.counter,
                "hash": f.hash.to_string(), "sig": b64url_encode(&f.sig),
            })).collect::<Vec<_>>(),
            "frontier": self.frontier.iter().map(|(d, c)| {
                let mut e = change_json(c);
                e["d"] = Value::from(d.to_string());
                e
            }).collect::<Vec<_>>(),
            "marks": self.marks.iter()
                .map(|(d, m)| json!({ "d": d.to_string(), "s": m.s, "h": m.h.to_string() }))
                .collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> Result<Self> {
        let o = v.as_object().ok_or(EnrollError::Shape)?;
        let list = |k: &str| o.get(k).and_then(Value::as_array).ok_or(EnrollError::Shape);
        let own = match o.get("own") {
            Some(Value::Null) => None,
            Some(c) => Some(change_from(c)?),
            None => return Err(EnrollError::Shape),
        };
        let view = match o.get("view") {
            Some(Value::Null) => None,
            Some(w) => Some((hash_v(w.get("h"))?, u64_v(w.get("gen"))?)),
            None => return Err(EnrollError::Shape),
        };
        Ok(Checkpoint {
            head: hash_v(o.get("head"))?,
            removals: list("removals")?
                .iter()
                .map(|h| hash_v(Some(h)))
                .collect::<Result<_>>()?,
            own,
            view,
            counter: u64_v(o.get("counter"))?,
            floors: list("floors")?
                .iter()
                .map(|f| {
                    Ok(SignedFile {
                        device: f
                            .get("device")
                            .and_then(Value::as_str)
                            .and_then(Id16::parse)
                            .ok_or(EnrollError::Shape)?,
                        counter: u64_v(f.get("counter"))?,
                        hash: hash_v(f.get("hash"))?,
                        sig: bytes64(f.get("sig"))?,
                    })
                })
                .collect::<Result<_>>()?,
            frontier: list("frontier")?
                .iter()
                .map(|e| Ok((device_v(e.get("d"))?, change_from(e)?)))
                .collect::<Result<_>>()?,
            marks: list("marks")?
                .iter()
                .map(|m| {
                    Ok((
                        device_v(m.get("d"))?,
                        Mark {
                            s: u64_v(m.get("s"))?,
                            h: hash_v(m.get("h"))?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
        })
    }
}

/// SP24 step 6: the `enroll-ok`, written after the `enroll` record: it names the
/// transcript, the enrollment and the checkpoint, and carries the naming key
/// sealed to the applicant's `kx`.
#[allow(clippy::too_many_arguments)]
pub fn enroll_ok(
    library: &Id16,
    answering: &Answering,
    t: &Hash32,
    enrollment: &Hash32,
    checkpoint: &Checkpoint,
    naming_key: &[u8; 32],
    device: &Keypair,
    writer: &Id16,
) -> Result<Vec<u8>> {
    // A `kx` of small order would make the shared secret zero.
    let sealed =
        crypto::seal_naming_key(&answering.request.kx, t, naming_key)?.ok_or(EnrollError::Shape)?;
    Ok(write(
        "enroll-ok",
        library,
        json!({
            "applicant": answering.request.applicant.to_string(),
            "t": t.to_string(),
            "enrollment": enrollment.to_string(),
            "checkpoint": checkpoint.to_json(),
            "namingKey": {
                "epk": b64url_encode(&sealed.epk),
                "nonce": b64url_encode(&sealed.nonce),
                "ct": b64url_encode(&sealed.ct),
            },
        }),
        device,
        writer,
    ))
}

/// SP24 step 7: what the applicant takes from the `enroll-ok` it accepted.
#[derive(Debug)]
pub struct Joined {
    /// The enrolling device.
    pub enroller: DeviceId,
    pub enrollment: Hash32,
    pub naming_key: [u8; 32],
    /// The checkpoint, less any change or floor whose signature doesn't hold:
    /// those are ignored (SY34).
    pub checkpoint: Checkpoint,
}

impl Joined {
    /// SP18, SP28: the checkpoint as floors, installed before any library data is
    /// read. The enrolling device's floor is its own change and view generation;
    /// every device's counter is the one before the device file the checkpoint
    /// names, since a floor's counter is the last file accepted (`device_file::
    /// accept`), and that file is still to be read here: it is accepted, and
    /// anything older refused (SP18). Floors are never what it has applied.
    /// A device's frontier entry and accepted-by-all mark are changes its chain must
    /// pass with those hashes: the highest is the floor's change, the others its
    /// earlier checkpoints (SP18).
    pub fn floors(&self) -> BTreeMap<String, device_store::Floor> {
        let c = &self.checkpoint;
        let mut out: BTreeMap<String, device_store::Floor> = c
            .floors
            .iter()
            .map(|f| {
                (
                    f.device.to_string(),
                    device_store::Floor {
                        counter: f.counter.saturating_sub(1),
                        ..Default::default()
                    },
                )
            })
            .collect();
        out.insert(
            self.enroller.to_string(),
            device_store::Floor {
                s: c.own.as_ref().map_or(0, |o| o.s),
                h: c.own.as_ref().map(|o| o.h.to_string()),
                gen: c.view.map_or(0, |(_, gen)| gen),
                counter: c.counter.saturating_sub(1),
                earlier: Vec::new(),
            },
        );
        let changes = c
            .frontier
            .iter()
            .map(|(d, ch)| (*d, ch.s, ch.h))
            .chain(c.marks.iter().map(|(d, m)| (*d, m.s, m.h)));
        for (d, s, h) in changes {
            if d == self.enroller {
                continue;
            }
            let f = out.entry(d.to_string()).or_default();
            let mut all: Vec<(u64, String)> = f
                .earlier
                .iter()
                .map(|e| (e.s, e.h.clone()))
                .chain(f.h.clone().map(|fh| (f.s, fh)))
                .chain(std::iter::once((s, h.to_string())))
                .collect();
            all.sort();
            all.dedup();
            let (top_s, top_h) = all.pop().expect("one was just added");
            f.s = top_s;
            f.h = Some(top_h);
            f.earlier = all
                .into_iter()
                .filter(|(es, _)| *es < top_s)
                .map(|(s, h)| device_store::Checkpoint { s, h })
                .collect();
        }
        out
    }
}

/// SP24 step 7, once the applicant's reader has said It matches: an `enroll-ok`
/// that names this device's `t`, signed by the key in `t` (the device it revealed
/// to), whose enrollment names this device's key in a membership where that key is
/// trusted. The naming key must open; a change or floor in the checkpoint whose
/// signature doesn't hold is dropped.
pub fn join(
    bytes: &[u8],
    library: &Id16,
    applicant: &Applicant,
    answer: &Answer,
    device_key: &[u8; 32],
    membership: &Membership,
    records: &BTreeMap<Hash32, Record>,
) -> Result<Joined> {
    let e = open(bytes, "enroll-ok", library, device_key)?;
    let b = &e.body;
    let (t, _) = code_applicant(library, applicant, answer, device_key);
    let me = applicant.request.applicant;
    if id(b, "device")? != answer.device || id(b, "applicant")? != me || hash(b, "t")? != t {
        return Err(EnrollError::Mismatch);
    }
    let enrollment = hash(b, "enrollment")?;
    let names_me = records.get(&enrollment).is_some_and(|r| {
        matches!(&r.body, Body::Enroll { device, key, by: Some(by), sas: Some(s), .. }
            if *device == me && *key == applicant.request.key && *by == answer.device && *s == t)
    });
    if !names_me || membership.trusted(&me).is_none() {
        return Err(EnrollError::NotTrusted);
    }
    let k = b
        .get("namingKey")
        .and_then(Value::as_object)
        .ok_or(EnrollError::Shape)?;
    let sealed = crypto::SealedKey {
        epk: bytes32(k, "epk")?,
        nonce: k
            .get("nonce")
            .and_then(Value::as_str)
            .and_then(b64url_decode)
            .and_then(|v| v.try_into().ok())
            .ok_or(EnrollError::Shape)?,
        ct: k
            .get("ct")
            .and_then(Value::as_str)
            .and_then(b64url_decode)
            .ok_or(EnrollError::Shape)?,
    };
    let naming_key =
        crypto::open_naming_key(&applicant.kx_secret, &t, &sealed).ok_or(EnrollError::NamingKey)?;
    let mut checkpoint = Checkpoint::from_json(b.get("checkpoint").ok_or(EnrollError::Shape)?)?;
    let signed = |key: &[u8; 32],
                  type_: &str,
                  signer: DeviceId,
                  place: &Place,
                  h: &Hash32,
                  sig: &[u8; 64]| {
        device_id(key) == signer
            && crypto::verify(
                key,
                &envelope::message(type_, library, &Signer::Device(signer), place, h),
                sig,
            )
    };
    // SY34: what doesn't verify is ignored, never installed.
    checkpoint.own = checkpoint.own.filter(|c| {
        let place = Place {
            seq: c.s,
            prev: c.prev,
            vroot: Some(c.vroot),
        };
        signed(device_key, "change", answer.device, &place, &c.h, &c.sig)
    });
    checkpoint.floors.retain(|f| {
        f.device != answer.device
            && f.device != me
            && membership.key(&f.device).is_some_and(|key| {
                let place = Place {
                    seq: f.counter,
                    ..Place::default()
                };
                signed(&key, "device", f.device, &place, &f.hash, &f.sig)
            })
    });
    // A frontier entry, by the device it names, with that device's signature.
    checkpoint.frontier.retain(|(d, c)| {
        *d != answer.device
            && *d != me
            && membership.key(d).is_some_and(|key| {
                let place = Place {
                    seq: c.s,
                    prev: c.prev,
                    vroot: Some(c.vroot),
                };
                signed(&key, "change", *d, &place, &c.h, &c.sig)
            })
    });
    checkpoint.marks.retain(|(d, m)| *d != me && m.s > 0);
    Ok(Joined {
        enroller: answer.device,
        enrollment,
        naming_key,
        checkpoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::trust::Body;

    const LIB: Id16 = Id16([7; 16]);
    const W: Id16 = Id16([8; 16]);
    const NOW: u64 = 1_760_000_000_000;

    /// One whole exchange: the applicant's request, D's answer and the reveal.
    fn exchange() -> (Applicant, Keypair, Answering, Answer, Reveal) {
        let (applicant, req) = request(&LIB, &W, "iPad").unwrap();
        let d = Keypair::from_seed(&[1; 32]);
        let request = open_request(&req, &LIB).unwrap();
        let (answering, ans) = answer(&LIB, &request, &d, &W).unwrap();
        let read = open_answer(&ans, &LIB, &d.public()).unwrap();
        let rev = reveal(&LIB, &applicant, &read, &W).unwrap();
        let revealed = open_reveal(&rev, &LIB, &answering).unwrap();
        (applicant, d, answering, read, revealed)
    }

    #[test]
    fn both_sides_show_the_same_code_from_the_bytes_they_kept() {
        let (applicant, d, answering, read, revealed) = exchange();
        let (t, code) = code_enrolled(&LIB, &answering, &revealed, &d.public());
        let (t2, code2) = code_applicant(&LIB, &applicant, &read, &d.public());
        assert_eq!(t, t2);
        assert_eq!(code, code2);
        assert!(code < 1_000_000);
    }

    #[test]
    fn a_swapped_answer_shows_another_code() {
        // A stranger S answers the same request; the applicant reveals to S's
        // answer, and D, which answered too, acts only on a reveal naming its own.
        let (applicant, req) = request(&LIB, &W, "iPad").unwrap();
        let request = open_request(&req, &LIB).unwrap();
        let d = Keypair::from_seed(&[1; 32]);
        let s = Keypair::from_seed(&[66; 32]);
        let (answering, _) = answer(&LIB, &request, &d, &W).unwrap();
        let (_, forged) = answer(&LIB, &request, &s, &W).unwrap();
        let read = open_answer(&forged, &LIB, &s.public()).unwrap();
        let rev = reveal(&LIB, &applicant, &read, &W).unwrap();
        assert_eq!(
            open_reveal(&rev, &LIB, &answering).unwrap_err(),
            EnrollError::Mismatch
        );
        // And S's answer doesn't open as D's.
        assert_eq!(
            open_answer(&forged, &LIB, &d.public()).unwrap_err(),
            EnrollError::Signature
        );
    }

    #[test]
    fn a_reveal_carries_its_applicants_signature_and_nothing_else() {
        let (applicant, d, answering, read, _) = exchange();
        // A stranger's signature listed before the applicant's: refused, so the
        // `pop` can only be the applicant's own.
        let s = Keypair::from_seed(&[66; 32]);
        let fields = json!({ "applicant": applicant.request.applicant.to_string(),
                "device": applicant.request.applicant.to_string(), "writer": W.to_string(),
                "request": applicant.request.hash.to_string(), "answer": read.hash.to_string(),
                "enroller": d.device().to_string(), "na": b64url_encode(&applicant.na) });
        let two = trust::sign(
            "enroll-reveal",
            &LIB,
            fields,
            &[
                (&s, Signer::Device(s.device()), 0),
                (
                    &applicant.key,
                    Signer::Device(applicant.request.applicant),
                    0,
                ),
            ],
        )
        .0;
        assert_eq!(
            open_reveal(&two, &LIB, &answering).unwrap_err(),
            EnrollError::Signature
        );
    }

    #[test]
    fn a_name_is_cut_to_the_label_limit_and_a_longer_one_refused() {
        let (applicant, req) = request(&LIB, &W, &"é".repeat(100)).unwrap();
        assert_eq!(
            applicant.request.name.chars().count(),
            trust::MAX_LABEL_CHARS
        );
        assert_eq!(
            open_request(&req, &LIB).unwrap().name,
            applicant.request.name
        );
        let long = trust::sign(
            "enroll-request",
            &LIB,
            json!({ "applicant": applicant.request.applicant.to_string(),
                    "device": applicant.request.applicant.to_string(), "writer": W.to_string(),
                    "key": b64url_encode(&applicant.request.key), "kx": b64url_encode(&applicant.request.kx),
                    "name": "x".repeat(65), "commit": applicant.request.commit.to_string() }),
            &[(&applicant.key, Signer::Device(applicant.request.applicant), 0)],
        )
        .0;
        assert_eq!(open_request(&long, &LIB).unwrap_err(), EnrollError::Shape);
    }

    #[test]
    fn a_reveal_must_open_the_commitment() {
        let (applicant, d, answering, read, _) = exchange();
        // Another `na` than the one committed to.
        let other = Applicant {
            key: Keypair::from_seed(&applicant.key.seed()),
            kx_secret: applicant.kx_secret,
            na: [3; 32],
            request: applicant.request.clone(),
        };
        let rev = reveal(&LIB, &other, &read, &W).unwrap();
        assert_eq!(
            open_reveal(&rev, &LIB, &answering).unwrap_err(),
            EnrollError::Commit
        );
        let _ = d;
    }

    #[test]
    fn a_request_is_signed_by_the_key_its_applicant_id_names() {
        let (applicant, _) = request(&LIB, &W, "iPad").unwrap();
        // Someone else's key signing a request that names the applicant's key.
        let s = Keypair::from_seed(&[66; 32]);
        let forged = trust::sign(
            "enroll-request",
            &LIB,
            json!({ "applicant": applicant.request.applicant.to_string(),
                    "device": applicant.request.applicant.to_string(), "writer": W.to_string(),
                    "key": b64url_encode(&applicant.request.key), "kx": b64url_encode(&[2; 32]),
                    "name": "iPad", "commit": applicant.request.commit.to_string() }),
            &[(&s, Signer::Device(applicant.request.applicant), 0)],
        )
        .0;
        assert_eq!(
            open_request(&forged, &LIB).unwrap_err(),
            EnrollError::Signature
        );
        // Nor one signed by its key but naming another applicant.
        let other = trust::sign(
            "enroll-request",
            &LIB,
            json!({ "applicant": s.device().to_string(),
                    "device": applicant.request.applicant.to_string(), "writer": W.to_string(),
                    "key": b64url_encode(&applicant.request.key), "kx": b64url_encode(&[2; 32]),
                    "name": "iPad", "commit": applicant.request.commit.to_string() }),
            &[(
                &applicant.key,
                Signer::Device(applicant.request.applicant),
                0,
            )],
        )
        .0;
        assert_eq!(
            open_request(&other, &LIB).unwrap_err(),
            EnrollError::Signature
        );
        // Nor one whose payload builds more values than a request needs.
        let mut fields = json!({ "applicant": applicant.request.applicant.to_string(),
                "device": applicant.request.applicant.to_string(), "writer": W.to_string(),
                "key": b64url_encode(&applicant.request.key), "kx": b64url_encode(&[2; 32]),
                "name": "iPad", "commit": applicant.request.commit.to_string() });
        fields["pad"] = Value::from(vec![0u64; 100]);
        let padded = trust::sign(
            "enroll-request",
            &LIB,
            fields,
            &[(
                &applicant.key,
                Signer::Device(applicant.request.applicant),
                0,
            )],
        )
        .0;
        assert!(matches!(
            open_request(&padded, &LIB),
            Err(EnrollError::Envelope(_))
        ));
        // A file past 16 KB isn't read.
        assert_eq!(
            open_request(&vec![b' '; MAX_FILE + 1], &LIB).unwrap_err(),
            EnrollError::TooLarge
        );
    }

    #[test]
    fn the_enroll_record_carries_the_reveal_and_its_pop() {
        let (_, d, answering, _, revealed) = exchange();
        let (t, _) = code_enrolled(&LIB, &answering, &revealed, &d.public());
        let head = Hash32([4; 32]);
        let (bytes, name) = enroll(&LIB, &head, &answering, &revealed, &t, &d, 1, Hlc(NOW, 1));
        // `trust::open` checks the reveal names this enroller and applicant and that
        // the applicant's key signed it.
        let r = trust::open(&bytes, &LIB, &name).unwrap();
        assert!(
            matches!(&r.body, Body::Enroll { by: Some(b), sas: Some(s), .. } if *b == d.device() && *s == t)
        );
        assert!(r.signed_by(&Signer::Device(d.device()), &d.public(), 1));
    }

    const NOW_MS: i64 = NOW as i64;
    const OLD: i64 = NOW_MS - 40 * 24 * 60 * 60 * 1000;

    /// D's library: its genesis, then D enrols P (`n` 1), then the applicant
    /// (`n` 2) from the exchange.
    struct Lib {
        records: BTreeMap<Hash32, Record>,
        genesis: Hash32,
        enrollment: Hash32,
        t: Hash32,
    }

    fn signed_reveal(applicant: &Keypair, enroller: &DeviceId) -> (Vec<u8>, [u8; 64]) {
        let payload = crate::sync::json::canonical(&json!({
            "type": "enroll-reveal", "library": LIB.to_string(),
            "applicant": applicant.device().to_string(), "enroller": enroller.to_string(),
        }));
        let pop = applicant.sign(&envelope::message(
            "enroll-reveal",
            &LIB,
            &Signer::Device(applicant.device()),
            &Place::default(),
            &crypto::payload_hash(&payload),
        ));
        (payload, pop)
    }

    fn library(d: &Keypair, p: &Keypair, answering: &Answering, revealed: &Reveal) -> Lib {
        let mut records = BTreeMap::new();
        let mut put = |(bytes, name): (Vec<u8>, Hash32)| {
            records.insert(name, trust::open(&bytes, &LIB, &name).unwrap());
            name
        };
        let c = Keypair::from_seed(&[100; 32]);
        let genesis = put(trust::sign(
            "genesis",
            &LIB,
            json!({ "device": d.device().to_string(), "key": b64url_encode(&d.public()), "label": "first",
                    "clock": [NOW, 0],
                    "code": { "nonce": b64url_encode(&[5; 16]), "codeKey": b64url_encode(&c.public()) } }),
            &[(d, Signer::Device(d.device()), 0), (&c, Signer::Code, 0)],
        ));
        let (reveal, pop) = signed_reveal(p, &d.device());
        let ep = put(trust::sign(
            "enroll",
            &LIB,
            json!({ "prev": genesis.to_string(), "device": p.device().to_string(),
                    "key": b64url_encode(&p.public()), "label": "P", "clock": [NOW, 1],
                    "by": d.device().to_string(), "n": 1, "sas": "dd".repeat(32),
                    "reveal": b64url_encode(&reveal), "pop": b64url_encode(&pop) }),
            &[(d, Signer::Device(d.device()), 1)],
        ));
        let (t, _) = code_enrolled(&LIB, answering, revealed, &d.public());
        let enrollment = put(enroll(
            &LIB,
            &ep,
            answering,
            revealed,
            &t,
            d,
            2,
            Hlc(NOW, 2),
        ));
        Lib {
            records,
            genesis,
            enrollment,
            t,
        }
    }

    fn membership(l: &Lib, me: DeviceId) -> Membership {
        let seen: BTreeMap<Hash32, i64> = l.records.keys().map(|h| (*h, OLD)).collect();
        let none = std::collections::BTreeSet::new();
        crate::sync::membership::compute(&crate::sync::membership::Inputs {
            genesis: l.genesis,
            records: &l.records,
            first_seen: &seen,
            confirmed: &none,
            checkpoint: &none,
            held: &|_, _| None,
            accepted: &none,
            now_ms: NOW_MS,
            me: Some(me),
        })
        .unwrap()
    }

    /// D's checkpoint: its own first change, view generation 2, counter 5, and P's
    /// device file 3.
    fn checkpoint(d: &Keypair, p: &Keypair, head: Hash32) -> Checkpoint {
        let change = crate::sync::chain::sign_change(d, &LIB, &W, 1, None, &[]);
        let csig = envelope::open(&change.bytes, "change", &LIB).unwrap().sigs[0].sig;
        let file = crate::sync::device_file::write(p, &LIB, &W, 3, &Default::default()).unwrap();
        let fsig = envelope::open(&file.bytes, "device", &LIB).unwrap().sigs[0].sig;
        Checkpoint {
            head,
            removals: vec![],
            own: Some(SignedChange {
                s: 1,
                h: change.hash,
                prev: None,
                vroot: envelope::vroot(&[]),
                sig: csig,
            }),
            view: Some((Hash32([6; 32]), 2)),
            counter: 5,
            floors: vec![SignedFile {
                device: p.device(),
                counter: 3,
                hash: file.hash,
                sig: fsig,
            }],
            frontier: vec![],
            marks: vec![],
        }
    }

    #[test]
    fn the_applicant_joins_with_the_naming_key_and_the_checkpoint_as_floors() {
        let (applicant, d, answering, read, revealed) = exchange();
        let p = Keypair::from_seed(&[2; 32]);
        let l = library(&d, &p, &answering, &revealed);
        let m = membership(&l, applicant.request.applicant);
        let cp = checkpoint(&d, &p, l.genesis);
        let ok = enroll_ok(&LIB, &answering, &l.t, &l.enrollment, &cp, &[9; 32], &d, &W).unwrap();
        let joined = join(&ok, &LIB, &applicant, &read, &d.public(), &m, &l.records).unwrap();
        assert_eq!(joined.naming_key, [9; 32]);
        assert_eq!(joined.checkpoint, cp);
        let floors = joined.floors();
        let mine = &floors[&d.device().to_string()];
        assert_eq!((mine.s, mine.gen), (1, 2));
        assert_eq!(mine.h, Some(cp.own.as_ref().unwrap().h.to_string()));
        // Installed, the floors let the device read P's checkpointed file 3, and
        // refuse an older one.
        use crate::sync::device_file::{accept, Accepted};
        use crate::sync::device_store::{DeviceStore, Memory};
        let ks = Memory::default();
        let mut store = DeviceStore::default();
        store.identity(&LIB).unwrap().floors = floors;
        let file = |n: u64| {
            crate::sync::device_file::write(&p, &LIB, &W, n, &Default::default())
                .unwrap()
                .bytes
        };
        let none = |_: &str| None;
        assert!(matches!(
            accept(
                &ks,
                &mut store,
                &LIB,
                &p.device(),
                &p.public(),
                &file(2),
                &none
            )
            .unwrap(),
            Accepted::Stale
        ));
        assert!(matches!(
            accept(
                &ks,
                &mut store,
                &LIB,
                &p.device(),
                &p.public(),
                &file(3),
                &none
            )
            .unwrap(),
            Accepted::New(..)
        ));
    }

    #[test]
    fn a_checkpoint_entry_whose_signature_doesnt_hold_is_ignored() {
        let (applicant, d, answering, read, revealed) = exchange();
        let p = Keypair::from_seed(&[2; 32]);
        let l = library(&d, &p, &answering, &revealed);
        let m = membership(&l, applicant.request.applicant);
        let mut cp = checkpoint(&d, &p, l.genesis);
        // A floor naming P's file 4 with file 3's signature; a change with another
        // hash than the one signed.
        cp.floors[0].counter = 4;
        cp.own.as_mut().unwrap().h = Hash32([1; 32]);
        let ok = enroll_ok(&LIB, &answering, &l.t, &l.enrollment, &cp, &[9; 32], &d, &W).unwrap();
        let joined = join(&ok, &LIB, &applicant, &read, &d.public(), &m, &l.records).unwrap();
        assert!(joined.checkpoint.floors.is_empty());
        assert!(joined.checkpoint.own.is_none());
        assert!(!joined.floors().contains_key(&p.device().to_string()));
    }

    #[test]
    fn an_enroll_ok_is_accepted_only_for_this_transcript_and_a_trusted_enrollment() {
        let (applicant, d, answering, read, revealed) = exchange();
        let p = Keypair::from_seed(&[2; 32]);
        let l = library(&d, &p, &answering, &revealed);
        let m = membership(&l, applicant.request.applicant);
        let cp = checkpoint(&d, &p, l.genesis);
        // Another transcript.
        let other = enroll_ok(
            &LIB,
            &answering,
            &Hash32([9; 32]),
            &l.enrollment,
            &cp,
            &[9; 32],
            &d,
            &W,
        )
        .unwrap();
        assert_eq!(
            join(&other, &LIB, &applicant, &read, &d.public(), &m, &l.records).unwrap_err(),
            EnrollError::Mismatch
        );
        // A membership in which this device isn't trusted: its enrollment isn't held.
        let ok = enroll_ok(&LIB, &answering, &l.t, &l.enrollment, &cp, &[9; 32], &d, &W).unwrap();
        let mut without = l.records.clone();
        without.remove(&l.enrollment);
        let l2 = Lib {
            records: without,
            genesis: l.genesis,
            enrollment: l.enrollment,
            t: l.t,
        };
        let m2 = membership(&l2, applicant.request.applicant);
        assert_eq!(
            join(&ok, &LIB, &applicant, &read, &d.public(), &m2, &l2.records).unwrap_err(),
            EnrollError::NotTrusted
        );
        // Held, but in a membership that trusts nobody (another genesis joined).
        let elsewhere = Lib {
            records: l.records.clone(),
            genesis: Hash32([0; 32]),
            enrollment: l.enrollment,
            t: l.t,
        };
        let m3 = membership(&elsewhere, applicant.request.applicant);
        assert_eq!(
            join(&ok, &LIB, &applicant, &read, &d.public(), &m3, &l.records).unwrap_err(),
            EnrollError::NotTrusted
        );
        // Signed by another device than the one revealed to.
        let s = Keypair::from_seed(&[66; 32]);
        let forged =
            enroll_ok(&LIB, &answering, &l.t, &l.enrollment, &cp, &[9; 32], &s, &W).unwrap();
        assert_eq!(
            join(
                &forged,
                &LIB,
                &applicant,
                &read,
                &d.public(),
                &m,
                &l.records
            )
            .unwrap_err(),
            EnrollError::Signature
        );
    }

    #[test]
    fn a_naming_key_sealed_to_another_kx_doesnt_open() {
        let (applicant, d, answering, read, revealed) = exchange();
        let p = Keypair::from_seed(&[2; 32]);
        let l = library(&d, &p, &answering, &revealed);
        let m = membership(&l, applicant.request.applicant);
        let cp = checkpoint(&d, &p, l.genesis);
        let mut elsewhere = Answering {
            request: answering.request.clone(),
            answer: answering.answer.clone(),
        };
        elsewhere.request.kx = crypto::x25519_public(&[3; 32]);
        let ok = enroll_ok(&LIB, &elsewhere, &l.t, &l.enrollment, &cp, &[9; 32], &d, &W).unwrap();
        assert_eq!(
            join(&ok, &LIB, &applicant, &read, &d.public(), &m, &l.records).unwrap_err(),
            EnrollError::NamingKey
        );
    }

    #[test]
    fn the_checkpoints_frontier_and_marks_become_floors_once_their_signatures_hold() {
        let (applicant, d, answering, read, revealed) = exchange();
        let p = Keypair::from_seed(&[2; 32]);
        let l = library(&d, &p, &answering, &revealed);
        let m = membership(&l, applicant.request.applicant);
        let mut cp = checkpoint(&d, &p, l.genesis);
        // D applied P through change 3, and holds P's changes through 2 as accepted
        // by all.
        let mut prev = None;
        let mut signed = Vec::new();
        for s in 1..=3u64 {
            let c = crate::sync::chain::sign_change(&p, &LIB, &W, s, prev, &[]);
            let sig = envelope::open(&c.bytes, "change", &LIB).unwrap().sigs[0].sig;
            signed.push(SignedChange {
                s,
                h: c.hash,
                prev,
                vroot: envelope::vroot(&[]),
                sig,
            });
            prev = Some(c.hash);
        }
        cp.frontier = vec![(p.device(), signed[2].clone())];
        cp.marks = vec![(
            p.device(),
            Mark {
                s: 2,
                h: signed[1].h,
            },
        )];
        let ok = enroll_ok(&LIB, &answering, &l.t, &l.enrollment, &cp, &[9; 32], &d, &W).unwrap();
        let joined = join(&ok, &LIB, &applicant, &read, &d.public(), &m, &l.records).unwrap();
        let f = &joined.floors()[&p.device().to_string()];
        assert_eq!((f.s, f.h.clone()), (3, Some(signed[2].h.to_string())));
        assert_eq!(
            f.earlier,
            vec![device_store::Checkpoint {
                s: 2,
                h: signed[1].h.to_string()
            }]
        );
        assert_eq!(f.counter, 2);
        // A frontier entry with another change's signature is dropped; the mark
        // alone stays a floor.
        let mut forged = cp.clone();
        forged.frontier[0].1.sig = signed[0].sig;
        let ok = enroll_ok(
            &LIB,
            &answering,
            &l.t,
            &l.enrollment,
            &forged,
            &[9; 32],
            &d,
            &W,
        )
        .unwrap();
        let joined = join(&ok, &LIB, &applicant, &read, &d.public(), &m, &l.records).unwrap();
        assert!(joined.checkpoint.frontier.is_empty());
        let f = &joined.floors()[&p.device().to_string()];
        assert_eq!((f.s, f.earlier.len()), (2, 0));
    }
}
