//! SY23, SP16, SP37 (5): damaged and missing blobs. A blob this pass read and found
//! damaged is set aside, listed in the device file's `refused` and not read again
//! until storage holds something else under that name; a blob a live version names
//! that storage lost or damaged is uploaded again from the validated copy this
//! device holds, at most once in 24 hours by its own vouched clock, and listed in
//! `repairs`. PS4: this module orders the reads, the checks, the upload and the
//! saves; `files` reads and writes, `journal` keeps the copies, `publish::live_named`
//! says what a live version names.
//!
//! What a pass checks for damage, which is not every blob in storage:
//!
//! 1. the blobs it read for its own use (steps (4) and (5)), when they fail the
//!    check, and only those it holds a copy of go on to a repair;
//! 2. the blobs peers list in `refused` (step (3)): each is read once, and not again
//!    for an hour, and only if a live version names it and this device holds a copy;
//! 3. the blobs a live version names, whose body this device holds, that `blobs/`
//!    does not list: from a listing the pass took for its other steps, or one taken
//!    here once an hour (PROVISIONAL), so a quiet library isn't listed every pass.
//!
//! The order, and what a stop between two of them leaves: the intent to repair is
//! saved first (`done` false), then the copy is written (SP7's temporary name and
//! rename), then `done` is saved. A crash or a budget stop between the first save and
//! the last leaves a pending repair, which the next pass finishes without waiting out
//! the 24 hours: it reads the object again, and writes only if it is still damaged or
//! missing, so the `repairs` entry is never lost and a good object is not written over.
//! Nothing here applies a version: a repair puts bytes under a name a live version
//! already names (SY11: content any live version names is never removed, so no
//! reservation or hold, SP33 and SP34, comes before it).

use super::crypto::blob_name;
use super::files::{self, Read};
use super::folder::{Folder, Listed};
use super::guard;
use super::ids::{identity_hash, Hash32};
use super::journal;
use super::pass::{save_state, Device, Error, Meter, Report};
use super::publish;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// SY23, SP16: any one object is repaired at most once in 24 hours by this device's clock.
pub const REPAIR_GAP_MS: i64 = 24 * 60 * 60 * 1000;
/// PROVISIONAL: a `repairs` entry stays in the device file for twice the repair gap,
/// so a peer that reads the file once a day still sees it.
pub const REPAIRS_KEPT_MS: i64 = 2 * REPAIR_GAP_MS;
/// PROVISIONAL: an object found damaged is read again after this long even when its
/// size is the same, because a size is all `stat` says of what storage holds (a
/// replacement of the same length, or a half-synced file, shows no other change).
pub const RETRY_MS: i64 = 60 * 60 * 1000;
/// PROVISIONAL: an object checked, on a peer's word or by a read, isn't checked on a
/// peer's word again for this long, so a peer that lists a good object in every
/// device file can't make this device read it every pass.
pub const RECHECK_MS: i64 = 60 * 60 * 1000;
/// PROVISIONAL: `blobs/` is listed here for the missing check at most this often.
pub const LIST_EVERY_MS: i64 = 60 * 60 * 1000;
/// PROVISIONAL bounds on what is kept (and so on `State` and the device file): the
/// objects found damaged, the repairs listed, the peers' reports waiting to be
/// checked, and the objects checked. At a bound the oldest of the first, second and
/// fourth go; a report past the third is not taken, and a repair past the second
/// waits for older entries to expire.
pub const MAX_REFUSED: usize = 256;
pub const MAX_REPAIRED: usize = 512;
pub const MAX_HINTS: usize = 256;
pub const MAX_CHECKED: usize = 512;

/// SY23: an object found damaged, as storage showed it then.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refused {
    /// The plain hash of the body the name stands for.
    pub hash: String,
    /// The size `stat` gave, which is how a replacement is told without a read.
    pub size: u64,
    /// This device's vouched time when it was found.
    pub at: i64,
}

/// SY23, SP16: an object this device repaired, or is about to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repaired {
    pub hash: String,
    pub at: i64,
    /// False between the save that says it will write and the one that says it did.
    pub done: bool,
}

/// PS5: what a device remembers of damaged and missing blobs between passes, by keyed
/// name (64 hex digits, as SP16's lists say). Every time is this device's own vouched
/// time (SY17); nothing a peer sent is kept but a name and a hash to check.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Objects {
    pub refused: BTreeMap<String, Refused>,
    pub repaired: BTreeMap<String, Repaired>,
    /// Peers' `refused` entries waiting to be checked: name to plain hash.
    pub hints: BTreeMap<String, String>,
    /// When an object was last read and found good, or repaired.
    pub checked: BTreeMap<String, i64>,
    /// When this device last had `blobs/` listed whole.
    pub listed: Option<i64>,
}

/// SP37 (5): what a pass did about damaged and missing blobs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Handled {
    /// Keyed names of the blobs this pass read and found damaged.
    pub refused: Vec<String>,
    /// Keyed names of the blobs this pass wrote again.
    pub repaired: Vec<String>,
    /// Repairs left for a later pass at `MAX_REPAIRED`.
    pub deferred: u64,
}

impl Objects {
    /// SY23: whether a blob found damaged is left unread: storage shows the size it
    /// showed then, and the retry hour hasn't passed.
    pub fn leave(&self, name: &str, hash: &Hash32, size: u64, now: i64) -> bool {
        self.refused.get(name).is_some_and(|r| {
            r.hash == hash.to_string() && r.size == size && now.saturating_sub(r.at) < RETRY_MS
        })
    }

