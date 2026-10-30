//! SP17, SY11: a generation's signed index, shards, spine and open state.
//!
//! Pure bytes: no compaction, persistence, publication or SP18 reading order.
//! Parts contain no paths, and the index names only their hashes and sizes. Build
//! the parts, hash and sign the index payload, then derive every path from that
//! payload hash (SP5), under `state/‹device›/‹index hash›/`; there is no hash cycle.
//!
//! PROVISIONAL: binary size units match device_file; the shared value budget and
//! rotation cap below bound decoded memory, as device_file's overflow budget does.

use super::chain::Change;
use super::cleanup::{Name, Week};
use super::crypto::{self, Keypair};
use super::envelope::{self, EnvelopeError, Place, Signer};
use super::hold::{self, Hold, ItemId, Kind, Reserve};
use super::ids::{b64url_decode, b64url_encode, identity_hash, DeviceId, Hash32, Id16};
use super::json::{self, Numbers, MAX_SAFE_INTEGER};
use super::record::{self, Entry, Key};
use super::trust::Mark;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// SP8, SP17: PROVISIONAL 8 MB, in the same units as device overflow files.
pub const MAX_PART: usize = super::device_file::MAX_OVERFLOW_FILE;
/// SP8: the signed index, including its envelope, is at most 1 MB.
pub const MAX_INDEX: usize = 1 << 20;
/// SP3, SY28: PROVISIONAL combined budget for the index and all decoded parts.
pub const MAX_VALUES: u64 = 1_000_000;
/// SP17, SP27, SY28: PROVISIONAL retained succession references. Rotations are
/// rare; 64 bounds the otherwise unbounded list independently of its byte size.
pub const MAX_ROTATIONS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartRef {
    pub hash: Hash32,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShardRef {
    pub lo: u16,
    pub hi: u16,
    pub hash: Hash32,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpineRef {
    pub from: u64,
    pub to: u64,
    pub hash: Hash32,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub gen: u64,
    pub prev: Option<Hash32>,
    pub through: Mark,
    pub shards: Vec<ShardRef>,
    pub spine: Vec<SpineRef>,
    pub open: PartRef,
}

/// SP17: one retirement, kept across generations until SP36 permits removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retirement {
    pub v: Entry,
    pub objects: Vec<Name>,
}

impl Retirement {
    /// SP17, SP36: inputs to cleanup; this conversion makes no removal decision.
    pub fn weeks(&self) -> impl Iterator<Item = Week> + '_ {
        self.objects.iter().map(|name| Week {
            name: *name,
            retiring: self.v,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub key: Key,
    /// SP11, SY9: the full version, including history and unknown fields.
    pub version: Value,
    /// Empty writes no `week` member; the decided encoding otherwise is a list.
    pub week: Vec<Retirement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shard {
    pub records: Vec<Record>,
    pub marks: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpineEntry {
    pub s: u64,
    pub h: Hash32,
    pub p: Option<Hash32>,
    pub sig: [u8; 64],
    /// SP17: unpadded base64url, 48 bytes per version in change-file order.
    pub v: String,
}

impl SpineEntry {
    /// SP6, SP17: retain a checked change's proof, including its exact vroot list.
    pub fn from_change(change: &Change) -> Self {
        let v: Vec<u8> = change
            .versions
            .iter()
            .flat_map(|v| {
                envelope::vlist_entry(v["key"].as_str().unwrap_or(""), &record::version_hash(v))
            })
            .collect();
        Self {
            s: change.seq,
            h: change.hash,
            p: change.prev,
            sig: change.sig,
            v: b64url_encode(&v),
        }
    }

    fn write(&self) -> Value {
        json!({"s": self.s, "h": self.h.to_string(),
            "p": self.p.map(|h| h.to_string()), "sig": b64url_encode(&self.sig), "v": self.v})
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Open {
    pub reserve: Vec<(ItemId, Reserve)>,
    pub hold: Vec<(ItemId, Hold)>,
    /// SP27: payload hashes of signed rotate records, resolved by the caller.
    pub rotations: Vec<Hash32>,
}

impl Open {
    fn write(&self) -> Value {
        json!({
            "reserve": self.reserve.iter().map(|(id, r)| json!({"id": item_id(id), "item": r.write()})).collect::<Vec<_>>(),
            "hold": self.hold.iter().map(|(id, h)| json!({"id": item_id(id), "item": h.write()})).collect::<Vec<_>>(),
            "rotations": self.rotations.iter().map(|h| json!({"hash": h.to_string()})).collect::<Vec<_>>()
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub index_path: String,
    pub index_bytes: Vec<u8>,
    pub parts: Vec<(String, Vec<u8>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub gen: u64,
    pub prev: Option<Hash32>,
    pub through: Mark,
    pub hash: Hash32,
    pub index: Index,
    pub shards: Vec<Shard>,
    pub spine: Vec<SpineEntry>,
    pub open: Open,
}

#[derive(Debug, thiserror::Error)]
pub enum ViewError {
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
    #[error(transparent)]
    Json(#[from] json::JsonError),
    #[error(transparent)]
    Record(#[from] record::RecordError),
    #[error(transparent)]
    Hold(#[from] hold::HoldError),
    #[error("larger than SP8/SP17 allows")]
    TooLarge,
    #[error("not SP17's view shape")]
    Shape,
    #[error("a view format this Linen doesn't know")]
    Format,
    #[error("written by another device than expected")]
    Device,
    #[error("not signed by its device for its generation, or an invalid spine signature")]
    Signature,
    #[error("a part doesn't match its named hash or size")]
    Part,
    #[error("shard ranges don't cover 0–65535 in order, or a record is in the wrong shard")]
    Shards,
    #[error("a key appears twice")]
    DuplicateKey,
    #[error("the spine doesn't link through the named change, or shards don't hold all its latest versions")]
    Spine,
    #[error("a part isn't in storage yet: {0}")]
    Missing(String),
}

type Result<T> = std::result::Result<T, ViewError>;

fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value> {
    v.get(name).ok_or(ViewError::Shape)
}

fn list<'a>(v: &'a Value, name: &str) -> Result<&'a [Value]> {
    field(v, name)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or(ViewError::Shape)
}

fn hash(v: &Value) -> Result<Hash32> {
    v.as_str().and_then(Hash32::parse).ok_or(ViewError::Shape)
}

fn optional_hash(v: &Value) -> Result<Option<Hash32>> {
    if v.is_null() {
        Ok(None)
    } else {
        hash(v).map(Some)
    }
}

fn sequence(v: &Value) -> Result<u64> {
    v.as_u64()
        .filter(|s| (1..=MAX_SAFE_INTEGER).contains(s))
        .ok_or(ViewError::Shape)
}

fn device_id(v: &Value) -> Result<DeviceId> {
    v.as_str().and_then(Id16::parse).ok_or(ViewError::Shape)
}

fn entry(e: &Entry) -> Value {
    json!({"d": e.device.to_string(), "s": e.seq, "h": e.hash.to_string()})
}

fn read_entry(v: &Value) -> Result<Entry> {
    Ok(Entry {
        device: device_id(field(v, "d")?)?,
        seq: sequence(field(v, "s")?)?,
        hash: hash(field(v, "h")?)?,
    })
}

fn item_id(id: &ItemId) -> Value {
    json!({"d": id.device.to_string(), "s": id.seq, "i": id.index})
}

fn read_item_id(v: &Value) -> Result<ItemId> {
    Ok(ItemId {
        device: device_id(field(v, "d")?)?,
        seq: sequence(field(v, "s")?)?,
        index: field(v, "i")?
            .as_u64()
            .filter(|i| *i <= MAX_SAFE_INTEGER)
            .and_then(|i| usize::try_from(i).ok())
            .ok_or(ViewError::Shape)?,
    })
}

fn read_open(v: &Value, device: &DeviceId, through: u64) -> Result<Open> {
    let (rs, hs, rotations) = (list(v, "reserve")?, list(v, "hold")?, list(v, "rotations")?);
    if rs.len() > hold::MAX_RESERVATIONS
        || hs.len() > hold::MAX_HOLDS
        || rotations.len() > MAX_ROTATIONS
    {
        return Err(ViewError::Shape);
    }
    let mut reserve = Vec::new();
    let mut hold = Vec::new();
    let mut ids = HashSet::new();
    // SP17: open carries only this device's items already published by through.
    let own_id = |v: &Value| -> Result<ItemId> {
        let id = read_item_id(v)?;
        if id.device != *device {
            return Err(ViewError::Device);
        }
        if id.seq > through {
            return Err(ViewError::Shape);
        }
        Ok(id)
    };
    for r in rs {
        let id = own_id(field(r, "id")?)?;
        if !ids.insert((id.device, id.seq, id.index)) {
            return Err(ViewError::Shape);
        }
        reserve.push((
            id,
            Reserve::read(field(r, "item")?).ok_or(ViewError::Shape)?,
        ));
    }
    ids.clear();
    for h in hs {
        let id = own_id(field(h, "id")?)?;
        if !ids.insert((id.device, id.seq, id.index)) {
            return Err(ViewError::Shape);
        }
        hold.push((id, Hold::read(field(h, "item")?).ok_or(ViewError::Shape)?));
    }
    // SP33, SP34: enforce the byte limits too, before the caller uses open state.
    hold::read_lists(
        &reserve.iter().map(|(_, r)| r.write()).collect::<Vec<_>>(),
        &hold.iter().map(|(_, h)| h.write()).collect::<Vec<_>>(),
    )?;
    Ok(Open {
        reserve,
        hold,
        rotations: rotations
            .iter()
            .map(|v| hash(field(v, "hash")?))
            .collect::<Result<_>>()?,
    })
}

fn record_value(r: &Record) -> Value {
    let mut v = json!({"key": r.key.as_str(), "version": r.version});
    if !r.week.is_empty() {
        v["week"] = json!(r.week.iter().map(|w| json!({"v": entry(&w.v), "objects": w.objects.iter().map(|n| json!({
            "kind": match n.kind { Kind::Blob => "blob", Kind::Book => "book" }, "name": n.name.to_string()
        })).collect::<Vec<_>>() })).collect::<Vec<_>>());
    }
    v
}

fn read_record(v: &Value, device: &DeviceId) -> Result<Record> {
    let key = field(v, "key")?
        .as_str()
        .and_then(Key::parse)
        .map(|(k, _)| k)
        .ok_or(ViewError::Shape)?;
    let version = field(v, "version")?.clone();
    if version.get("key").and_then(Value::as_str) != Some(key.as_str()) {
        return Err(ViewError::Shape);
    }
    if version
        .get("device")
        .and_then(Value::as_str)
        .and_then(Id16::parse)
        != Some(*device)
    {
        return Err(ViewError::Device);
    }
    // SP11, SY9: validate known versions; keep newer major formats whole.
    record::check_version(version.clone(), *device)?;
    let mut week = Vec::new();
    if v.get("week").is_some() {
        for w in list(v, "week")? {
            let retiring = read_entry(field(w, "v")?)?;
            let raw = list(w, "objects")?;
            if raw.is_empty() {
                return Err(ViewError::Shape);
            }
            let objects = raw
                .iter()
                .map(|n| {
                    Ok(Name {
                        kind: match field(n, "kind")?.as_str() {
                            Some("blob") => Kind::Blob,
                            Some("book") => Kind::Book,
                            _ => return Err(ViewError::Shape),
                        },
                        name: hash(field(n, "name")?)?,
                    })
                })
                .collect::<Result<_>>()?;
            week.push(Retirement {
                v: retiring,
                objects,
            });
        }
    }
    Ok(Record { key, version, week })
}

fn bucket(key: &Key) -> u16 {
    let h = key.spine_hash();
    u16::from_be_bytes([h[0], h[1]])
}

impl PartRef {
    fn read(v: &Value) -> Result<Self> {
        let size = field(v, "size")?
            .as_u64()
            .filter(|s| *s <= MAX_PART as u64)
            .ok_or(ViewError::TooLarge)?;
        Ok(Self {
            hash: hash(field(v, "hash")?)?,
            size,
        })
    }

    fn write(&self) -> Value {
        json!({"hash": self.hash.to_string(), "size": self.size})
    }
}

impl Index {
    fn write(&self, library: &Id16, device: &DeviceId, writer: &Id16) -> Value {
        json!({"type": "view", "format": 1, "library": library.to_string(),
            "device": device.to_string(), "writer": writer.to_string(),
            "gen": self.gen, "prev": self.prev.map(|h| h.to_string()),
            "through": {"s": self.through.s, "h": self.through.h.to_string()},
            "shards": self.shards.iter().map(|s| json!({"lo": s.lo, "hi": s.hi, "hash": s.hash.to_string(), "size": s.size})).collect::<Vec<_>>(),
            "spine": self.spine.iter().map(|s| json!({"from": s.from, "to": s.to, "hash": s.hash.to_string(), "size": s.size})).collect::<Vec<_>>(),
            "open": self.open.write()})
    }

    fn read(v: &Value) -> Result<Self> {
        match field(v, "format")?.as_u64() {
            Some(1) => {}
            Some(_) => return Err(ViewError::Format),
            None => return Err(ViewError::Shape),
        }
        device_id(field(v, "writer")?)?;
        let gen = sequence(field(v, "gen")?)?;
        let prev = optional_hash(field(v, "prev")?)?;
        if (gen == 1) != prev.is_none() {
            return Err(ViewError::Shape);
        }
        let through = field(v, "through")?;
        let through = Mark {
            s: sequence(field(through, "s")?)?,
            h: hash(field(through, "h")?)?,
        };
        let mut shards = Vec::new();
        let mut next = 0u32;
        for s in list(v, "shards")? {
            let part = PartRef::read(s)?;
            let bound = |name| -> Result<u16> {
                field(s, name)?
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or(ViewError::Shards)
            };
            let (lo, hi) = (bound("lo")?, bound("hi")?);
            if u32::from(lo) != next || hi < lo {
                return Err(ViewError::Shards);
            }
            next = u32::from(hi) + 1;
            shards.push(ShardRef {
                lo,
                hi,
                hash: part.hash,
                size: part.size,
            });
        }
        if next != 65536 {
            return Err(ViewError::Shards);
        }
        let mut spine = Vec::new();
        let mut next = 1;
        for s in list(v, "spine")? {
            let part = PartRef::read(s)?;
            let (from, to) = (sequence(field(s, "from")?)?, sequence(field(s, "to")?)?);
            if from != next || to < from {
                return Err(ViewError::Spine);
            }
            next = to + 1;
            spine.push(SpineRef {
                from,
                to,
                hash: part.hash,
                size: part.size,
            });
        }
        if next != through.s + 1 {
            return Err(ViewError::Spine);
        }
        Ok(Self {
            gen,
            prev,
            through,
            shards,
            spine,
            open: PartRef::read(field(v, "open")?)?,
        })
    }
}

fn read_spine(
    v: &Value,
    library: &Id16,
    device: &DeviceId,
    key: &[u8; 32],
) -> Result<(SpineEntry, Vec<[u8; 48]>)> {
    let s = sequence(field(v, "s")?)?;
    let h = hash(field(v, "h")?)?;
    let p = optional_hash(field(v, "p")?)?;
    let sig = field(v, "sig")?
        .as_str()
        .and_then(b64url_decode)
        .and_then(|b| b.try_into().ok())
        .ok_or(ViewError::Signature)?;
    let v = field(v, "v")?.as_str().ok_or(ViewError::Shape)?.to_owned();
    let bytes = b64url_decode(&v).ok_or(ViewError::Spine)?;
    if bytes.len() % 48 != 0 {
        return Err(ViewError::Spine);
    }
    let entries: Vec<[u8; 48]> = bytes
        .chunks_exact(48)
        .map(|b| b.try_into().unwrap())
        .collect();
    // SP6, SP17: verify the change's signature, including p and vroot, without
    // its original payload. A changed v list therefore cannot vouch for versions.
    let place = Place {
        seq: s,
        prev: p,
        vroot: Some(envelope::vroot(&entries)),
    };
    if !crypto::verify(
        key,
        &envelope::message("change", library, &Signer::Device(*device), &place, &h),
        &sig,
    ) {
        return Err(ViewError::Signature);
    }
    Ok((SpineEntry { s, h, p, sig, v }, entries))
}

fn directory(device: &DeviceId, hash: &Hash32) -> String {
    format!("state/{device}/{hash}")
}

/// SP18: only the previous-index link; floors and generation ordering belong to
/// the caller, along with checking the older index's own signature.
pub fn follows(newer: &View, older_hash: &Hash32) -> bool {
    newer.prev == Some(*older_hash)
}

/// SP17, SY28: authenticate the index before fetching, and authenticate each
/// part's size and hash before parsing. Only Missing is retryable here.
pub fn open(
    index_bytes: &[u8],
    library: &Id16,
    device: &DeviceId,
    key: &[u8; 32],
    fetch: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<View> {
    if index_bytes.len() > MAX_INDEX {
        return Err(ViewError::TooLarge);
    }
    let e = envelope::open_within(index_bytes, "view", library, MAX_VALUES)?;
    if e.body
        .get("device")
        .and_then(Value::as_str)
        .and_then(Id16::parse)
        != Some(*device)
    {
        return Err(ViewError::Device);
    }
    let raw = Value::Object(e.body.clone());
    let index = Index::read(&raw)?;
    if e.sigs.len() != 1
        || !e.verified(
            &Signer::Device(*device),
            key,
            library,
            &Place {
                seq: index.gen,
                ..Place::default()
            },
        )
    {
        return Err(ViewError::Signature);
    }
    let left = MAX_VALUES
        .checked_sub(json::count_values(&raw))
        .ok_or(json::JsonError::TooManyValues)?;
    read_parts(index, e.hash, library, device, key, fetch, left)
}

#[allow(clippy::too_many_arguments)]
fn read_parts(
    index: Index,
    index_hash: Hash32,
    library: &Id16,
    device: &DeviceId,
    key: &[u8; 32],
    fetch: &dyn Fn(&str) -> Option<Vec<u8>>,
    mut left: u64,
) -> Result<View> {
    let dir = directory(device, &index_hash);
    let mut part = |hash: Hash32, size: u64| -> Result<Value> {
        // SP8, SP17: reject an oversized declaration before requesting bytes.
        if size > MAX_PART as u64 {
            return Err(ViewError::TooLarge);
        }
        let path = format!("{dir}/{hash}.json");
        let bytes = fetch(&path).ok_or(ViewError::Missing(path))?;
        if bytes.len() > MAX_PART {
            return Err(ViewError::TooLarge);
        }
        if bytes.len() as u64 != size || identity_hash(&bytes) != hash {
            return Err(ViewError::Part);
        }
        let (v, remaining) = json::parse_within(&bytes, Numbers::Integers, left)?;
        left = remaining;
        Ok(v)
    };
    let mut keys = HashSet::new();
    let mut shards = Vec::new();
    for s in &index.shards {
        let v = part(s.hash, s.size)?;
        let mut records = Vec::new();
        for r in list(&v, "records")? {
            let r = read_record(r, device)?;
            if !(s.lo..=s.hi).contains(&bucket(&r.key)) {
                return Err(ViewError::Shards);
            }
            if !keys.insert(r.key.clone()) {
                return Err(ViewError::DuplicateKey);
            }
            records.push(r);
        }
        let marks = list(&v, "marks")?
            .iter()
            .map(read_entry)
            .collect::<Result<_>>()?;
        shards.push(Shard { records, marks });
    }
    let mut spine: Vec<SpineEntry> = Vec::new();
    let mut latest: BTreeMap<[u8; 16], Hash32> = BTreeMap::new();
    for s in &index.spine {
        let v = part(s.hash, s.size)?;
        let entries = v.as_array().ok_or(ViewError::Shape)?;
        if entries.len() as u64 != s.to - s.from + 1 {
            return Err(ViewError::Spine);
        }
        for (i, raw) in entries.iter().enumerate() {
            let (e, versions) = read_spine(raw, library, device, key)?;
            let previous = spine.last().map(|e| e.h);
            if e.s != s.from + i as u64 || e.p != previous {
                return Err(ViewError::Spine);
            }
            // SP17: entries are in sequence order, then v-list order; replacing
            // a key hash therefore keeps its latest occurrence across all parts.
            for v in versions {
                latest.insert(
                    v[..16].try_into().unwrap(),
                    Hash32(v[16..].try_into().unwrap()),
                );
            }
            spine.push(e);
        }
    }
    if spine.last().map(|e| Mark { s: e.s, h: e.h }) != Some(index.through) {
        return Err(ViewError::Spine);
    }
    // SP17: shards and spine describe the same complete set of latest versions.
    // This shared validator also refuses inconsistent writer input before signing.
    for record in shards.iter().flat_map(|s| &s.records) {
        if latest.remove(&record.key.spine_hash()) != Some(record::version_hash(&record.version)) {
            return Err(ViewError::Spine);
        }
    }
    if !latest.is_empty() {
        return Err(ViewError::Spine);
    }
    let open = read_open(
        &part(index.open.hash, index.open.size)?,
        device,
        index.through.s,
    )?;
    Ok(View {
        gen: index.gen,
        prev: index.prev,
        through: index.through,
        hash: index_hash,
        index,
        shards,
        spine,
        open,
    })
}

fn shard_value(records: &[Value], marks: &[Value]) -> Value {
    json!({"records": records, "marks": marks})
}

fn add_part(v: &Value, parts: &mut Vec<(Hash32, Vec<u8>)>, limit: usize) -> Result<PartRef> {
    let bytes = json::canonical(v);
    if bytes.len() > limit {
        return Err(ViewError::TooLarge);
    }
    let part = PartRef {
        hash: identity_hash(&bytes),
        size: bytes.len() as u64,
    };
    parts.push((part.hash, bytes));
    Ok(part)
}

/// SP17: encode a complete generation, returning parts to publish before the
/// index. `marks` carry their keys only for routing; the wire holds version IDs.
/// An unsplittable two-byte hash bucket, one spine entry or open part over the
/// limit is refused. No file is signed if its format or budgets fail validation.
#[allow(clippy::too_many_arguments)]
pub fn write(
    key: &Keypair,
    library: &Id16,
    writer: &Id16,
    gen: u64,
    prev: Option<Hash32>,
    through: Mark,
    records: &[Record],
    marks: &[(Key, Entry)],
    spine_entries: &[SpineEntry],
    open: &Open,
) -> Result<Written> {
    write_within(
        key,
        library,
        writer,
        gen,
        prev,
        through,
        records,
        marks,
        spine_entries,
        open,
        MAX_PART,
    )
}

#[allow(clippy::too_many_arguments)]
fn write_within(
    key: &Keypair,
    library: &Id16,
    writer: &Id16,
    gen: u64,
    prev: Option<Hash32>,
    through: Mark,
    records: &[Record],
    marks: &[(Key, Entry)],
    spine_entries: &[SpineEntry],
    open: &Open,
    limit: usize,
) -> Result<Written> {
    let limit = limit.min(MAX_PART);
    let mut buckets: BTreeMap<u16, (Vec<Value>, Vec<Value>)> = BTreeMap::new();
    for r in records {
        buckets
            .entry(bucket(&r.key))
            .or_default()
            .0
            .push(record_value(r));
    }
    for (key, mark) in marks {
        buckets.entry(bucket(key)).or_default().1.push(entry(mark));
    }
    let mut parts = Vec::new();
    let mut shards = Vec::new();
    let (mut rs, mut ms) = (Vec::new(), Vec::new());
    let mut lo = 0;
    let empty_size = json::canonical(&shard_value(&[], &[])).len();
    let mut size = empty_size;
    for (bucket, (records, marks)) in buckets {
        let cost = |items: &[Value]| {
            items
                .iter()
                .map(|v| json::canonical(v).len() + 1)
                .sum::<usize>()
        };
        let (rcost, mcost) = (cost(&records), cost(&marks));
        let alone = empty_size
            + rcost.saturating_sub(usize::from(!records.is_empty()))
            + mcost.saturating_sub(usize::from(!marks.is_empty()));
        // SP17: a range cannot split one bucket; refuse rather than write overlaps.
        if alone > limit {
            return Err(ViewError::TooLarge);
        }
        let added = rcost.saturating_sub(usize::from(rs.is_empty() && !records.is_empty()))
            + mcost.saturating_sub(usize::from(ms.is_empty() && !marks.is_empty()));
        if size + added > limit {
            let p = add_part(&shard_value(&rs, &ms), &mut parts, limit)?;
            shards.push(ShardRef {
                lo,
                hi: bucket - 1,
                hash: p.hash,
                size: p.size,
            });
            lo = bucket;
            rs.clear();
            ms.clear();
            size = alone;
        } else {
            size += added;
        }
        rs.extend(records);
        ms.extend(marks);
    }
    let p = add_part(&shard_value(&rs, &ms), &mut parts, limit)?;
    shards.push(ShardRef {
        lo,
        hi: u16::MAX,
        hash: p.hash,
        size: p.size,
    });
    let mut spine = Vec::new();
    let mut entries = Vec::new();
    let mut from = 1;
    let mut to = 0;
    let mut size = 2;
    for e in spine_entries {
        let v = e.write();
        let bytes = json::canonical(&v).len();
        if bytes + 2 > limit {
            return Err(ViewError::TooLarge);
        }
        if size + bytes + usize::from(!entries.is_empty()) > limit {
            let p = add_part(&Value::Array(entries), &mut parts, limit)?;
            spine.push(SpineRef {
                from,
                to,
                hash: p.hash,
                size: p.size,
            });
            entries = Vec::new();
            size = 2;
            from = e.s;
        }
        size += bytes + usize::from(!entries.is_empty());
        entries.push(v);
        to = e.s;
    }
    if !entries.is_empty() {
        let p = add_part(&Value::Array(entries), &mut parts, limit)?;
        spine.push(SpineRef {
            from,
            to,
            hash: p.hash,
            size: p.size,
        });
    }
    let open = add_part(&open.write(), &mut parts, limit)?;
    let device = key.device();
    let index = Index {
        gen,
        prev,
        through,
        shards,
        spine,
        open,
    };
    let raw = index.write(library, &device, writer);
    let payload = json::canonical(&raw);
    // SP3, SY28: the writer uses the same strict parser and shared budget as open.
    let (raw, left) = json::parse_within(&payload, Numbers::Integers, MAX_VALUES)?;
    let index = Index::read(&raw)?;
    let index_hash = identity_hash(&payload);
    let dir = directory(&device, &index_hash);
    let parts: Vec<_> = parts
        .into_iter()
        .map(|(hash, bytes)| (format!("{dir}/{hash}.json"), bytes))
        .collect();
    read_parts(
        index,
        index_hash,
        library,
        &device,
        &key.public(),
        &|path| {
            parts
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, b)| b.clone())
        },
        left,
    )?;
    // SP8: determine the envelope's size before signing (SP3 has bounded sigs).
    if envelope::write(
        "view",
        &payload,
        &[envelope::Sig {
            by: Signer::Device(device),
            sig: [0; 64],
        }],
    )
    .len()
        > MAX_INDEX
    {
        return Err(ViewError::TooLarge);
    }
    let sig = envelope::sign(
        key,
        Signer::Device(device),
        "view",
        library,
        &Place {
            seq: gen,
            ..Place::default()
        },
        &payload,
    );
    let index_bytes = envelope::write("view", &payload, &[sig]);
    Ok(Written {
        index_path: format!("{dir}/index.json"),
        index_bytes,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::chain::{open_change, sign_change};

    struct Fixture {
        key: Keypair,
        library: Id16,
        writer: Id16,
        records: Vec<Record>,
        marks: Vec<(Key, Entry)>,
        spine: Vec<SpineEntry>,
        open: Open,
    }

    impl Fixture {
        fn new(count: usize) -> Self {
            let key = Keypair::from_seed(&[1; 32]);
            let library = Id16([2; 16]);
            let writer = Id16([3; 16]);
            let mut records = Vec::new();
            let mut spine = Vec::new();
            let mut prev = None;
            for i in 0..count {
                let k = format!("place:00000000-0000-4000-8000-{i:012}");
                let version = json!({"format": 1, "key": k, "device": key.device().to_string(),
                    "clock": [1760000000000u64 + i as u64, 0], "history": [],
                    "body": {"hash": Hash32([7; 32]).to_string(), "size": 20},
                    "future": {"kept": true}});
                let c = sign_change(
                    &key,
                    &library,
                    &writer,
                    i as u64 + 1,
                    prev,
                    std::slice::from_ref(&version),
                );
                let c = open_change(&c.bytes, &library, &key.device(), &key.public()).unwrap();
                prev = Some(c.hash);
                spine.push(SpineEntry::from_change(&c));
                records.push(Record {
                    key: Key::parse(&k).unwrap().0,
                    version,
                    week: Vec::new(),
                });
            }
            let mark = Entry {
                device: key.device(),
                seq: 1,
                hash: record::version_hash(&records[0].version),
            };
            records[0].week = vec![Retirement {
                v: mark,
                objects: vec![
                    Name {
                        kind: Kind::Blob,
                        name: Hash32([4; 32]),
                    },
                    Name {
                        kind: Kind::Book,
                        name: Hash32([5; 32]),
                    },
                ],
            }];
            let id = ItemId {
                device: key.device(),
                seq: 1,
                index: 0,
            };
            let open = Open {
                reserve: vec![(
                    id,
                    Reserve {
                        kind: Kind::Blob,
                        name: Hash32([6; 32]),
                        size: 20,
                    },
                )],
                hold: vec![(
                    id,
                    Hold {
                        kind: Kind::Blob,
                        name: Hash32([8; 32]),
                        hash: Hash32([9; 32]),
                        size: 20,
                        for_key: records[0].key.clone(),
                    },
                )],
                rotations: vec![Hash32([10; 32])],
            };
            let marks = vec![(records[0].key.clone(), mark)];
            Self {
                key,
                library,
                writer,
                records,
                marks,
                spine,
                open,
            }
        }

        fn through(&self) -> Mark {
            let e = self.spine.last().unwrap();
            Mark { s: e.s, h: e.h }
        }

        fn append_change(&mut self, versions: &[Value]) {
            let seq = self.spine.len() as u64 + 1;
            let prev = self.spine.last().map(|e| e.h);
            let c = sign_change(&self.key, &self.library, &self.writer, seq, prev, versions);
            let c = open_change(
                &c.bytes,
                &self.library,
                &self.key.device(),
                &self.key.public(),
            )
            .unwrap();
            self.spine.push(SpineEntry::from_change(&c));
        }

        fn try_write(&self, records: &[Record], open: &Open) -> Result<Written> {
            write(
                &self.key,
                &self.library,
                &self.writer,
                1,
                None,
                self.through(),
                records,
                &self.marks,
                &self.spine,
                open,
            )
        }

        fn write(&self, limit: usize) -> Written {
            write_within(
                &self.key,
                &self.library,
                &self.writer,
                1,
                None,
                self.through(),
                &self.records,
                &self.marks,
                &self.spine,
                &self.open,
                limit,
            )
            .unwrap()
        }

        fn read(&self, w: &Written) -> Result<View> {
            open(
                &w.index_bytes,
                &self.library,
                &self.key.device(),
                &self.key.public(),
                &|path| fetch(w, path),
            )
        }

        fn payload(&self, w: &Written) -> Value {
            Value::Object(
                envelope::open(&w.index_bytes, "view", &self.library)
                    .unwrap()
                    .body,
            )
        }

        // SP5: re-sign the changed index and relocate parts to its new payload
        // hash. A structural refusal must survive authentic hashes/signatures.
        fn reindex(&self, w: &mut Written, p: Value) {
            let bytes = json::canonical(&p);
            let dir = directory(&self.key.device(), &identity_hash(&bytes));
            for (path, _) in &mut w.parts {
                *path = format!("{dir}/{}", path.rsplit('/').next().unwrap());
            }
            let sig = envelope::sign(
                &self.key,
                Signer::Device(self.key.device()),
                "view",
                &self.library,
                &Place {
                    seq: p["gen"].as_u64().unwrap(),
                    ..Place::default()
                },
                &bytes,
            );
            w.index_path = format!("{dir}/index.json");
            w.index_bytes = envelope::write("view", &bytes, &[sig]);
        }

        fn edit_index(&self, w: &mut Written, edit: impl FnOnce(&mut Value)) {
            let mut p = self.payload(w);
            edit(&mut p);
            self.reindex(w, p);
        }

        fn edit_part(
            &self,
            w: &mut Written,
            field: &str,
            at: Option<usize>,
            edit: impl FnOnce(&mut Value),
        ) {
            let mut p = self.payload(w);
            let r = if let Some(at) = at {
                &mut p[field][at]
            } else {
                &mut p[field]
            };
            let suffix = format!("/{}.json", r["hash"].as_str().unwrap());
            let (path, bytes) = w
                .parts
                .iter_mut()
                .find(|(path, _)| path.ends_with(&suffix))
                .unwrap();
            let mut v = json::parse(bytes, Numbers::Integers).unwrap();
            edit(&mut v);
            *bytes = json::canonical(&v);
            let h = identity_hash(bytes);
            *path = format!("{}/{}.json", path.rsplit_once('/').unwrap().0, h);
            r["hash"] = json!(h.to_string());
            r["size"] = json!(bytes.len());
            self.reindex(w, p);
        }

        fn resign_spine(&self, v: &mut Value) {
            let bytes = b64url_decode(v["v"].as_str().unwrap()).unwrap();
            let list: Vec<[u8; 48]> = bytes
                .chunks_exact(48)
                .map(|b| b.try_into().unwrap())
                .collect();
            let place = Place {
                seq: v["s"].as_u64().unwrap(),
                prev: optional_hash(&v["p"]).unwrap(),
                vroot: Some(envelope::vroot(&list)),
            };
            let sig = self.key.sign(&envelope::message(
                "change",
                &self.library,
                &Signer::Device(self.key.device()),
                &place,
                &hash(&v["h"]).unwrap(),
            ));
            v["sig"] = json!(b64url_encode(&sig));
        }
    }

    fn fetch(w: &Written, path: &str) -> Option<Vec<u8>> {
        w.parts
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, b)| b.clone())
    }

    fn all_records(v: &View) -> BTreeMap<Key, Record> {
        v.shards
            .iter()
            .flat_map(|s| s.records.iter())
            .map(|r| (r.key.clone(), r.clone()))
            .collect()
    }

    #[test]
    fn a_view_round_trips() {
        let f = Fixture::new(4);
        let w = write(
            &f.key,
            &f.library,
            &f.writer,
            1,
            None,
            f.through(),
            &f.records,
            &f.marks,
            &f.spine,
            &f.open,
        )
        .unwrap();
        let v = f.read(&w).unwrap();
        assert_eq!((v.gen, v.prev, v.through), (1, None, f.through()));
        assert_eq!(v.spine, f.spine);
        assert_eq!(v.open, f.open);
        let id = json!({"d": f.key.device().to_string(), "s": 1, "i": 0});
        assert_eq!(
            f.open.write(),
            json!({
                "reserve": [{"id": id, "item": {"kind": "blob", "name": Hash32([6; 32]).to_string(), "size": 20}}],
                "hold": [{"id": id, "item": {"kind": "blob", "name": Hash32([8; 32]).to_string(),
                    "hash": Hash32([9; 32]).to_string(), "size": 20, "for": f.records[0].key.as_str()}}],
                "rotations": [{"hash": Hash32([10; 32]).to_string()}]
            })
        );
        assert_eq!(
            all_records(&v),
            f.records
                .iter()
                .map(|r| (r.key.clone(), r.clone()))
                .collect()
        );
        assert_eq!(
            v.shards
                .iter()
                .flat_map(|s| s.marks.iter())
                .copied()
                .collect::<Vec<_>>(),
            vec![f.marks[0].1]
        );
        let dir = directory(&f.key.device(), &v.hash);
        assert_eq!(w.index_path, format!("{dir}/index.json"));
        for (path, bytes) in &w.parts {
            assert_eq!(*path, format!("{dir}/{}.json", identity_hash(bytes)));
        }
        let envelope = envelope::open(&w.index_bytes, "view", &f.library).unwrap();
        assert!(envelope.verified(
            &Signer::Device(f.key.device()),
            &f.key.public(),
            &f.library,
            &Place {
                seq: 1,
                ..Place::default()
            }
        ));
        assert!(!envelope.verified(
            &Signer::Device(f.key.device()),
            &f.key.public(),
            &f.library,
            &Place {
                seq: 2,
                ..Place::default()
            }
        ));
        assert!(!follows(&v, &v.hash));
        let mut next = w.clone();
        f.edit_index(&mut next, |p| {
            p["gen"] = json!(2);
            p["prev"] = json!(v.hash.to_string());
        });
        let next = f.read(&next).unwrap();
        assert!(follows(&next, &v.hash));
        assert!(!follows(&next, &Hash32([99; 32])));
    }

    #[test]
    fn a_view_larger_than_one_shard_is_published_and_read_back_whole() {
        let f = Fixture::new(48);
        let limit = 1500;
        let w = f.write(limit);
        let v = f.read(&w).unwrap();
        assert!(v.index.shards.len() > 2);
        assert!(v.index.spine.len() > 2);
        assert!(w.parts.iter().all(|(_, bytes)| bytes.len() <= limit));
        assert!(w.parts.iter().map(|(_, b)| b.len()).sum::<usize>() > limit);
        assert_eq!(
            all_records(&v),
            f.records
                .iter()
                .map(|r| (r.key.clone(), r.clone()))
                .collect()
        );
        assert_eq!(v.spine, f.spine);
        assert_eq!(v.open, f.open);
        assert_eq!(v.index.shards.first().unwrap().lo, 0);
        assert_eq!(v.index.shards.last().unwrap().hi, 65535);
        for pair in v.index.shards.windows(2) {
            assert_eq!(u32::from(pair[0].hi) + 1, u32::from(pair[1].lo));
        }
        assert_eq!(
            v.shards
                .iter()
                .flat_map(|s| &s.marks)
                .copied()
                .collect::<Vec<_>>(),
            vec![f.marks[0].1]
        );
    }

    #[test]
    fn shards_with_a_gap_or_overlap_are_refused() {
        let f = Fixture::new(8);
        let original = f.write(1500);
        for overlap in [false, true] {
            let mut w = original.clone();
            f.edit_index(&mut w, |p| {
                let lo = p["shards"][1]["lo"].as_u64().unwrap();
                p["shards"][1]["lo"] = json!(if overlap { lo - 1 } else { lo + 1 });
            });
            assert!(matches!(f.read(&w), Err(ViewError::Shards)));
        }
        let mut w = original.clone();
        f.edit_index(&mut w, |p| p["shards"].as_array_mut().unwrap().swap(0, 1));
        assert!(matches!(f.read(&w), Err(ViewError::Shards)));
        for (at, field, value) in [(0, "lo", 1), (0, "hi", 65536)] {
            let mut w = original.clone();
            f.edit_index(&mut w, |p| p["shards"][at][field] = json!(value));
            assert!(matches!(f.read(&w), Err(ViewError::Shards)));
        }
        let mut w = original;
        f.edit_index(&mut w, |p| {
            p["shards"].as_array_mut().unwrap().last_mut().unwrap()["hi"] = json!(65534);
        });
        assert!(matches!(f.read(&w), Err(ViewError::Shards)));
    }

    #[test]
    fn a_record_in_the_wrong_shard_is_refused() {
        let f = Fixture::new(8);
        let mut w = f.write(1500);
        let p = f.payload(&w);
        let hi = p["shards"][0]["hi"].as_u64().unwrap() as u16;
        let wrong = (0..)
            .map(|i| Key::parse(&format!("setting:wrong{i}")).unwrap().0)
            .find(|k| bucket(k) > hi)
            .unwrap();
        f.edit_part(&mut w, "shards", Some(0), |v| {
            v["records"][0]["key"] = json!(wrong.as_str());
            v["records"][0]["version"]["key"] = json!(wrong.as_str());
        });
        assert!(matches!(f.read(&w), Err(ViewError::Shards)));
    }

    #[test]
    fn a_part_that_doesnt_match_its_hash_or_is_too_large_is_refused() {
        let f = Fixture::new(4);
        let original = f.write(MAX_PART);
        for field in ["shards", "spine", "open"] {
            let p = f.payload(&original);
            let r = if field == "open" {
                &p[field]
            } else {
                &p[field][0]
            };
            let suffix = format!("/{}.json", r["hash"].as_str().unwrap());
            let mut w = original.clone();
            let (_, bytes) = w
                .parts
                .iter_mut()
                .find(|(p, _)| p.ends_with(&suffix))
                .unwrap();
            // Same length, invalid JSON: authentication must happen first.
            bytes[0] = b'!';
            assert!(matches!(f.read(&w), Err(ViewError::Part)));
            let mut w = original.clone();
            f.edit_index(&mut w, |p| {
                let r = if field == "open" {
                    &mut p[field]
                } else {
                    &mut p[field][0]
                };
                r["size"] = json!(r["size"].as_u64().unwrap() + 1);
            });
            assert!(matches!(f.read(&w), Err(ViewError::Part)));
            let mut w = original.clone();
            w.parts
                .iter_mut()
                .find(|(p, _)| p.ends_with(&suffix))
                .unwrap()
                .1 = vec![b' '; MAX_PART + 1];
            assert!(matches!(f.read(&w), Err(ViewError::TooLarge)));
            let mut w = original.clone();
            f.edit_index(&mut w, |p| {
                let r = if field == "open" {
                    &mut p[field]
                } else {
                    &mut p[field][0]
                };
                r["size"] = json!(MAX_PART + 1);
            });
            assert!(matches!(
                open(
                    &w.index_bytes,
                    &f.library,
                    &f.key.device(),
                    &f.key.public(),
                    &|_| panic!("oversized declarations must not fetch")
                ),
                Err(ViewError::TooLarge)
            ));
        }
        assert!(matches!(
            open(
                &vec![b' '; MAX_INDEX + 1],
                &f.library,
                &f.key.device(),
                &f.key.public(),
                &|_| panic!("oversized index must not fetch")
            ),
            Err(ViewError::TooLarge)
        ));
    }

    #[test]
    fn a_spine_that_doesnt_link_or_end_at_through_is_refused() {
        let f = Fixture::new(4);
        let original = f.write(MAX_PART);
        let mut w = original.clone();
        f.edit_part(&mut w, "spine", Some(0), |v| {
            v[1]["p"] = json!(Hash32([99; 32]).to_string());
            f.resign_spine(&mut v[1]);
        });
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        let mut w = original.clone();
        f.edit_index(&mut w, |p| {
            p["through"]["h"] = json!(Hash32([99; 32]).to_string())
        });
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        let mut w = original.clone();
        f.edit_index(&mut w, |p| p["through"]["s"] = json!(5));
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        let mut w = original.clone();
        f.edit_part(&mut w, "spine", Some(0), |v| {
            v.as_array_mut().unwrap().remove(1);
        });
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        let mut w = original;
        f.edit_part(&mut w, "spine", Some(0), |v| {
            v[1]["s"] = json!(3);
            f.resign_spine(&mut v[1]);
        });
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        let mut w = f.write(1000);
        f.edit_index(&mut w, |p| p["spine"][1]["from"] = json!(99));
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
    }

    #[test]
    fn a_missing_part_is_retryable() {
        let f = Fixture::new(4);
        let w = f.write(MAX_PART);
        for missing in &w.parts {
            let read = open(
                &w.index_bytes,
                &f.library,
                &f.key.device(),
                &f.key.public(),
                &|path| {
                    if path == missing.0 {
                        None
                    } else {
                        fetch(&w, path)
                    }
                },
            );
            assert!(matches!(read, Err(ViewError::Missing(path)) if path == missing.0));
            assert!(f.read(&w).is_ok());
        }
    }

    #[test]
    fn an_index_signed_by_another_device_or_for_another_library_is_refused() {
        let f = Fixture::new(4);
        let w = f.write(MAX_PART);
        let no_fetch =
            |_: &str| -> Option<Vec<u8>> { panic!("unauthenticated index must not fetch") };
        let other = Keypair::from_seed(&[88; 32]);
        assert!(matches!(
            open(
                &w.index_bytes,
                &f.library,
                &other.device(),
                &other.public(),
                &no_fetch
            ),
            Err(ViewError::Device)
        ));
        assert!(matches!(
            open(
                &w.index_bytes,
                &Id16([88; 16]),
                &f.key.device(),
                &f.key.public(),
                &no_fetch
            ),
            Err(ViewError::Envelope(EnvelopeError::Library))
        ));
        let p = json::canonical(&f.payload(&w));
        let sig = envelope::sign(
            &other,
            Signer::Device(other.device()),
            "view",
            &f.library,
            &Place {
                seq: 1,
                ..Place::default()
            },
            &p,
        );
        assert!(matches!(
            open(
                &envelope::write("view", &p, &[sig]),
                &f.library,
                &f.key.device(),
                &f.key.public(),
                &no_fetch
            ),
            Err(ViewError::Signature)
        ));
        let sig = envelope::sign(
            &f.key,
            Signer::Device(f.key.device()),
            "view",
            &f.library,
            &Place {
                seq: 2,
                ..Place::default()
            },
            &p,
        );
        assert!(matches!(
            open(
                &envelope::write("view", &p, &[sig]),
                &f.library,
                &f.key.device(),
                &f.key.public(),
                &no_fetch
            ),
            Err(ViewError::Signature)
        ));
        let mut altered = w.clone();
        f.edit_index(&mut altered, |p| p["type"] = json!("device"));
        assert!(matches!(
            f.read(&altered),
            Err(ViewError::Envelope(EnvelopeError::Type))
        ));
    }

    #[test]
    fn a_week_entry_and_marks_are_kept() {
        let mut f = Fixture::new(4);
        let v = f.read(&f.write(MAX_PART)).unwrap();
        let r = &all_records(&v)[&f.records[0].key];
        assert_eq!(r.week, f.records[0].week);
        assert_eq!(
            record_value(r)["week"],
            json!([{
                "v": {"d": f.key.device().to_string(), "s": 1, "h": f.marks[0].1.hash.to_string()},
                "objects": [
                    {"kind": "blob", "name": Hash32([4; 32]).to_string()},
                    {"kind": "book", "name": Hash32([5; 32]).to_string()}
                ]
            }])
        );
        let weeks = r
            .week
            .iter()
            .flat_map(Retirement::weeks)
            .collect::<Vec<_>>();
        assert_eq!(weeks.len(), 2);
        for (w, n) in weeks.iter().zip(&f.records[0].week[0].objects) {
            assert_eq!(w.name, *n);
            assert_eq!(w.retiring, f.marks[0].1);
        }
        assert!(record_value(&f.records[1]).get("week").is_none());
        // SP17: a second retirement and generation retain all earlier names.
        f.records[0].week.push(Retirement {
            v: Entry {
                seq: 2,
                ..f.marks[0].1
            },
            objects: vec![Name {
                kind: Kind::Blob,
                name: Hash32([66; 32]),
            }],
        });
        let next = write(
            &f.key,
            &f.library,
            &f.writer,
            2,
            Some(v.hash),
            f.through(),
            &f.records,
            &f.marks,
            &f.spine,
            &f.open,
        )
        .unwrap();
        let next = f.read(&next).unwrap();
        assert_eq!(
            all_records(&next)[&f.records[0].key].week,
            f.records[0].week
        );
        assert_eq!(
            next.shards
                .iter()
                .flat_map(|s| &s.marks)
                .copied()
                .collect::<Vec<_>>(),
            vec![f.marks[0].1]
        );
    }

    #[test]
    fn a_malformed_week_is_refused() {
        let f = Fixture::new(1);
        let original = f.write(MAX_PART);
        let good = record_value(&f.records[0])["week"].clone();
        let mut cases = vec![Value::Null, json!([{}])];
        for (field, value) in [
            ("s", json!(0)),
            ("d", json!("wrong")),
            ("h", json!("wrong")),
        ] {
            let mut bad = good.clone();
            bad[0]["v"][field] = value;
            cases.push(bad);
        }
        let mut bad = good.clone();
        bad[0]["objects"] = json!([]);
        cases.push(bad);
        let mut bad = good.clone();
        bad[0]["objects"][0]["kind"] = json!("other");
        cases.push(bad);
        let mut bad = good;
        bad[0]["objects"][0]["name"] = json!("wrong");
        cases.push(bad);
        for bad in cases {
            let mut w = original.clone();
            f.edit_part(&mut w, "shards", Some(0), |v| v["records"][0]["week"] = bad);
            assert!(matches!(f.read(&w), Err(ViewError::Shape)));
        }
    }

    #[test]
    fn a_malformed_open_is_refused() {
        let f = Fixture::new(1);
        let original = f.write(MAX_PART);
        let good = f.open.write();
        let mut cases = Vec::new();
        for field in ["reserve", "hold", "rotations"] {
            let mut bad = good.clone();
            bad.as_object_mut().unwrap().remove(field);
            cases.push(bad);
            let mut bad = good.clone();
            bad[field] = Value::Null;
            cases.push(bad);
        }
        for field in ["reserve", "hold"] {
            let mut bad = good.clone();
            bad[field][0]["id"]["s"] = json!(0);
            cases.push(bad);
            let mut bad = good.clone();
            bad[field][0]["id"]["i"] = json!(-1);
            cases.push(bad);
            let mut bad = good.clone();
            bad[field][0]["item"]["kind"] = json!("other");
            cases.push(bad);
            let mut bad = good.clone();
            let duplicate = bad[field][0].clone();
            bad[field].as_array_mut().unwrap().push(duplicate);
            cases.push(bad);
        }
        for (field, limit) in [
            ("reserve", hold::MAX_RESERVATIONS),
            ("hold", hold::MAX_HOLDS),
            ("rotations", MAX_ROTATIONS),
        ] {
            let mut bad = good.clone();
            bad[field] = Value::Array(vec![good[field][0].clone(); limit + 1]);
            cases.push(bad);
        }
        let mut bad = good.clone();
        bad["hold"][0]["item"]["for"] = json!("wrong");
        cases.push(bad);
        let mut bad = good.clone();
        bad["rotations"][0]["hash"] = json!("wrong");
        cases.push(bad);
        let mut bad = good;
        bad["reserve"][0]["item"]["size"] = json!(hold::MAX_RESERVED_BYTES + 1);
        cases.push(bad);
        for bad in cases {
            let mut w = original.clone();
            f.edit_part(&mut w, "open", None, |v| *v = bad);
            assert!(f.read(&w).is_err());
        }
    }

    #[test]
    fn duplicate_keys_are_refused() {
        let f = Fixture::new(1);
        let mut w = f.write(MAX_PART);
        f.edit_part(&mut w, "shards", Some(0), |v| {
            let duplicate = v["records"][0].clone();
            v["records"].as_array_mut().unwrap().push(duplicate);
        });
        assert!(matches!(f.read(&w), Err(ViewError::DuplicateKey)));
    }

    #[test]
    fn malformed_spine_lists_and_signatures_are_refused() {
        let f = Fixture::new(1);
        let original = f.write(MAX_PART);
        for bad in [
            "!".to_owned(),
            b64url_encode(&[0; 47]),
            b64url_encode(&[0; 49]),
        ] {
            let mut w = original.clone();
            f.edit_part(&mut w, "spine", Some(0), |v| v[0]["v"] = json!(bad));
            assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        }
        for bad in [
            "!".to_owned(),
            b64url_encode(&[0; 63]),
            b64url_encode(&[0; 65]),
            b64url_encode(&[0; 64]),
        ] {
            let mut w = original.clone();
            f.edit_part(&mut w, "spine", Some(0), |v| v[0]["sig"] = json!(bad));
            assert!(matches!(f.read(&w), Err(ViewError::Signature)));
        }
        let mut w = original;
        f.edit_part(&mut w, "spine", Some(0), |v| {
            let mut bytes = b64url_decode(v[0]["v"].as_str().unwrap()).unwrap();
            bytes[47] ^= 1;
            v[0]["v"] = json!(b64url_encode(&bytes));
        });
        assert!(
            matches!(f.read(&w), Err(ViewError::Signature)),
            "vroot is covered by the original change signature"
        );
    }

    #[test]
    fn an_unsplittable_bucket_is_refused() {
        let mut f = Fixture::new(2);
        let mut seen = BTreeMap::new();
        let (a, b) = (0..=65536)
            .find_map(|i| {
                let key = Key::parse(&format!("setting:collision{i}")).unwrap().0;
                seen.insert(bucket(&key), key.clone())
                    .map(|previous| (previous, key))
            })
            .unwrap();
        assert_eq!(bucket(&a), bucket(&b));
        for (r, key) in f.records.iter_mut().zip([a, b]) {
            r.key = key;
            r.version["key"] = json!(r.key.as_str());
            r.version["padding"] = json!("x".repeat(300));
            r.week.clear();
            assert!(json::canonical(&record_value(r)).len() < 1000);
        }
        // SP17: isolate the size refusal with records whose signed spine agrees.
        f.spine.clear();
        f.marks.clear();
        for version in f
            .records
            .iter()
            .map(|r| r.version.clone())
            .collect::<Vec<_>>()
        {
            f.append_change(std::slice::from_ref(&version));
        }
        assert!(f.try_write(&f.records, &Open::default()).is_ok());
        assert!(matches!(
            write_within(
                &f.key,
                &f.library,
                &f.writer,
                1,
                None,
                f.through(),
                &f.records,
                &[],
                &f.spine,
                &Open::default(),
                1000
            ),
            Err(ViewError::TooLarge)
        ));
    }

    #[test]
    fn a_views_parts_share_one_value_budget() {
        let f = Fixture::new(4);
        let w = f.write(1500);
        let view = f.read(&w).unwrap();
        let count = w
            .parts
            .iter()
            .map(|(_, b)| json::count_values(&json::parse(b, Numbers::Integers).unwrap()))
            .sum::<u64>();
        assert!(matches!(
            read_parts(
                view.index.clone(),
                view.hash,
                &f.library,
                &f.key.device(),
                &f.key.public(),
                &|path| fetch(&w, path),
                count - 1
            ),
            Err(ViewError::Json(json::JsonError::TooManyValues))
        ));
        assert!(read_parts(
            view.index,
            view.hash,
            &f.library,
            &f.key.device(),
            &f.key.public(),
            &|path| fetch(&w, path),
            count
        )
        .is_ok());
        let mut bad = w;
        f.edit_part(&mut bad, "open", None, |v| {
            v["unknown"] = json!(vec![0; MAX_VALUES as usize])
        });
        assert!(matches!(
            f.read(&bad),
            Err(ViewError::Json(json::JsonError::TooManyValues))
        ));
    }

    #[test]
    fn an_empty_change_and_empty_shard_round_trip() {
        let mut f = Fixture::new(1);
        let change = sign_change(&f.key, &f.library, &f.writer, 1, None, &[]);
        let change =
            open_change(&change.bytes, &f.library, &f.key.device(), &f.key.public()).unwrap();
        f.spine = vec![SpineEntry::from_change(&change)];
        f.records.clear();
        f.marks.clear();
        f.open = Open::default();
        let w = f.write(MAX_PART);
        let v = f.read(&w).unwrap();
        assert_eq!(
            v.shards,
            vec![Shard {
                records: vec![],
                marks: vec![]
            }]
        );
        assert_eq!(v.open, Open::default());
        assert_eq!(v.spine[0].v, "");
        assert_eq!(
            f.open.write(),
            json!({"reserve": [], "hold": [], "rotations": []})
        );
    }

    #[test]
    fn a_shard_with_a_stale_version_is_refused() {
        let mut f = Fixture::new(4);
        let older = f.records[0].version.clone();
        let mut latest = older.clone();
        latest["clock"][1] = json!(1);
        latest["history"] = json!([entry(&f.marks[0].1)]);
        latest["body"]["hash"] = json!(Hash32([11; 32]).to_string());
        f.append_change(std::slice::from_ref(&latest));
        f.records[0].version = latest;
        // SP17: the latest occurrence can be in a later spine part.
        let split = f.read(&f.write(1500)).unwrap();
        assert!(split.index.spine.len() > 1);
        assert_eq!(all_records(&split)[&f.records[0].key], f.records[0]);
        let mut w = f.write(MAX_PART);
        f.edit_part(&mut w, "shards", Some(0), |v| {
            let r = v["records"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|r| r["key"].as_str() == Some(f.records[0].key.as_str()))
                .unwrap();
            r["version"] = older.clone();
        });
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        let mut records = f.records.clone();
        records[0].version = older;
        assert!(matches!(
            f.try_write(&records, &f.open),
            Err(ViewError::Spine)
        ));
    }

    #[test]
    fn a_shard_version_absent_from_the_spine_is_refused() {
        let f = Fixture::new(1);
        for absent_key in [false, true] {
            let mut records = f.records.clone();
            if absent_key {
                records[0].key = Key::parse("setting:absent").unwrap().0;
                records[0].version["key"] = json!(records[0].key.as_str());
            } else {
                records[0].version["body"]["hash"] = json!(Hash32([99; 32]).to_string());
            }
            let mut w = f.write(MAX_PART);
            f.edit_part(&mut w, "shards", Some(0), |v| {
                v["records"][0] = record_value(&records[0])
            });
            assert!(matches!(f.read(&w), Err(ViewError::Spine)));
            assert!(matches!(
                f.try_write(&records, &f.open),
                Err(ViewError::Spine)
            ));
        }
    }

    #[test]
    fn a_key_in_the_spine_missing_from_the_shards_is_refused() {
        let f = Fixture::new(2);
        let mut w = f.write(MAX_PART);
        f.edit_part(&mut w, "shards", Some(0), |v| {
            v["records"].as_array_mut().unwrap().remove(1);
        });
        assert!(matches!(f.read(&w), Err(ViewError::Spine)));
        assert!(matches!(
            f.try_write(&f.records[..1], &f.open),
            Err(ViewError::Spine)
        ));
    }

    #[test]
    fn open_items_of_another_device_or_beyond_through_are_refused() {
        let f = Fixture::new(2);
        let original = f.write(MAX_PART);
        let id = f.open.reserve[0].0;
        for field in ["reserve", "hold"] {
            for bad in [
                ItemId {
                    device: Id16([99; 16]),
                    ..id
                },
                ItemId { seq: 0, ..id },
                ItemId {
                    seq: f.through().s + 1,
                    ..id
                },
            ] {
                let mut w = original.clone();
                f.edit_part(&mut w, "open", None, |v| v[field][0]["id"] = item_id(&bad));
                assert!(f.read(&w).is_err(), "{field}: {bad:?}");
                let mut open = f.open.clone();
                if field == "reserve" {
                    open.reserve[0].0 = bad;
                } else {
                    open.hold[0].0 = bad;
                }
                assert!(f.try_write(&f.records, &open).is_err(), "{field}: {bad:?}");
            }
        }
        // SP17: through is inclusive for both kinds of item.
        let mut open = f.open.clone();
        open.reserve[0].0.seq = f.through().s;
        open.hold[0].0.seq = f.through().s;
        let w = f.try_write(&f.records, &open).unwrap();
        assert_eq!(f.read(&w).unwrap().open, open);
    }

    #[test]
    fn a_part_at_the_limit_is_accepted() {
        let f = Fixture::new(1);
        let mut w = f.write(MAX_PART);
        let mut p = f.payload(&w);
        let suffix = format!("/{}.json", p["shards"][0]["hash"].as_str().unwrap());
        let (path, bytes) = w
            .parts
            .iter_mut()
            .find(|(path, _)| path.ends_with(&suffix))
            .unwrap();
        bytes.resize(MAX_PART, b' ');
        let hash = identity_hash(bytes);
        *path = format!("{}/{}.json", path.rsplit_once('/').unwrap().0, hash);
        p["shards"][0]["hash"] = json!(hash.to_string());
        p["shards"][0]["size"] = json!(MAX_PART);
        f.reindex(&mut w, p);
        let view = f.read(&w).unwrap();
        assert_eq!(all_records(&view)[&f.records[0].key], f.records[0]);
        assert_eq!(view.index.shards[0].size, MAX_PART as u64);
    }
}
