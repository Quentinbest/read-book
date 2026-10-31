//! SP17, SP15, SY11: build this device's view and save it before upload.
//!
//! Pure library code: the caller supplies opened own changes read from storage,
//! an advanced ledger and verified SP36 release decisions. Nothing here runs a
//! sync pass or removes a file. Upload parts first and the index last; acknowledge
//! each path with `chain::uploaded`, and resume interrupted uploads with `pending`.

use super::ages::Marks;
use super::chain::Change;
use super::cleanup::Name;
use super::crypto::{blob_name, book_name};
use super::device_store::{DeviceStore, Keystore, LibraryState, StoreError, Unsent};
use super::hold::{Kind, Ledger};
use super::ids::{b64url_encode, identity_hash, Hash32, Id16};
use super::journal::{self, JournalError, Library};
use super::json::MAX_SAFE_INTEGER;
use super::merge::Placed;
use super::record::{self, Checked, Content, Entry, Key, Version};
use super::trust::Mark;
use super::view::{self, Open, Record, Retirement, SpineEntry, View, ViewError, Written};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, thiserror::Error)]
pub enum CompactError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    View(#[from] ViewError),
    #[error("own changes don't continue the previous view")]
    Link,
    #[error("a version or its book-file body is missing from the journal")]
    Missing,
    #[error("the built view doesn't continue the device store's generation or chain")]
    Stale,
}

type Result<T> = std::result::Result<T, CompactError>;

/// SP17: assembled inputs, tied to the library and previous generation they read.
/// Fields stay private so publication cannot accidentally mix separate builds.
#[derive(Clone, Debug)]
pub struct Built {
    library: Id16,
    device: super::ids::DeviceId,
    previous: Option<(u64, Hash32)>,
    through: Mark,
    records: Vec<Record>,
    marks: Vec<(Key, Entry)>,
    spine: Vec<SpineEntry>,
    open: Open,
}

fn raw(conn: &Connection, lib: &Library, key: &Key, id: Entry) -> Result<Value> {
    let raw = journal::version_raw(conn, lib, key, id)?.ok_or(CompactError::Missing)?;
    let raw: Value = serde_json::from_str(&raw).map_err(|_| CompactError::Missing)?;
    if record::version_hash(&raw) != id.hash
        || raw["key"].as_str() != Some(key.as_str())
        || raw["device"].as_str() != Some(id.device.to_string().as_str())
    {
        return Err(CompactError::Missing);
    }
    Ok(raw)
}

/// SP9, SP12: a version names its body blob, plus a bookFile body's EPUB.
pub(crate) fn names(conn: &Connection, naming: &[u8; 32], v: &Version) -> Result<BTreeSet<Name>> {
    let mut out = BTreeSet::new();
    if let Content::Body { hash, .. } = v.content {
        out.insert(Name {
            kind: Kind::Blob,
            name: Hash32::parse(&blob_name(naming, &hash)).expect("SP9 hex"),
        });
        if v.kind == record::Kind::BookFile {
            let body = journal::body(conn, &hash)?.ok_or(CompactError::Missing)?;
            if identity_hash(&body) != hash {
                return Err(CompactError::Missing);
            }
            let body: Value = serde_json::from_slice(&body).map_err(|_| CompactError::Missing)?;
            let hash = body["hash"]
                .as_str()
                .and_then(Hash32::parse)
                .ok_or(CompactError::Missing)?;
            out.insert(Name {
                kind: Kind::Book,
                name: Hash32::parse(&book_name(naming, &hash)).expect("SP9 hex"),
            });
        }
    }
    Ok(out)
}

fn checked(raw: Value, device: super::ids::DeviceId) -> Result<Option<Version>> {
    match record::check_version(raw, device).map_err(JournalError::from)? {
        Checked::Version(v) => Ok(Some(v)),
        // SY9: retain future formats whole; never guess their content semantics.
        Checked::NewerFormat(_) => Ok(None),
    }
}

fn retired(
    conn: &Connection,
    lib: &Library,
    naming: &[u8; 32],
    v: &Version,
) -> Result<BTreeSet<Name>> {
    let current = names(conn, naming, v)?;
    let mut before = BTreeSet::new();
    // SP11, SP17: look up exact predecessors, including superseded rows, as
    // restore::displaced does. Other writers' inherited history entries can be
    // ancestors of the own predecessor, so they must not retire again on every
    // later edit. Preserve the own predecessor and other causal heads only.
    let mut parents = Vec::new();
    for id in &v.history {
        if journal::version_raw(conn, lib, &v.key, *id)?.is_none()
            && journal::spine_contains(conn, lib, &v.key, *id)?
        {
            // SP17, SP36, #111: pruning follows an acknowledged, aged newer
            // descendant: objects it stopped naming are already retired safely;
            // objects it still names are live. No retirement needs protecting
            // again. Peer-view spine-only versions (#179) also contribute nothing:
            // this device cannot have edited content it never held.
            continue;
        }
        if let Some(parent) = checked(raw(conn, lib, &v.key, *id)?, id.device)? {
            parents.push(Placed {
                version: parent,
                seq: id.seq,
            });
        }
    }
    for parent in &parents {
        if parent.version.device == lib.device
            || !parents
                .iter()
                .any(|p| p.id() != parent.id() && p.descends_from(parent))
        {
            before.extend(names(conn, naming, &parent.version)?);
        }
    }
    Ok(before.difference(&current).copied().collect())
}

