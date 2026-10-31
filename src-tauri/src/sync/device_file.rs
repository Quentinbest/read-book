//! SP16: a device's device file, `devices/‹device›.json`, signed with its counter,
//! and the overflow files its lists spill into past 256 KB. SP18, SP29: a peer's
//! device file is accepted only with a higher counter than the last, and only when
//! it doesn't contradict the newest signed state this device knows of that peer
//! (its floor).
//!
//! PROVISIONAL: where SP16 leaves the bytes open, the choices here are Delegated
//! to Claude, not approved (`docs/decisions.md`, “The device file”); each place
//! cites its item.
//!
//! The lists (`frontier`, `inherited`, `views`, `ages`, `marks`, `refused`,
//! `repairs`, `quarantined`, `proofs`) are filled and checked by the parts of Stage
//! 1c that own them; here they are carried, split and joined again as arrays.

use super::crypto::{self, Keypair};
use super::device_store::{Checkpoint, DeviceStore, Floor, Keystore, StoreError, Unsent};
use super::envelope::{self, EnvelopeError, Place, Signer};
use super::ids::{b64url_encode, identity_hash, DeviceId, Hash32, Id16};
use super::json::{self, Numbers, MAX_SAFE_INTEGER};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// SP8: a device file is at most 256 KB, and an overflow file at most 8 MB.
pub const MAX_DEVICE_FILE: usize = 256 << 10;
pub const MAX_OVERFLOW_FILE: usize = 8 << 20;

/// SP16: the lists a device file carries, in the order they are moved to overflow
/// files when the file is too large (the largest first).
pub const LISTS: [&str; 9] = [
    "frontier",
    "inherited",
    "views",
    "ages",
    "marks",
    "refused",
    "repairs",
    "quarantined",
    "proofs",
];

/// The most list items and their fields one device file's overflow parts may
/// build in memory, so a few valid 8 MB parts of tiny values can't exhaust it.
/// PROVISIONAL (decisions.md, “The device file”, (6)).
pub const MAX_OVERFLOW_VALUES: u64 = 250_000;

/// SP16: a name is cut to 64 characters. PROVISIONAL: Unicode characters, not
/// bytes (decisions.md, (2)).
pub const MAX_NAME_CHARS: usize = 64;

pub fn device_path(device: &DeviceId) -> String {
    format!("devices/{device}.json")
}

pub fn overflow_path(device: &DeviceId, hash: &Hash32) -> String {
    format!("devices/{device}/{hash}.json")
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("larger than SP8 allows")]
    TooLarge,
    #[error("not SP16's device file")]
    Shape,
    #[error("a device-file format this Linen doesn't know")]
    Format,
    #[error("written by another device, or with another key, than the one expected")]
    Device,
    #[error("not signed by its device for its counter")]
    Signature,
    #[error("an overflow file doesn't match what the device file names")]
    Overflow,
    #[error("an overflow file isn't in storage yet")]
    Missing(String),
    #[error("contradicts the newest signed state this device knows of it")]
    BelowFloor,
}

type Result<T> = std::result::Result<T, DeviceError>;

/// SP16: what a device file says, beyond its writer and counter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
    /// The platform's name for the device, cut to 64 characters (SY7).
    pub name: String,
    /// Its clock when written (SP10), and whether it is marked as running ahead
    /// (SY17). PROVISIONAL: written as `{ "hlc": [ms, n], "ahead" }` (decisions.md,
    /// (1)).
    pub clock: (u64, u16),
    pub ahead: bool,
    /// The membership head it has accepted (SP20).
    pub head: Option<Hash32>,
    /// Its latest change, `{ s, h }`, or none before its first.
    pub own: Option<(u64, Hash32)>,
    /// Its newest view's index hash, or none before its first compaction.
    pub view: Option<Hash32>,
    /// SP16's lists, by name; a list not given is empty.
    pub lists: BTreeMap<String, Vec<Value>>,
}

/// SP16: one overflow file a device file names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overflow {
    pub field: String,
    pub part: u64,
    pub hash: Hash32,
    pub size: u64,
}

/// SP30, SP16: exact copied bytes or an encoded ancestry bundle to name beside
/// the list parts. A bundle's wrapper must carry this same `part`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extra {
    pub field: String,
    pub part: u64,
    pub bytes: Vec<u8>,
}

/// A device file, opened and checked against its device's key.
#[derive(Clone, Debug)]
pub struct DeviceFile {
    pub device: DeviceId,
    pub writer: Id16,
    pub counter: u64,
    pub key: [u8; 32],
    /// Its fields, with each list holding only the items in the file itself;
    /// `lists` joins them with their overflow parts.
    pub fields: Fields,
    pub overflow: Vec<Overflow>,
    /// Its payload hash (SP5).
    pub hash: Hash32,
}

/// A signed device file and its overflow files, by path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub counter: u64,
    pub hash: Hash32,
    pub path: String,
    pub bytes: Vec<u8>,
    pub parts: Vec<(String, Vec<u8>)>,
}

