//! SP5, SP6: the signed envelope every protocol file but a body or a book is in.
//!
//! `{ "sync": 1, "type": T, "payload": B, "sigs": [ { "by": S, "sig": G } … ] }`:
//! `B` is the payload's exact bytes in base64url, so nothing is re-encoded before it
//! is checked, and a file is named, chained and attested by its payload hash, never
//! by its envelope. Each signature covers the file's type, library, signer and place
//! in the signer's history (SP6). Which signers a type needs is the membership's
//! rule (SP19, SP20); this module opens envelopes and checks one signature at a time.

use super::crypto::{self, Input, Keypair};
use super::ids::{b64url_decode, b64url_encode, device_id, identity_hash, DeviceId, Hash32, Id16};
use super::json::{self, JsonError, Numbers};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// SP4: the suite every envelope names. A new suite is a new number, never a
/// negotiation.
pub const SUITE: u64 = 1;

/// Who signed: a device, or the recovery code's key (SP22).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signer {
    Device(DeviceId),
    Code,
}

impl Signer {
    /// SP6: the signer's 16 bytes, zero for a code key.
    fn id(&self) -> [u8; 16] {
        match self {
            Signer::Device(d) => d.0,
            Signer::Code => [0; 16],
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "code" => Some(Signer::Code),
            _ => Id16::parse(s).map(Signer::Device),
        }
    }

    fn to_json(self) -> String {
        match self {
            Signer::Device(d) => d.to_string(),
            Signer::Code => "code".into(),
        }
    }
}

/// SP6: where a signature puts its file in the signer's history. `seq` is a change
/// file's sequence, a view's generation, a device file's counter or an
/// enrollment's `n` (0 when the type has none); `prev` and `vroot` are a change
/// file's (zeros for every other type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub seq: u64,
    pub prev: Option<Hash32>,
    pub vroot: Option<Hash32>,
}

/// SP6: the bytes a signature is over. The domain and the type are length-prefixed
/// like every variable field, and the two `0x00` bytes are literal (decisions.md,
/// “SP4's exact inputs and the envelope”, (1)).
pub fn message(
    type_: &str,
    library: &Id16,
    signer: &Signer,
    place: &Place,
    payload_hash: &Hash32,
) -> Vec<u8> {
    let zero = [0u8; 32];
    Input::domain("linen-sync/sig/v1")
        .fixed(&[0])
        .var(type_.as_bytes())
        .fixed(&[0])
        .fixed(&library.0)
        .fixed(&signer.id())
        .seq(place.seq)
        .fixed(place.prev.as_ref().map_or(&zero, |h| &h.0))
        .fixed(place.vroot.as_ref().map_or(&zero, |h| &h.0))
        .fixed(&payload_hash.0)
        .bytes()
        .to_vec()
}

/// SP6, SP17: one version in a change file's version list, as a spine keeps it:
/// 16 bytes of `H(key)`, then the version hash.
pub fn vlist_entry(key: &str, version: &Hash32) -> [u8; 48] {
    let mut e = [0u8; 48];
    e[..16].copy_from_slice(&identity_hash(key.as_bytes()).0[..16]);
    e[16..].copy_from_slice(&version.0);
    e
}

/// SP6: `vroot`, `H("linen-sync/vlist/v1" ‖ the version list)`, the list in the
/// file's order, as one variable field, length-prefixed (decisions.md, (2)).
pub fn vroot(entries: &[[u8; 48]]) -> Hash32 {
    Input::domain("linen-sync/vlist/v1")
        .var(&entries.concat())
        .hash()
}

/// One signature in an envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sig {
    pub by: Signer,
    pub sig: [u8; 64],
}

/// SP6: signs a payload for its place.
pub fn sign(
    key: &Keypair,
    by: Signer,
    type_: &str,
    library: &Id16,
    place: &Place,
    payload: &[u8],
) -> Sig {
    let hash = crypto::payload_hash(payload);
    Sig {
        by,
        sig: key.sign(&message(type_, library, &by, place, &hash)),
    }
}

