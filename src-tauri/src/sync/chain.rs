//! SP14, SP15: a device's chain of change files, signed, and published so that a
//! crash never makes it sign two files for one sequence.
//!
//! A change file is `changes/‹device›/‹sequence›.json`, signed by its device with
//! its sequence, `prev` (the previous file's payload hash) and `vroot` in the
//! signature (SP6), so a chain can be followed link by link and the versions it
//! held checked from its signatures alone. Its payload is written in RFC 8785's
//! canonical form: the rows `journal::take_unsent` fixed always sign to the same
//! bytes, so a change numbered before a crash is signed again identically.

use super::apply::{self, Batch, Received};
use super::crypto::Keypair;
use super::device_store::{DeviceStore, Keystore, StoreError, Unsent};
use super::envelope::{self, EnvelopeError, Place, Signer};
use super::ids::{b64url_encode, DeviceId, Hash32, Id16};
use super::journal::{self, JournalError, Library};
use super::json::{self, MAX_SAFE_INTEGER};
use super::record::version_hash;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashSet;

/// SP8: a change file is at most 8 MB.
pub const MAX_CHANGE_BYTES: usize = 8 << 20;

/// SP8: the versions one change takes, in their written form. The payload goes in
/// the envelope as base64url, 4/3 of its size, so 5 MiB of versions, with the
/// payload's other fields and the signature, stays under `MAX_CHANGE_BYTES`; a
/// larger backlog becomes several changes.
pub const MAX_VERSION_BYTES: usize = 5 << 20;

/// SP8: a change file's path; the sequence is 12 decimal digits, zero-padded.
pub fn change_path(device: &DeviceId, seq: u64) -> String {
    format!("changes/{device}/{seq:012}.json")
}

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    #[error("larger than SP8's 8 MB")]
    TooLarge,
    #[error("not SP14's change payload")]
    Shape,
    #[error("a change format this Linen doesn't know")]
    Format,
    #[error("written by another device than the one expected")]
    Device,
    #[error("a version of one key twice")]
    DuplicateKey,
    #[error("not signed by its device for its place")]
    Signature,
    #[error("doesn't follow the chain this device holds")]
    Link,
    #[error("the device store has no identity for this library and device")]
    NoIdentity,
}

type Result<T> = std::result::Result<T, ChainError>;

/// SP6, SP17: the version list a change's `vroot` covers, in the file's order.
fn vroot_of(versions: &[Value]) -> Hash32 {
    let entries: Vec<[u8; 48]> = versions
        .iter()
        .map(|v| envelope::vlist_entry(v["key"].as_str().unwrap_or(""), &version_hash(v)))
        .collect();
    envelope::vroot(&entries)
}

/// SP14: the payload, in canonical form.
fn payload(
    library: &Id16,
    device: &DeviceId,
    writer: &Id16,
    seq: u64,
    prev: Option<&Hash32>,
    versions: &[Value],
) -> Vec<u8> {
    json::canonical(&json!({
        "type": "change",
        "format": 1,
        "library": library.to_string(),
        "device": device.to_string(),
        "writer": writer.to_string(),
        "seq": seq,
        "prev": prev.map(|h| h.to_string()),
        "versions": versions,
        // Reservations and holds (SP33, SP34) come with the rest of Stage 1c.
        "reserve": [],
        "hold": [],
    }))
}

/// SP14: one signed change file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    pub seq: u64,
    /// Its payload hash (SP5).
    pub hash: Hash32,
    pub path: String,
    pub bytes: Vec<u8>,
}

/// SP14, SP6: signs the change `seq` of `versions`, after `prev`.
pub fn sign_change(
    key: &Keypair,
    library: &Id16,
    writer: &Id16,
    seq: u64,
    prev: Option<Hash32>,
    versions: &[Value],
) -> Signed {
    let device = key.device();
    let body = payload(library, &device, writer, seq, prev.as_ref(), versions);
    let place = Place {
        seq,
        prev,
        vroot: Some(vroot_of(versions)),
    };
    let sig = envelope::sign(
        key,
        Signer::Device(device),
        "change",
        library,
        &place,
        &body,
    );
    Signed {
        seq,
        hash: super::crypto::payload_hash(&body),
        path: change_path(&device, seq),
        bytes: envelope::write("change", &body, &[sig]),
    }
}