/// SP13, SP17: directly addressed book children at the deletion's frontier.
/// Include this writer through the deleting change, so child tombstones written
/// together contribute their predecessor objects too. Other writers are bounded
/// by covers; later edits outside it belong to SY18's conflict, not this deletion.
fn book_contents(
    conn: &Connection,
    lib: &Library,
    naming: &[u8; 32],
    v: &Version,
    seq: u64,
) -> Result<BTreeSet<Name>> {
    let Content::Deleted { covers, .. } = &v.content else {
        return Ok(BTreeSet::new());
    };
    let book = v.key.as_str().strip_prefix("book:").expect("book key");
    let mut through: BTreeMap<_, _> = covers.iter().map(|e| (e.device, e.seq)).collect();
    through.insert(lib.device, seq);
    let mut out = BTreeSet::new();
    for (device, through) in through {
        let mut stmt = conn
            .prepare(
                "SELECT key, seq, raw FROM sync_versions WHERE library = ?1 AND device = ?2
             AND seq <= ?3 AND applied IN (1, 2)
             AND (key = ?4 OR key = ?5 OR key LIKE ?6) ORDER BY seq DESC",
            )
            .map_err(JournalError::from)?;
        let rows = stmt
            .query_map(
                params![
                    lib.id.to_string(),
                    device.to_string(),
                    through as i64,
                    format!("bookFile:{book}"),
                    format!("place:{book}"),
                    format!("bookSetting:{book}:%")
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(JournalError::from)?;
        let mut seen = HashSet::new();
        for row in rows {
            let (key, at, value) = row.map_err(JournalError::from)?;
            if !seen.insert(key) {
                continue;
            }
            let value = serde_json::from_str(&value).map_err(|_| CompactError::Missing)?;
            if let Some(child) = checked(value, device)? {
                out.extend(names(conn, naming, &child)?);
                if device == lib.device
                    && at == seq as i64
                    && matches!(child.content, Content::Deleted { .. })
                {
                    out.extend(retired(conn, lib, naming, &child)?);
                }
            }
        }
    }
    Ok(out)
}

/// SP17, SY11: assemble the next view from checked own files in sequence.
/// `None` before the first change: SP17 has no empty `through` encoding.
/// The naming key converts plain hashes to SP9 storage names. `released` must
/// attest SP36 lines 2 and 3, including acknowledgement and the last hold ending.
///
/// SP17/SY18 limitation: a book deletion also retains directly addressed files,
/// place and per-book settings from the journal at its causal frontier. Complete
/// aggregation of its highlights and notes needs historical book membership,
/// which the journal does not directly provide; Stage 4 must supply that. Their
/// individual retirements and SP13's paired highlight/note remain covered here.
#[allow(clippy::too_many_arguments)]
pub fn build(
    conn: &Connection,
    lib: &Library,
    naming: &[u8; 32],
    previous: Option<&View>,
    changes: &[Change],
    ledger: &Ledger,
    marks: &Marks,
    // SP35, SY17: `Vouched::publishes_ages()`. While the clock is marked ahead or
    // has jumped, no new mark is published; marks a view already holds stay.
    marks_publishable: bool,
    rotations: &[Hash32],
    released: &dyn Fn(&Entry) -> bool,
) -> Result<Option<Built>> {
    let mut spine = previous.map(|v| v.spine.clone()).unwrap_or_default();
    let mut through = previous.map(|v| v.through);
    let mut records: BTreeMap<Key, Record> = previous
        .into_iter()
        .flat_map(|v| &v.shards)
        .flat_map(|s| &s.records)
        .map(|r| {
            let mut r = r.clone();
            r.week.retain(|w| !released(&w.v));
            (r.key.clone(), r)
        })
        .collect();
    for change in changes {
        let expected = through.map_or(1, |m| m.s + 1);
        if change.device != lib.device
            || change.seq != expected
            || change.prev != through.map(|m| m.h)
        {
            return Err(CompactError::Link);
        }
        let mut retirements = BTreeMap::<Key, (Entry, BTreeSet<Name>)>::new();
        for value in &change.versions {
            let key = value["key"]
                .as_str()
                .and_then(Key::opaque)
                .ok_or(CompactError::Missing)?;
            let id = Entry {
                device: lib.device,
                seq: change.seq,
                hash: record::version_hash(value),
            };
            // SP17, #111: an own superseded payload may already be pruned;
            // the checked change supplied to this build still holds its bytes.
            let pruned = journal::version_raw(conn, lib, &key, id)?.is_none()
                && journal::spine_contains(conn, lib, &key, id)?;
            let value = if pruned {
                value.clone()
            } else {
                raw(conn, lib, &key, id)?
            };
            if let Some(v) = checked(value.clone(), lib.device)? {
                // SP17, SP36, #111: a pruned version's descendant is accepted by
                // all and past its week, and came after it, so this version's own
                // retirements are past theirs too; its body (a book file's
                // metadata, say) may already be gone, so none is computed.
                if !pruned && !released(&id) {
                    let mut objects = retired(conn, lib, naming, &v)?;
                    if v.kind == record::Kind::Book {
                        objects.extend(book_contents(conn, lib, naming, &v, change.seq)?);
                    }
                    retirements.insert(key.clone(), (id, objects));
                }
            }
            records
                .entry(key.clone())
                .and_modify(|r| r.version = value.clone())
                .or_insert(Record {
                    key,
                    version: value,
                    week: Vec::new(),
                });
        }
        // SP13: the highlight also retains the note deleted in this change.
        // An already-deleted note has no new tombstone and is left alone.
        let paired: Vec<_> = retirements
            .iter()
            .filter(|(key, _)| {
                key.kind() == record::Kind::Annotation
                    && records[*key].version.get("deleted").is_some()
            })
            .filter_map(|(key, _)| {
                let note = Key::parse(&key.as_str().replacen("annotation:", "note:", 1))?.0;
                records.get(&note)?.version.get("deleted")?;
                change
                    .versions
                    .iter()
                    .find(|v| v["key"].as_str() == Some(note.as_str()))?;
                Some((key.clone(), note))
            })
            .collect();
        for (key, note) in paired {
            let note = checked(records[&note].version.clone(), lib.device)?
                .ok_or(CompactError::Missing)?;
            // SP13: the note's own retirement may already be released; that
            // never shortens the highlight's separate retirement.
            let objects = retired(conn, lib, naming, &note)?;
            retirements
                .get_mut(&key)
                .expect("collected above")
                .1
                .extend(objects);
        }
        for (key, (v, objects)) in retirements {
            if !objects.is_empty() {
                records
                    .get_mut(&key)
                    .expect("inserted above")
                    .week
                    .push(Retirement {
                        v,
                        objects: objects.into_iter().collect(),
                    });
            }
        }
        spine.push(SpineEntry::from_change(change));
        through = Some(Mark {
            s: change.seq,
            h: change.hash,
        });
    }
    let Some(through) = through else {
        return Ok(None);
    };
    // SP35: marks already moved into the previous view remain permanent. Pending
    // marks may concern peers, so route them with the journal, not the own spine.
    let mut routed = BTreeMap::<Key, HashSet<Entry>>::new();
    let carried: HashSet<Entry> = previous
        .into_iter()
        .flat_map(|v| &v.shards)
        .flat_map(|s| &s.marks)
        .copied()
        .collect();
    let mut ids: HashSet<Entry> = if marks_publishable {
        marks.pending().copied().collect()
    } else {
        HashSet::new()
    };
    ids.extend(&carried);
    let mut known_keys = None;
    for id in ids {
        let text: Option<String> = conn
            .query_row(
                "SELECT key FROM sync_versions WHERE library = ?1 AND device = ?2 AND seq = ?3 AND hash = ?4",
                params![lib.id.to_string(), id.device.to_string(), id.seq as i64, id.hash.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(JournalError::from)?;
        let mut key = text.as_deref().and_then(Key::opaque).or_else(|| {
            // An unchanged own record can survive even without its journal row.
            records
                .values()
                .find(|r| record::version_hash(&r.version) == id.hash)
                .map(|r| r.key.clone())
        });
        if key.is_none() {
            // SP17, SP35, #111: the pruned version's prefix still routes its
            // permanent mark. A retained descendant supplies the key text.
            if known_keys.is_none() {
                let texts: Vec<String> = conn
                    .prepare("SELECT DISTINCT key FROM sync_versions WHERE library = ?1")
                    .map_err(JournalError::from)?
                    .query_map([lib.id.to_string()], |r| r.get(0))
                    .map_err(JournalError::from)?
                    .collect::<std::result::Result<_, _>>()
                    .map_err(JournalError::from)?;
                let keys: BTreeMap<Vec<u8>, Key> = texts
                    .iter()
                    .filter_map(|s| Key::opaque(s))
                    .chain(records.keys().cloned())
                    .map(|k| {
                        (
                            super::envelope::vlist_entry(k.as_str(), &id.hash)[..16].to_vec(),
                            k,
                        )
                    })
                    .collect();
                known_keys = Some(keys);
            }
            let prefix: Option<Vec<u8>> = conn.query_row(
                "SELECT key_hash FROM sync_spine_versions WHERE library = ?1 AND device = ?2 AND seq = ?3 AND hash = ?4",
                params![lib.id.to_string(), id.device.to_string(), id.seq as i64, id.hash.to_string()], |r| r.get(0),
            ).optional().map_err(JournalError::from)?;
            key = prefix.and_then(|p| known_keys.as_ref().and_then(|keys| keys.get(&p)).cloned());
        }
        // SP17, SP35: a mark goes in the shard its key falls in, whoever wrote the
        // key; a key only another device wrote has no record here but its mark
        // still moves into the view, or it would stay pending for good.
        if let Some(key) = key {
            routed.entry(key).or_default().insert(id);
        } else if carried.contains(&id) {
            // SP35: a permanent mark cannot silently disappear if its routing
            // evidence was lost. A pending mark whose version this device doesn't
            // hold at all has no key to route by and waits.
            return Err(CompactError::Missing);
        }
    }
    let mut marks: Vec<_> = routed
        .into_iter()
        .flat_map(|(key, ids)| ids.into_iter().map(move |id| (key.clone(), id)))
        .collect();
    marks.sort_by_key(|(k, id)| (k.clone(), id.device, id.seq, id.hash));
    Ok(Some(Built {
        library: lib.id,
        device: lib.device,
        previous: previous.map(|v| (v.gen, v.hash)),
        through,
        records: records.into_values().collect(),
        marks,
        spine,
        open: Open {
            reserve: ledger.open_reservations(lib.device).collect(),
            hold: ledger.open_holds(lib.device).collect(),
            rotations: rotations.to_vec(),
        },
    }))
}

fn last_view(state: &LibraryState) -> Result<Option<(u64, Hash32)>> {
    let gen = match state.rest.get("gen") {
        None => 0,
        Some(v) => v
            .as_u64()
            .filter(|g| *g <= MAX_SAFE_INTEGER)
            .ok_or(StoreError::Unreadable)?,
    };
    let hash = match state.rest.get("view") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .and_then(Hash32::parse)
                .ok_or(StoreError::Unreadable)?,
        ),
    };
    match (gen, hash) {
        (0, None) => Ok(None),
        (1.., Some(h)) => Ok(Some((gen, h))),
        _ => Err(StoreError::Unreadable.into()),
    }
}

/// SP15: queued view bytes, checked against the stored generation, index hash
/// and signed part references; parts first, index last. Already uploaded parts
/// may have left `unsent`. A retry never constructs or signs another generation
/// while any of this view remains queued.
pub fn pending(store: &DeviceStore, library: &Id16) -> Result<Vec<(String, Vec<u8>)>> {
    let Some(state) = store.library(library) else {
        return Ok(Vec::new());
    };
    let files: Vec<_> = state
        .unsent
        .iter()
        .filter(|u| u.path.starts_with("state/"))
        .map(|u| Ok((u.path.clone(), u.bytes().ok_or(StoreError::Unreadable)?)))
        .collect::<Result<_>>()?;
    if files.is_empty() {
        return Ok(files);
    }
    let device = state.device_id()?;
    let (gen, hash) = last_view(state)?.ok_or(StoreError::Unreadable)?;
    let dir = format!("state/{device}/{hash}");
    let path = format!("{dir}/index.json");
    let bytes = files
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, b)| b)
        .ok_or(StoreError::Unreadable)?;
    let (index, actual, _) = view::open_index(bytes, library, &device, &state.key()?.public())?;
    if index.gen != gen || actual != hash {
        return Err(StoreError::Unreadable.into());
    }
    let parts: BTreeMap<_, _> = index
        .shards
        .iter()
        .map(|r| (r.hash, r.size))
        .chain(index.spine.iter().map(|r| (r.hash, r.size)))
        .chain([(index.open.hash, index.open.size)])
        .collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (p, b) in &files {
        if !seen.insert(p) {
            return Err(StoreError::Unreadable.into());
        }
        if *p == path {
            continue;
        }
        let h = identity_hash(b);
        if *p != format!("{dir}/{h}.json") || parts.get(&h) != Some(&(b.len() as u64)) {
            return Err(StoreError::Unreadable.into());
        }
        out.push((p.clone(), b.clone()));
    }
    out.push((path, bytes.clone()));
    Ok(out)
}