/// SP5: the envelope's bytes.
pub fn write(type_: &str, payload: &[u8], sigs: &[Sig]) -> Vec<u8> {
    let sigs: Vec<Value> = sigs
        .iter()
        .map(|s| json!({ "by": s.by.to_json(), "sig": b64url_encode(&s.sig) }))
        .collect();
    json!({
        "sync": SUITE,
        "type": type_,
        "payload": b64url_encode(payload),
        "sigs": sigs,
    })
    .to_string()
    .into_bytes()
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnvelopeError {
    #[error("the envelope isn't valid JSON: {0}")]
    Json(JsonError),
    #[error("the envelope isn't SP5's shape")]
    Shape,
    #[error("a suite this Linen doesn't know")]
    Suite,
    #[error("not the type the reader expected")]
    Type,
    #[error("signed for another library")]
    Library,
    #[error("the payload isn't valid JSON: {0}")]
    Payload(JsonError),
    #[error("a device named twice, or one signature twice")]
    DuplicateSigner,
}

/// An envelope opened and checked for its shape, type and library; its signatures
/// are checked one at a time (`verified`), against what the membership says.
#[derive(Clone, Debug)]
pub struct Envelope {
    pub type_: String,
    /// The payload's exact bytes.
    pub payload: Vec<u8>,
    /// The payload, parsed (SP3).
    pub body: Map<String, Value>,
    /// SP5: the payload hash, the file's identity.
    pub hash: Hash32,
    pub sigs: Vec<Sig>,
}

fn exactly(o: &Map<String, Value>, fields: &[&str]) -> bool {
    o.len() == fields.len() && fields.iter().all(|f| o.contains_key(*f))
}

/// SP5: opens an envelope of the type the reader expected, for its library. The
/// caller has checked the file's size against SP8's limit before reading it. A
/// payload whose `type` or `library` differs is refused before any signature is
/// checked.
pub fn open(bytes: &[u8], expected_type: &str, library: &Id16) -> Result<Envelope, EnvelopeError> {
    open_within(bytes, expected_type, library, u64::MAX)
}

/// `open`, the payload building at most `max_values` values (`json::parse_within`),
/// so a file of many tiny values is refused before it takes much memory.
pub fn open_within(
    bytes: &[u8],
    expected_type: &str,
    library: &Id16,
    max_values: u64,
) -> Result<Envelope, EnvelopeError> {
    let v = json::parse(bytes, Numbers::Integers).map_err(EnvelopeError::Json)?;
    let o = v.as_object().ok_or(EnvelopeError::Shape)?;
    // Nothing in the envelope beyond its four fields is signed, so nothing else is
    // read: an unknown field is refused, not kept, as are no signatures, a device
    // named twice and one signature twice (decisions.md, (5)). Several code keys
    // may sign one file (SP19: a removal, confirm or merge carries the current
    // code's and the next code's), each checked with the key its head names.
    if !exactly(o, &["sync", "type", "payload", "sigs"]) {
        return Err(EnvelopeError::Shape);
    }
    if o["sync"].as_u64() != Some(SUITE) {
        return Err(EnvelopeError::Suite);
    }
    let type_ = o["type"].as_str().ok_or(EnvelopeError::Shape)?;
    if type_ != expected_type {
        return Err(EnvelopeError::Type);
    }
    let payload = o["payload"]
        .as_str()
        .and_then(b64url_decode)
        .ok_or(EnvelopeError::Shape)?;
    let body = match json::parse_within(&payload, Numbers::Integers, max_values)
        .map_err(EnvelopeError::Payload)?
        .0
    {
        Value::Object(m) => m,
        _ => return Err(EnvelopeError::Shape),
    };
    // SP5: the payload repeats `type` and `library`.
    if body.get("type").and_then(Value::as_str) != Some(type_) {
        return Err(EnvelopeError::Type);
    }
    if body
        .get("library")
        .and_then(Value::as_str)
        .and_then(Id16::parse)
        != Some(*library)
    {
        return Err(EnvelopeError::Library);
    }
    let raw = o["sigs"].as_array().ok_or(EnvelopeError::Shape)?;
    if raw.is_empty() {
        return Err(EnvelopeError::Shape);
    }
    let (mut devices, mut codes) = (HashSet::new(), HashSet::new());
    let mut sigs = Vec::with_capacity(raw.len());
    for s in raw {
        let s = s.as_object().ok_or(EnvelopeError::Shape)?;
        if !exactly(s, &["by", "sig"]) {
            return Err(EnvelopeError::Shape);
        }
        let by = s["by"]
            .as_str()
            .and_then(Signer::parse)
            .ok_or(EnvelopeError::Shape)?;
        let sig: [u8; 64] = s["sig"]
            .as_str()
            .and_then(b64url_decode)
            .and_then(|b| b.try_into().ok())
            .ok_or(EnvelopeError::Shape)?;
        let repeated = match by {
            Signer::Device(d) => !devices.insert(d),
            Signer::Code => !codes.insert(sig),
        };
        if repeated {
            return Err(EnvelopeError::DuplicateSigner);
        }
        sigs.push(Sig { by, sig });
    }
    Ok(Envelope {
        type_: type_.to_owned(),
        hash: crypto::payload_hash(&payload),
        payload,
        body,
        sigs,
    })
}

impl Envelope {
    /// Whether `by` signed this file for `place`, with `key`. A device's key must be
    /// the one its ID names (SP1); a code key is whatever the sealed head names.
    /// Checked here, at every verification (decisions.md, (6)).
    pub fn verified(&self, by: &Signer, key: &[u8; 32], library: &Id16, place: &Place) -> bool {
        if let Signer::Device(d) = by {
            if device_id(key) != *d {
                return false;
            }
        }
        self.sigs.iter().any(|s| {
            s.by == *by
                && crypto::verify(
                    key,
                    &message(&self.type_, library, by, place, &self.hash),
                    &s.sig,
                )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::tests::{hex, vectors};

    fn hexv(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    struct Fixture {
        library: Id16,
        key: Keypair,
        payload: Vec<u8>,
        place: Place,
    }

    fn fixture() -> Fixture {
        let v = vectors();
        let s = &v["signature"];
        let keys: Vec<&str> = s["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap())
            .collect();
        let entries: Vec<[u8; 48]> = keys
            .iter()
            .zip(s["version_hashes"].as_array().unwrap())
            .map(|(k, h)| vlist_entry(k, &Hash32(hex(h))))
            .collect();
        let root = vroot(&entries);
        assert_eq!(root, Hash32(hex(&s["vroot"])));
        Fixture {
            library: Id16(hex(&v["library"])),
            key: Keypair::from_seed(&hex(&v["seed"])),
            payload: s["payload"].as_str().unwrap().as_bytes().to_vec(),
            place: Place {
                seq: s["seq"].as_u64().unwrap(),
                prev: Some(Hash32(hex(&s["prev"]))),
                vroot: Some(root),
            },
        }
    }

    #[test]
    fn signatures_match_the_vectors() {
        let f = fixture();
        let v = vectors();
        let s = &v["signature"];
        let by = Signer::Device(f.key.device());
        let hash = crypto::payload_hash(&f.payload);
        assert_eq!(hash, Hash32(hex(&s["payload_hash"])));
        assert_eq!(
            message("change", &f.library, &by, &f.place, &hash),
            hexv(s["message"].as_str().unwrap())
        );
        let sig = sign(&f.key, by, "change", &f.library, &f.place, &f.payload);
        assert_eq!(b64url_encode(&sig.sig), s["sig"].as_str().unwrap());
        // A code key's: the signer zero, and no place.
        assert_eq!(
            message(
                "genesis",
                &f.library,
                &Signer::Code,
                &Place::default(),
                &hash
            ),
            hexv(s["code_message"].as_str().unwrap())
        );
    }

    #[test]
    fn an_envelope_round_trips_and_is_verified_for_its_place() {
        let f = fixture();
        let by = Signer::Device(f.key.device());
        let sig = sign(&f.key, by, "change", &f.library, &f.place, &f.payload);
        let bytes = write("change", &f.payload, std::slice::from_ref(&sig));
        let e = open(&bytes, "change", &f.library).unwrap();
        assert_eq!(e.payload, f.payload);
        assert_eq!(e.hash, crypto::payload_hash(&f.payload));
        assert_eq!(e.body["seq"], 7);
        assert!(e.verified(&by, &f.key.public(), &f.library, &f.place));
        // SP6: signed for one place, it counts for no other.
        for other in [
            Place { seq: 8, ..f.place },
            Place {
                prev: None,
                ..f.place
            },
            Place {
                vroot: Some(Hash32([1; 32])),
                ..f.place
            },
        ] {
            assert!(!e.verified(&by, &f.key.public(), &f.library, &other));
        }
        // Nor for another signer, or a key its ID doesn't name.
        let other = Keypair::from_seed(&[1; 32]);
        assert!(!e.verified(&Signer::Code, &f.key.public(), &f.library, &f.place));
        assert!(!e.verified(&by, &other.public(), &f.library, &f.place));
        assert!(!e.verified(
            &Signer::Device(other.device()),
            &other.public(),
            &f.library,
            &f.place
        ));
    }

    #[test]
    fn a_file_signed_for_one_purpose_isnt_accepted_as_another() {
        let f = fixture();
        let by = Signer::Device(f.key.device());
        let sig = sign(&f.key, by, "change", &f.library, &f.place, &f.payload);
        let bytes = write("change", &f.payload, std::slice::from_ref(&sig));
        // Read as another type, or for another library: refused before any signature.
        assert_eq!(
            open(&bytes, "view", &f.library).unwrap_err(),
            EnvelopeError::Type
        );
        assert_eq!(
            open(&bytes, "change", &Id16([0; 16])).unwrap_err(),
            EnvelopeError::Library
        );
        // An envelope relabelled with another type doesn't match its payload.
        let relabelled = write("view", &f.payload, std::slice::from_ref(&sig));
        assert_eq!(
            open(&relabelled, "view", &f.library).unwrap_err(),
            EnvelopeError::Type
        );
        // A payload changed by one byte has another hash, so the signature fails.
        let changed = String::from_utf8(f.payload.clone())
            .unwrap()
            .replace(r#""seq":7"#, r#""seq":8"#)
            .into_bytes();
        assert_ne!(changed, f.payload);
        let forged = open(&write("change", &changed, &[sig]), "change", &f.library).unwrap();
        assert!(!forged.verified(&by, &f.key.public(), &f.library, &f.place));
    }

    #[test]
    fn a_removal_carries_its_device_and_two_code_keys() {
        // SP19: a `remove` is signed by an enrolled device, the current code's key
        // and the next code's key; both code keys sign as "code".
        let f = fixture();
        let (current, next) = (Keypair::from_seed(&[2; 32]), Keypair::from_seed(&[3; 32]));
        let payload = format!(r#"{{"type":"remove","library":"{}"}}"#, f.library).into_bytes();
        let none = Place::default();
        let by = Signer::Device(f.key.device());
        let sigs = [
            sign(&f.key, by, "remove", &f.library, &none, &payload),
            sign(
                &current,
                Signer::Code,
                "remove",
                &f.library,
                &none,
                &payload,
            ),
            sign(&next, Signer::Code, "remove", &f.library, &none, &payload),
        ];
        let e = open(&write("remove", &payload, &sigs), "remove", &f.library).unwrap();
        assert!(e.verified(&by, &f.key.public(), &f.library, &none));
        assert!(e.verified(&Signer::Code, &current.public(), &f.library, &none));
        assert!(e.verified(&Signer::Code, &next.public(), &f.library, &none));
        // A code key that didn't sign isn't found among them.
        let other = Keypair::from_seed(&[4; 32]);
        assert!(!e.verified(&Signer::Code, &other.public(), &f.library, &none));
    }

    #[test]
    fn an_envelope_not_of_sp5s_shape_is_refused() {
        let f = fixture();
        let sig = sign(
            &f.key,
            Signer::Code,
            "change",
            &f.library,
            &f.place,
            &f.payload,
        );
        let good: Value =
            serde_json::from_slice(&write("change", &f.payload, std::slice::from_ref(&sig)))
                .unwrap();
        let with = |edit: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            edit(&mut v);
            open(v.to_string().as_bytes(), "change", &f.library).unwrap_err()
        };
        assert_eq!(with(&|v| v["sync"] = json!(2)), EnvelopeError::Suite);
        assert_eq!(with(&|v| v["extra"] = json!(1)), EnvelopeError::Shape);
        assert_eq!(with(&|v| v["sigs"] = json!([])), EnvelopeError::Shape);
        // The same signature twice adds nothing: refused.
        assert_eq!(
            with(&|v| v["sigs"] = json!([v["sigs"][0], v["sigs"][0]])),
            EnvelopeError::DuplicateSigner
        );
        // One device twice, even with two different signatures: refused.
        let d = Signer::Device(f.key.device());
        let first = sign(&f.key, d, "change", &f.library, &f.place, &f.payload);
        let second = Sig {
            by: d,
            sig: [1; 64],
        };
        assert_eq!(
            open(
                &write("change", &f.payload, &[first, second]),
                "change",
                &f.library
            )
            .unwrap_err(),
            EnvelopeError::DuplicateSigner
        );
        assert_eq!(
            with(&|v| v["sigs"][0]["by"] = json!("A".repeat(32))),
            EnvelopeError::Shape
        );
        assert_eq!(
            with(&|v| v["sigs"][0]["sig"] = json!("AAAA")),
            EnvelopeError::Shape
        );
        assert_eq!(
            with(&|v| v["payload"] = json!(format!("{}=", v["payload"].as_str().unwrap()))),
            EnvelopeError::Shape
        );
        assert_eq!(
            with(&|v| v["payload"] = json!(b64url_encode(b"[1]"))),
            EnvelopeError::Shape
        );
        assert!(matches!(
            with(&|v| v["payload"] = json!(b64url_encode(br#"{"a":1,"a":2}"#))),
            EnvelopeError::Payload(JsonError::DuplicateKey)
        ));
        // A duplicate key in the envelope itself.
        let dup = format!(
            r#"{{"sync":1,"sync":1,"type":"change","payload":"{}","sigs":[]}}"#,
            b64url_encode(&f.payload)
        );
        assert!(matches!(
            open(dup.as_bytes(), "change", &f.library).unwrap_err(),
            EnvelopeError::Json(JsonError::DuplicateKey)
        ));
    }
}
