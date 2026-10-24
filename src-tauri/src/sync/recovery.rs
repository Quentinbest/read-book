//! SP22, SP23, SP25: the recovery code as protocol.
//!
//! A code seals a head: the head's record names the code's public key and is
//! signed with it, and the head's `escrow` holds the naming key sealed under a key
//! only the code derives. A device joining by code finds the head its code opens,
//! takes the naming key from that head's escrow, and enrols on it (SP25). A new code
//! is made, shown and confirmed before the record it seals is written, and its
//! escrow before that record (SP23).
//!
//! PROVISIONAL: where the protocol leaves room, the reading here is Delegated to
//! Claude, not approved (`docs/decisions.md`, “The recovery code as protocol”).

use super::clock::Hlc;
use super::crypto::{self, CodeKeys, CryptoError, Keypair, RecoveryCode};
use super::envelope::Signer;
use super::ids::{b64url_encode, Hash32, Id16};
use super::trust::{self, Body, Record};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// SP22 (prov.): a device joining by code tries at most this many sealed heads.
pub const MAX_CODE_TRIES: usize = 64;

/// SP22: the sealed head `code` opens among `records`, and the code's keys for it.
/// The heads first seen last are tried first, since a code is most often the
/// latest one, and at most `MAX_CODE_TRIES` of them: each try is a full Argon2id
/// derivation. PROVISIONAL (decisions.md, “The recovery code as protocol”, (1)).
pub fn find_head(
    code: &RecoveryCode,
    library: &Id16,
    records: &BTreeMap<Hash32, Record>,
    first_seen: &BTreeMap<Hash32, i64>,
) -> Option<(Hash32, CodeKeys)> {
    let mut heads: Vec<(i64, Hash32, &trust::Seal)> = records
        .iter()
        .filter_map(|(h, r)| {
            r.seal()
                .map(|s| (first_seen.get(h).copied().unwrap_or(i64::MAX), *h, s))
        })
        .collect();
    heads.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    heads
        .into_iter()
        .take(MAX_CODE_TRIES)
        .find_map(|(_, h, seal)| {
            let keys = code.keys(library, &seal.nonce);
            (keys.signing.public() == seal.code_key).then_some((h, keys))
        })
}

/// SP22: the naming key from `head`'s escrow. Only an escrow signed by the head's
/// code key that opens counts: a stranger's escrow (another signer, or one that
/// doesn't open) is ignored, so it can't replace the reader's.
pub fn naming_key(
    keys: &CodeKeys,
    library: &Id16,
    head: &Hash32,
    records: &BTreeMap<Hash32, Record>,
) -> Option<[u8; 32]> {
    let code_key = keys.signing.public();
    records.values().find_map(|r| match &r.body {
        Body::Escrow { head: h, nonce, ct }
            if h == head && r.signed_by(&Signer::Code, &code_key, 0) =>
        {
            crypto::open_escrow(&keys.escrow, library, head, nonce, ct)
        }
        _ => None,
    })
}

/// SP23: a sealing record made with a new code, and that code's escrow for it.
pub struct Staged {
    /// The new code, to show the reader.
    pub code: RecoveryCode,
    /// The escrow, written first (step 3).
    pub escrow: (Vec<u8>, Hash32),
    /// The record it seals, written only after the escrow (step 4): its name is
    /// the new head.
    pub record: (Vec<u8>, Hash32),
}

/// SP23 (1), (3): makes the new code, then the record it seals (`genesis`,
/// `remove`, `confirm` or `merge`: `fields` without `code`, signed by `signers`
/// and the new code), and the escrow of the naming key for that record's head. The
/// head is the record's payload hash, so the escrow can name it before the record
/// is published. The caller shows the code, has the reader confirm it
/// (`confirms_last_group`), writes the escrow, and only then the record.
pub fn stage(
    type_: &str,
    library: &Id16,
    mut fields: Value,
    signers: &[(&Keypair, Signer, u64)],
    naming_key: &[u8; 32],
) -> Result<Staged, CryptoError> {
    let code = RecoveryCode::generate()?;
    let nonce: [u8; 16] = crypto::random()?;
    let keys = code.keys(library, &nonce);
    fields["code"] = json!({
        "nonce": b64url_encode(&nonce),
        "codeKey": b64url_encode(&keys.signing.public()),
    });
    let mut all: Vec<(&Keypair, Signer, u64)> = signers.to_vec();
    all.push((&keys.signing, Signer::Code, 0));
    let record = trust::sign(type_, library, fields, &all);
    let (enonce, ct) = crypto::seal_escrow(&keys.escrow, library, &record.1, naming_key)?;
    let escrow = trust::sign(
        "escrow",
        library,
        json!({
            "head": record.1.to_string(),
            "nonce": b64url_encode(&enonce),
            "ct": b64url_encode(&ct),
        }),
        &[(&keys.signing, Signer::Code, 0)],
    );
    Ok(Staged {
        code,
        escrow,
        record,
    })
}