/// SP15, SP28: sign and durably queue the next generation before returning it.
/// `None` if there is no chain yet or an earlier upload still needs `pending`.
/// Marks leave the device-file queue only after the keystore save succeeds.
pub fn publish(
    conn: &Connection,
    lib: &Library,
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    built: Option<&Built>,
    marks: &mut Marks,
) -> Result<Option<Written>> {
    publish_within(conn, lib, ks, store, built, marks, view::MAX_PART)
}

#[allow(clippy::too_many_arguments)]
fn publish_within(
    conn: &Connection,
    lib: &Library,
    ks: &dyn Keystore,
    store: &mut DeviceStore,
    built: Option<&Built>,
    marks: &mut Marks,
    limit: usize,
) -> Result<Option<Written>> {
    let state = store.library(&lib.id).ok_or(StoreError::Unreadable)?;
    if state.device_id()? != lib.device {
        return Err(CompactError::Stale);
    }
    if !pending(store, &lib.id)?.is_empty() {
        return Ok(None);
    }
    let Some(built) = built else {
        return Ok(None);
    };
    let before = last_view(state)?;
    if built.library != lib.id
        || built.device != lib.device
        || built.previous != before
        || state.seq < built.through.s
        || (state.seq == built.through.s && state.last_hash()? != Some(built.through.h))
    {
        return Err(CompactError::Stale);
    }
    let held: Option<String> = conn
        .query_row(
            "SELECT hash FROM sync_changes WHERE library = ?1 AND device = ?2 AND seq = ?3",
            params![
                lib.id.to_string(),
                lib.device.to_string(),
                built.through.s as i64
            ],
            |r| r.get(0),
        )
        .optional()
        .map_err(JournalError::from)?;
    if held.as_deref().and_then(Hash32::parse) != Some(built.through.h) {
        return Err(CompactError::Stale);
    }
    let gen = before.map_or(0, |(g, _)| g);
    if gen >= MAX_SAFE_INTEGER {
        return Err(StoreError::Unreadable.into());
    }
    let written = view::write_within(
        &state.key()?,
        &lib.id,
        &state.writer_id()?,
        gen + 1,
        before.map(|(_, h)| h),
        built.through,
        &built.records,
        &built.marks,
        &built.spine,
        &built.open,
        limit,
    )?;
    let (_, hash, _) = view::open_index(
        &written.index_bytes,
        &lib.id,
        &lib.device,
        &state.key()?.public(),
    )?;
    // SP15 step 3: clone, save, then expose the new state and bytes. A failed
    // save leaves both the caller's store and pending marks untouched.
    let mut next = store.clone();
    let state = next.library_mut(&lib.id).expect("checked above");
    state.rest.insert("gen".into(), Value::from(gen + 1));
    state
        .rest
        .insert("view".into(), Value::from(hash.to_string()));
    let mut seen = HashSet::new();
    for (path, bytes) in written
        .parts
        .iter()
        .map(|(p, b)| (p, b))
        .chain(std::iter::once((&written.index_path, &written.index_bytes)))
    {
        if seen.insert(path) {
            state.unsent.push(Unsent {
                path: path.clone(),
                bytes: b64url_encode(bytes),
            });
        }
    }
    next.save(ks)?;
    *store = next;
    marks.moved_into_view(&built.marks.iter().map(|(_, id)| *id).collect::<Vec<_>>());
    Ok(Some(written))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::apply::tests::{device_with, highlight, LIB};
    use crate::sync::chain;
    use crate::sync::crypto::Keypair;
    use crate::sync::device_store::Memory;
    use crate::sync::hold::{Hold, ItemId, Reserve};
    use crate::sync::journal::LocalContent;
    use crate::sync::retention::{Ages, Vouched, WEEK_MS};
    use serde_json::json;
    use std::sync::Mutex;

    const NAMING: [u8; 32] = [42; 32];
    const NOTE: &str = "note:00000000-0000-4000-8000-000000000001";

    pub(crate) struct Device {
        pub(crate) db: Store,
        pub(crate) lib: Library,
        pub(crate) ks: Memory,
        pub(crate) store: DeviceStore,
        marks: Marks,
    }

    impl Device {
        pub(crate) fn new() -> Self {
            let ks = Memory::default();
            let mut store = DeviceStore::default();
            let device = store.identity(&LIB).unwrap().device_id().unwrap();
            store.save(&ks).unwrap();
            let (db, lib) = device_with(device, "book");
            Self {
                db,
                lib,
                ks,
                store,
                marks: Marks::default(),
            }
        }

        pub(crate) fn key(&self) -> Keypair {
            self.store.library(&self.lib.id).unwrap().key().unwrap()
        }

        pub(crate) fn local(&self, text: &str, content: LocalContent) -> Version {
            journal::record_local(
                self.db.conn(),
                &self.lib,
                &Key::parse(text).unwrap().0,
                content,
                1_760_000_000_000,
            )
            .unwrap()
        }

        pub(crate) fn note(&self, text: &str, body: &str) -> Version {
            self.local(
                text,
                LocalContent::Body(json!({"text": body}).to_string().into_bytes()),
            )
        }

        fn delete(&self, text: &str) -> Version {
            self.local(
                text,
                LocalContent::Deleted {
                    covers: Vec::new(),
                    identity: None,
                },
            )
        }

        pub(crate) fn change(&mut self) -> Change {
            let signed = chain::publish(self.db.conn(), &self.lib, &self.ks, &mut self.store)
                .unwrap()
                .unwrap();
            let c = chain::open_change(
                &signed.bytes,
                &self.lib.id,
                &self.lib.device,
                &self.key().public(),
            )
            .unwrap();
            // SP15: uploaded own changes are no longer local inputs. The caller
            // keeps the opened file read from storage and supplies it to build.
            chain::uploaded(&self.ks, &mut self.store, &self.lib.id, &signed.path).unwrap();
            c
        }

        pub(crate) fn build(
            &self,
            previous: Option<&View>,
            changes: &[Change],
            released: &dyn Fn(&Entry) -> bool,
        ) -> Built {
            build(
                self.db.conn(),
                &self.lib,
                &NAMING,
                previous,
                changes,
                &Ledger::default(),
                &self.marks,
                true,
                &[],
                released,
            )
            .unwrap()
            .unwrap()
        }

        pub(crate) fn save(&mut self, built: &Built) -> Written {
            publish(
                self.db.conn(),
                &self.lib,
                &self.ks,
                &mut self.store,
                Some(built),
                &mut self.marks,
            )
            .unwrap()
            .unwrap()
        }

        pub(crate) fn open(&self, w: &Written) -> View {
            view::open(
                &w.index_bytes,
                &self.lib.id,
                &self.lib.device,
                &self.key().public(),
                &|p| w.parts.iter().find(|(q, _)| q == p).map(|(_, b)| b.clone()),
            )
            .unwrap()
        }

        pub(crate) fn acknowledge(&mut self, w: &Written) {
            for path in w.parts.iter().map(|(p, _)| p).chain([&w.index_path]) {
                chain::uploaded(&self.ks, &mut self.store, &self.lib.id, path).unwrap();
            }
        }

        pub(crate) fn mark(&mut self, id: Entry) {
            let mut ages = Ages::default();
            ages.saw(id, 0);
            let mut clock = Vouched::new(0);
            clock.tick(0, 0);
            clock.tick(WEEK_MS, WEEK_MS);
            super::super::ages::lists(&ages, &mut self.marks, &[id], &clock);
        }
    }

    fn records(v: &View) -> BTreeMap<String, &Record> {
        v.shards
            .iter()
            .flat_map(|s| &s.records)
            .map(|r| (r.key.to_string(), r))
            .collect()
    }

    fn id(c: &Change, v: &Version) -> Entry {
        Entry {
            device: c.device,
            seq: c.seq,
            hash: v.hash,
        }
    }

    #[test]
    fn a_compacted_view_holds_every_own_key_at_its_latest_version() {
        let mut d = Device::new();
        let a = NOTE;
        let b = "note:00000000-0000-4000-8000-000000000002";
        let c = "note:00000000-0000-4000-8000-000000000003";
        d.note(a, "old a");
        let one = d.change();
        d.note(a, "new a");
        d.note(b, "old b");
        let two = d.change();
        d.note(b, "new b");
        d.note(c, "c");
        let three = d.change();
        let changes = [one, two, three];
        let built = d.build(None, &changes, &|_| false);
        let w = d.save(&built);
        let view = d.open(&w);
        let actual = records(&view);
        let expected: BTreeMap<_, _> = changes
            .iter()
            .flat_map(|c| &c.versions)
            .map(|v| (v["key"].as_str().unwrap().to_owned(), v))
            .collect();
        assert_eq!(actual.len(), 3);
        for (key, latest) in expected {
            assert_eq!(&actual[&key].version, latest);
        }
        assert_eq!(
            view.spine,
            changes
                .iter()
                .map(SpineEntry::from_change)
                .collect::<Vec<_>>()
        );
        assert_eq!(view.through.s, 3);
        assert_eq!(
            records(&view)[a].week.len(),
            1,
            "the edited body is retired"
        );
    }

    #[test]
    fn the_next_generation_chains_to_the_previous() {
        let mut d = Device::new();
        d.note(NOTE, "first");
        let one = d.change();
        let first = d.build(None, &[one], &|_| false);
        let w = d.save(&first);
        let old = d.open(&w);
        d.acknowledge(&w);
        let unchanged = "note:00000000-0000-4000-8000-000000000002";
        d.note(NOTE, "next");
        d.note(unchanged, "unchanged");
        let two = d.change();
        let second = d.build(Some(&old), std::slice::from_ref(&two), &|_| false);
        let w = d.save(&second);
        let next = d.open(&w);
        assert_eq!(next.gen, old.gen + 1);
        assert!(view::follows(&next, &old.hash));
        assert_eq!(next.prev, Some(old.hash));
        assert_eq!(&next.spine[..old.spine.len()], &old.spine);
        assert_eq!(next.spine.last(), Some(&SpineEntry::from_change(&two)));
        d.acknowledge(&w);
        // No new change is needed for release/mark/open-state updates. Records
        // not changed since the view need no journal lookup.
        d.db.conn()
            .execute(
                "DELETE FROM sync_versions WHERE library = ?1",
                [d.lib.id.to_string()],
            )
            .unwrap();
        let third = d.build(Some(&next), &[], &|_| false);
        let w = d.save(&third);
        let last = d.open(&w);
        assert_eq!(last.gen, 3);
        assert_eq!(records(&last), records(&next));
    }

    #[test]
    fn a_deleted_notes_body_stays_in_week_until_released() {
        let mut d = Device::new();
        let body = d.note(NOTE, "keep me");
        let one = d.change();
        let deletion = d.delete(NOTE);
        let two = d.change();
        let retiring = id(&two, &deletion);
        let built = d.build(None, &[one, two], &|_| false);
        let w = d.save(&built);
        let first = d.open(&w);
        let expected = names(d.db.conn(), &NAMING, &body).unwrap();
        assert_eq!(
            records(&first)[NOTE].week,
            vec![Retirement {
                v: retiring,
                objects: expected.into_iter().collect(),
            }]
        );
        d.acknowledge(&w);
        let built = d.build(Some(&first), &[], &|_| false);
        let w = d.save(&built);
        let next = d.open(&w);
        assert_eq!(records(&next)[NOTE].week, records(&first)[NOTE].week);
        d.acknowledge(&w);
        let built = d.build(Some(&next), &[], &|v| *v == retiring);
        let w = d.save(&built);
        let last = d.open(&w);
        assert!(records(&last)[NOTE].week.is_empty());
        assert!(records(&last)[NOTE].version.get("deleted").is_some());
    }

    #[test]
    fn marks_and_open_items_are_carried() {
        let mut d = Device::new();
        let v = d.note(NOTE, "marked");
        let one = d.change();
        let marked = id(&one, &v);
        d.mark(marked);
        let absent = "note:00000000-0000-4000-8000-000000000002";
        // SP17, SP35: a mark for a key only a peer wrote still moves into the view,
        // in the shard its key falls in, though this device has no record there.
        let peer = Keypair::from_seed(&[8; 32]);
        let pv = json!({"format": 1, "key": absent, "device": peer.device().to_string(),
            "clock": [1, 0], "history": [], "deleted": {"covers": []}});
        let peers = Entry {
            device: peer.device(),
            seq: 1,
            hash: record::version_hash(&pv),
        };
        d.db.conn()
            .execute(
                "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied)
            VALUES (?1, ?2, ?3, 1, ?4, ?5, 1)",
                params![
                    d.lib.id.to_string(),
                    absent,
                    peers.device.to_string(),
                    peers.hash.to_string(),
                    pv.to_string()
                ],
            )
            .unwrap();
        d.mark(peers);
        let reserve = Reserve {
            kind: Kind::Blob,
            name: Hash32([1; 32]),
            size: 12,
        };
        let hold = Hold {
            kind: Kind::Blob,
            name: Hash32([2; 32]),
            hash: Hash32([3; 32]),
            size: 20,
            for_key: Key::parse(NOTE).unwrap().0,
        };
        let signed = chain::publish_with(
            d.db.conn(),
            &d.lib,
            &d.ks,
            &mut d.store,
            std::slice::from_ref(&reserve),
            std::slice::from_ref(&hold),
            &Ledger::default(),
            &BTreeSet::new(),
        )
        .unwrap()
        .unwrap();
        let two =
            chain::open_change(&signed.bytes, &d.lib.id, &d.lib.device, &d.key().public()).unwrap();
        let clock = Vouched::new(0);
        let mut ledger = Ledger::default();
        ledger.receive(&one, &[], false, &clock).unwrap();
        ledger.receive(&two, &[], false, &clock).unwrap();
        let foreign = chain::sign_change_with(
            &peer,
            &d.lib.id,
            &Id16([9; 16]),
            1,
            None,
            &[],
            std::slice::from_ref(&reserve),
            std::slice::from_ref(&hold),
        );
        let foreign =
            chain::open_change(&foreign.bytes, &d.lib.id, &peer.device(), &peer.public()).unwrap();
        ledger.receive(&foreign, &[], false, &clock).unwrap();
        let rotation = Hash32([5; 32]);
        let built = build(
            d.db.conn(),
            &d.lib,
            &NAMING,
            None,
            &[one, two],
            &ledger,
            &d.marks,
            true,
            &[rotation],
            &|_| false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(d.marks.pending().count(), 2);
        let w = d.save(&built);
        let view = d.open(&w);
        let carried = |view: &View| {
            view.shards
                .iter()
                .flat_map(|s| &s.marks)
                .copied()
                .collect::<HashSet<_>>()
        };
        assert_eq!(carried(&view), HashSet::from([marked, peers]));
        assert_eq!(d.marks.pending().count(), 0);
        let item = ItemId {
            device: d.lib.device,
            seq: 2,
            index: 0,
        };
        assert_eq!(view.open.reserve, vec![(item, reserve)]);
        assert_eq!(view.open.hold, vec![(item, hold)]);
        assert_eq!(view.open.rotations, vec![rotation]);
        d.acknowledge(&w);
        let built = d.build(Some(&view), &[], &|_| false);
        let w = d.save(&built);
        assert_eq!(carried(&d.open(&w)), HashSet::from([marked, peers]));
    }

    #[test]
    fn no_new_mark_is_published_while_the_clock_is_untrusted() {
        // SP35, SY17: marks earned while the clock is marked ahead or has jumped
        // wait; marks a view already holds stay.
        let mut d = Device::new();
        let v = d.note(NOTE, "marked");
        let one = d.change();
        let earned = id(&one, &v);
        d.mark(earned);
        let built = build(
            d.db.conn(),
            &d.lib,
            &NAMING,
            None,
            &[one],
            &Ledger::default(),
            &d.marks,
            false,
            &[],
            &|_| false,
        )
        .unwrap()
        .unwrap();
        let w = d.save(&built);
        let view = d.open(&w);
        assert!(view.shards.iter().all(|s| s.marks.is_empty()));
        assert_eq!(d.marks.pending().copied().collect::<Vec<_>>(), vec![earned]);
        // Once the clock is clear, the mark moves into the next view.
        d.acknowledge(&w);
        let built = d.build(Some(&view), &[], &|_| false);
        let w = d.save(&built);
        assert!(d.open(&w).shards.iter().any(|s| s.marks.contains(&earned)));
    }

    #[test]
    fn a_crash_after_saving_reuploads_the_same_view() {
        let mut d = Device::new();
        d.note(NOTE, "saved");
        let change = d.change();
        let built = d.build(None, &[change], &|_| false);
        let w = d.save(&built);
        let saved = d.ks.load().unwrap();
        d.store = DeviceStore::load(&d.ks).unwrap();
        let queued = pending(&d.store, &d.lib.id).unwrap();
        assert_eq!(
            queued.last(),
            Some(&(w.index_path.clone(), w.index_bytes.clone()))
        );
        assert_eq!(&queued[..queued.len() - 1], &w.parts);
        // A later local edit cannot make the retry sign another view.
        d.note(NOTE, "after the crash");
        assert_eq!(
            publish(
                d.db.conn(),
                &d.lib,
                &d.ks,
                &mut d.store,
                Some(&built),
                &mut d.marks
            )
            .unwrap(),
            None
        );
        assert_eq!(d.ks.load().unwrap(), saved);
        assert_eq!(pending(&d.store, &d.lib.id).unwrap(), queued);
        // Part-way upload: already acknowledged parts need not be in pending.
        chain::uploaded(&d.ks, &mut d.store, &d.lib.id, &w.parts[0].0).unwrap();
        assert_eq!(pending(&d.store, &d.lib.id).unwrap(), queued[1..]);
    }

    struct Refuses(Mutex<Option<Vec<u8>>>);
    impl Keystore for Refuses {
        fn load(&self) -> std::result::Result<Option<Vec<u8>>, StoreError> {
            Ok(None)
        }
        fn save(&self, bytes: &[u8]) -> std::result::Result<(), StoreError> {
            *self.0.lock().unwrap() = Some(bytes.to_vec());
            Err(StoreError::Keychain(-25308))
        }
    }

    #[test]
    fn a_crash_before_saving_signs_the_generation_again() {
        let mut d = Device::new();
        let v = d.note(NOTE, "signed");
        let change = d.change();
        let marked = id(&change, &v);
        d.mark(marked);
        let built = d.build(None, &[change], &|_| false);
        let before = d.store.clone();
        let refuses = Refuses(Mutex::new(None));
        assert!(matches!(
            publish(
                d.db.conn(),
                &d.lib,
                &refuses,
                &mut d.store,
                Some(&built),
                &mut d.marks
            ),
            Err(CompactError::Store(StoreError::Keychain(_)))
        ));
        assert_eq!(d.store, before);
        assert_eq!(DeviceStore::load(&d.ks).unwrap(), before);
        assert!(pending(&d.store, &d.lib.id).unwrap().is_empty());
        assert_eq!(d.marks.pending().copied().collect::<Vec<_>>(), vec![marked]);
        // Capture step 2's signed bytes, whose step 3 save failed. The retry
        // signs generation 1 afresh and produces exactly the same stored bytes.
        let attempted = Memory::default();
        attempted
            .save(refuses.0.lock().unwrap().as_ref().unwrap())
            .unwrap();
        let first = DeviceStore::load(&attempted).unwrap();
        let signed = pending(&first, &d.lib.id).unwrap();
        let w = d.save(&built);
        assert_eq!(d.open(&w).gen, 1);
        assert_eq!(pending(&d.store, &d.lib.id).unwrap(), signed);
        assert_eq!(d.store, first);
        assert_eq!(d.marks.pending().count(), 0);
    }

    #[test]
    fn a_library_needing_several_shards_compacts_and_reads_back_whole() {
        let mut d = Device::new();
        let mut changes = Vec::new();
        for i in 0..60 {
            let key = format!("note:00000000-0000-4000-8000-{i:012}");
            d.note(&key, &format!("body {i}"));
            changes.push(d.change());
        }
        let built = d.build(None, &changes, &|_| false);
        // SP17: the production publication path and writer, with a smaller part
        // limit only. Both records and the whole-chain spine require many parts.
        let w = publish_within(
            d.db.conn(),
            &d.lib,
            &d.ks,
            &mut d.store,
            Some(&built),
            &mut d.marks,
            1500,
        )
        .unwrap()
        .unwrap();
        let view = d.open(&w);
        assert!(view.index.shards.len() > 1);
        assert!(view.index.spine.len() > 1);
        assert!(w.parts.iter().all(|(_, b)| b.len() <= 1500));
        assert_eq!(records(&view).len(), changes.len());
        assert_eq!(view.through.h, changes.last().unwrap().hash);
        for c in &changes {
            let value = &c.versions[0];
            assert_eq!(
                &records(&view)[value["key"].as_str().unwrap()].version,
                value
            );
        }
        let reloaded = DeviceStore::load(&d.ks).unwrap();
        let queued = pending(&reloaded, &d.lib.id).unwrap();
        let again = view::open(
            &queued.last().unwrap().1,
            &d.lib.id,
            &d.lib.device,
            &d.key().public(),
            &|path| {
                queued
                    .iter()
                    .find(|(p, _)| p == path)
                    .map(|(_, b)| b.clone())
            },
        )
        .unwrap();
        assert_eq!(view, again);
    }

    #[test]
    fn gaps_unlinked_changes_and_stale_builds_publish_nothing() {
        let mut d = Device::new();
        assert!(build(
            d.db.conn(),
            &d.lib,
            &NAMING,
            None,
            &[],
            &Ledger::default(),
            &d.marks,
            true,
            &[],
            &|_| false
        )
        .unwrap()
        .is_none());
        d.note(NOTE, "one");
        let one = d.change();
        d.note(NOTE, "two");
        let two = d.change();
        assert!(matches!(
            build(
                d.db.conn(),
                &d.lib,
                &NAMING,
                None,
                std::slice::from_ref(&two),
                &Ledger::default(),
                &d.marks,
                true,
                &[],
                &|_| false
            ),
            Err(CompactError::Link)
        ));
        let mut wrong = two.clone();
        wrong.prev = Some(Hash32([9; 32]));
        assert!(matches!(
            build(
                d.db.conn(),
                &d.lib,
                &NAMING,
                None,
                &[one.clone(), wrong],
                &Ledger::default(),
                &d.marks,
                true,
                &[],
                &|_| false
            ),
            Err(CompactError::Link)
        ));
        let built = d.build(None, &[one, two], &|_| false);
        let w = d.save(&built);
        d.acknowledge(&w);
        let before = d.store.clone();
        assert!(matches!(
            publish(
                d.db.conn(),
                &d.lib,
                &d.ks,
                &mut d.store,
                Some(&built),
                &mut d.marks
            ),
            Err(CompactError::Stale)
        ));
        assert_eq!(d.store, before);
    }

    #[test]
    fn missing_ancestors_and_corrupt_queued_bytes_are_errors() {
        let mut d = Device::new();
        d.note(NOTE, "one");
        let one = d.change();
        d.delete(NOTE);
        let two = d.change();
        let built = d.build(None, &[one, two], &|_| false);
        let w = d.save(&built);
        let mut bad = d.store.clone();
        bad.library_mut(&d.lib.id).unwrap().unsent[0].bytes = b64url_encode(b"changed");
        assert!(pending(&bad, &d.lib.id).is_err());
        let mut bad = d.store.clone();
        bad.library_mut(&d.lib.id).unwrap().unsent[0].path = "state/elsewhere.json".into();
        assert!(pending(&bad, &d.lib.id).is_err());
        d.acknowledge(&w);
        d.note(NOTE, "restore");
        let three = d.change();
        d.note(NOTE, "four");
        let four = d.change();
        d.db.conn()
            .execute(
                "DELETE FROM sync_versions WHERE library = ?1 AND seq = ?2",
                params![d.lib.id.to_string(), three.seq as i64 - 1],
            )
            .unwrap();
        assert!(matches!(
            build(
                d.db.conn(),
                &d.lib,
                &NAMING,
                Some(&d.open(&w)),
                &[three, four],
                &Ledger::default(),
                &d.marks,
                true,
                &[],
                &|_| false
            ),
            Err(CompactError::Missing)
        ));
    }

    #[test]
    fn replaced_files_retire_only_objects_the_new_version_stopped_naming() {
        let mut d = Device::new();
        let key = "bookFile:00000000-0000-4000-8000-000000000001";
        let file = |n| {
            LocalContent::Body(
                json!({"hash": Hash32([n; 32]).to_string(), "size": 10,
            "title": "Book", "authors": [], "language": null, "identifier": null,
            "layout": "reflowable"})
                .to_string()
                .into_bytes(),
            )
        };
        let old = d.local(key, file(1));
        let one = d.change();
        d.local(key, file(2));
        let two = d.change();
        d.local(key, file(2));
        let three = d.change();
        let built = d.build(None, &[one, two.clone(), three], &|_| false);
        let w = d.save(&built);
        let view = d.open(&w);
        let rs = records(&view);
        assert_eq!(rs[key].week.len(), 1);
        assert_eq!(rs[key].week[0].v.seq, two.seq);
        assert_eq!(
            rs[key].week[0]
                .objects
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            names(d.db.conn(), &NAMING, &old).unwrap()
        );
    }

    #[test]
    fn a_highlights_week_includes_the_note_deleted_with_it() {
        let mut d = Device::new();
        d.db.save_annotation(&highlight("book", Some("paired note"), 1_760_000_001_000))
            .unwrap();
        let one = d.change();
        let note = one
            .versions
            .iter()
            .find(|v| v["key"].as_str().unwrap().starts_with("note:"))
            .unwrap();
        let hash = Hash32::parse(note["body"]["hash"].as_str().unwrap()).unwrap();
        let name = Name {
            kind: Kind::Blob,
            name: Hash32::parse(&blob_name(&NAMING, &hash)).unwrap(),
        };
        d.db.delete_annotation(crate::sync::apply::tests::ANNO)
            .unwrap();
        let two = d.change();
        let note_id = Entry {
            device: d.lib.device,
            seq: two.seq,
            hash: record::version_hash(
                two.versions
                    .iter()
                    .find(|v| v["key"].as_str().unwrap().starts_with("note:"))
                    .unwrap(),
            ),
        };
        // Releasing the note separately cannot release the highlight's copy.
        let built = d.build(None, &[one, two], &|v| *v == note_id);
        let w = d.save(&built);
        let view = d.open(&w);
        let annotation = view
            .shards
            .iter()
            .flat_map(|s| &s.records)
            .find(|r| r.key.kind() == record::Kind::Annotation)
            .unwrap();
        assert!(annotation.week[0].objects.contains(&name));
        assert_eq!(annotation.week[0].objects.len(), 2);
    }

    #[test]
    fn a_book_deletion_keeps_its_file_place_and_per_book_settings() {
        let mut d = Device::new();
        let book = "00000000-0000-4000-8000-000000000001";
        let book_key = format!("book:{book}");
        let file = d.local(
            &format!("bookFile:{book}"),
            LocalContent::Body(
                json!({
                    "hash": Hash32([7; 32]).to_string(), "size": 10, "title": "Book", "authors": [],
                    "language": null, "identifier": null, "layout": "reflowable"
                })
                .to_string()
                .into_bytes(),
            ),
        );
        let place = d.local(
            &format!("place:{book}"),
            LocalContent::Body(br#"{"cfi":"/6/4","fraction":0.5,"chapter":null}"#.to_vec()),
        );
        let setting = d.local(
            &format!("bookSetting:{book}:publisherStyles"),
            LocalContent::Body(br#"{"value":"false"}"#.to_vec()),
        );
        let metadata = d.local(
            &book_key,
            LocalContent::Body(
                json!({"syncId": book, "aliases": []})
                    .to_string()
                    .into_bytes(),
            ),
        );
        let one = d.change();
        // A peer's directly addressed child is read only through the deletion's
        // covers, even when this journal already holds a later version of it.
        let peer = Keypair::from_seed(&[8; 32]);
        let remote_key = format!("bookSetting:{book}:pageWidth");
        let mut remote = Vec::new();
        let mut prev = None;
        let mut history = Vec::new();
        let mut cutoff = None;
        for seq in 1..=2 {
            let body = format!(r#"{{"value":"{seq}"}}"#).into_bytes();
            let hash = journal::put_body(d.db.conn(), &body).unwrap();
            let value = json!({"format": 1, "key": remote_key, "device": peer.device().to_string(),
                "clock": [seq, 0], "history": history, "body": {"hash": hash.to_string(), "size": body.len()}});
            let signed = chain::sign_change(
                &peer,
                &d.lib.id,
                &Id16([9; 16]),
                seq,
                prev,
                std::slice::from_ref(&value),
            );
            let v = checked(value.clone(), peer.device()).unwrap().unwrap();
            d.db.conn()
                .execute(
                    "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
                    params![
                        d.lib.id.to_string(),
                        remote_key,
                        peer.device().to_string(),
                        seq as i64,
                        v.hash.to_string(),
                        value.to_string()
                    ],
                )
                .unwrap();
            cutoff.get_or_insert(Entry {
                device: peer.device(),
                seq,
                hash: signed.hash,
            });
            history =
                vec![json!({"d": peer.device().to_string(), "s": seq, "h": v.hash.to_string()})];
            prev = Some(signed.hash);
            remote.push(v);
        }
        d.local(
            &book_key,
            LocalContent::Deleted {
                covers: vec![cutoff.unwrap()],
                identity: Some(json!({"hashes": [Hash32([7; 32]).to_string()], "idTitle": null})),
            },
        );
        let two = d.change();
        let built = d.build(None, &[one, two], &|_| false);
        let w = d.save(&built);
        let view = d.open(&w);
        let expected: BTreeSet<_> = [&file, &place, &setting, &metadata, &remote[0]]
            .into_iter()
            .flat_map(|v| names(d.db.conn(), &NAMING, v).unwrap())
            .collect();
        assert_eq!(
            records(&view)[&book_key].week[0]
                .objects
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert_eq!(expected.len(), 6);
        assert!(expected.is_disjoint(&names(d.db.conn(), &NAMING, &remote[1]).unwrap()));
    }

    #[test]
    fn an_inherited_remote_ancestor_does_not_retire_again_on_later_edits() {
        let mut d = Device::new();
        let peer = Keypair::from_seed(&[8; 32]);
        let hash = journal::put_body(d.db.conn(), br#"{"text":"remote"}"#).unwrap();
        let ancestor = json!({"format": 1, "key": NOTE, "device": peer.device().to_string(),
            "clock": [1, 0], "history": [], "body": {"hash": hash.to_string(), "size": 17}});
        let e = Entry {
            device: peer.device(),
            seq: 1,
            hash: record::version_hash(&ancestor),
        };
        d.db.conn()
            .execute(
                "INSERT INTO sync_versions (library, key, device, seq, hash, raw, applied)
            VALUES (?1, ?2, ?3, 1, ?4, ?5, 2)",
                params![
                    d.lib.id.to_string(),
                    NOTE,
                    e.device.to_string(),
                    e.hash.to_string(),
                    ancestor.to_string()
                ],
            )
            .unwrap();
        let mut first = d.note(NOTE, "first own");
        first.raw["history"] =
            json!([{"d": e.device.to_string(), "s": e.seq, "h": e.hash.to_string()}]);
        d.db.conn().execute("UPDATE sync_versions SET raw = ?1, hash = ?2 WHERE library = ?3 AND key = ?4 AND seq IS NULL",
            params![first.raw.to_string(), record::version_hash(&first.raw).to_string(), d.lib.id.to_string(), NOTE]).unwrap();
        let one = d.change();
        let second = d.note(NOTE, "second own");
        let two = d.change();
        assert!(second.history.contains(&e));
        let built = d.build(None, &[one, two], &|_| false);
        let w = d.save(&built);
        let view = d.open(&w);
        let rs = records(&view);
        assert_eq!(rs[NOTE].week.len(), 2);
        let old = Name {
            kind: Kind::Blob,
            name: Hash32::parse(&blob_name(&NAMING, &hash)).unwrap(),
        };
        assert!(rs[NOTE].week[0].objects.contains(&old));
        assert!(!rs[NOTE].week[1].objects.contains(&old));
    }
}