    pub fn damaged(&mut self, name: &str, hash: &Hash32, size: u64, now: i64) {
        self.refused.insert(
            name.to_owned(),
            Refused {
                hash: hash.to_string(),
                size,
                at: now,
            },
        );
        while self.refused.len() > MAX_REFUSED {
            let oldest = self
                .refused
                .iter()
                .min_by_key(|(_, r)| r.at)
                .map(|(n, _)| n.clone());
            oldest.map(|n| self.refused.remove(&n));
        }
        self.checked.remove(name);
    }

    /// The blob was read and is the body its name says.
    pub fn good(&mut self, name: &str, now: i64) {
        self.refused.remove(name);
        self.checked.insert(name.to_owned(), now);
        while self.checked.len() > MAX_CHECKED {
            let oldest = self
                .checked
                .iter()
                .min_by_key(|(_, t)| **t)
                .map(|(n, _)| n.clone());
            oldest.map(|n| self.checked.remove(&n));
        }
    }

    /// SP16: peers' `refused` entries are only reasons to look, never believed: one
    /// whose name isn't the keyed name of its hash is dropped, and the rest wait
    /// for `repair`, which checks them against storage.
    pub fn hint(&mut self, naming: &[u8; 32], items: Option<&Vec<Value>>) {
        for item in items.into_iter().flatten() {
            let (Some(name), Some(hash)) = (
                item["name"].as_str(),
                item["hash"].as_str().and_then(Hash32::parse),
            ) else {
                continue;
            };
            if blob_name(naming, &hash) != name {
                continue;
            }
            if self.hints.len() >= MAX_HINTS {
                break;
            }
            self.hints
                .entry(name.to_owned())
                .or_insert(hash.to_string());
        }
    }

    /// Whether a peer's report of this object waits for the hour since it was last
    /// looked at.
    fn throttled(&self, name: &str, now: i64) -> bool {
        self.checked
            .get(name)
            .is_some_and(|t| now.saturating_sub(*t) < RECHECK_MS)
    }

    /// SP16: `repairs` keeps an entry for `REPAIRS_KEPT_MS`.
    fn prune(&mut self, now: i64) {
        self.repaired
            .retain(|_, r| !r.done || now.saturating_sub(r.at) < REPAIRS_KEPT_MS);
    }

    /// SP16: the device file's `refused`, `{ "name", "hash" }` each.
    pub fn refused_list(&self) -> Vec<Value> {
        self.refused
            .iter()
            .map(|(name, r)| json!({"name": name, "hash": r.hash}))
            .collect()
    }

    /// SP16: the device file's `repairs`: the repairs written; one begun and not yet
    /// finished isn't listed.
    pub fn repairs_list(&self) -> Vec<Value> {
        self.repaired
            .iter()
            .filter(|(_, r)| r.done)
            .map(|(name, r)| json!({"name": name, "hash": r.hash}))
            .collect()
    }
}

/// What reading one blob came to.
pub(super) enum Blob {
    Good(Vec<u8>),
    /// Bytes that aren't the body the name says, or more than the limit.
    Damaged,
    /// Not damaged and not good: gone, or not a blob to read. Set aside.
    Other,
}

/// SY23, SY28, SP9: a blob read no further than `limit`, and checked against the
/// plain hash its name was made from. The bytes read count against the budget.
pub(super) fn read_blob<F: Folder>(
    folder: &F,
    listed: &Listed<F::Token>,
    hash: &Hash32,
    limit: u64,
    report: &mut Report,
) -> Result<Blob, Error> {
    let (why, damaged) = match files::read_file_within(folder, listed, limit)? {
        Read::Bytes(bytes) => {
            report.bytes_read += bytes.len() as u64;
            if identity_hash(&bytes) == *hash {
                return Ok(Blob::Good(bytes));
            }
            ("its bytes aren't the body its name says", true)
        }
        Read::TooLarge => ("over its limit", true),
        Read::Outside => ("outside the layout", false),
        Read::Candidate { .. } => ("a candidate copy", false),
        Read::Gone => ("gone", false),
    };
    report.set_aside.push((listed.name.clone(), why.into()));
    Ok(if damaged { Blob::Damaged } else { Blob::Other })
}

type Blobs<T> = Option<BTreeMap<String, Listed<T>>>;

/// One object to look at.
struct Look {
    name: String,
    hash: Hash32,
    /// Known damaged or missing already: no read first.
    known: bool,
    /// A repair begun on an earlier pass: not held to the 24 hours, nor to a recheck.
    pending: bool,
}