fn hash_or_null(h: &Option<Hash32>) -> Value {
    h.map_or(Value::Null, |h| json!(h.to_string()))
}

/// Who writes a device file, and at which counter.
struct Writer<'a> {
    library: &'a Id16,
    device: DeviceId,
    writer: &'a Id16,
    counter: u64,
    key: [u8; 32],
}

fn payload(
    w: &Writer<'_>,
    f: &Fields,
    lists: &BTreeMap<&str, Vec<Value>>,
    overflow: &[Overflow],
) -> Vec<u8> {
    let mut p = json!({
        "type": "device",
        "format": 1,
        "library": w.library.to_string(),
        "device": w.device.to_string(),
        "writer": w.writer.to_string(),
        "counter": w.counter,
        "key": b64url_encode(&w.key),
        "name": f.name.chars().take(MAX_NAME_CHARS).collect::<String>(),
        "clock": { "hlc": [f.clock.0, f.clock.1], "ahead": f.ahead },
        "head": hash_or_null(&f.head),
        "own": f.own.map_or(Value::Null, |(s, h)| json!({ "s": s, "h": h.to_string() })),
        "view": hash_or_null(&f.view),
        "overflow": overflow.iter().map(|o| json!({
            "field": o.field, "part": o.part, "hash": o.hash.to_string(), "size": o.size,
        })).collect::<Vec<_>>(),
    });
    for name in LISTS {
        p[name] = Value::Array(lists.get(name).cloned().unwrap_or_default());
    }
    json::canonical(&p)
}

/// SP16: an overflow part's bytes, `{ "field", "part", "items" }`.
fn part_bytes(field: &str, part: u64, items: &[Value]) -> Vec<u8> {
    json::canonical(&json!({ "field": field, "part": part, "items": items }))
}

/// Splits `items` into parts of at most `MAX_OVERFLOW_FILE` each. An item too
/// large for a part of its own is refused: no part may pass SP8's limit.
fn parts(field: &str, items: &[Value]) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let mut current: Vec<Value> = Vec::new();
    let mut size = 0usize;
    // The part's own wrapping, generously.
    let wrap = part_bytes(field, MAX_SAFE_INTEGER, &[]).len();
    for item in items {
        let n = item.to_string().len() + 1;
        if wrap + n > MAX_OVERFLOW_FILE {
            return Err(DeviceError::TooLarge);
        }
        if !current.is_empty() && wrap + size + n > MAX_OVERFLOW_FILE {
            out.push(part_bytes(field, out.len() as u64, &current));
            current.clear();
            size = 0;
        }
        size += n;
        current.push(item.clone());
    }
    if !current.is_empty() {
        out.push(part_bytes(field, out.len() as u64, &current));
    }
    Ok(out)
}

/// SP16: writes and signs a device file at `counter`. Lists that would take the
/// file past 256 KB move to overflow files, the largest first, until it fits.
/// PROVISIONAL: whole lists move, and no manifest is written (decisions.md, (3)).
pub fn write(
    key: &Keypair,
    library: &Id16,
    writer: &Id16,
    counter: u64,
    fields: &Fields,
) -> Result<Written> {
    write_with_overflow(key, library, writer, counter, fields, &[])
}

/// SP30, SP16: `write` with proof copies and bundles. With no extras this writes
/// exactly the same bytes as `write`; existing callers need not carry attachments.
pub fn write_with_overflow(
    key: &Keypair,
    library: &Id16,
    writer: &Id16,
    counter: u64,
    fields: &Fields,
    extras: &[Extra],
) -> Result<Written> {
    let device = key.device();
    let w = Writer {
        library,
        device,
        writer,
        counter,
        key: key.public(),
    };
    let mut lists: BTreeMap<&str, Vec<Value>> = LISTS
        .iter()
        .map(|n| (*n, fields.lists.get(*n).cloned().unwrap_or_default()))
        .collect();
    let mut overflow = Vec::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for extra in extras {
        if !matches!(extra.field.as_str(), "copy" | "bundle") {
            return Err(DeviceError::Overflow);
        }
        if extra.bytes.len() > MAX_OVERFLOW_FILE || extra.part > MAX_SAFE_INTEGER {
            return Err(DeviceError::TooLarge);
        }
        if extra.field == "bundle" {
            let v = json::parse_within(&extra.bytes, Numbers::Integers, MAX_OVERFLOW_VALUES)
                .map_err(|_| DeviceError::Overflow)?
                .0;
            if v.get("field").and_then(Value::as_str) != Some("bundle")
                || v.get("part").and_then(Value::as_u64) != Some(extra.part)
                || v.get("items").and_then(Value::as_array).is_none()
            {
                return Err(DeviceError::Overflow);
            }
        }
        let hash = identity_hash(&extra.bytes);
        overflow.push(Overflow {
            field: extra.field.clone(),
            part: extra.part,
            hash,
            size: extra.bytes.len() as u64,
        });
        files.push((overflow_path(&device, &hash), extra.bytes.clone()));
    }
    loop {
        let body = payload(&w, fields, &lists, &overflow);
        let sig = envelope::sign(
            key,
            Signer::Device(device),
            "device",
            library,
            &Place {
                seq: counter,
                ..Place::default()
            },
            &body,
        );
        let bytes = envelope::write("device", &body, &[sig]);
        if bytes.len() <= MAX_DEVICE_FILE {
            // The parts must fit a reader's value budget (`lists`), or no peer would
            // take the file.
            let mut values = 0u64;
            for (_, b) in files.iter().skip(extras.len()) {
                let v = json::parse(b, Numbers::Integers).map_err(|_| DeviceError::Overflow)?;
                values += json::count_values(&v);
            }
            if values > MAX_OVERFLOW_VALUES {
                return Err(DeviceError::TooLarge);
            }
            return Ok(Written {
                counter,
                hash: crypto::payload_hash(&body),
                path: device_path(&device),
                bytes,
                parts: files,
            });
        }
        // The largest list still in the file moves out whole.
        let Some((name, items)) = lists
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .max_by_key(|(_, v)| Value::Array((*v).clone()).to_string().len())
            .map(|(n, v)| (*n, v.clone()))
        else {
            // Nothing left to move: the overflow list itself is too long (a manifest,
            // SP16, would hold it; no device carries that many parts yet).
            return Err(DeviceError::TooLarge);
        };
        for (i, b) in parts(name, &items)?.into_iter().enumerate() {
            let hash = identity_hash(&b);
            overflow.push(Overflow {
                field: name.to_owned(),
                part: i as u64,
                hash,
                size: b.len() as u64,
            });
            files.push((overflow_path(&device, &hash), b));
        }
        lists.insert(name, Vec::new());
    }
}