/// SP15: this device's next change, numbered, signed, and kept in the device store
/// (step 3) before it is returned for upload (step 4). Changes already signed and
/// not yet seen in storage are uploaded again from `pending`, never signed again.
/// `None` when nothing is waiting to be published.
pub fn publish(
    conn: &Connection,
    lib: &Library,
    ks: &dyn Keystore,
    store: &mut DeviceStore,
) -> Result<Option<Signed>> {
    publish_within(conn, lib, ks, store, MAX_VERSION_BYTES)
}

fn publish_within(
    conn: &Connection,
    lib: &Library,
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    max_bytes: usize,
) -> Result<Option<Signed>> {
    let state = store.library(&lib.id).ok_or(ChainError::NoIdentity)?;
    if state.device_id()? != lib.device {
        return Err(ChainError::NoIdentity);
    }
    let key = state.key()?;
    let writer = state.writer_id()?;
    let (last, last_hash) = (state.seq, state.last_hash()?);
    // SP14: a chain head is a sequence with its hash, or nothing before the first;
    // anything else would sign a `prev` no peer accepts, after the numbering had
    // already used the sequence. Checked before anything is numbered.
    if (last == 0) != last_hash.is_none() || last >= MAX_SAFE_INTEGER {
        return Err(ChainError::Store(StoreError::Unreadable));
    }
    // Step 2: number (the journal fixes the rows) and sign.
    let Some((seq, versions)) = journal::take_unsent_within(conn, lib, last, max_bytes)? else {
        return Ok(None);
    };
    debug_assert_eq!(seq, last + 1);
    let raws: Vec<Value> = versions.iter().map(|v| v.raw.clone()).collect();
    let signed = sign_change(&key, &lib.id, &writer, seq, last_hash, &raws);
    // SP13: covers name this change by its payload hash.
    conn.execute(
        "INSERT OR REPLACE INTO sync_changes (library, device, seq, hash) VALUES (?1, ?2, ?3, ?4)",
        params![
            lib.id.to_string(),
            lib.device.to_string(),
            seq as i64,
            signed.hash.to_string()
        ],
    )
    .map_err(JournalError::from)?;
    // Step 3: the sequence, hash and signed bytes, saved before anything is uploaded.
    let state = store.library_mut(&lib.id).expect("checked above");
    state.seq = seq;
    state.hash = Some(signed.hash.to_string());
    state.unsent.push(Unsent {
        path: signed.path.clone(),
        bytes: b64url_encode(&signed.bytes),
    });
    store.save(ks)?;
    Ok(Some(signed))
}

/// SP15: what this device signed and hasn't yet seen in storage, to upload again,
/// oldest first. Bytes that don't decode are an error, never skipped: their
/// sequence is used, so nothing else would ever publish it.
pub fn pending(store: &DeviceStore, lib: &Id16) -> Result<Vec<(String, Vec<u8>)>> {
    let Some(state) = store.library(lib) else {
        return Ok(Vec::new());
    };
    state
        .unsent
        .iter()
        .map(|u| {
            u.bytes()
                .map(|b| (u.path.clone(), b))
                .ok_or(ChainError::Store(StoreError::Unreadable))
        })
        .collect()
}

/// SP15: storage holds `path` now; its bytes leave the device store.
pub fn uploaded(ks: &dyn Keystore, store: &mut DeviceStore, lib: &Id16, path: &str) -> Result<()> {
    if let Some(state) = store.library_mut(lib) {
        let before = state.unsent.len();
        state.unsent.retain(|u| u.path != path);
        if state.unsent.len() != before {
            store.save(ks)?;
        }
    }
    Ok(())
}