/// SP37 (5): repair. `false` in `report.stopped` when a budget ran out.
pub(super) fn repair<F: Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    naming: &[u8; 32],
    now: i64,
) -> Result<(), Error> {
    let (conn, lib) = (device.conn, device.lib);
    // SY26: while the reader is asked or sync is paused nothing is read.
    if guard::asking(conn, &lib)?.is_some() || guard::paused(conn, &lib)? {
        return Ok(());
    }
    device.state.objects.prune(now);
    let o = &device.state.objects;
    let due = o
        .listed
        .map_or(true, |t| now.saturating_sub(t) >= LIST_EVERY_MS);
    let hinted = o.hints.keys().any(|n| !o.throttled(n, now));
    if blobs.is_none() && (due || hinted || o.repaired.values().any(|r| !r.done)) {
        // PS6: counts against the name budget, and shares the listing with step (9).
        if !publish::list_blobs(folder, meter, report, blobs)? {
            return Ok(());
        }
    }
    let mut looks: Vec<Look> = Vec::new();
    let add = |looks: &mut Vec<Look>, name: &str, hash: &str, known: bool, pending: bool| {
        if let Some(hash) = Hash32::parse(hash) {
            match looks.iter_mut().find(|l| l.name == name) {
                Some(l) => {
                    l.known |= known;
                    l.pending |= pending;
                }
                None => looks.push(Look {
                    name: name.to_owned(),
                    hash,
                    known,
                    pending,
                }),
            }
        }
    };
    let o = &device.state.objects;
    for (name, r) in o.repaired.iter().filter(|(_, r)| !r.done) {
        add(&mut looks, name, &r.hash, false, true);
    }
    for name in &report.objects.refused {
        if let Some(r) = o.refused.get(name) {
            add(&mut looks, name, &r.hash, true, false);
        }
    }
    for (name, hash) in &o.hints {
        add(&mut looks, name, hash, false, false);
    }
    // The missing check: bodies this device holds that a live version names. While
    // this device has changes of its own to publish it isn't made: a body of a version
    // not yet in storage is written by step (9), after SP33's reservation, and not here.
    let publishing = publish::anything_to_publish(device)?;
    if let Some(listing) = blobs.as_ref().filter(|_| !publishing) {
        let held: Vec<String> = conn
            .prepare(
                "SELECT DISTINCT json_extract(v.raw, '$.body.hash') FROM sync_versions v
                 WHERE v.library = ?1 AND v.seq IS NOT NULL AND v.applied = 1
                   AND json_extract(v.raw, '$.body.hash') IS NOT NULL
                   AND EXISTS (SELECT 1 FROM sync_bodies b
                               WHERE b.hash = json_extract(v.raw, '$.body.hash'))",
            )
            .map_err(journal::JournalError::from)?
            .query_map([lib.id.to_string()], |r| r.get(0))
            .map_err(journal::JournalError::from)?
            .collect::<Result<_, _>>()
            .map_err(journal::JournalError::from)?;
        for hash in held {
            let Some(parsed) = Hash32::parse(&hash) else {
                continue;
            };
            let name = blob_name(naming, &parsed);
            if !listing.contains_key(&format!("blobs/{name}")) {
                add(&mut looks, &name, &hash, true, false);
            }
        }
    }
    for look in looks {
        if let Some(stop) = meter.check(report) {
            report.stopped = Some(stop);
            return Ok(());
        }
        if !handle(device, folder, meter, report, blobs, &look, now)? {
            return Ok(());
        }
    }
    // Only a pass that looked at everything the listing showed counts as having
    // listed: a stop leaves the check due, so the next pass looks again.
    if blobs.is_some() {
        device.state.objects.listed = Some(now);
    }
    Ok(())
}

