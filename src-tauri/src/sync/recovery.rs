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
use super::ids::{b64url_encode, DeviceId, Hash32, Id16};
use super::membership::Membership;
use super::trust::{self, Body, Mark, Record};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// SP22 (prov.): a device joining by code tries at most this many sealed heads.
pub const MAX_CODE_TRIES: usize = 64;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

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

/// SP25: a device the reader is asked about (“Keep ‹device›? · Keep · Remove”)
/// before anything is applied: its label, enrollment date and key fingerprint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToDecide {
    pub device: DeviceId,
    pub label: String,
    /// When the record that enrolled it (or rotated it in) was made, by its clock.
    pub enrolled_ms: u64,
    pub fingerprint: String,
}

/// SP25: every device the membership trusts but this one, oldest enrollment
/// first. A device suspended here is asked about too: the reader decides it.
pub fn to_decide(
    membership: &Membership,
    records: &BTreeMap<Hash32, Record>,
    me: &DeviceId,
) -> Vec<ToDecide> {
    let st = &membership.state;
    let mut out: Vec<ToDecide> = st
        .members
        .values()
        .filter(|m| m.device != *me && st.trusted(&m.device).is_some())
        .map(|m| ToDecide {
            device: m.device,
            label: m.label.clone(),
            enrolled_ms: match records.get(&m.record).map(|r| &r.body) {
                Some(
                    Body::Genesis { clock, .. }
                    | Body::Enroll { clock, .. }
                    | Body::Rotate { clock, .. },
                ) => clock.ms(),
                _ => 0,
            },
            fingerprint: crypto::fingerprint(&m.key),
        })
        .collect();
    out.sort_by(|a, b| {
        a.enrolled_ms
            .cmp(&b.enrolled_ms)
            .then(a.device.cmp(&b.device))
    });
    out
}

/// SP25: a removed device's cutoff when joining by code: the latest change of it
/// that every other enrolled device's frontier includes (`vouched`, what each
/// device file vouches for, SP29), the joining device not counted among them since
/// it has applied nothing. With no other enrolled device, its whole verified
/// history (`latest`, the latest change of it this device holds and has checked).
/// `None` when nothing of it stands. `held` names the device's change at a
/// sequence.
pub fn recovery_cutoff(
    removed: &DeviceId,
    me: &DeviceId,
    enrolled: &BTreeSet<DeviceId>,
    vouched: &BTreeMap<DeviceId, BTreeMap<DeviceId, u64>>,
    latest: u64,
    held: &dyn Fn(u64) -> Option<Hash32>,
) -> Option<Mark> {
    let others: Vec<&DeviceId> = enrolled
        .iter()
        .filter(|e| *e != removed && *e != me)
        .collect();
    let s = if others.is_empty() {
        latest
    } else {
        others
            .iter()
            .map(|e| {
                vouched
                    .get(*e)
                    .and_then(|v| v.get(removed))
                    .copied()
                    .unwrap_or(0)
            })
            .min()
            .unwrap_or(0)
    };
    (s > 0).then(|| held(s).map(|h| Mark { s, h })).flatten()
}

/// SP25, SP23: the reader's Removes, as one `remove` on `prev` sealed with a new
/// code: signed by the joining device and the code entered (`code`), each device
/// with its cutoff, no view, and its cascade (SP26). Written as `stage` says: the
/// escrow, then the record.
#[allow(clippy::too_many_arguments)]
pub fn stage_removal(
    library: &Id16,
    prev: &Hash32,
    removes: &[(DeviceId, Option<Mark>)],
    device: &Keypair,
    code: &CodeKeys,
    naming_key: &[u8; 32],
    clock: Hlc,
) -> Result<Staged, CryptoError> {
    let removes: Vec<Value> = removes
        .iter()
        .map(|(d, c)| {
            json!({ "device": d.to_string(),
                    "cutoff": c.map(|m| json!({ "s": m.s, "h": m.h.to_string() })),
                    "view": null, "cascade": true })
        })
        .collect();
    stage(
        "remove",
        library,
        json!({ "prev": prev.to_string(), "removes": removes, "clock": [clock.ms(), clock.n()] }),
        &[
            (device, Signer::Device(device.device()), 0),
            (&code.signing, Signer::Code, 0),
        ],
        naming_key,
    )
}