/// SP14: a change file, opened and checked against its device's key.
#[derive(Clone, Debug)]
pub struct Change {
    pub device: DeviceId,
    pub writer: Id16,
    pub seq: u64,
    pub prev: Option<Hash32>,
    pub hash: Hash32,
    pub vroot: Hash32,
    pub versions: Vec<Value>,
    pub reserve: Vec<Value>,
    pub hold: Vec<Value>,
    /// The device's signature, which frontier entries for this change carry (SP16).
    pub sig: [u8; 64],
}

fn hash_field(v: &Value) -> Option<Option<Hash32>> {
    match v {
        Value::Null => Some(None),
        Value::String(s) => Hash32::parse(s).map(Some),
        _ => None,
    }
}

/// SP14, SP28: opens `device`'s change file. Its payload must name that device and
/// a sequence of 1 or more, with `prev` null exactly for the first; each version
/// must be the device's own, one per key; and the device's signature must cover
/// the sequence, `prev` and the versions (`vroot`). Whether it follows the chain
/// this device holds is `receive_change`'s check.
pub fn open_change(
    bytes: &[u8],
    library: &Id16,
    device: &DeviceId,
    key: &[u8; 32],
) -> Result<Change> {
    if bytes.len() > MAX_CHANGE_BYTES {
        return Err(ChainError::TooLarge);
    }
    let e = envelope::open(bytes, "change", library)?;
    let b = &e.body;
    match b.get("format").and_then(Value::as_u64) {
        Some(1) => {}
        Some(_) => return Err(ChainError::Format),
        None => return Err(ChainError::Shape),
    }
    if b.get("device").and_then(Value::as_str) != Some(&device.to_string()) {
        return Err(ChainError::Device);
    }
    let writer = b
        .get("writer")
        .and_then(Value::as_str)
        .and_then(Id16::parse)
        .ok_or(ChainError::Shape)?;
    let seq = b
        .get("seq")
        .and_then(Value::as_u64)
        .filter(|s| (1..=MAX_SAFE_INTEGER).contains(s))
        .ok_or(ChainError::Shape)?;
    let prev = b
        .get("prev")
        .and_then(hash_field)
        .ok_or(ChainError::Shape)?;
    if prev.is_none() != (seq == 1) {
        return Err(ChainError::Shape);
    }
    let list = |name: &str| -> Result<Vec<Value>> {
        b.get(name)
            .and_then(Value::as_array)
            .cloned()
            .ok_or(ChainError::Shape)
    };
    let (versions, reserve, hold) = (list("versions")?, list("reserve")?, list("hold")?);
    let mut keys = HashSet::new();
    for v in &versions {
        if !v.is_object() {
            return Err(ChainError::Shape);
        }
        // SP11: a version whose device isn't its change file's is refused.
        if v.get("device").and_then(Value::as_str) != Some(&device.to_string()) {
            return Err(ChainError::Device);
        }
        let k = v
            .get("key")
            .and_then(Value::as_str)
            .ok_or(ChainError::Shape)?;
        if !keys.insert(k.to_owned()) {
            return Err(ChainError::DuplicateKey);
        }
    }
    // A change has one signer, its device.
    if e.sigs.len() != 1 {
        return Err(ChainError::Signature);
    }
    let vroot = vroot_of(&versions);
    let place = Place {
        seq,
        prev,
        vroot: Some(vroot),
    };
    let by = Signer::Device(*device);
    if !e.verified(&by, key, library, &place) {
        return Err(ChainError::Signature);
    }
    Ok(Change {
        device: *device,
        writer,
        seq,
        prev,
        hash: e.hash,
        vroot,
        versions,
        reserve,
        hold,
        sig: e.sigs[0].sig,
    })
}