/// SP23 (2): the reader typed the code's last group, read back as the code is
/// (upper case, spaces and hyphens dropped, `O` as `0`, `I` and `L` as `1`).
pub fn confirms_last_group(code: &RecoveryCode, typed: &str) -> bool {
    let shown = code.display();
    let last = shown.rsplit(' ').next().unwrap_or_default();
    let typed: String = typed
        .chars()
        .filter(|c| *c != ' ' && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect();
    !last.is_empty() && typed == last
}

/// SP25: the `enroll` by code on `head`: signed by the code's key and the joining
/// device's own key, both with sequence 0, so it counts toward no device's quota.
pub fn enroll_by_code(
    library: &Id16,
    head: &Hash32,
    keys: &CodeKeys,
    device: &Keypair,
    label: &str,
    clock: Hlc,
) -> (Vec<u8>, Hash32) {
    trust::sign(
        "enroll",
        library,
        json!({
            "prev": head.to_string(),
            "device": device.device().to_string(),
            "key": b64url_encode(&device.public()),
            "label": label,
            "clock": [clock.ms(), clock.n()],
            "by": "code",
            "sas": null,
        }),
        &[
            (&keys.signing, Signer::Code, 0),
            (device, Signer::Device(device.device()), 0),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: Id16 = Id16([7; 16]);
    const NOW: u64 = 1_760_000_000_000;

    fn open(rec: &(Vec<u8>, Hash32)) -> Record {
        trust::open(&rec.0, &LIB, &rec.1).unwrap()
    }

    /// A genesis staged with its code, and its escrow: what a library's first
    /// device writes (SP23).
    fn first(device: &Keypair, naming: &[u8; 32]) -> Staged {
        stage(
            "genesis",
            &LIB,
            json!({ "device": device.device().to_string(), "key": b64url_encode(&device.public()),
                    "label": "first", "clock": [NOW, 0] }),
            &[(device, Signer::Device(device.device()), 0)],
            naming,
        )
        .unwrap()
    }

    #[test]
    fn a_staged_head_is_sealed_by_its_new_code_and_escrows_the_naming_key() {
        let a = Keypair::from_seed(&[1; 32]);
        let naming = [9u8; 32];
        let s = first(&a, &naming);
        let genesis = open(&s.record);
        let escrow = open(&s.escrow);
        let mut records = BTreeMap::new();
        records.insert(s.record.1, genesis);
        records.insert(s.escrow.1, escrow);
        // The code opens the head it sealed, and the head's escrow gives the key.
        let (head, keys) = find_head(&s.code, &LIB, &records, &BTreeMap::new()).unwrap();
        assert_eq!(head, s.record.1);
        assert!(records[&head].signed_by(&Signer::Code, &keys.signing.public(), 0));
        assert_eq!(naming_key(&keys, &LIB, &head, &records), Some(naming));
        // Another code opens nothing.
        let other = RecoveryCode::generate().unwrap();
        assert!(find_head(&other, &LIB, &records, &BTreeMap::new()).is_none());
    }

    #[test]
    fn a_strangers_escrow_doesnt_replace_the_readers() {
        let a = Keypair::from_seed(&[1; 32]);
        let s = first(&a, &[9; 32]);
        let mut records = BTreeMap::new();
        records.insert(s.record.1, open(&s.record));
        // A stranger, holding no code, writes an escrow for the same head with a
        // key of their own; another escrow is signed by the code but doesn't open.
        let stranger = Keypair::from_seed(&[66; 32]);
        let forged = trust::sign(
            "escrow",
            &LIB,
            json!({ "head": s.record.1.to_string(), "nonce": b64url_encode(&[0; 24]), "ct": b64url_encode(&[1; 48]) }),
            &[(&stranger, Signer::Code, 0)],
        );
        records.insert(forged.1, open(&forged));
        let (head, keys) = find_head(&s.code, &LIB, &records, &BTreeMap::new()).unwrap();
        let junk = trust::sign(
            "escrow",
            &LIB,
            json!({ "head": head.to_string(), "nonce": b64url_encode(&[0; 24]), "ct": b64url_encode(&[1; 48]) }),
            &[(&keys.signing, Signer::Code, 0)],
        );
        records.insert(junk.1, open(&junk));
        assert_eq!(naming_key(&keys, &LIB, &head, &records), None);
        // The reader's own escrow still opens beside them.
        records.insert(s.escrow.1, open(&s.escrow));
        assert_eq!(naming_key(&keys, &LIB, &head, &records), Some([9; 32]));
    }

    #[test]
    fn the_reader_confirms_a_code_by_its_last_group() {
        let code = RecoveryCode::generate().unwrap();
        let shown = code.display();
        let last = shown.rsplit(' ').next().unwrap();
        assert!(confirms_last_group(&code, last));
        assert!(confirms_last_group(
            &code,
            &format!(" {} ", last.to_lowercase())
        ));
        assert!(!confirms_last_group(&code, ""));
        assert!(!confirms_last_group(
            &code,
            shown.split(' ').next().unwrap()
        ));
    }

    #[test]
    fn an_enrollment_by_code_is_signed_by_the_code_and_the_new_key() {
        let a = Keypair::from_seed(&[1; 32]);
        let s = first(&a, &[9; 32]);
        let mut records = BTreeMap::new();
        records.insert(s.record.1, open(&s.record));
        let (head, keys) = find_head(&s.code, &LIB, &records, &BTreeMap::new()).unwrap();
        let joining = Keypair::from_seed(&[2; 32]);
        let e = open(&enroll_by_code(
            &LIB,
            &head,
            &keys,
            &joining,
            "iPad",
            Hlc(NOW, 1),
        ));
        assert!(matches!(&e.body, Body::Enroll { by: None, prev, .. } if *prev == head));
        assert!(e.signed_by(&Signer::Code, &keys.signing.public(), 0));
        assert!(e.signed_by(&Signer::Device(joining.device()), &joining.public(), 0));
    }
}