/// One object: whether a live version names it, whether this device holds a copy,
/// whether storage has it damaged or lost, and then the repair. `false` when a
/// budget stopped it.
fn handle<F: Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    look: &Look,
    now: i64,
) -> Result<bool, Error> {
    let Look { name, hash, .. } = look;
    let path = format!("blobs/{name}");
    // A peer's report of an object looked at within the hour waits for the hour, so
    // a peer that lists a good object in every device file costs one read an hour.
    if !look.known && !look.pending && device.state.objects.throttled(name, now) {
        return Ok(true);
    }
    // A peer's report is looked at once.
    device.state.objects.hints.remove(name);
    let o = &device.state.objects;
    // SP16: once in 24 hours by this device's clock, whatever it has seen.
    if !look.pending
        && o.repaired
            .get(name)
            .is_some_and(|r| now.saturating_sub(r.at) < REPAIR_GAP_MS)
    {
        return Ok(true);
    }
    // SY23: of which this device holds a validated copy; the copy is the body the
    // name stands for or nothing is written.
    let copy = journal::body(device.conn, hash)?.filter(|b| identity_hash(b) == *hash);
    // SP37 (5): a blob a live version names.
    let live = copy.is_some() && publish::live_named(device, hash)?;
    let Some(copy) = copy.filter(|_| live) else {
        // A repair begun earlier that can't be finished is dropped.
        device
            .state
            .objects
            .repaired
            .retain(|n, r| r.done || n != name);
        return Ok(true);
    };
    let listed = blobs.as_ref().and_then(|b| b.get(&path)).cloned();
    let damaged = match (&listed, look.known) {
        (None, _) | (_, true) => true,
        (Some(listed), false) => {
            // The copy's size bounds the read: a larger object isn't this body.
            match read_blob(folder, listed, hash, copy.len() as u64, report)? {
                Blob::Good(_) => {
                    device.state.objects.good(name, now);
                    // A pending repair whose write got through before a crash.
                    if let Some(r) = device.state.objects.repaired.get_mut(name) {
                        r.done = true;
                    }
                    false
                }
                Blob::Damaged => true,
                Blob::Other => false,
            }
        }
    };
    if !damaged {
        return Ok(true);
    }
    // SY23: the intent is saved before the write; a stop after it leaves it pending.
    let objects = &mut device.state.objects;
    if !look.pending {
        if objects.repaired.len() >= MAX_REPAIRED && !objects.repaired.contains_key(name) {
            report.objects.deferred += 1;
            return Ok(true);
        }
        objects.repaired.insert(
            name.clone(),
            Repaired {
                hash: hash.to_string(),
                at: now,
                done: false,
            },
        );
    }
    save_state(device)?;
    if !publish::put(folder, &path, &copy, meter, report)? {
        return Ok(false);
    }
    let objects = &mut device.state.objects;
    if let Some(r) = objects.repaired.get_mut(name) {
        r.done = true;
    }
    objects.refused.remove(name);
    objects.checked.insert(name.clone(), now);
    report.objects.repaired.push(name.clone());
    save_state(device)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AnnotationRow;
    use crate::sync::apply::tests::{highlight, ANNO};
    use crate::sync::chain;
    use crate::sync::device_file::{self, DeviceFile};
    use crate::sync::folder::{self, Page};
    use crate::sync::pass::{run, Budgets, Readings, State, Stopped};
    use crate::sync::testkit::{self, Kit, KitDevice, NAMING};
    use std::cell::Cell;
    use std::io;

    const HOUR: u64 = RETRY_MS as u64;
    const SECOND: &str = "2b3c4d5e-6f7a-4b1c-9d2e-4f5a6b7c8d9e";
    const THIRD: &str = "3c4d5e6f-7a8b-4c1d-8e2f-5a6b7c8d9e0f";

    /// This device's clock at `ms` after its first pass (its vouched time, SY17).
    fn at(ms: u64) -> Readings {
        Readings {
            mono_ms: 1_000,
            wall_ms: 1_900_000_000_000 + ms,
            clock_ahead: false,
            same_boot: false,
        }
    }

    /// A device's folder that records its writes and can fail the first write to
    /// `blobs/`, before it is made or after (the process died before it was noted),
    /// and moves a clock past the time budget when one is made.
    struct Probe<'a, F: Folder> {
        inner: F,
        writes: Vec<String>,
        crash: Option<bool>,
        clock: Option<&'a Cell<u64>>,
    }

    impl<F: Folder> Folder for Probe<'_, F> {
        type Token = F::Token;
        fn list(&self, dir: &str, cursor: Option<&str>) -> io::Result<Page<F::Token>> {
            self.inner.list(dir, cursor)
        }
        fn stat(&self, token: &F::Token) -> io::Result<Option<u64>> {
            self.inner.stat(token)
        }
        fn read(&self, token: &F::Token, limit: Option<u64>) -> io::Result<folder::Read> {
            self.inner.read(token, limit)
        }
        fn write(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
            let blob = path.starts_with("blobs/");
            if let Some(after) = self.crash.filter(|_| blob) {
                self.crash = None;
                if after {
                    self.inner.write(path, bytes)?;
                    self.writes.push(path.to_owned());
                }
                return Err(io::Error::other("injected crash"));
            }
            self.inner.write(path, bytes)?;
            self.writes.push(path.to_owned());
            if let Some(clock) = self.clock.filter(|_| blob) {
                clock.set(120_000);
            }
            Ok(())
        }
        fn remove(&mut self, token: &F::Token) -> io::Result<()> {
            self.inner.remove(token)
        }
    }

    fn probe(kit: &Kit, i: usize) -> Probe<'static, folder::View> {
        Probe {
            inner: kit.folder.view(&kit.devices[i].lib.device.to_string()),
            writes: Vec::new(),
            crash: None,
            clock: None,
        }
    }

    fn pass_on<F: Folder>(
        kit: &mut Kit,
        i: usize,
        ms: u64,
        budgets: &Budgets,
        folder: &mut F,
        now: &dyn Fn() -> u64,
    ) -> Result<Report, Error> {
        let kd = &mut kit.devices[i];
        let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
        let mut device = Device {
            conn: kd.db.conn(),
            lib: kd.lib,
            store: &mut kd.store,
            ks: &kd.ks,
            state,
            name: "Test Mac",
        };
        run(&mut device, folder, at(ms), budgets, now)
    }

    fn pass(kit: &mut Kit, i: usize, ms: u64) -> Report {
        let mut p = probe(kit, i);
        pass_on(kit, i, ms, &Budgets::sy2(), &mut p, &|| 0).unwrap()
    }

    fn get(kd: &KitDevice, path: &str) -> Option<Vec<u8>> {
        let dir = path.rsplit_once('/').unwrap().0;
        let entry = kd
            .folder
            .list(dir, None)
            .unwrap()
            .entries
            .into_iter()
            .find(|e| e.name == path)?;
        match files::read_file(&kd.folder, &entry).unwrap() {
            Read::Bytes(b) => Some(b),
            _ => None,
        }
    }

    fn put(kit: &mut Kit, path: &str, bytes: &[u8]) {
        Folder::write(&mut kit.devices[1].folder, path, bytes).unwrap();
    }

    fn reads(kit: &Kit, i: usize, path: &str) -> usize {
        kit.folder
            .read_count(&kit.devices[i].lib.device.to_string(), path)
    }

    fn shown(kit: &Kit, i: usize) -> Vec<String> {
        let mut ids: Vec<String> = kit.devices[i]
            .db
            .annotations("book")
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        ids.sort();
        ids
    }

    /// Device 1 publishes a highlight with no note: its body's path, bytes and hash.
    fn publish(kit: &mut Kit, id: &str, n: u8) -> (String, Vec<u8>, Hash32) {
        let row = AnnotationRow {
            id: id.into(),
            cfi_range: format!("epubcfi(/6/4!/4/{},/1:0,/1:4)", 2 * n),
            ..highlight("book", None, 1_760_000_000_500)
        };
        kit.devices[1].db.save_annotation(&row).unwrap();
        let seq = kit.devices[1].publish_pending().unwrap();
        let change = kit.devices[1].own_changes(seq).pop().unwrap();
        let hash = change
            .versions
            .iter()
            .find_map(|v| v["body"]["hash"].as_str().and_then(Hash32::parse))
            .unwrap();
        let path = format!("blobs/{}", blob_name(&NAMING, &hash));
        let body = get(&kit.devices[1], &path).unwrap();
        (path, body, hash)
    }

    fn name_of(hash: &Hash32) -> String {
        blob_name(&NAMING, hash)
    }

    fn entry(hash: &Hash32) -> Value {
        json!({"name": name_of(hash), "hash": hash.to_string()})
    }

    fn list_of(kit: &Kit, i: usize, list: &str) -> Vec<Value> {
        let kd = &kit.devices[i];
        let bytes = get(kd, &device_file::device_path(&kd.lib.device)).unwrap();
        let file: DeviceFile =
            device_file::open(&bytes, &kd.lib.id, &kd.lib.device, &kd.key.public()).unwrap();
        file.fields.lists[list].clone()
    }

    fn state_of(kit: &Kit, i: usize) -> State {
        State::load(kit.devices[i].db.conn(), &kit.devices[i].lib.id).unwrap()
    }

    /// Device `i` has found this object damaged, as an earlier pass would have left it.
    fn seed_refused(kit: &mut Kit, i: usize, hash: &Hash32, size: u64) {
        let mut state = state_of(kit, i);
        let refused = Refused {
            hash: hash.to_string(),
            size,
            at: 0,
        };
        state.objects.refused.insert(name_of(hash), refused);
        let kd = &kit.devices[i];
        let tx = kd.db.conn().unchecked_transaction().unwrap();
        state.save(&tx, &kd.lib.id).unwrap();
        tx.commit().unwrap();
    }

    /// Three highlights by device 1, read and applied by device 0; the second and
    /// third bodies then lost from storage. Returns their paths and bytes.
    fn lost_two() -> (Kit, Vec<(String, Vec<u8>)>) {
        let mut kit = testkit::library(2);
        publish(&mut kit, ANNO, 1);
        let lost = vec![publish(&mut kit, SECOND, 2), publish(&mut kit, THIRD, 3)];
        pass(&mut kit, 0, 0);
        assert_eq!(shown(&kit, 0).len(), 3);
        for (path, ..) in &lost {
            kit.folder.vanish(path);
        }
        (kit, lost.into_iter().map(|(p, b, _)| (p, b)).collect())
    }

    #[test]
    fn a_damaged_blob_is_refused_listed_and_not_read_again_until_storage_holds_another() {
        // SY23, SY28, SP16: a body whose bytes aren't what its name says, and one over
        // its type's limit, are set aside and listed in `refused`; neither is read again
        // until `stat` shows another size (or an hour has passed), and a healed one is
        // taken and leaves the list.
        let mut kit = testkit::library(2);
        let (path, body, hash) = publish(&mut kit, ANNO, 1);
        let (huge, _, hash2) = publish(&mut kit, SECOND, 2);
        put(&mut kit, &path, b"not the body");
        put(&mut kit, &huge, &vec![0u8; (4 << 20) + 1]);
        let report = pass(&mut kit, 0, 0);
        assert_eq!(report.objects.refused.len(), 2, "{report:?}");
        assert_eq!((reads(&kit, 0, &path), reads(&kit, 0, &huge)), (1, 0));
        let listed = list_of(&kit, 0, "refused");
        assert_eq!(listed.len(), 2);
        assert!(listed.contains(&entry(&hash)) && listed.contains(&entry(&hash2)));
        // Not read again: the same size, a quarter of an hour on.
        let report = pass(&mut kit, 0, 0);
        assert!(report.objects.refused.is_empty(), "nothing new: {report:?}");
        pass(&mut kit, 0, HOUR / 4);
        assert_eq!(reads(&kit, 0, &path), 1);
        // Something else under the name: another size is read at once.
        put(&mut kit, &path, b"damaged again, longer");
        let report = pass(&mut kit, 0, HOUR / 4);
        assert_eq!(report.objects.refused, vec![name_of(&hash)]);
        assert_eq!(reads(&kit, 0, &path), 2);
        // The same size again is left for the hour: its limit, which `stat` can't tell.
        put(&mut kit, &path, b"DAMAGED AGAIN, LONGER");
        pass(&mut kit, 0, HOUR / 3);
        assert_eq!(reads(&kit, 0, &path), 2);
        pass(&mut kit, 0, HOUR / 4 + HOUR);
        assert_eq!(reads(&kit, 0, &path), 3);
        // Storage heals: the body is taken, applied, and leaves the list.
        put(&mut kit, &path, &body);
        let report = pass(&mut kit, 0, HOUR / 4 + HOUR + 1_000);
        assert_eq!(reads(&kit, 0, &path), 4);
        assert_eq!(
            (report.late.fetched, report.late.applied),
            (1, 1),
            "{report:?}"
        );
        assert_eq!(shown(&kit, 0), vec![ANNO.to_string()]);
        assert_eq!(list_of(&kit, 0, "refused"), vec![entry(&hash2)]);
        assert_eq!(reads(&kit, 0, &huge), 0, "an oversize object is never read");
    }

    #[test]
    fn a_blob_storage_lost_is_written_again_from_this_devices_copy_once_a_day_and_a_peer_fetches_it(
    ) {
        // SY23, SP16, SP37 (5): a blob a live version names that `blobs/` lacks is
        // uploaded again from the copy this device holds and listed in `repairs`; the
        // same object isn't written again within 24 hours of vouched time, and is after.
        let mut kit = testkit::library(3);
        let (path, body, hash) = publish(&mut kit, ANNO, 1);
        pass(&mut kit, 0, 0);
        assert_eq!(shown(&kit, 0), vec![ANNO.to_string()]);
        kit.folder.vanish(&path);
        // `blobs/` isn't listed on a quiet pass (an hour apart at most): not noticed yet.
        let report = pass(&mut kit, 0, 60_000);
        assert!(report.objects.repaired.is_empty() && get(&kit.devices[0], &path).is_none());
        let report = pass(&mut kit, 0, HOUR);
        assert_eq!(report.objects.repaired, vec![name_of(&hash)], "{report:?}");
        assert_eq!(get(&kit.devices[0], &path), Some(body));
        assert_eq!(list_of(&kit, 0, "repairs"), vec![entry(&hash)]);
        // A device that never had the body fetches it and shows the highlight.
        pass(&mut kit, 2, 0);
        assert_eq!(shown(&kit, 2), vec![ANNO.to_string()]);
        // Lost again: not written again until 24 hours after the repair.
        kit.folder.vanish(&path);
        for ms in [2 * HOUR, 25 * HOUR - 1_000] {
            let report = pass(&mut kit, 0, ms);
            assert!(report.objects.repaired.is_empty(), "{ms}: {report:?}");
            assert!(get(&kit.devices[0], &path).is_none());
        }
        let report = pass(&mut kit, 0, 26 * HOUR);
        assert_eq!(report.objects.repaired.len(), 1, "{report:?}");
        assert!(get(&kit.devices[0], &path).is_some());
    }

    #[test]
    fn each_budget_stops_a_repair_and_the_next_pass_takes_it_up() {
        // PS6, SY23: names (the listing), bytes and time each stop a pass in its
        // repair, with what was written kept; the next pass writes the rest, once each.
        // What a pass has read when it reaches the repair, from the names case.
        let mut upto = 0;
        for case in ["names", "bytes", "time"] {
            let (mut kit, lost) = lost_two();
            pass(&mut kit, 0, 60_000);
            let steady = pass(&mut kit, 0, 61_000);
            let clock = Cell::new(0u64);
            let mut budgets = Budgets::sy2();
            let smallest = lost.iter().map(|(_, b)| b.len() as u64).min().unwrap();
            match case {
                "names" => budgets.names = steady.names_listed,
                "bytes" => budgets.bytes = upto + smallest - 1,
                _ => {}
            }
            let mut first = probe(&kit, 0);
            first.clock = (case == "time").then_some(&clock);
            let report = pass_on(&mut kit, 0, HOUR, &budgets, &mut first, &|| clock.get()).unwrap();
            let (stop, done) = match case {
                "names" => (Stopped::Names, 0),
                "bytes" => (Stopped::Bytes, 1),
                _ => (Stopped::Time, 1),
            };
            assert_eq!(report.stopped, Some(stop), "{case}: {report:?}");
            assert_eq!(report.objects.repaired.len(), done, "{case}");
            upto = upto.max(report.bytes_read);
            let mut second = probe(&kit, 0);
            let report = pass_on(
                &mut kit,
                0,
                HOUR + 1_000,
                &Budgets::sy2(),
                &mut second,
                &|| 0,
            )
            .unwrap();
            assert_eq!(report.stopped, None, "{case}: {report:?}");
            assert_eq!(report.objects.repaired.len(), 2 - done, "{case}");
            let mut once: Vec<_> = first.writes.iter().chain(&second.writes).collect();
            once.retain(|p| p.starts_with("blobs/"));
            assert_eq!(once.len(), 2, "{case}: each blob written once: {once:?}");
            for (path, body) in &lost {
                assert_eq!(get(&kit.devices[0], path).as_ref(), Some(body), "{case}");
            }
            assert_eq!(list_of(&kit, 0, "repairs").len(), 2, "{case}");
        }
    }

    #[test]
    fn a_crash_between_the_write_and_the_save_neither_repeats_the_upload_nor_loses_the_entry() {
        // PS5, SP15: the intent is saved before the write. A crash before the write
        // leaves it pending, and the next pass writes; a crash after the write leaves it
        // pending too, and the next pass finds the object good and writes nothing more.
        for after in [false, true] {
            let mut kit = testkit::library(2);
            let (path, body, hash) = publish(&mut kit, ANNO, 1);
            pass(&mut kit, 0, 0);
            kit.folder.vanish(&path);
            let mut crashing = probe(&kit, 0);
            crashing.crash = Some(after);
            let result = pass_on(&mut kit, 0, HOUR, &Budgets::sy2(), &mut crashing, &|| 0);
            assert!(result.is_err(), "{after}");
            let saved = state_of(&kit, 0).objects.repaired;
            assert_eq!(
                saved.len(),
                1,
                "{after}: the intent was saved before the write"
            );
            assert!(!saved.values().next().unwrap().done);
            let mut next = probe(&kit, 0);
            // Not due, not held to the 24 hours: a pending repair is finished.
            let report =
                pass_on(&mut kit, 0, HOUR + 1_000, &Budgets::sy2(), &mut next, &|| 0).unwrap();
            let written = crashing.writes.iter().chain(&next.writes);
            assert_eq!(
                written.filter(|p| p.starts_with("blobs/")).count(),
                1,
                "{after}"
            );
            assert_eq!(
                report.objects.repaired.len(),
                usize::from(!after),
                "{after}"
            );
            assert_eq!(get(&kit.devices[0], &path), Some(body));
            assert!(state_of(&kit, 0).objects.repaired.values().all(|r| r.done));
            assert_eq!(list_of(&kit, 0, "repairs"), vec![entry(&hash)], "{after}");
            // And nothing again afterwards.
            let mut last = probe(&kit, 0);
            pass_on(&mut kit, 0, 2 * HOUR, &Budgets::sy2(), &mut last, &|| 0).unwrap();
            assert!(
                !last.writes.iter().any(|p| p.starts_with("blobs/")),
                "{after}"
            );
        }
    }

    #[test]
    fn a_blob_a_peer_found_damaged_is_repaired_by_a_device_with_a_copy_and_fetched_again() {
        // SY23, SP16: device 2 reads a damaged body and lists it in `refused`; device 0,
        // which holds the body, reads that list in step (3), finds the object damaged and
        // writes it again; device 2 sees another size and fetches it.
        let mut kit = testkit::library(3);
        let (path, body, hash) = publish(&mut kit, ANNO, 1);
        pass(&mut kit, 0, 0);
        put(&mut kit, &path, b"not the body");
        let report = pass(&mut kit, 2, 0);
        assert_eq!(report.objects.refused, vec![name_of(&hash)], "{report:?}");
        assert_eq!(list_of(&kit, 2, "refused"), vec![entry(&hash)]);
        assert!(shown(&kit, 2).is_empty());
        // Device 0 had found it damaged itself on an earlier pass that stopped before
        // repairing (a budget): the entry is on its list until it is repaired.
        seed_refused(&mut kit, 0, &hash, 12);
        // Device 0 read that body good a minute ago: the report waits for the hour.
        let report = pass(&mut kit, 0, 60_000);
        assert!(report.objects.repaired.is_empty(), "{report:?}");
        let report = pass(&mut kit, 0, HOUR);
        assert_eq!(report.objects.repaired, vec![name_of(&hash)], "{report:?}");
        assert_eq!(get(&kit.devices[0], &path), Some(body));
        assert!(
            list_of(&kit, 0, "refused").is_empty(),
            "repaired, so not refused"
        );
        let report = pass(&mut kit, 2, HOUR + 60_000);
        assert_eq!(report.late.applied + report.received.len(), 1, "{report:?}");
        assert_eq!(shown(&kit, 2), vec![ANNO.to_string()]);
        assert_eq!(reads(&kit, 2, &path), 2);
        assert!(list_of(&kit, 2, "refused").is_empty());
    }

    #[test]
    fn a_peers_report_cant_make_this_device_read_a_good_blob_every_pass_or_write_it() {
        // SP16: a peer's `refused` is only a reason to look. A good object is read once an
        // hour, never written over; a name that isn't the keyed name of its hash and an
        // object no live version names are never looked at.
        let mut kit = testkit::library(3);
        let (path, body, hash) = publish(&mut kit, ANNO, 1);
        pass(&mut kit, 0, 0);
        let stranger = identity_hash(b"no version names this");
        let lists = || {
            BTreeMap::from([(
                "refused".to_owned(),
                vec![
                    entry(&hash),
                    json!({"name": "ab".repeat(32), "hash": hash.to_string()}),
                    entry(&stranger),
                ],
            )])
        };
        let before = reads(&kit, 0, &path);
        for (n, ms) in [HOUR / 6, HOUR / 3, HOUR + HOUR / 6]
            .into_iter()
            .enumerate()
        {
            kit.devices[2].publish_device_file_with(lists());
            let report = pass(&mut kit, 0, ms);
            assert_eq!(report.accepted.len(), 1, "{n}: {report:?}");
            assert!(report.objects.repaired.is_empty() && report.objects.refused.is_empty());
            // Looked at the first time and again only after the hour.
            assert_eq!(reads(&kit, 0, &path), before + usize::from(n == 2), "{n}");
            // (Read through device 1's view: a read through device 0's would be counted.)
            assert_eq!(get(&kit.devices[1], &path), Some(body.clone()));
        }
        assert!(state_of(&kit, 0).objects.hints.is_empty());
    }

    #[test]
    fn a_peers_reports_stop_at_the_byte_budget_and_are_taken_up_later() {
        // PS6: each looked-at object is a read; a pass that has used its bytes stops
        // before the next, and the reports it didn't reach are looked at by the next pass.
        let mut kit = testkit::library(3);
        let (p1, _, h1) = publish(&mut kit, ANNO, 1);
        let (p2, _, h2) = publish(&mut kit, SECOND, 2);
        pass(&mut kit, 0, 0);
        let lists = BTreeMap::from([("refused".to_owned(), vec![entry(&h1), entry(&h2)])]);
        kit.devices[2].publish_device_file_with(lists);
        // The pass's reads up to the first report, from a pass that stops at the listing.
        let mut names = Budgets::sy2();
        names.names = pass(&mut kit, 0, 61_000).names_listed;
        let mut probe_pass = probe(&kit, 0);
        let upto = pass_on(&mut kit, 0, 2 * HOUR, &names, &mut probe_pass, &|| 0)
            .unwrap()
            .bytes_read;
        let mut budgets = Budgets::sy2();
        budgets.bytes = upto + 1;
        let mut again = probe(&kit, 0);
        let report = pass_on(&mut kit, 0, 3 * HOUR, &budgets, &mut again, &|| 0).unwrap();
        assert_eq!(report.stopped, Some(Stopped::Bytes), "{report:?}");
        let read = |kit: &Kit| (reads(kit, 0, &p1), reads(kit, 0, &p2));
        let (a, b) = read(&kit);
        assert_eq!(
            a + b,
            2 + 1,
            "both read when applied, and one report looked at"
        );
        pass(&mut kit, 0, 4 * HOUR);
        let (a, b) = read(&kit);
        assert_eq!(a + b, 2 + 2, "the other the next pass");
    }

    #[test]
    fn nothing_is_written_that_isnt_the_body_or_that_no_live_version_names() {
        // SY23, SP37 (5): a copy that doesn't hash to its name is never uploaded, and
        // neither is the body of a record that was deleted.
        let mut kit = testkit::library(2);
        let (path, _, hash) = publish(&mut kit, ANNO, 1);
        pass(&mut kit, 0, 0);
        kit.devices[0]
            .db
            .conn()
            .execute(
                "UPDATE sync_bodies SET body = x'6e6f7420697420' WHERE hash = ?1",
                [hash.to_string()],
            )
            .unwrap();
        kit.folder.vanish(&path);
        let report = pass(&mut kit, 0, HOUR);
        assert!(report.objects.repaired.is_empty() && get(&kit.devices[0], &path).is_none());
        // A deleted highlight: the copy is held, no live version names it, and a peer
        // reports the object damaged.
        let mut kit = testkit::library(3);
        let (path, _, hash) = publish(&mut kit, ANNO, 1);
        pass(&mut kit, 0, 0);
        kit.devices[1].db.delete_annotation(ANNO).unwrap();
        kit.devices[1].publish_pending().unwrap();
        pass(&mut kit, 0, 60_000);
        assert!(shown(&kit, 0).is_empty());
        assert!(
            journal::has_body(kit.devices[0].db.conn(), &hash).unwrap(),
            "held"
        );
        put(&mut kit, &path, b"not the body");
        let lists = BTreeMap::from([("refused".to_owned(), vec![entry(&hash)])]);
        kit.devices[2].publish_device_file_with(lists);
        let report = pass(&mut kit, 0, HOUR);
        assert_eq!(report.accepted.len(), 1, "{report:?}");
        assert!(report.objects.repaired.is_empty(), "{report:?}");
        assert_eq!(get(&kit.devices[1], &path), Some(b"not the body".to_vec()));
    }

    #[test]
    fn a_body_of_a_change_this_device_hasnt_published_is_left_to_step_nine() {
        // SP33: a numbered version not yet in storage has its body written by step (9),
        // after its reservation, and not by the repair, which would be an upload no
        // reservation preceded.
        let mut kit = testkit::library(2);
        pass(&mut kit, 0, 0);
        let row = highlight("book", None, 1_760_000_000_500);
        kit.devices[0].db.save_annotation(&row).unwrap();
        let kd = &mut kit.devices[0];
        chain::publish(kd.db.conn(), &kd.lib, &kd.ks, &mut kd.store)
            .unwrap()
            .unwrap();
        let mut log = probe(&kit, 0);
        pass_on(&mut kit, 0, HOUR, &Budgets::sy2(), &mut log, &|| 0).unwrap();
        let blobs = log
            .writes
            .iter()
            .filter(|p| p.starts_with("blobs/"))
            .count();
        assert_eq!(blobs, 1, "{:?}", log.writes);
    }

    #[test]
    fn while_sync_is_paused_nothing_is_repaired() {
        // SY26: a paused library reads and writes nothing here; the repair is made
        // once the reader resumes.
        let (mut kit, lost) = lost_two();
        let set = |kit: &Kit, state: &str| {
            kit.devices[0]
                .db
                .conn()
                .execute("UPDATE sync_libraries SET state = ?1", [state])
                .unwrap();
        };
        let before: String = kit.devices[0]
            .db
            .conn()
            .query_row("SELECT state FROM sync_libraries", [], |r| r.get(0))
            .unwrap();
        set(&kit, "paused");
        let report = pass(&mut kit, 0, HOUR);
        assert!(report.objects.repaired.is_empty(), "{report:?}");
        for (path, _) in &lost {
            assert!(get(&kit.devices[0], path).is_none());
        }
        set(&kit, &before);
        let report = pass(&mut kit, 0, HOUR + 1_000);
        assert_eq!(report.objects.repaired.len(), 2, "{report:?}");
        for (path, body) in &lost {
            assert_eq!(get(&kit.devices[0], path).as_ref(), Some(body));
        }
    }

    #[test]
    fn what_is_kept_has_a_bound() {
        // PS5: a peer can't grow `State` or the device file without bound.
        let mut o = Objects::default();
        let items: Vec<Value> = (0..2 * MAX_HINTS as u32)
            .map(|n| {
                let mut bytes = [0u8; 32];
                bytes[..4].copy_from_slice(&n.to_be_bytes());
                let hash = Hash32(bytes);
                json!({"name": blob_name(&NAMING, &hash), "hash": hash.to_string()})
            })
            .collect();
        o.hint(&NAMING, Some(&items));
        assert_eq!(o.hints.len(), MAX_HINTS);
        for n in 0..(MAX_REFUSED as u64 + 5) {
            o.damaged(&format!("{n:064}"), &Hash32([1; 32]), 1, n as i64);
        }
        assert_eq!(o.refused.len(), MAX_REFUSED);
        assert!(
            !o.refused.contains_key(&format!("{:064}", 0)),
            "the oldest went"
        );
        for n in 0..(MAX_CHECKED as u64 + 5) {
            o.good(&format!("{n:064}"), n as i64);
        }
        assert_eq!(o.checked.len(), MAX_CHECKED);
    }
}