/// SP14: a peer's change file, checked and, when it is the next in the chain this
/// device holds, received (SY9–SY13). One beyond a gap waits; one that doesn't
/// follow the chain held, or differs from one already held at its sequence, is
/// refused. `bodies` are the blobs its versions name, as fetched.
pub fn receive_change(
    conn: &Connection,
    lib: &Library,
    bytes: &[u8],
    device: &DeviceId,
    key: &[u8; 32],
    bodies: Vec<Vec<u8>>,
    now_ms: i64,
) -> Result<Received> {
    let c = open_change(bytes, &lib.id, device, key)?;
    let held = |seq: u64| -> Result<Option<Hash32>> {
        let h: Option<String> = conn
            .query_row(
                "SELECT hash FROM sync_changes WHERE library = ?1 AND device = ?2 AND seq = ?3",
                params![lib.id.to_string(), device.to_string(), seq as i64],
                |r| r.get(0),
            )
            .optional()
            .map_err(JournalError::from)?;
        Ok(h.as_deref().and_then(Hash32::parse))
    };
    let have = apply::seen(conn, lib, *device)?;
    if c.seq <= have {
        // Already held: the same file again is a repeat; another for that sequence
        // isn't this device's chain (SP30 makes a proof of it).
        return match held(c.seq)? {
            Some(h) if h == c.hash => Ok(Received::Repeat),
            _ => Err(ChainError::Link),
        };
    }
    if c.seq > have + 1 {
        return Ok(Received::Waits);
    }
    let expected = if have == 0 { None } else { held(have)? };
    if have > 0 && expected.is_none() {
        // The chain held has no hash for its last change: nothing can be linked.
        return Err(ChainError::Link);
    }
    if c.prev != expected {
        return Err(ChainError::Link);
    }
    let batch = Batch {
        device: c.device,
        seq: c.seq,
        hash: c.hash,
        versions: c.versions,
        bodies,
    };
    Ok(apply::receive(conn, lib, &batch, now_ms)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::apply::tests::{device_with, highlight, note_of};
    use crate::sync::device_store::Memory;

    /// A device whose ID its key names, with its device store.
    fn signed_device(book: &str) -> (Store, Library, Memory, DeviceStore) {
        let ks = Memory::default();
        let mut store = DeviceStore::default();
        let lib_id = crate::sync::apply::tests::LIB;
        let id = store.identity(&lib_id).unwrap().device_id().unwrap();
        store.save(&ks).unwrap();
        let (s, lib) = device_with(id, book);
        (s, lib, ks, store)
    }

    fn key_of(store: &DeviceStore, lib: &Library) -> [u8; 32] {
        store.library(&lib.id).unwrap().key().unwrap().public()
    }

    fn bodies(s: &Store, c: &Signed, lib: &Library) -> Vec<Vec<u8>> {
        let ch = open_change(&c.bytes, &lib.id, &lib.device, &[0; 32]);
        // Opened only to read its versions: the key check is the receiver's.
        let versions = match ch {
            Err(ChainError::Signature) => {
                let e = envelope::open(&c.bytes, "change", &lib.id).unwrap();
                e.body["versions"].as_array().unwrap().clone()
            }
            other => other.unwrap().versions,
        };
        versions
            .iter()
            .filter_map(|v| v["body"]["hash"].as_str())
            .filter_map(|h| journal::body(s.conn(), &Hash32::parse(h).unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn a_signed_change_arrives_and_its_chain_links() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let (b, lb, _kb, _sb) = signed_device("book-b");
        let now = crate::store::now_ms();
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        assert_eq!(one.seq, 1);
        assert_eq!(one.path, change_path(&la.device, 1));
        let key = key_of(&sa, &la);
        let got = receive_change(
            b.conn(),
            &lb,
            &one.bytes,
            &la.device,
            &key,
            bodies(&a, &one, &la),
            now,
        )
        .unwrap();
        assert!(matches!(got, Received::Applied { .. }), "{got:?}");
        assert_eq!(note_of(&b, "book-b"), Some(Some("first".into())));
        a.save_annotation(&highlight("book-a", Some("second"), 1_760_000_002_000))
            .unwrap();
        let two = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        let opened = open_change(&two.bytes, &la.id, &la.device, &key).unwrap();
        assert_eq!(opened.prev, Some(one.hash));
        receive_change(
            b.conn(),
            &lb,
            &two.bytes,
            &la.device,
            &key,
            bodies(&a, &two, &la),
            now,
        )
        .unwrap();
        assert_eq!(note_of(&b, "book-b"), Some(Some("second".into())));
        // The same file again is a repeat.
        assert_eq!(
            receive_change(b.conn(), &lb, &two.bytes, &la.device, &key, vec![], now).unwrap(),
            Received::Repeat
        );
    }

    #[test]
    fn a_change_that_doesnt_follow_the_chain_held_is_refused() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let (b, lb, _kb, _sb) = signed_device("book-b");
        let now = crate::store::now_ms();
        let key = key_of(&sa, &la);
        let akey = sa.library(&la.id).unwrap().key().unwrap();
        let writer = sa.library(&la.id).unwrap().writer_id().unwrap();
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        receive_change(
            b.conn(),
            &lb,
            &one.bytes,
            &la.device,
            &key,
            bodies(&a, &one, &la),
            now,
        )
        .unwrap();
        // Signed by A's key, but after another first change: refused.
        let forked = sign_change(&akey, &la.id, &writer, 2, Some(Hash32([7; 32])), &[]);
        assert!(matches!(
            receive_change(b.conn(), &lb, &forked.bytes, &la.device, &key, vec![], now),
            Err(ChainError::Link)
        ));
        // Another file for a sequence already held: refused, not a repeat.
        let other_one = sign_change(&akey, &la.id, &writer, 1, None, &[]);
        assert!(matches!(
            receive_change(
                b.conn(),
                &lb,
                &other_one.bytes,
                &la.device,
                &key,
                vec![],
                now
            ),
            Err(ChainError::Link)
        ));
        // One beyond a gap waits.
        let three = sign_change(&akey, &la.id, &writer, 3, Some(Hash32([7; 32])), &[]);
        assert_eq!(
            receive_change(b.conn(), &lb, &three.bytes, &la.device, &key, vec![], now).unwrap(),
            Received::Waits
        );
    }

    #[test]
    fn a_change_is_checked_against_its_device_and_its_signature() {
        let (_a, la, _ka, sa) = signed_device("book-a");
        let akey = sa.library(&la.id).unwrap().key().unwrap();
        let writer = sa.library(&la.id).unwrap().writer_id().unwrap();
        let other = Keypair::from_seed(&[4; 32]);
        let v = |device: &DeviceId, key: &str| json!({ "format": 1, "key": key, "device": device.to_string(), "clock": [1, 0], "history": [], "deleted": { "covers": [] } });
        let good = sign_change(&akey, &la.id, &writer, 1, None, &[v(&la.device, "note:x")]);
        assert!(open_change(&good.bytes, &la.id, &la.device, &akey.public()).is_ok());
        // Another device's key, or read as another device's file.
        assert!(matches!(
            open_change(&good.bytes, &la.id, &la.device, &other.public()),
            Err(ChainError::Signature)
        ));
        assert!(matches!(
            open_change(&good.bytes, &la.id, &other.device(), &other.public()),
            Err(ChainError::Device)
        ));
        // A version written by another device, or two versions of one key.
        let foreign = sign_change(
            &akey,
            &la.id,
            &writer,
            1,
            None,
            &[v(&other.device(), "note:x")],
        );
        assert!(matches!(
            open_change(&foreign.bytes, &la.id, &la.device, &akey.public()),
            Err(ChainError::Device)
        ));
        let twice = sign_change(
            &akey,
            &la.id,
            &writer,
            1,
            None,
            &[v(&la.device, "note:x"), v(&la.device, "note:x")],
        );
        assert!(matches!(
            open_change(&twice.bytes, &la.id, &la.device, &akey.public()),
            Err(ChainError::DuplicateKey)
        ));
        // A first change with a `prev`, or a later one without.
        let first_with_prev = sign_change(&akey, &la.id, &writer, 1, Some(Hash32([1; 32])), &[]);
        assert!(matches!(
            open_change(&first_with_prev.bytes, &la.id, &la.device, &akey.public()),
            Err(ChainError::Shape)
        ));
        let later_without = sign_change(&akey, &la.id, &writer, 2, None, &[]);
        assert!(matches!(
            open_change(&later_without.bytes, &la.id, &la.device, &akey.public()),
            Err(ChainError::Shape)
        ));
        // The versions are covered by the signature: one swapped in is refused.
        let e: Value = serde_json::from_slice(&good.bytes).unwrap();
        let mut p: Value = serde_json::from_slice(
            &crate::sync::ids::b64url_decode(e["payload"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        p["versions"][0]["key"] = json!("note:y");
        let mut swapped = e.clone();
        swapped["payload"] = json!(b64url_encode(&json::canonical(&p)));
        assert!(matches!(
            open_change(
                swapped.to_string().as_bytes(),
                &la.id,
                &la.device,
                &akey.public()
            ),
            Err(ChainError::Signature)
        ));
        // Larger than SP8 allows: refused before it is read.
        assert!(matches!(
            open_change(
                &vec![b' '; MAX_CHANGE_BYTES + 1],
                &la.id,
                &la.device,
                &akey.public()
            ),
            Err(ChainError::TooLarge)
        ));
    }

    #[test]
    fn a_crash_before_upload_uploads_the_same_bytes_and_signs_nothing_twice() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        // Linen stops before the upload: what was saved in step 3 comes back.
        let reloaded = DeviceStore::load(&ka).unwrap();
        assert_eq!(
            pending(&reloaded, &la.id).unwrap(),
            vec![(one.path.clone(), one.bytes.clone())]
        );
        assert_eq!(reloaded.library(&la.id).unwrap().seq, 1);
        // Nothing new: nothing is signed, whatever is pending.
        let mut store = reloaded;
        assert_eq!(publish(a.conn(), &la, &ka, &mut store).unwrap(), None);
        uploaded(&ka, &mut store, &la.id, &one.path).unwrap();
        assert!(pending(&DeviceStore::load(&ka).unwrap(), &la.id)
            .unwrap()
            .is_empty());
        // A queued change whose bytes don't decode is an error, not skipped.
        let mut broken = DeviceStore::load(&ka).unwrap();
        broken.library_mut(&la.id).unwrap().unsent.push(Unsent {
            path: change_path(&la.device, 2),
            bytes: "not base64url!".into(),
        });
        assert!(matches!(
            pending(&broken, &la.id),
            Err(ChainError::Store(StoreError::Unreadable))
        ));
    }

    #[test]
    fn a_crash_after_numbering_signs_the_same_file_again() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let before = sa.clone();
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        // The journal kept the numbering, but the device store didn't (Linen stopped
        // between the two): the same rows are signed again, to the same bytes.
        let mut store = before;
        store.save(&ka).unwrap();
        let again = publish(a.conn(), &la, &ka, &mut store).unwrap().unwrap();
        assert_eq!(again, one);
    }

    #[test]
    fn a_backlog_becomes_several_changes_each_within_the_limit() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let (b, lb, _kb, _sb) = signed_device("book-b");
        let now = crate::store::now_ms();
        let key = key_of(&sa, &la);
        // A highlight with a note, then deleted: its tombstone and its note's must
        // go in one change (SP13). Then places for many books, a backlog.
        a.save_annotation(&highlight("book-a", Some("the note"), 1_760_000_001_000))
            .unwrap();
        let first = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        receive_change(
            b.conn(),
            &lb,
            &first.bytes,
            &la.device,
            &key,
            bodies(&a, &first, &la),
            now,
        )
        .unwrap();
        a.delete_annotation(crate::sync::apply::tests::ANNO)
            .unwrap();
        for i in 0..20 {
            journal::record_local(
                a.conn(),
                &la,
                &crate::sync::record::Key::parse(&format!("place:00000000-0000-4000-8000-{i:012}"))
                    .unwrap()
                    .0,
                journal::LocalContent::Body(
                    br#"{"cfi":"/6/4","fraction":0.5,"chapter":null}"#.to_vec(),
                ),
                1_760_000_002_000,
            )
            .unwrap();
        }
        // A bound so small each change takes one group: the two tombstones, then
        // one place a change.
        let mut changes = Vec::new();
        while let Some(c) = publish_within(a.conn(), &la, &ka, &mut sa, 1).unwrap() {
            changes.push(c);
        }
        assert_eq!(changes.len(), 21);
        let mut keys = Vec::new();
        for c in &changes {
            let opened = open_change(&c.bytes, &la.id, &la.device, &key).unwrap();
            let ks: Vec<String> = opened
                .versions
                .iter()
                .map(|v| v["key"].as_str().unwrap().to_owned())
                .collect();
            // The highlight's tombstone and its note's are in the same change.
            assert_eq!(
                ks.iter().any(|k| k.starts_with("annotation:")),
                ks.iter().any(|k| k.starts_with("note:")),
                "{ks:?}"
            );
            keys.extend(ks);
            receive_change(b.conn(), &lb, &c.bytes, &la.device, &key, vec![], now).unwrap();
        }
        // Every version went out once, in order, and B applied the deletion.
        assert_eq!(keys.len(), 22);
        assert_eq!(keys.iter().collect::<HashSet<_>>().len(), 22);
        assert!(b.annotations("book-b").unwrap().is_empty());
    }

    #[test]
    fn the_version_bound_keeps_a_change_within_sp8s_limit() {
        // The payload's own fields and the envelope around it, measured on an empty
        // change, with room for the longest IDs and sequence a change can carry.
        let key = Keypair::from_seed(&[1; 32]);
        let empty = sign_change(
            &key,
            &Id16([1; 16]),
            &Id16([2; 16]),
            MAX_SAFE_INTEGER,
            Some(Hash32([3; 32])),
            &[],
        );
        let e = envelope::open(&empty.bytes, "change", &Id16([1; 16])).unwrap();
        let payload = e.payload.len() + MAX_VERSION_BYTES;
        let around = empty.bytes.len() - b64url_encode(&e.payload).len();
        assert!(payload.div_ceil(3) * 4 + around <= MAX_CHANGE_BYTES);
    }

    #[test]
    fn an_inconsistent_chain_head_numbers_nothing() {
        let (mut a, la, ka, sa) = signed_device("book-a");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let unsent = || journal::unsent(a.conn(), &la).unwrap().len();
        let before = unsent();
        assert!(before > 0);
        for (seq, hash) in [
            (1, None),
            (0, Some(Hash32([1; 32]).to_string())),
            (MAX_SAFE_INTEGER, Some(Hash32([1; 32]).to_string())),
        ] {
            let mut store = sa.clone();
            let state = store.library_mut(&la.id).unwrap();
            state.seq = seq;
            state.hash = hash;
            assert!(matches!(
                publish(a.conn(), &la, &ka, &mut store),
                Err(ChainError::Store(StoreError::Unreadable))
            ));
            // Nothing was numbered: the versions are still unsent.
            assert_eq!(unsent(), before);
        }
    }

    #[test]
    fn a_store_behind_the_library_publishes_nothing() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let stale = sa.clone();
        publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        a.save_annotation(&highlight("book-a", Some("second"), 1_760_000_002_000))
            .unwrap();
        publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        a.save_annotation(&highlight("book-a", Some("third"), 1_760_000_003_000))
            .unwrap();
        // SY7: the device store went back two changes; the journal knows a later
        // signed change, so nothing is signed for a sequence already used.
        let mut back = stale;
        assert!(matches!(
            publish(a.conn(), &la, &ka, &mut back),
            Err(ChainError::Journal(JournalError::StoreBehind(0)))
        ));
    }
}