fn opt_hash(v: Option<&Value>) -> Result<Option<Hash32>> {
    match v {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Hash32::parse(s).map(Some).ok_or(DeviceError::Shape),
        _ => Err(DeviceError::Shape),
    }
}

/// SP16: opens `device`'s device file, checked against the key that device's ID
/// names and signed for its counter. The caller has fetched at most
/// `MAX_DEVICE_FILE` bytes.
pub fn open(bytes: &[u8], library: &Id16, device: &DeviceId, key: &[u8; 32]) -> Result<DeviceFile> {
    if bytes.len() > MAX_DEVICE_FILE {
        return Err(DeviceError::TooLarge);
    }
    let e = envelope::open(bytes, "device", library)?;
    let b = &e.body;
    match b.get("format").and_then(Value::as_u64) {
        Some(1) => {}
        Some(_) => return Err(DeviceError::Format),
        None => return Err(DeviceError::Shape),
    }
    if b.get("device").and_then(Value::as_str) != Some(&device.to_string()) {
        return Err(DeviceError::Device);
    }
    let named: [u8; 32] = b
        .get("key")
        .and_then(Value::as_str)
        .and_then(crate::sync::ids::b64url_decode)
        .and_then(|k| k.try_into().ok())
        .ok_or(DeviceError::Shape)?;
    if named != *key || crate::sync::ids::device_id(key) != *device {
        return Err(DeviceError::Device);
    }
    let writer = b
        .get("writer")
        .and_then(Value::as_str)
        .and_then(Id16::parse)
        .ok_or(DeviceError::Shape)?;
    let counter = b
        .get("counter")
        .and_then(Value::as_u64)
        .filter(|c| (1..=MAX_SAFE_INTEGER).contains(c))
        .ok_or(DeviceError::Shape)?;
    let name = b
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| n.chars().count() <= MAX_NAME_CHARS)
        .ok_or(DeviceError::Shape)?
        .to_owned();
    let clock = b
        .get("clock")
        .and_then(Value::as_object)
        .ok_or(DeviceError::Shape)?;
    let hlc = clock
        .get("hlc")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 2)
        .and_then(|a| Some((a[0].as_u64()?, u16::try_from(a[1].as_u64()?).ok()?)))
        // SP10: milliseconds are 48 bits.
        .filter(|(ms, n)| super::clock::Hlc(*ms, *n).is_valid())
        .ok_or(DeviceError::Shape)?;
    let ahead = clock
        .get("ahead")
        .and_then(Value::as_bool)
        .ok_or(DeviceError::Shape)?;
    let own = match b.get("own") {
        Some(Value::Null) => None,
        Some(Value::Object(o)) => Some((
            o.get("s")
                .and_then(Value::as_u64)
                .filter(|s| (1..=MAX_SAFE_INTEGER).contains(s))
                .ok_or(DeviceError::Shape)?,
            o.get("h")
                .and_then(Value::as_str)
                .and_then(Hash32::parse)
                .ok_or(DeviceError::Shape)?,
        )),
        _ => return Err(DeviceError::Shape),
    };
    let mut lists = BTreeMap::new();
    for name in LISTS {
        let items = b
            .get(name)
            .and_then(Value::as_array)
            .ok_or(DeviceError::Shape)?;
        lists.insert(name.to_owned(), items.clone());
    }
    let overflow = b
        .get("overflow")
        .and_then(Value::as_array)
        .ok_or(DeviceError::Shape)?
        .iter()
        .map(|o| {
            let field = o
                .get("field")
                .and_then(Value::as_str)
                .ok_or(DeviceError::Shape)?;
            if !LISTS.contains(&field) && !matches!(field, "copy" | "bundle") {
                // A manifest (SP16) is read by a Linen that writes one.
                return Err(DeviceError::Shape);
            }
            Ok(Overflow {
                field: field.to_owned(),
                part: o
                    .get("part")
                    .and_then(Value::as_u64)
                    .ok_or(DeviceError::Shape)?,
                hash: o
                    .get("hash")
                    .and_then(Value::as_str)
                    .and_then(Hash32::parse)
                    .ok_or(DeviceError::Shape)?,
                size: o
                    .get("size")
                    .and_then(Value::as_u64)
                    .filter(|s| *s <= MAX_OVERFLOW_FILE as u64)
                    .ok_or(DeviceError::Shape)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // A device file has one signer, its device, for its counter (SP6).
    if e.sigs.len() != 1
        || !e.verified(
            &Signer::Device(*device),
            key,
            library,
            &Place {
                seq: counter,
                ..Place::default()
            },
        )
    {
        return Err(DeviceError::Signature);
    }
    Ok(DeviceFile {
        device: *device,
        writer,
        counter,
        key: *key,
        fields: Fields {
            name,
            clock: hlc,
            ahead,
            head: opt_hash(b.get("head"))?,
            own,
            view: opt_hash(b.get("view"))?,
            lists,
        },
        overflow,
        hash: e.hash,
    })
}

impl DeviceFile {
    /// SP16: each list whole, the file's own items followed by its overflow parts in
    /// `part` order, each part checked against its hash, size and field. `fetch`
    /// reads one overflow file by path; one not there yet is `Missing`.
    pub fn lists(
        &self,
        fetch: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<BTreeMap<String, Vec<Value>>> {
        self.lists_within(fetch, MAX_OVERFLOW_VALUES)
    }

    /// `lists`, its parts building at most `max_values` values in all.
    fn lists_within(
        &self,
        fetch: &dyn Fn(&str) -> Option<Vec<u8>>,
        max_values: u64,
    ) -> Result<BTreeMap<String, Vec<Value>>> {
        let mut left = max_values;
        let mut lists = self.fields.lists.clone();
        let mut by_field: BTreeMap<&str, Vec<&Overflow>> = BTreeMap::new();
        for o in &self.overflow {
            // SP30: a proof's copies and bundles aren't lists. Reading the lists
            // never fetches them: only the proof reader does (`relay::read`), and
            // it checks each one's hash and size before using it.
            if matches!(o.field.as_str(), "copy" | "bundle") {
                continue;
            }
            by_field.entry(o.field.as_str()).or_default().push(o);
        }
        for (field, mut ps) in by_field {
            ps.sort_by_key(|o| o.part);
            for (i, o) in ps.iter().enumerate() {
                if o.part != i as u64 {
                    return Err(DeviceError::Overflow);
                }
                let path = overflow_path(&self.device, &o.hash);
                let bytes = fetch(&path).ok_or(DeviceError::Missing(path))?;
                if bytes.len() as u64 != o.size || identity_hash(&bytes) != o.hash {
                    return Err(DeviceError::Overflow);
                }
                let (v, rest) = match json::parse_within(&bytes, Numbers::Integers, left) {
                    Ok(got) => got,
                    Err(json::JsonError::TooManyValues) => return Err(DeviceError::TooLarge),
                    Err(_) => return Err(DeviceError::Overflow),
                };
                left = rest;
                let ok = v.get("field").and_then(Value::as_str) == Some(field)
                    && v.get("part").and_then(Value::as_u64) == Some(o.part);
                let items = v.get("items").and_then(Value::as_array).filter(|_| ok);
                lists
                    .entry(field.to_owned())
                    .or_default()
                    .extend(items.ok_or(DeviceError::Overflow)?.iter().cloned());
            }
        }
        Ok(lists)
    }
}

/// SP16, SP15: this device's next device file, signed at the next counter and kept
/// in the device store (it replaces an earlier one not yet uploaded, which it
/// supersedes) before it is returned for upload. The store changes only once the
/// save succeeds. PROVISIONAL: only the newest waits (decisions.md, (4)).
pub fn publish(
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    library: &Id16,
    fields: &Fields,
) -> Result<Written> {
    let state = store.library(library).ok_or(StoreError::Unreadable)?;
    let key = state.key()?;
    let writer = state.writer_id()?;
    // A stored counter out of range is refused before anything is added to it.
    if state.counter >= MAX_SAFE_INTEGER {
        return Err(DeviceError::Store(StoreError::Unreadable));
    }
    let counter = state.counter + 1;
    let written = write(&key, library, &writer, counter, fields)?;
    let mut next = store.clone();
    let state = next.library_mut(library).expect("checked above");
    state.counter = counter;
    state.unsent.retain(|u| !u.path.starts_with("devices/"));
    for (path, bytes) in std::iter::once((&written.path, &written.bytes))
        .chain(written.parts.iter().map(|(p, b)| (p, b)))
    {
        state.unsent.push(Unsent {
            path: path.clone(),
            bytes: b64url_encode(bytes),
        });
    }
    next.save(ks)?;
    *store = next;
    Ok(written)
}

/// SP15: this device's device file and overflow files, signed and not yet seen in
/// storage, to upload again: the parts first, then the file that names them. The
/// file is opened as a peer would, and must be at the stored counter.
pub fn pending(store: &DeviceStore, library: &Id16) -> Result<Vec<(String, Vec<u8>)>> {
    let Some(state) = store.library(library) else {
        return Ok(Vec::new());
    };
    let device = state.device_id()?;
    let key = state.key()?.public();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut main = None;
    for u in state
        .unsent
        .iter()
        .filter(|u| u.path.starts_with("devices/"))
    {
        let bytes = u.bytes().ok_or(StoreError::Unreadable)?;
        if u.path == device_path(&device) {
            let f = open(&bytes, library, &device, &key)?;
            if f.counter != state.counter {
                return Err(DeviceError::Store(StoreError::Unreadable));
            }
            main = Some((u.path.clone(), bytes, f));
        } else {
            files.push((u.path.clone(), bytes));
        }
    }
    let Some((path, bytes, f)) = main else {
        return if files.is_empty() {
            Ok(Vec::new())
        } else {
            Err(DeviceError::Store(StoreError::Unreadable))
        };
    };
    // SP15: parts already uploaded have left the queue; every part still queued
    // is one the file names, with that hash and size.
    for (p, b) in &files {
        let named = f.overflow.iter().any(|o| {
            overflow_path(&device, &o.hash) == *p
                && identity_hash(b) == o.hash
                && b.len() as u64 == o.size
        });
        if !named {
            return Err(DeviceError::Store(StoreError::Unreadable));
        }
    }
    files.push((path, bytes));
    Ok(files)
}

/// What reading a peer's device file came to.
#[derive(Debug)]
pub enum Accepted {
    /// Newer than any accepted before, with its lists whole: its floor moved up
    /// to it.
    New(Box<DeviceFile>, BTreeMap<String, Vec<Value>>),
    /// A counter at or below the last accepted: ignored (SP18), not an error.
    Stale,
}

/// SP18, SP29: a peer's device file is accepted only with a higher counter than
/// the last accepted, and only when its `own` doesn't contradict the floor (a
/// latest change before the floor's, or the floor's sequence with another hash).
/// Its overflow parts are fetched and checked first (`fetch`), so a part not there
/// yet (`Missing`) or not as named leaves the floor where it was and the file can
/// be read again. Accepting it raises the floor: its counter, and its `own` when
/// that is newer. The store changes only once the save succeeds. PROVISIONAL: no `own` while the
/// floor has a change is refused too (decisions.md, (5)).
pub fn accept(
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    library: &Id16,
    device: &DeviceId,
    key: &[u8; 32],
    bytes: &[u8],
    fetch: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Accepted> {
    let f = open(bytes, library, device, key)?;
    let state = store.library(library).ok_or(StoreError::Unreadable)?;
    let floor = state
        .floors
        .get(&device.to_string())
        .cloned()
        .unwrap_or_default();
    if f.counter <= floor.counter {
        return Ok(Accepted::Stale);
    }
    let held = floor.change()?;
    match (held, f.fields.own) {
        (Some((s, _)), None) if s > 0 => return Err(DeviceError::BelowFloor),
        (Some((s, _)), Some((o, _))) if o < s => return Err(DeviceError::BelowFloor),
        (Some((s, h)), Some((o, oh))) if o == s && oh != h => return Err(DeviceError::BelowFloor),
        _ => {}
    }
    let lists = f.lists(fetch)?;
    let mut raised = Floor {
        counter: f.counter,
        ..floor
    };
    if let Some((o, oh)) = f.fields.own {
        if held.map_or(true, |(s, _)| o > s) {
            // The checkpoint it replaces stays until the chain applied here passes
            // it: a newer `own` proves nothing about what lies before it.
            if let Some((s, h)) = held {
                raised.earlier.push(Checkpoint {
                    s,
                    h: h.to_string(),
                });
            }
            raised.s = o;
            raised.h = Some(oh.to_string());
        }
    }
    let mut next = store.clone();
    next.library_mut(library)
        .expect("checked above")
        .floors
        .insert(device.to_string(), raised);
    next.save(ks)?;
    *store = next;
    Ok(Accepted::New(Box::new(f), lists))
}

/// SP18: every change `device`'s floor names (its newest and the earlier ones the
/// chain applied here hasn't passed), which `chain::receive_run` holds its change
/// files to.
pub fn floor_of(
    store: &DeviceStore,
    library: &Id16,
    device: &DeviceId,
) -> Result<Vec<(u64, Hash32)>> {
    Ok(match store.library(library) {
        Some(state) => match state.floors.get(&device.to_string()) {
            Some(f) => f.checkpoints()?,
            None => Vec::new(),
        },
        None => Vec::new(),
    })
}

/// SP18: raise the floor after accepting a view, saving before changing memory.
pub fn raise_view_generation(
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    library: &Id16,
    device: &DeviceId,
    gen: u64,
) -> Result<()> {
    let mut next = store.clone();
    let state = next.library_mut(library).ok_or(StoreError::Unreadable)?;
    let floor = state.floors.entry(device.to_string()).or_default();
    if gen > floor.gen {
        floor.gen = gen;
        next.save(ks)?;
        *store = next;
    }
    Ok(())
}

/// SP18: once the chain applied here reaches `applied` (checked against the
/// checkpoints by `chain::receive_run`), the earlier checkpoints at or below it
/// are passed and leave the floor. The store changes only once the save succeeds.
pub fn prune_floors(
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    library: &Id16,
    device: &DeviceId,
    applied: u64,
) -> Result<()> {
    let mut next = store.clone();
    let Some(f) = next
        .library_mut(library)
        .and_then(|st| st.floors.get_mut(&device.to_string()))
    else {
        return Ok(());
    };
    let before = f.earlier.len();
    f.earlier.retain(|c| c.s > applied);
    if f.earlier.len() != before {
        next.save(ks)?;
        *store = next;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::device_store::Memory;

    const LIB: Id16 = Id16([7; 16]);

    fn fields() -> Fields {
        let mut lists = BTreeMap::new();
        lists.insert("frontier".to_owned(), vec![json!({ "d": "x", "s": 1 })]);
        Fields {
            name: "Quentin's MacBook".into(),
            clock: (1_760_000_000_000, 3),
            ahead: false,
            head: Some(Hash32([4; 32])),
            own: Some((2, Hash32([5; 32]))),
            view: None,
            lists,
        }
    }

    fn device() -> (Memory, DeviceStore, Keypair) {
        let ks = Memory::default();
        let mut store = DeviceStore::default();
        let key = store.identity(&LIB).unwrap().key().unwrap();
        store.save(&ks).unwrap();
        (ks, store, key)
    }

    fn no_files(_: &str) -> Option<Vec<u8>> {
        None
    }

    #[test]
    fn a_device_file_round_trips_and_is_checked_against_its_key() {
        let (_ks, _store, key) = device();
        let w = write(&key, &LIB, &Id16([2; 16]), 1, &fields()).unwrap();
        assert_eq!(w.path, device_path(&key.device()));
        assert!(w.parts.is_empty());
        let f = open(&w.bytes, &LIB, &key.device(), &key.public()).unwrap();
        assert_eq!((f.counter, f.writer), (1, Id16([2; 16])));
        let mut expected = fields();
        for name in LISTS {
            expected.lists.entry(name.to_owned()).or_default();
        }
        assert_eq!(f.fields, expected);
        assert_eq!(f.lists(&no_files).unwrap(), expected.lists);
        // Another key, or read as another device's file.
        let other = Keypair::from_seed(&[9; 32]);
        assert!(matches!(
            open(&w.bytes, &LIB, &key.device(), &other.public()),
            Err(DeviceError::Device)
        ));
        assert!(matches!(
            open(&w.bytes, &LIB, &other.device(), &other.public()),
            Err(DeviceError::Device)
        ));
        // Signed for its counter: the same payload signed for another counter fails.
        let e: Value = serde_json::from_slice(&w.bytes).unwrap();
        let payload = crate::sync::ids::b64url_decode(e["payload"].as_str().unwrap()).unwrap();
        let wrong = envelope::sign(
            &key,
            Signer::Device(key.device()),
            "device",
            &LIB,
            &Place {
                seq: 2,
                ..Place::default()
            },
            &payload,
        );
        let resigned = envelope::write("device", &payload, &[wrong]);
        assert!(matches!(
            open(&resigned, &LIB, &key.device(), &key.public()),
            Err(DeviceError::Signature)
        ));
    }

    #[test]
    fn a_clock_beyond_48_bits_is_refused() {
        let (_ks, _store, key) = device();
        let mut f = fields();
        f.clock = (1 << 48, 0);
        let w = write(&key, &LIB, &Id16([2; 16]), 1, &f).unwrap();
        assert!(matches!(
            open(&w.bytes, &LIB, &key.device(), &key.public()),
            Err(DeviceError::Shape)
        ));
    }

    #[test]
    fn a_name_is_cut_to_64_characters() {
        let (_ks, _store, key) = device();
        let mut f = fields();
        f.name = "é".repeat(80);
        let w = write(&key, &LIB, &Id16([2; 16]), 1, &f).unwrap();
        let opened = open(&w.bytes, &LIB, &key.device(), &key.public()).unwrap();
        assert_eq!(opened.fields.name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn long_lists_go_to_overflow_files_and_come_back_whole() {
        let (_ks, _store, key) = device();
        let mut f = fields();
        let frontier: Vec<Value> = (0..4000)
            .map(|i| json!({ "d": format!("{i:032x}"), "s": i, "h": "ab".repeat(32), "sig": "c".repeat(86) }))
            .collect();
        f.lists.insert("frontier".into(), frontier.clone());
        let w = write(&key, &LIB, &Id16([2; 16]), 1, &f).unwrap();
        assert!(w.bytes.len() <= MAX_DEVICE_FILE);
        assert!(!w.parts.is_empty());
        let opened = open(&w.bytes, &LIB, &key.device(), &key.public()).unwrap();
        assert!(opened.fields.lists["frontier"].is_empty());
        let fetch = |p: &str| w.parts.iter().find(|(q, _)| q == p).map(|(_, b)| b.clone());
        assert_eq!(opened.lists(&fetch).unwrap()["frontier"], frontier);
        // A part not there yet waits; one changed is refused.
        assert!(matches!(
            opened.lists(&no_files),
            Err(DeviceError::Missing(_))
        ));
        let changed = |p: &str| {
            fetch(p).map(|mut b| {
                let at = b.len() - 3;
                b[at] ^= 1;
                b
            })
        };
        assert!(matches!(opened.lists(&changed), Err(DeviceError::Overflow)));
    }

    #[test]
    fn publishing_moves_the_counter_only_once_saved_and_queues_the_newest() {
        struct Refuses;
        impl Keystore for Refuses {
            fn load(&self) -> std::result::Result<Option<Vec<u8>>, StoreError> {
                Ok(None)
            }
            fn save(&self, _: &[u8]) -> std::result::Result<(), StoreError> {
                Err(StoreError::Keychain(-25308))
            }
        }
        let (ks, mut store, key) = device();
        let before = store.clone();
        assert!(publish(&Refuses, &mut store, &LIB, &fields()).is_err());
        assert_eq!(store, before);
        let one = publish(&ks, &mut store, &LIB, &fields()).unwrap();
        let two = publish(&ks, &mut store, &LIB, &fields()).unwrap();
        assert_eq!((one.counter, two.counter), (1, 2));
        // Only the newest is queued: it supersedes the first.
        let reloaded = DeviceStore::load(&ks).unwrap();
        assert_eq!(
            pending(&reloaded, &LIB).unwrap(),
            vec![(device_path(&key.device()), two.bytes.clone())]
        );
        // A queued file at another counter than the store's is refused.
        let mut wrong = reloaded.clone();
        wrong.library_mut(&LIB).unwrap().counter = 5;
        assert!(pending(&wrong, &LIB).is_err());
    }

    #[test]
    fn an_item_too_large_for_a_part_and_a_counter_out_of_range_are_refused() {
        let (ks, mut store, key) = device();
        let mut f = fields();
        f.lists
            .insert("proofs".into(), vec![json!("x".repeat(MAX_OVERFLOW_FILE))]);
        assert!(matches!(
            write(&key, &LIB, &Id16([2; 16]), 1, &f),
            Err(DeviceError::TooLarge)
        ));
        // Refused before signing: the counter didn't move.
        assert!(publish(&ks, &mut store, &LIB, &f).is_err());
        assert_eq!(store.library(&LIB).unwrap().counter, 0);
        for counter in [MAX_SAFE_INTEGER, u64::MAX] {
            let mut bad = store.clone();
            bad.library_mut(&LIB).unwrap().counter = counter;
            assert!(matches!(
                publish(&ks, &mut bad, &LIB, &fields()),
                Err(DeviceError::Store(StoreError::Unreadable))
            ));
        }
    }

    #[test]
    fn a_file_past_the_readers_value_budget_is_never_signed() {
        let (ks, mut store, key) = device();
        let mut f = fields();
        f.lists.insert(
            "marks".into(),
            (0..MAX_OVERFLOW_VALUES).map(|i| json!(i)).collect(),
        );
        assert!(matches!(
            write(&key, &LIB, &Id16([2; 16]), 1, &f),
            Err(DeviceError::TooLarge)
        ));
        assert!(publish(&ks, &mut store, &LIB, &f).is_err());
        assert_eq!(store.library(&LIB).unwrap().counter, 0);
    }

    #[test]
    fn a_device_file_with_overflow_is_queued_with_its_parts_first() {
        let (ks, mut store, key) = device();
        let mut f = fields();
        f.lists.insert(
            "repairs".into(),
            (0..4000)
                .map(|i| json!({ "name": format!("{i:064x}"), "hash": "ab".repeat(32) }))
                .collect(),
        );
        let w = publish(&ks, &mut store, &LIB, &f).unwrap();
        let queued = pending(&store, &LIB).unwrap();
        assert_eq!(queued.len(), w.parts.len() + 1);
        assert_eq!(queued.last().unwrap().0, device_path(&key.device()));
        // SP15: a part uploaded leaves the queue, and the rest still go, the file
        // last: an upload stopped part-way resumes.
        let (first, _) = &queued[0];
        uploaded_part(&ks, &mut store, first);
        let rest = pending(&store, &LIB).unwrap();
        assert_eq!(rest.len(), w.parts.len());
        assert_eq!(rest.last().unwrap().0, device_path(&key.device()));
        // A queued part the file doesn't name is an error, never uploaded.
        let mut stranger = store.clone();
        stranger.library_mut(&LIB).unwrap().unsent.insert(
            0,
            Unsent {
                path: overflow_path(&key.device(), &Hash32([1; 32])),
                bytes: b64url_encode(b"{}"),
            },
        );
        assert!(pending(&stranger, &LIB).is_err());
    }

    fn uploaded_part(ks: &Memory, store: &mut DeviceStore, path: &str) {
        let mut next = store.clone();
        next.library_mut(&LIB)
            .unwrap()
            .unsent
            .retain(|u| u.path != path);
        next.save(ks).unwrap();
        *store = next;
    }

    #[test]
    fn a_peers_file_is_accepted_only_with_its_parts_and_within_the_value_budget() {
        let (ks, mut store, _mine) = device();
        let peer = Keypair::from_seed(&[3; 32]);
        let (pd, pk) = (peer.device(), peer.public());
        let mut f = fields();
        f.lists
            .insert("marks".into(), (0..60_000).map(|i| json!(i)).collect());
        let w = write(&peer, &LIB, &Id16([2; 16]), 1, &f).unwrap();
        assert!(!w.parts.is_empty());
        // A part not in storage yet: nothing accepted, so the file can be read again.
        assert!(matches!(
            accept(&ks, &mut store, &LIB, &pd, &pk, &w.bytes, &no_files),
            Err(DeviceError::Missing(_))
        ));
        assert!(store.library(&LIB).unwrap().floors.is_empty());
        let fetch = |p: &str| w.parts.iter().find(|(q, _)| q == p).map(|(_, b)| b.clone());
        let Accepted::New(_, lists) =
            accept(&ks, &mut store, &LIB, &pd, &pk, &w.bytes, &fetch).unwrap()
        else {
            panic!("accepted");
        };
        assert_eq!(lists["marks"].len(), 60_000);
        // Past the value budget, the parts are refused before they are all built.
        let opened = open(&w.bytes, &LIB, &pd, &pk).unwrap();
        assert!(matches!(
            opened.lists_within(&fetch, 1000),
            Err(DeviceError::TooLarge)
        ));
    }

    #[test]
    fn a_peers_device_file_is_accepted_only_above_its_floor() {
        let (ks, mut store, _mine) = device();
        let peer = Keypair::from_seed(&[3; 32]);
        let (pd, pk) = (peer.device(), peer.public());
        let file = |counter: u64, own: Option<(u64, Hash32)>| {
            let mut f = fields();
            f.own = own;
            write(&peer, &LIB, &Id16([2; 16]), counter, &f)
                .unwrap()
                .bytes
        };
        let h = |n: u8| Hash32([n; 32]);
        assert!(matches!(
            accept(
                &ks,
                &mut store,
                &LIB,
                &pd,
                &pk,
                &file(2, Some((5, h(5)))),
                &no_files
            )
            .unwrap(),
            Accepted::New(..)
        ));
        let floor = store.library(&LIB).unwrap().floors[&pd.to_string()].clone();
        assert_eq!(
            (floor.counter, floor.s, floor.h.clone()),
            (2, 5, Some(h(5).to_string()))
        );
        // The same counter, or a lower one: ignored.
        for counter in [2, 1] {
            assert!(matches!(
                accept(
                    &ks,
                    &mut store,
                    &LIB,
                    &pd,
                    &pk,
                    &file(counter, Some((6, h(6)))),
                    &no_files
                )
                .unwrap(),
                Accepted::Stale
            ));
        }
        // A higher counter naming a latest change before the floor, the floor's
        // sequence with another hash, or none at all: refused.
        for own in [Some((4, h(4))), Some((5, h(9))), None] {
            assert!(matches!(
                accept(&ks, &mut store, &LIB, &pd, &pk, &file(3, own), &no_files),
                Err(DeviceError::BelowFloor)
            ));
        }
        // A higher counter with a newer change raises the floor, and it was saved.
        accept(
            &ks,
            &mut store,
            &LIB,
            &pd,
            &pk,
            &file(3, Some((7, h(7)))),
            &no_files,
        )
        .unwrap();
        // The checkpoint it replaced stays until the chain applied here passes it.
        let saved = DeviceStore::load(&ks).unwrap();
        assert_eq!(
            floor_of(&saved, &LIB, &pd).unwrap(),
            vec![(5, h(5)), (7, h(7))]
        );
        prune_floors(&ks, &mut store, &LIB, &pd, 4).unwrap();
        assert_eq!(floor_of(&store, &LIB, &pd).unwrap().len(), 2);
        prune_floors(&ks, &mut store, &LIB, &pd, 5).unwrap();
        let pruned = DeviceStore::load(&ks).unwrap();
        assert_eq!(floor_of(&pruned, &LIB, &pd).unwrap(), vec![(7, h(7))]);
    }
}
