//! SP24: enrollment by a short code, steps 1–6 up to the `enroll` record.
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
use super::envelope::{self, EnvelopeError, Place, Signer};
use super::ids::{b64url_decode, b64url_encode, device_id, DeviceId, Hash32, Id16};
use super::trust;
use serde_json::{json, Map, Value};

/// SP24 (SY34): a request, answer or reveal is at most 16 KB.
pub const MAX_FILE: usize = 16 * 1024;
/// The most values one of these files' payloads may build (SP8): a request has a
/// dozen. PROVISIONAL (decisions.md, “Enrollment by a short code”, (3)).
pub const MAX_VALUES: u64 = 64;

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
    if bytes.len() > MAX_FILE {
        return Err(EnrollError::TooLarge);
    }
    let e = envelope::open_within(bytes, type_, library, MAX_VALUES)?;
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
    if !e.verified(&Signer::Device(device), key, library, &Place::default()) {
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
}