/// SP23: escrows naming a head that never appeared, first seen over a day ago:
/// they count for nothing, and any device removes them.
pub fn stale_escrows(
    records: &BTreeMap<Hash32, Record>,
    first_seen: &BTreeMap<Hash32, i64>,
    now_ms: i64,
) -> Vec<Hash32> {
    records
        .iter()
        .filter(|(h, r)| {
            matches!(&r.body, Body::Escrow { head, .. } if !records.contains_key(head))
                && first_seen.get(*h).is_some_and(|t| *t < now_ms - DAY_MS)
        })
        .map(|(h, _)| *h)
        .collect()
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

    /// A's library joined by B with the code: A's genesis (sealed by code key
    /// `c`), and B's enrollment by code on it, confirmed.
    fn joined() -> (BTreeMap<Hash32, Record>, Hash32, Keypair, Keypair, CodeKeys) {
        let (a, b) = (Keypair::from_seed(&[1; 32]), Keypair::from_seed(&[2; 32]));
        let keys = crypto::code_keys(&[3; 32]);
        let mut records = BTreeMap::new();
        let g = trust::sign(
            "genesis",
            &LIB,
            json!({ "device": a.device().to_string(), "key": b64url_encode(&a.public()), "label": "Mac",
                    "clock": [NOW, 0], "code": { "nonce": b64url_encode(&[5; 16]),
                    "codeKey": b64url_encode(&keys.signing.public()) } }),
            &[
                (&a, Signer::Device(a.device()), 0),
                (&keys.signing, Signer::Code, 0),
            ],
        );
        records.insert(g.1, open(&g));
        let e = enroll_by_code(&LIB, &g.1, &keys, &b, "iPad", Hlc(NOW + 5, 1));
        records.insert(e.1, open(&e));
        (records, g.1, a, b, keys)
    }

    fn membership_of(
        records: &BTreeMap<Hash32, Record>,
        genesis: Hash32,
        me: DeviceId,
    ) -> Membership {
        let none = BTreeSet::new();
        let confirmed: BTreeSet<Hash32> = records
            .iter()
            .filter(|(_, r)| matches!(r.body, Body::Enroll { by: None, .. }))
            .map(|(h, _)| *h)
            .collect();
        crate::sync::membership::compute(&crate::sync::membership::Inputs {
            genesis,
            records,
            first_seen: &BTreeMap::new(),
            confirmed: &confirmed,
            checkpoint: &none,
            held: &|_, _| None,
            accepted: &none,
            now_ms: NOW as i64,
            me: Some(me),
            quarantined: &BTreeMap::new(),
            heads: &BTreeMap::new(),
        })
        .unwrap()
    }

    #[test]
    fn the_reader_is_asked_about_every_trusted_device_but_this_one() {
        let (records, g, a, b, _) = joined();
        let m = membership_of(&records, g, b.device());
        let list = to_decide(&m, &records, &b.device());
        assert_eq!(
            list,
            vec![ToDecide {
                device: a.device(),
                label: "Mac".into(),
                enrolled_ms: NOW,
                fingerprint: crypto::fingerprint(&a.public()),
            }]
        );
    }

    #[test]
    fn a_removal_by_the_joining_device_and_the_code_counts() {
        let (mut records, g, a, b, keys) = joined();
        let cut = Mark {
            s: 4,
            h: Hash32([4; 32]),
        };
        // Built on B's own enrollment, where B is a member; sealed by the code it
        // entered, the genesis's.
        let joined_at = *records
            .iter()
            .find(|(_, r)| matches!(r.body, Body::Enroll { .. }))
            .unwrap()
            .0;
        let staged = stage_removal(
            &LIB,
            &joined_at,
            &[(a.device(), Some(cut))],
            &b,
            &keys,
            &[9; 32],
            Hlc(NOW + 9, 0),
        )
        .unwrap();
        let rm = open(&staged.record);
        assert!(matches!(&rm.body, Body::Remove { removes, .. }
            if removes.len() == 1 && removes[0].device == a.device() && removes[0].cutoff == Some(cut)));
        records.insert(staged.record.1, rm);
        let m = membership_of(&records, g, b.device());
        assert!(m.trusted(&a.device()).is_none());
        assert!(m.trusted(&b.device()).is_some());
        assert_eq!(m.state.cutoffs[&a.device()], Some(cut));
        // Its escrow names the new head, so it isn't stale.
        records.insert(staged.escrow.1, open(&staged.escrow));
        let day = 24 * 60 * 60 * 1000;
        let seen: BTreeMap<Hash32, i64> = [(staged.escrow.1, NOW as i64 - 2 * day)]
            .into_iter()
            .collect();
        assert!(stale_escrows(&records, &seen, NOW as i64).is_empty());
        // Without its head, a day after it was first seen, it is.
        records.remove(&staged.record.1);
        assert_eq!(
            stale_escrows(&records, &seen, NOW as i64),
            vec![staged.escrow.1]
        );
        let fresh: BTreeMap<Hash32, i64> = [(staged.escrow.1, NOW as i64 - day / 2)]
            .into_iter()
            .collect();
        assert!(stale_escrows(&records, &fresh, NOW as i64).is_empty());
    }

    #[test]
    fn a_removed_devices_cutoff_is_what_every_other_device_included() {
        let id = |n: u8| Id16([n; 16]);
        let (d, me, e, f) = (id(1), id(2), id(3), id(4));
        let held = |s: u64| (s <= 9).then_some(Hash32([s as u8; 32]));
        let enrolled: BTreeSet<DeviceId> = [d, me, e, f].into_iter().collect();
        let mut vouched: BTreeMap<DeviceId, BTreeMap<DeviceId, u64>> = BTreeMap::new();
        vouched.insert(e, [(d, 7)].into_iter().collect());
        vouched.insert(f, [(d, 5)].into_iter().collect());
        // The joining device, which has applied nothing, isn't waited for.
        assert_eq!(
            recovery_cutoff(&d, &me, &enrolled, &vouched, 9, &held),
            Some(Mark {
                s: 5,
                h: Hash32([5; 32])
            })
        );
        // A device whose file names none of it: nothing of it stands.
        vouched.insert(f, BTreeMap::new());
        assert_eq!(
            recovery_cutoff(&d, &me, &enrolled, &vouched, 9, &held),
            None
        );
        // No other enrolled device: its whole verified history.
        let two: BTreeSet<DeviceId> = [d, me].into_iter().collect();
        assert_eq!(
            recovery_cutoff(&d, &me, &two, &vouched, 9, &held),
            Some(Mark {
                s: 9,
                h: Hash32([9; 32])
            })
        );
    }
}
