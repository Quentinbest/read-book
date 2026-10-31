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
use super::hold::{self, Hold, Ledger, Reserve};
use super::ids::{b64url_encode, DeviceId, Hash32, Id16};
use super::journal::{self, JournalError, Library};
use super::json::{self, MAX_SAFE_INTEGER};
use super::record::{version_hash, Key};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};

/// SP8: a change file is at most 8 MB.
pub const MAX_CHANGE_BYTES: usize = 8 << 20;

/// SP8: the versions one change takes, in their written form. The payload goes in
/// the envelope as base64url, 4/3 of its size, so 5 MiB of versions, with the
/// payload's other fields and the signature, stays under `MAX_CHANGE_BYTES`; a
/// larger backlog becomes several changes.
pub const MAX_VERSION_BYTES: usize = 5 << 20;

/// The most values (`json::parse_within`) one change file's payload may build, so
/// a file of many tiny values can't exhaust memory; this device's own changes are
/// held to it before they are numbered. PROVISIONAL (decisions.md, “The device
/// file”, (7)).
pub const MAX_CHANGE_VALUES: u64 = 1_000_000;

/// The payload's own fields beyond its versions, generously.
const PAYLOAD_VALUES: u64 = 64;

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
    #[error("a removed device's view cannot be received")]
    Removed,
    #[error("the view generation is older than the floor")]
    StaleView,
    #[error("the device store has no identity for this library and device")]
    NoIdentity,
    #[error(transparent)]
    Hold(#[from] hold::HoldError),
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
#[allow(clippy::too_many_arguments)]
fn payload(
    library: &Id16,
    device: &DeviceId,
    writer: &Id16,
    seq: u64,
    prev: Option<&Hash32>,
    versions: &[Value],
    reserve: &[Reserve],
    hold: &[Hold],
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
        // SP33, SP34: empty lists retain the original signed bytes.
        "reserve": reserve.iter().map(Reserve::write).collect::<Vec<_>>(),
        "hold": hold.iter().map(Hold::write).collect::<Vec<_>>(),
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
    sign_change_with(key, library, writer, seq, prev, versions, &[], &[])
}

/// SP33, SP34: signs the supplied lists along with the versions. Like
/// `sign_change`, this is a low-level signer; publication checks the ledger's
/// limits first, and reception checks the complete change before fetching.
#[allow(clippy::too_many_arguments)]
pub fn sign_change_with(
    key: &Keypair,
    library: &Id16,
    writer: &Id16,
    seq: u64,
    prev: Option<Hash32>,
    versions: &[Value],
    reserve: &[Reserve],
    hold: &[Hold],
) -> Signed {
    let device = key.device();
    let body = payload(
        library,
        &device,
        writer,
        seq,
        prev.as_ref(),
        versions,
        reserve,
        hold,
    );
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
    publish_within(
        conn,
        lib,
        ks,
        store,
        MAX_VERSION_BYTES,
        MAX_CHANGE_VALUES - PAYLOAD_VALUES,
    )
}

fn publish_within(
    conn: &Connection,
    lib: &Library,
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    max_bytes: usize,
    max_values: u64,
) -> Result<Option<Signed>> {
    publish_within_with(
        conn,
        lib,
        ks,
        store,
        max_bytes,
        max_values,
        &[],
        &[],
        &Ledger::default(),
        &BTreeSet::new(),
    )
}

/// SP33, SP34, SP15: publishes lists after checking this device's open ledger.
/// Lists can form a change without versions. The caller keeps copies of the
/// objects and uses `Ledger::may_publish_naming` before a later naming change;
/// syncing and persisting the ledger belong to Stage 2.
/// `withheld` includes new holds' target keys and `Ledger::withheld_keys` for
/// earlier holds, plus keys whose content still needs restoring in storage.
#[allow(clippy::too_many_arguments)]
pub fn publish_with(
    conn: &Connection,
    lib: &Library,
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    reserve: &[Reserve],
    hold: &[Hold],
    ledger: &Ledger,
    withheld: &BTreeSet<Key>,
) -> Result<Option<Signed>> {
    publish_within_with(
        conn,
        lib,
        ks,
        store,
        MAX_VERSION_BYTES,
        MAX_CHANGE_VALUES - PAYLOAD_VALUES,
        reserve,
        hold,
        ledger,
        withheld,
    )
}

#[allow(clippy::too_many_arguments)]
fn publish_within_with(
    conn: &Connection,
    lib: &Library,
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    max_bytes: usize,
    max_values: u64,
    reserve: &[Reserve],
    hold: &[Hold],
    ledger: &Ledger,
    withheld: &BTreeSet<Key>,
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
    // SP15: the journal fixes versions and lists together. A retry returns
    // those exact inputs, ignoring lists and withheld keys passed on this call.
    let Some(numbered) = journal::take_unsent_within_with(
        conn, lib, last, max_bytes, max_values, reserve, hold, withheld, ledger,
    )
    .map_err(|e| match e {
        JournalError::Lists(e) => ChainError::Hold(e),
        e => ChainError::Journal(e),
    })?
    else {
        return Ok(None);
    };
    let seq = numbered.seq;
    debug_assert_eq!(seq, last + 1);
    let raws: Vec<Value> = numbered.versions.iter().map(|v| v.raw.clone()).collect();
    let signed = sign_change_with(
        &key,
        &lib.id,
        &writer,
        seq,
        last_hash,
        &raws,
        &numbered.reserve,
        &numbered.hold,
    );
    if signed.bytes.len() > MAX_CHANGE_BYTES {
        return Err(ChainError::TooLarge);
    }
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
    // The store changes only once the save succeeds: after a failed save the same
    // rows are signed again on the next try (the journal kept the numbering), and
    // nothing unsaved is ever handed out for upload.
    let mut next = store.clone();
    let state = next.library_mut(&lib.id).expect("checked above");
    state.seq = seq;
    state.hash = Some(signed.hash.to_string());
    state.unsent.push(Unsent {
        path: signed.path.clone(),
        bytes: b64url_encode(&signed.bytes),
    });
    next.save(ks)?;
    *store = next;
    Ok(Some(signed))
}

/// SP15: what this device signed and hasn't yet seen in storage, to upload again,
/// oldest first. Each is opened as a peer would open it, with this device's key,
/// and must sit at the path its sequence names. Together they are the end of the
/// chain the device store records: in order, each linked to the one before when
/// they are consecutive, and the newest the stored head itself. One that doesn't
/// fit is an error, never skipped or uploaded: its sequence is used, so nothing
/// else would publish it, and a peer would refuse it or what follows it.
pub fn pending(store: &DeviceStore, lib: &Id16) -> Result<Vec<(String, Vec<u8>)>> {
    let Some(state) = store.library(lib) else {
        return Ok(Vec::new());
    };
    let device = state.device_id()?;
    let key = state.key()?.public();
    let unreadable = || ChainError::Store(StoreError::Unreadable);
    let mut out = Vec::with_capacity(state.unsent.len());
    let mut before: Option<Change> = None;
    // Device files share the list (SP28); `device_file::pending` checks those.
    for u in state
        .unsent
        .iter()
        .filter(|u| u.path.starts_with("changes/"))
    {
        let bytes = u.bytes().ok_or(StoreError::Unreadable)?;
        let c = open_change(&bytes, lib, &device, &key)?;
        if u.path != change_path(&device, c.seq) {
            return Err(unreadable());
        }
        if let Some(b) = &before {
            if c.seq <= b.seq || (c.seq == b.seq + 1 && c.prev != Some(b.hash)) {
                return Err(unreadable());
            }
        }
        out.push((u.path.clone(), bytes));
        before = Some(c);
    }
    if let Some(last) = before {
        if last.seq != state.seq || Some(last.hash) != state.last_hash()? {
            return Err(unreadable());
        }
    }
    Ok(out)
}

/// SP15: storage holds `path` now; its bytes leave the device store.
pub fn uploaded(ks: &dyn Keystore, store: &mut DeviceStore, lib: &Id16, path: &str) -> Result<()> {
    let mut next = store.clone();
    if let Some(state) = next.library_mut(lib) {
        let before = state.unsent.len();
        state.unsent.retain(|u| u.path != path);
        if state.unsent.len() != before {
            next.save(ks)?;
            *store = next;
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
    let e = envelope::open_within(bytes, "change", library, MAX_CHANGE_VALUES)?;
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
    // SP33, SP34, #122: refuse malformed or oversized lists before application.
    hold::read_lists(&reserve, &hold)?;
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
/// refused. SP18: so is one at the sequence of a checkpoint of the peer's floor
/// (`floor`, from `device_file::floor_of`) with another hash, since the chain must
/// pass it, and one below a checkpoint waits for the run that shows it does
/// (`receive_run`).
/// `bodies` are the blobs its versions name, as fetched.
#[allow(clippy::too_many_arguments)]
pub fn receive_change(
    conn: &Connection,
    lib: &Library,
    bytes: &[u8],
    device: &DeviceId,
    key: &[u8; 32],
    floor: &[(u64, Hash32)],
    bodies: Vec<Vec<u8>>,
    now_ms: i64,
) -> Result<Received> {
    let mut got = receive_run(
        conn,
        lib,
        device,
        key,
        floor,
        vec![(bytes.to_vec(), bodies)],
        now_ms,
    )?;
    Ok(got.remove(0))
}

/// SP14, SP18: a run of a peer's change files, in sequence order, each with the
/// blobs its versions name. Each is received as `receive_change` says. While this
/// device holds less of the peer's chain than a checkpoint of its floor, though,
/// none is applied until the run is seen to lead to it: consecutive from what is
/// held, linked by `prev`, matching every checkpoint it passes, and reaching the
/// lowest one not yet passed. Every checkpoint the chain held has passed must be
/// the change held there. A run that
/// stops short waits, whole; one that leads anywhere else is refused, whole, so a
/// fork below the floor never reaches the library. Only the run's sequences,
/// links and hashes are kept while it is checked; each change is decoded again,
/// and applied, one at a time, so a run's decoded size is one change's.
pub fn receive_run(
    conn: &Connection,
    lib: &Library,
    device: &DeviceId,
    key: &[u8; 32],
    floor: &[(u64, Hash32)],
    files: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
    now_ms: i64,
) -> Result<Vec<Received>> {
    // The run's chain metadata: each change opened and checked, then dropped.
    struct Meta {
        seq: u64,
        prev: Option<Hash32>,
        hash: Hash32,
    }
    let opened = files
        .iter()
        .map(|(b, _)| {
            open_change(b, &lib.id, device, key).map(|c| Meta {
                seq: c.seq,
                prev: c.prev,
                hash: c.hash,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let have = apply::seen(conn, lib, *device)?;
    // Checkpoints the chain held has passed must be the changes held there, or
    // this device followed another chain than the floor's.
    for &(s, h) in floor {
        if have >= s && held_hash(conn, lib, device, s)?.is_some_and(|held| held != h) {
            return Err(ChainError::Link);
        }
    }
    if let Some(lowest) = floor.iter().map(|(s, _)| *s).filter(|s| *s > have).min() {
        let mut prev = held_hash(conn, lib, device, have)?;
        if have > 0 && prev.is_none() {
            return Err(ChainError::Link);
        }
        let mut reached = false;
        for (next, c) in (have + 1..).zip(opened.iter().filter(|c| c.seq > have)) {
            if c.seq != next {
                break;
            }
            if c.prev != prev {
                return Err(ChainError::Link);
            }
            if floor.iter().any(|(s, h)| *s == c.seq && *h != c.hash) {
                return Err(ChainError::Link);
            }
            reached |= c.seq == lowest;
            prev = Some(c.hash);
        }
        if !reached {
            return Ok(opened.iter().map(|_| Received::Waits).collect());
        }
    }
    let mut out = Vec::with_capacity(files.len());
    for (bytes, bodies) in files {
        let c = open_change(&bytes, &lib.id, device, key)?;
        drop(bytes);
        out.push(receive_opened(conn, lib, floor, c, bodies, now_ms)?);
    }
    Ok(out)
}

/// The payload hash this device holds for `device`'s change `seq` (none for 0).
pub(crate) fn held_hash(
    conn: &Connection,
    lib: &Library,
    device: &DeviceId,
    seq: u64,
) -> Result<Option<Hash32>> {
    if seq == 0 {
        return Ok(None);
    }
    let h: Option<String> = conn
        .query_row(
            "SELECT hash FROM sync_changes WHERE library = ?1 AND device = ?2 AND seq = ?3",
            params![lib.id.to_string(), device.to_string(), seq as i64],
            |r| r.get(0),
        )
        .optional()
        .map_err(JournalError::from)?;
    Ok(h.as_deref().and_then(Hash32::parse))
}

fn receive_opened(
    conn: &Connection,
    lib: &Library,
    floor: &[(u64, Hash32)],
    c: Change,
    bodies: Vec<Vec<u8>>,
    now_ms: i64,
) -> Result<Received> {
    if floor.iter().any(|(s, h)| *s == c.seq && *h != c.hash) {
        return Err(ChainError::Link);
    }
    let have = apply::seen(conn, lib, c.device)?;
    if c.seq <= have {
        // Already held: the same file again is a repeat; another for that sequence
        // isn't this device's chain (SP30 makes a proof of it).
        return match held_hash(conn, lib, &c.device, c.seq)? {
            Some(h) if h == c.hash => Ok(Received::Repeat),
            _ => Err(ChainError::Link),
        };
    }
    if c.seq > have + 1 {
        return Ok(Received::Waits);
    }
    let expected = held_hash(conn, lib, &c.device, have)?;
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

    fn replacement(a: &Store, lib: &Library) -> (Key, Hold) {
        let key = Key::parse("setting:fontPx").unwrap().0;
        let body = br#"{"value":"18"}"#.to_vec();
        let hash = super::super::crypto::payload_hash(&body);
        let hold = Hold {
            kind: hold::Kind::Blob,
            name: Hash32([3; 32]),
            hash,
            size: body.len() as u64,
            for_key: key.clone(),
        };
        journal::record_local(
            a.conn(),
            lib,
            &key,
            journal::LocalContent::Body(body),
            1_760_000_001_000,
        )
        .unwrap();
        (key, hold)
    }

    #[test]
    fn a_naming_version_isnt_published_with_its_hold() {
        use crate::sync::{frontier, hold::Progress, retention::Vouched};
        use std::collections::BTreeMap;
        let (a, lib, ks, mut store) = signed_device("book-a");
        let (b, peer, _, _) = signed_device("book-b");
        let public = key_of(&store, &lib);
        // Restore/Keep already journalled their replacement before publishing.
        let (key, item) = replacement(&a, &lib);
        let mut ledger = Ledger::default();
        let first = super::publish_with(
            a.conn(),
            &lib,
            &ks,
            &mut store,
            &[],
            &[item],
            &ledger,
            &BTreeSet::from([key.clone()]),
        )
        .unwrap()
        .unwrap();
        let opened = open_change(&first.bytes, &lib.id, &lib.device, &public).unwrap();
        assert_eq!(opened.hold.len(), 1);
        assert!(opened.versions.is_empty());
        assert_eq!(journal::unsent(a.conn(), &lib).unwrap()[0].key, key);
        receive_change(
            b.conn(),
            &peer,
            &first.bytes,
            &lib.device,
            &public,
            &[],
            vec![],
            1_760_000_001_000,
        )
        .unwrap();
        assert_eq!(
            apply::seen(b.conn(), &peer, lib.device).unwrap(),
            first.seq,
            "the receive path accepts the hold-only change"
        );
        ledger
            .receive(&opened, &[], false, &Vouched::new(0))
            .unwrap();
        let held = |d: &DeviceId, s: u64| {
            (*d == lib.device && s == first.seq).then_some(frontier::Held {
                h: first.hash,
                prev: None,
                vroot: opened.vroot,
            })
        };
        let accepted = BTreeMap::new();
        let empty = BTreeSet::new();
        let make_progress = |accepted| Progress {
            accepted,
            held: &held,
            removed: &empty,
            all_listed_read: &empty,
            late_decided: &empty,
            choices: &[],
        };
        let withheld = ledger.withheld_keys(lib.device, &make_progress(&accepted));
        assert_eq!(withheld, BTreeSet::from([key.clone()]));
        assert!(ledger
            .withheld_keys(peer.device, &make_progress(&accepted))
            .is_empty());
        assert!(super::publish_with(
            a.conn(),
            &lib,
            &ks,
            &mut store,
            &[],
            &[],
            &ledger,
            &withheld
        )
        .unwrap()
        .is_none());
        let accepted = frontier::accepted_by_all(
            &BTreeSet::from([lib.device, peer.device]),
            &lib.device,
            &BTreeMap::new(),
            &BTreeMap::from([(peer.device, BTreeMap::from([(lib.device, first.seq)]))]),
            &BTreeMap::from([(lib.device, first.seq)]),
            &held,
            &accepted,
        );
        let withheld = ledger.withheld_keys(lib.device, &make_progress(&accepted));
        assert!(withheld.is_empty());
        let next = super::publish_with(
            a.conn(),
            &lib,
            &ks,
            &mut store,
            &[],
            &[],
            &ledger,
            &withheld,
        )
        .unwrap()
        .unwrap();
        let opened = open_change(&next.bytes, &lib.id, &lib.device, &public).unwrap();
        assert_eq!(opened.seq, first.seq + 1);
        assert_eq!(opened.prev, Some(first.hash));
        assert_eq!(opened.versions.len(), 1);
        assert_eq!(opened.versions[0]["key"], key.as_str());
        assert!(opened.hold.is_empty());
        assert!(journal::unsent(a.conn(), &lib).unwrap().is_empty());
    }

    #[test]
    fn a_retry_after_a_crash_signs_the_same_reservations_and_holds() {
        use std::sync::Mutex;
        struct Crash(Mutex<Option<Vec<u8>>>);
        impl Keystore for Crash {
            fn load(&self) -> std::result::Result<Option<Vec<u8>>, StoreError> {
                Ok(None)
            }
            fn save(&self, bytes: &[u8]) -> std::result::Result<(), StoreError> {
                // Observe the first attempted bytes, then simulate failing to save.
                *self.0.lock().unwrap() = Some(bytes.to_vec());
                Err(StoreError::Keychain(-25308))
            }
        }
        for lists_only in [false, true] {
            for legacy_retry in [false, true] {
                let (a, lib, ks, mut store) = signed_device("book-a");
                let (key, item) = replacement(&a, &lib);
                let withheld = if lists_only {
                    BTreeSet::from([key.clone()])
                } else {
                    BTreeSet::new()
                };
                let reserve = Reserve {
                    kind: hold::Kind::Book,
                    name: Hash32([4; 32]),
                    size: 10,
                };
                let before = store.clone();
                let crash = Crash(Mutex::new(None));
                assert!(matches!(
                    super::publish_with(
                        a.conn(),
                        &lib,
                        &crash,
                        &mut store,
                        std::slice::from_ref(&reserve),
                        std::slice::from_ref(&item),
                        &Ledger::default(),
                        &withheld
                    ),
                    Err(ChainError::Store(StoreError::Keychain(_)))
                ));
                assert_eq!(store, before);
                assert!(pending(&store, &lib.id).unwrap().is_empty());
                let capture = Memory::default();
                capture
                    .save(crash.0.lock().unwrap().as_ref().unwrap())
                    .unwrap();
                let attempted = DeviceStore::load(&capture).unwrap();
                let expected = pending(&attempted, &lib.id).unwrap().remove(0).1;
                let saved: (String, String) = a.conn().query_row(
                    "SELECT reserve, hold FROM sync_change_lists WHERE library = ?1 AND seq = 1",
                    [lib.id.to_string()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
                assert_eq!(
                    saved.0.as_bytes(),
                    json::canonical(&json!([reserve.write()]))
                );
                assert_eq!(saved.1.as_bytes(), json::canonical(&json!([item.write()])));
                let retry = if legacy_retry {
                    publish(a.conn(), &lib, &ks, &mut store)
                } else {
                    // Even invalid new lists and newly withheld keys are irrelevant
                    // to the already numbered payload.
                    let different = Hold {
                        size: u64::MAX,
                        ..item.clone()
                    };
                    super::publish_with(
                        a.conn(),
                        &lib,
                        &ks,
                        &mut store,
                        &[],
                        &[different],
                        &Ledger::default(),
                        &BTreeSet::from([key]),
                    )
                }
                .unwrap()
                .unwrap();
                assert_eq!(retry.bytes, expected);
                assert_eq!(
                    retry.hash,
                    attempted
                        .library(&lib.id)
                        .unwrap()
                        .last_hash()
                        .unwrap()
                        .unwrap()
                );
                assert_eq!(pending(&store, &lib.id).unwrap()[0].1, expected);
                let opened =
                    open_change(&retry.bytes, &lib.id, &lib.device, &key_of(&store, &lib)).unwrap();
                assert_eq!(opened.versions.is_empty(), lists_only);
                assert_eq!(opened.reserve, vec![reserve.write()]);
                assert_eq!(opened.hold, vec![item.write()]);
            }
        }
    }

    #[test]
    fn withholding_either_tombstone_keeps_the_highlight_and_note_together() {
        for prefix in ["annotation", "note"] {
            let (a, lib, ks, mut store) = signed_device("book-a");
            let uuid = "7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
            let keys: Vec<Key> = ["annotation", "note"]
                .iter()
                .map(|p| Key::parse(&format!("{p}:{uuid}")).unwrap().0)
                .collect();
            for key in &keys {
                journal::record_local(
                    a.conn(),
                    &lib,
                    key,
                    journal::LocalContent::Deleted {
                        covers: vec![],
                        identity: None,
                    },
                    1_760_000_001_000,
                )
                .unwrap();
            }
            let withheld = BTreeSet::from([Key::parse(&format!("{prefix}:{uuid}")).unwrap().0]);
            assert!(super::publish_with(
                a.conn(),
                &lib,
                &ks,
                &mut store,
                &[],
                &[],
                &Ledger::default(),
                &withheld
            )
            .unwrap()
            .is_none());
            assert_eq!(journal::unsent(a.conn(), &lib).unwrap().len(), 2);
            let sent = publish_within(a.conn(), &lib, &ks, &mut store, 1, 1)
                .unwrap()
                .unwrap();
            let opened =
                open_change(&sent.bytes, &lib.id, &lib.device, &key_of(&store, &lib)).unwrap();
            assert_eq!(
                opened.versions.len(),
                2,
                "SP13 pairing survives a one-value budget"
            );
        }
    }

    #[test]
    fn list_storage_and_numbering_roll_back_together_and_old_rows_retry_empty() {
        let (a, lib, ks, mut store) = signed_device("book-a");
        let (_, item) = replacement(&a, &lib);
        a.conn()
            .execute_batch(
                "CREATE TEMP TRIGGER refuse_lists BEFORE INSERT ON sync_change_lists
            BEGIN SELECT RAISE(ABORT, 'simulated write failure'); END;",
            )
            .unwrap();
        assert!(super::publish_with(
            a.conn(),
            &lib,
            &ks,
            &mut store,
            &[],
            std::slice::from_ref(&item),
            &Ledger::default(),
            &BTreeSet::new()
        )
        .is_err());
        assert_eq!(journal::unsent(a.conn(), &lib).unwrap().len(), 1);
        assert_eq!(store.library(&lib.id).unwrap().seq, 0);
        a.conn()
            .execute_batch("DROP TRIGGER refuse_lists;")
            .unwrap();
        let (seq, versions) = journal::take_unsent(a.conn(), &lib, 0).unwrap().unwrap();
        // Simulate a pre-migration numbered row, which had no persisted lists.
        a.conn()
            .execute(
                "DELETE FROM sync_change_lists WHERE library = ?1",
                [lib.id.to_string()],
            )
            .unwrap();
        let key = store.library(&lib.id).unwrap().key().unwrap();
        let writer = store.library(&lib.id).unwrap().writer_id().unwrap();
        let expected = sign_change(
            &key,
            &lib.id,
            &writer,
            seq,
            None,
            &versions.into_iter().map(|v| v.raw).collect::<Vec<_>>(),
        );
        let retried = super::publish_with(
            a.conn(),
            &lib,
            &ks,
            &mut store,
            &[],
            &[item],
            &Ledger::default(),
            &BTreeSet::new(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(retried, expected);
    }

    // Keep the earlier tests' calls unchanged; new withholding tests use the
    // production entry point explicitly, with their withheld keys.
    fn publish_with(
        conn: &Connection,
        lib: &Library,
        ks: &dyn Keystore,
        store: &mut DeviceStore,
        reserve: &[Reserve],
        hold: &[Hold],
        ledger: &Ledger,
    ) -> Result<Option<Signed>> {
        super::publish_with(
            conn,
            lib,
            ks,
            store,
            reserve,
            hold,
            ledger,
            &BTreeSet::new(),
        )
    }

    #[test]
    fn signed_changes_without_reservations_or_holds_are_unchanged() {
        let key = Keypair::from_seed(&[1; 32]);
        let (library, writer) = (Id16([2; 16]), Id16([3; 16]));
        for (seq, prev, versions) in [
            (1, None, vec![]),
            (
                2,
                Some(Hash32([4; 32])),
                vec![
                    json!({"key": "setting:fontPx", "device": key.device().to_string(), "body": {"hash": Hash32([5; 32]).to_string(), "size": 12}}),
                ],
            ),
        ] {
            // Frozen pre-SP33 payload construction: do not use payload() here.
            let original = json::canonical(&json!({
                "type": "change", "format": 1, "library": library.to_string(),
                "device": key.device().to_string(), "writer": writer.to_string(),
                "seq": seq, "prev": prev.map(|h| h.to_string()),
                "versions": versions, "reserve": [], "hold": [],
            }));
            let sig = envelope::sign(
                &key,
                Signer::Device(key.device()),
                "change",
                &library,
                &Place {
                    seq,
                    prev,
                    vroot: Some(vroot_of(&versions)),
                },
                &original,
            );
            let expected = envelope::write("change", &original, &[sig]);
            let legacy = sign_change(&key, &library, &writer, seq, prev, &versions);
            let with = sign_change_with(&key, &library, &writer, seq, prev, &versions, &[], &[]);
            assert_eq!(legacy.bytes, expected);
            assert_eq!(legacy, with);
        }
    }

    // A correctly signed peer can still send malformed lists. Signing raw JSON
    // here ensures rejection isn't just a failed signature or local preflight.
    fn peer_lists(reserve: Value, hold: Value) -> Result<Change> {
        let key = Keypair::from_seed(&[1; 32]);
        let library = Id16([2; 16]);
        let mut raw: Value = serde_json::from_slice(&payload(
            &library,
            &key.device(),
            &Id16([3; 16]),
            1,
            None,
            &[],
            &[],
            &[],
        ))
        .unwrap();
        raw["reserve"] = reserve;
        raw["hold"] = hold;
        let body = json::canonical(&raw);
        let sig = envelope::sign(
            &key,
            Signer::Device(key.device()),
            "change",
            &library,
            &Place {
                seq: 1,
                prev: None,
                vroot: Some(vroot_of(&[])),
            },
            &body,
        );
        open_change(
            &envelope::write("change", &body, &[sig]),
            &library,
            &key.device(),
            &key.public(),
        )
    }

    #[test]
    fn a_change_with_too_many_reserve_or_hold_items_is_refused_when_opened() {
        let r = json!({"kind": "book", "name": Hash32([1; 32]).to_string(), "size": 1});
        let h = json!({"kind": "blob", "name": Hash32([2; 32]).to_string(), "size": 1,
            "hash": Hash32([3; 32]).to_string(), "for": "setting:fontPx"});
        assert!(peer_lists(
            json!(vec![r.clone(); hold::MAX_RESERVATIONS]),
            json!(vec![h.clone(); hold::MAX_HOLDS])
        )
        .is_ok());
        assert!(matches!(
            peer_lists(json!(vec![r; hold::MAX_RESERVATIONS + 1]), json!([])),
            Err(ChainError::Hold(hold::HoldError::Reservations))
        ));
        assert!(matches!(
            peer_lists(json!([]), json!(vec![h; hold::MAX_HOLDS + 1])),
            Err(ChainError::Hold(hold::HoldError::Holds))
        ));
    }

    #[test]
    fn malformed_reservations_and_holds_refuse_the_whole_change() {
        let r = json!({"kind": "book", "name": Hash32([1; 32]).to_string(), "size": 1});
        let h = json!({"kind": "blob", "name": Hash32([2; 32]).to_string(), "size": 1,
            "hash": Hash32([3; 32]).to_string(), "for": "setting:fontPx"});
        for (field, value) in [
            ("kind", json!("other")),
            ("name", json!("../book")),
            ("name", json!("A".repeat(64))),
            ("size", json!(-1)),
            ("size", json!(1.5)),
            ("size", Value::Null),
        ] {
            let mut bad_r = r.clone();
            let mut bad_h = h.clone();
            bad_r[field] = value.clone();
            bad_h[field] = value;
            assert!(
                peer_lists(json!([bad_r]), json!([])).is_err(),
                "reserve {field}"
            );
            assert!(
                peer_lists(json!([]), json!([bad_h])).is_err(),
                "hold {field}"
            );
        }
        for (field, value) in [
            ("hash", json!("bad")),
            ("for", json!("note:bad")),
            ("size", json!(64 * 1024 + 1)),
        ] {
            let mut bad = h.clone();
            bad[field] = value;
            assert!(peer_lists(json!([]), json!([bad])).is_err(), "hold {field}");
        }
        for original in [&r, &h] {
            for field in original.as_object().unwrap().keys() {
                let mut bad = original.clone();
                bad.as_object_mut().unwrap().remove(field);
                let result = if original == &r {
                    peer_lists(json!([bad]), json!([]))
                } else {
                    peer_lists(json!([]), json!([bad]))
                };
                assert!(result.is_err(), "missing {field}");
            }
        }
        assert!(peer_lists(json!([null]), json!([])).is_err());
        assert!(peer_lists(json!([]), json!([0])).is_err());
        let opened = peer_lists(json!([r.clone()]), json!([h.clone()])).unwrap();
        let (reserve, holds) = hold::read_lists(&opened.reserve, &opened.hold).unwrap();
        assert_eq!(reserve[0].write(), r);
        assert_eq!(holds[0].write(), h);
    }

    #[test]
    fn publishing_lists_checks_the_local_ledger_and_saves_replayable_bytes() {
        let (a, lib, ks, mut store) = signed_device("book-a");
        let key = store.library(&lib.id).unwrap().key().unwrap();
        let item = Hold {
            kind: hold::Kind::Book,
            name: Hash32([1; 32]),
            hash: Hash32([2; 32]),
            size: 1,
            for_key: super::super::record::Key::parse(
                "bookFile:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6",
            )
            .unwrap()
            .0,
        };
        let mut ledger = Ledger::default();
        let signed = publish_with(
            a.conn(),
            &lib,
            &ks,
            &mut store,
            &[],
            &vec![item.clone(); hold::MAX_HOLDS],
            &ledger,
        )
        .unwrap()
        .unwrap();
        let opened = open_change(&signed.bytes, &lib.id, &lib.device, &key.public()).unwrap();
        assert_eq!(opened.hold.len(), hold::MAX_HOLDS);
        assert!(opened.versions.is_empty());
        ledger
            .receive(
                &opened,
                &[],
                false,
                &super::super::retention::Vouched::new(0),
            )
            .unwrap();
        let before = store.library(&lib.id).unwrap().seq;
        assert!(matches!(
            publish_with(a.conn(), &lib, &ks, &mut store, &[], &[item], &ledger),
            Err(ChainError::Hold(hold::HoldError::Holds))
        ));
        assert_eq!(store.library(&lib.id).unwrap().seq, before);
        let queued = pending(&store, &lib.id).unwrap();
        assert_eq!(queued, vec![(signed.path, signed.bytes)]);
        assert!(publish(a.conn(), &lib, &ks, &mut store).unwrap().is_none());
        let reserved = Reserve {
            kind: hold::Kind::Book,
            name: Hash32([3; 32]),
            size: 12,
        };
        let next = publish_with(
            a.conn(),
            &lib,
            &ks,
            &mut store,
            std::slice::from_ref(&reserved),
            &[],
            &ledger,
        )
        .unwrap()
        .unwrap();
        let opened = open_change(&next.bytes, &lib.id, &lib.device, &key.public()).unwrap();
        assert_eq!(opened.seq, before + 1);
        assert_eq!(opened.reserve, vec![reserved.write()]);
    }

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
            &[],
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
            &[],
            bodies(&a, &two, &la),
            now,
        )
        .unwrap();
        assert_eq!(note_of(&b, "book-b"), Some(Some("second".into())));
        // The same file again is a repeat.
        assert_eq!(
            receive_change(
                b.conn(),
                &lb,
                &two.bytes,
                &la.device,
                &key,
                &[],
                vec![],
                now
            )
            .unwrap(),
            Received::Repeat
        );
    }

    #[test]
    fn a_floor_below_the_chain_held_must_name_the_change_held() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let (b, lb, _kb, _sb) = signed_device("book-b");
        let now = crate::store::now_ms();
        let key = key_of(&sa, &la);
        let mut changes = Vec::new();
        for (i, text) in ["first", "second", "third"].iter().enumerate() {
            a.save_annotation(&highlight(
                "book-a",
                Some(text),
                1_760_000_001_000 + i as i64,
            ))
            .unwrap();
            changes.push(publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap());
        }
        for c in &changes[..2] {
            receive_change(
                b.conn(),
                &lb,
                &c.bytes,
                &la.device,
                &key,
                &[],
                bodies(&a, c, &la),
                now,
            )
            .unwrap();
        }
        // A newer floor at change 1 that isn't the change 1 held: change 3 is refused.
        let three = &changes[2];
        let wrong: &[(u64, Hash32)] = &[(1, Hash32([6; 32]))];
        assert!(matches!(
            receive_change(
                b.conn(),
                &lb,
                &three.bytes,
                &la.device,
                &key,
                wrong,
                vec![],
                now
            ),
            Err(ChainError::Link)
        ));
        // With the floor the chain held does reach, it's received.
        let right: &[(u64, Hash32)] = &[(1, changes[0].hash)];
        let got = receive_change(
            b.conn(),
            &lb,
            &three.bytes,
            &la.device,
            &key,
            right,
            bodies(&a, three, &la),
            now,
        )
        .unwrap();
        assert!(matches!(got, Received::Applied { .. }), "{got:?}");
    }

    #[test]
    fn a_change_at_the_floors_sequence_must_have_its_hash() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let (b, lb, _kb, _sb) = signed_device("book-b");
        let now = crate::store::now_ms();
        let key = key_of(&sa, &la);
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        // SP18: the floor says change 1 has another hash: the chain must lead to it.
        let floor: &[(u64, Hash32)] = &[(1, Hash32([6; 32]))];
        assert!(matches!(
            receive_change(
                b.conn(),
                &lb,
                &one.bytes,
                &la.device,
                &key,
                floor,
                vec![],
                now
            ),
            Err(ChainError::Link)
        ));
        // With the floor it does name, it's received.
        let got = receive_change(
            b.conn(),
            &lb,
            &one.bytes,
            &la.device,
            &key,
            &[(1, one.hash)],
            bodies(&a, &one, &la),
            now,
        )
        .unwrap();
        assert!(matches!(got, Received::Applied { .. }), "{got:?}");
    }

    #[test]
    fn a_fork_past_an_earlier_checkpoint_is_refused() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let (b, lb, _kb, _sb) = signed_device("book-b");
        let now = crate::store::now_ms();
        let key = key_of(&sa, &la);
        let akey = sa.library(&la.id).unwrap().key().unwrap();
        let writer = sa.library(&la.id).unwrap().writer_id().unwrap();
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        a.save_annotation(&highlight("book-a", Some("second"), 1_760_000_002_000))
            .unwrap();
        let two = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        // A's key, stolen, signs a fork from the start: 1, 2 and 3 of its own.
        let f1 = sign_change(&akey, &la.id, &writer, 1, None, &[]);
        let f2 = sign_change(&akey, &la.id, &writer, 2, Some(f1.hash), &[]);
        let f3 = sign_change(&akey, &la.id, &writer, 3, Some(f2.hash), &[]);
        // B had the true change 2 as its floor; a newer device file named the fork's
        // 3, and the floor kept 2 as an earlier checkpoint.
        let floor: &[(u64, Hash32)] = &[(2, two.hash), (3, f3.hash)];
        let fork: Vec<(Vec<u8>, Vec<Vec<u8>>)> = [&f1, &f2, &f3]
            .iter()
            .map(|c| (c.bytes.clone(), vec![]))
            .collect();
        assert!(matches!(
            receive_run(b.conn(), &lb, &la.device, &key, floor, fork, now),
            Err(ChainError::Link)
        ));
        assert_eq!(apply::seen(b.conn(), &lb, la.device).unwrap(), 0);
    }

    #[test]
    fn changes_below_the_floor_apply_only_once_their_run_reaches_it() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let (b, lb, _kb, _sb) = signed_device("book-b");
        let now = crate::store::now_ms();
        let key = key_of(&sa, &la);
        let akey = sa.library(&la.id).unwrap().key().unwrap();
        let writer = sa.library(&la.id).unwrap().writer_id().unwrap();
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        a.save_annotation(&highlight("book-a", Some("second"), 1_760_000_002_000))
            .unwrap();
        let two = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        let floor: &[(u64, Hash32)] = &[(2, two.hash)];
        // A fork of A's key below the floor: its own 1 and a 2 after it.
        let note = |text: &str| {
            json!({
                "format": 1, "key": format!("note:{}", crate::sync::apply::tests::ANNO),
                "device": la.device.to_string(), "clock": [1_760_000_009_000u64, 0],
                "history": [], "body": { "hash": crate::sync::ids::identity_hash(text.as_bytes()).to_string(), "size": text.len() },
            })
        };
        let f1 = sign_change(
            &akey,
            &la.id,
            &writer,
            1,
            None,
            &[note("{\"text\":\"forked\"}")],
        );
        let f2 = sign_change(&akey, &la.id, &writer, 2, Some(f1.hash), &[]);
        let run = |files: &[&Signed]| {
            receive_run(
                b.conn(),
                &lb,
                &la.device,
                &key,
                floor,
                files
                    .iter()
                    .map(|c| (c.bytes.clone(), bodies(&a, c, &la)))
                    .collect(),
                now,
            )
        };
        assert!(matches!(run(&[&f1, &f2]), Err(ChainError::Link)));
        // Refused whole: the fork's change 1 never reached the library.
        assert_eq!(apply::seen(b.conn(), &lb, la.device).unwrap(), 0);
        // Change 1 alone, below the floor: it waits, nothing applied.
        assert_eq!(run(&[&f1]).unwrap(), vec![Received::Waits]);
        assert_eq!(run(&[&one]).unwrap(), vec![Received::Waits]);
        assert_eq!(apply::seen(b.conn(), &lb, la.device).unwrap(), 0);
        assert_eq!(note_of(&b, "book-b"), None);
        // The run that reaches the floor applies, in order.
        let got = run(&[&one, &two]).unwrap();
        assert!(
            got.iter().all(|r| matches!(r, Received::Applied { .. })),
            "{got:?}"
        );
        assert_eq!(note_of(&b, "book-b"), Some(Some("second".into())));
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
            &[],
            bodies(&a, &one, &la),
            now,
        )
        .unwrap();
        // Signed by A's key, but after another first change: refused.
        let forked = sign_change(&akey, &la.id, &writer, 2, Some(Hash32([7; 32])), &[]);
        assert!(matches!(
            receive_change(
                b.conn(),
                &lb,
                &forked.bytes,
                &la.device,
                &key,
                &[],
                vec![],
                now
            ),
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
                &[],
                vec![],
                now
            ),
            Err(ChainError::Link)
        ));
        // One beyond a gap waits.
        let three = sign_change(&akey, &la.id, &writer, 3, Some(Hash32([7; 32])), &[]);
        assert_eq!(
            receive_change(
                b.conn(),
                &lb,
                &three.bytes,
                &la.device,
                &key,
                &[],
                vec![],
                now
            )
            .unwrap(),
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
        // A queued change that isn't what a peer would accept is an error, not
        // skipped or uploaded: bytes that don't decode, bytes changed after
        // signing, or a path that isn't its sequence's.
        let queued = |path: String, bytes: String| {
            let mut s = DeviceStore::load(&ka).unwrap();
            s.library_mut(&la.id)
                .unwrap()
                .unsent
                .push(Unsent { path, bytes });
            pending(&s, &la.id)
        };
        assert!(matches!(
            queued(one.path.clone(), "not base64url!".into()),
            Err(ChainError::Store(StoreError::Unreadable))
        ));
        let mut altered = one.bytes.clone();
        let at = altered.iter().position(|&b| b == b'"').unwrap();
        altered[at] = b' ';
        assert!(queued(one.path.clone(), b64url_encode(&altered)).is_err());
        assert!(matches!(
            queued(change_path(&la.device, 2), b64url_encode(&one.bytes)),
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
            &[],
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
        while let Some(c) = publish_within(a.conn(), &la, &ka, &mut sa, 1, u64::MAX).unwrap() {
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
            receive_change(b.conn(), &lb, &c.bytes, &la.device, &key, &[], vec![], now).unwrap();
        }
        // Every version went out once, in order, and B applied the deletion.
        assert_eq!(keys.len(), 22);
        assert_eq!(keys.iter().collect::<HashSet<_>>().len(), 22);
        assert!(b.annotations("book-b").unwrap().is_empty());
    }

    #[test]
    fn a_change_of_too_many_values_is_refused_and_never_signed_here() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        let key = sa.library(&la.id).unwrap().key().unwrap();
        let writer = sa.library(&la.id).unwrap().writer_id().unwrap();
        // A peer's change of a million tiny values in `reserve`: refused when opened.
        let mut p: Value = serde_json::from_slice(&json::canonical(
            &serde_json::from_slice::<Value>(
                &crate::sync::ids::b64url_decode(
                    serde_json::from_slice::<Value>(
                        &sign_change(&key, &la.id, &writer, 1, None, &[]).bytes,
                    )
                    .unwrap()["payload"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap(),
            )
            .unwrap(),
        ))
        .unwrap();
        p["reserve"] = Value::Array(vec![json!(0); MAX_CHANGE_VALUES as usize]);
        let body = json::canonical(&p);
        let place = Place {
            seq: 1,
            prev: None,
            vroot: Some(vroot_of(&[])),
        };
        let sig = envelope::sign(
            &key,
            Signer::Device(la.device),
            "change",
            &la.id,
            &place,
            &body,
        );
        let bytes = envelope::write("change", &body, &[sig]);
        assert!(matches!(
            open_change(&bytes, &la.id, &la.device, &key.public()),
            Err(ChainError::Envelope(EnvelopeError::Payload(
                json::JsonError::TooManyValues
            )))
        ));
        // Here, a value budget so small each change takes one group, as for bytes.
        a.save_annotation(&highlight("book-a", Some("one"), 1_760_000_001_000))
            .unwrap();
        a.save_position("book-a", "epubcfi(/6/4!/4/2/1:0)", 0.1, None)
            .unwrap();
        let first = publish_within(a.conn(), &la, &ka, &mut sa, usize::MAX, 1)
            .unwrap()
            .unwrap();
        let opened = open_change(&first.bytes, &la.id, &la.device, &key.public()).unwrap();
        assert_eq!(opened.versions.len(), 1);
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
    fn queued_changes_must_end_at_the_stored_head() {
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        a.save_annotation(&highlight("book-a", Some("second"), 1_760_000_002_000))
            .unwrap();
        let two = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        assert_eq!(pending(&sa, &la.id).unwrap().len(), 2);
        // A head whose hash isn't the newest queued change's.
        let mut wrong = sa.clone();
        wrong.library_mut(&la.id).unwrap().hash = Some(Hash32([9; 32]).to_string());
        assert!(pending(&wrong, &la.id).is_err());
        // Queued out of order, or a sequence the head is past with nothing after it.
        let mut swapped = sa.clone();
        swapped.library_mut(&la.id).unwrap().unsent.reverse();
        assert!(pending(&swapped, &la.id).is_err());
        let mut behind = sa.clone();
        behind.library_mut(&la.id).unwrap().unsent.pop();
        assert!(pending(&behind, &la.id).is_err());
        // A change 2 that doesn't link to the queued change 1.
        let key = sa.library(&la.id).unwrap().key().unwrap();
        let writer = sa.library(&la.id).unwrap().writer_id().unwrap();
        let forked = sign_change(&key, &la.id, &writer, 2, Some(Hash32([7; 32])), &[]);
        let mut unlinked = sa.clone();
        let st = unlinked.library_mut(&la.id).unwrap();
        st.unsent[1].bytes = b64url_encode(&forked.bytes);
        st.hash = Some(forked.hash.to_string());
        assert!(pending(&unlinked, &la.id).is_err());
        // Once 1 is in storage, 2 alone is still the end of the chain.
        uploaded(&ka, &mut sa, &la.id, &one.path).unwrap();
        assert_eq!(
            pending(&sa, &la.id).unwrap(),
            vec![(two.path.clone(), two.bytes.clone())]
        );
    }

    #[test]
    fn a_failed_save_changes_nothing_and_the_retry_signs_the_same_file() {
        struct Refuses;
        impl Keystore for Refuses {
            fn load(&self) -> std::result::Result<Option<Vec<u8>>, StoreError> {
                Ok(None)
            }
            fn save(&self, _: &[u8]) -> std::result::Result<(), StoreError> {
                Err(StoreError::Keychain(-25308))
            }
        }
        let (mut a, la, ka, mut sa) = signed_device("book-a");
        a.save_annotation(&highlight("book-a", Some("first"), 1_760_000_001_000))
            .unwrap();
        let before = sa.clone();
        assert!(matches!(
            publish(a.conn(), &la, &Refuses, &mut sa),
            Err(ChainError::Store(StoreError::Keychain(_)))
        ));
        // Nothing unsaved is handed out, and the head didn't move.
        assert_eq!(sa, before);
        assert!(pending(&sa, &la.id).unwrap().is_empty());
        // The retry signs the change the journal numbered, and saves it.
        let one = publish(a.conn(), &la, &ka, &mut sa).unwrap().unwrap();
        assert_eq!(one.seq, 1);
        assert_eq!(
            pending(&DeviceStore::load(&ka).unwrap(), &la.id)
                .unwrap()
                .len(),
            1
        );
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
