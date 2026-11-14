//! SP37 (4), SP18, SP17: reading a peer from its view. `pass::receive_changes` asks
//! here first, for a peer that is ahead of this device; PS4: this module orders the
//! reads and the saves, and each decision stays in the module that has it
//! (`view::open` checks the view, `read::receive_view` checks it against the floor
//! and applies it, `weeks` takes its claims, `device_file` raises the floor).
//!
//! The view is the one the peer's accepted device file names (SP18). It is needed
//! when what this device has applied of the peer is older than the view's `through`.
//! The order of reads, and what stopping between any two leaves:
//!
//! 1. The directory `state/‹peer›/‹index hash›/` is listed, then the index read and
//!    checked (it is at most 1 MB). What it says of `through` is noted in `State`, so a
//!    pass for a peer that is ahead but past the view doesn't read the index again.
//! 2. The parts the index names are all in the listing, or the view waits (SY28: a file
//!    storage hasn't synced yet). Their declared sizes are added up and held to the
//!    pass's byte budget before any is read (SP15), and each read is held to the size
//!    the index declares and to the budget.
//! 3. Bodies of the records newer than what is held are read, up to the run cap.
//! 4. The claims of the view's `week` entries and a mark that the view is landing are
//!    saved (SP17, PS5), the signature of its last change is kept as a candidate
//!    (SP16), and only then is the view applied. Then the floor's generation is raised,
//!    the claims proven, and the mark cleared in one save. A stop after the mark is
//!    saved is finished by `finish_landings` at the start of a later pass, ahead or
//!    not.
//!
//! A view that can't be used (a failed check, a part not as named, a floor it
//! contradicts) is set aside, and the peer's change files are read as if it had none.
//! A view that failed three times, or one the floor refuses, isn't read again until
//! the peer's device file names another (PROVISIONAL, `MAX_TRIES`), so a broken view
//! costs a pass its reads once and not every pass.

use super::apply::{self, Received};
use super::chain::ChainError;
use super::device_file;
use super::files::{self, Read};
use super::folder::{Folder, Listed};
use super::ids::{b64url_decode, DeviceId, Hash32};
use super::pass::{self, list_all, read_bytes, save_state, Device, Error, Meter, Report, Stopped};
use super::read;
use super::view::{self, View, ViewError};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Display;

/// PROVISIONAL: how many passes a named view may fail (a part not as named, a file
/// that doesn't check) before it is set aside until the peer names another.
pub const MAX_TRIES: u32 = 3;

type Blobs<T> = Option<BTreeMap<String, Listed<T>>>;

/// PS5, SP18: what this device knows of the view a peer's device file names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Noted {
    /// The index's hash, which names its directory.
    pub hash: Hash32,
    /// The last change it includes, once its index has been read and checked.
    pub through: Option<u64>,
    /// Passes in which it failed.
    pub tries: u32,
    /// Not read again until the peer names another view.
    pub refused: bool,
}

/// PS5, SP18: a view of a peer that this device is landing: saved before the view is
/// applied and cleared only once the peer's floor has its generation and the view's
/// claims are proven, so a stop anywhere in between is finished by a later pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Landing {
    /// The view's index hash, which names its directory.
    pub hash: Hash32,
    pub gen: u64,
    pub through: u64,
    /// Passes in which the view couldn't be read again to prove its claims.
    pub tries: u32,
}

/// One note for each trusted peer at most, and the views being landed.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Notes {
    #[serde(with = "super::pass::map")]
    pub peers: BTreeMap<DeviceId, Noted>,
    #[serde(default, with = "super::pass::map")]
    pub landing: BTreeMap<DeviceId, Landing>,
}

/// A view that was opened and checked, with the bodies of its newer records.
pub(super) struct Opened {
    pub view: View,
    bodies: Vec<Vec<u8>>,
}

pub(super) enum Got {
    Ready(Box<Opened>),
    /// No view is needed or usable now: the change files are read from what is held.
    Skip,
    /// A budget ran out; `report.stopped` says which.
    Halt,
}

fn fail(note: &mut Noted, report: &mut Report, path: String, why: impl Display) -> Got {
    note.tries += 1;
    note.refused |= note.tries >= MAX_TRIES;
    report.set_aside.push((path, why.to_string()));
    Got::Skip
}

/// SP18: the view `peer`'s device file names, when it is needed (`have` is what this
/// device has applied of the peer), opened and checked, with the bodies of the
/// records it brings. The note of it in `State` is saved on every way out.
#[allow(clippy::too_many_arguments)]
pub(super) fn open<F: Folder>(
    device: &mut Device<'_>,
    folder: &F,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    naming: &[u8; 32],
    peer: &DeviceId,
    key: &[u8; 32],
    named: &Hash32,
    have: u64,
) -> Result<Got, Error> {
    let mut note = match device.state.views.peers.get(peer) {
        Some(n) if n.hash == *named => n.clone(),
        _ => Noted {
            hash: *named,
            through: None,
            tries: 0,
            refused: false,
        },
    };
    let got = fetch(
        device, folder, meter, report, blobs, naming, peer, key, &mut note, have, false,
    );
    device.state.views.peers.insert(*peer, note);
    save_state(device)?;
    got
}

#[allow(clippy::too_many_arguments)]
fn fetch<F: Folder>(
    device: &Device<'_>,
    folder: &F,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    naming: &[u8; 32],
    peer: &DeviceId,
    key: &[u8; 32],
    note: &mut Noted,
    have: u64,
    finish: bool,
) -> Result<Got, Error> {
    let dir = format!("state/{peer}/{}", note.hash);
    if note.refused && !finish {
        report
            .set_aside
            .push((dir, "refused on an earlier pass".into()));
        return Ok(Got::Skip);
    }
    // SP18: a view this device is past isn't read, and its index isn't read again.
    if !finish && note.through.is_some_and(|through| through <= have) {
        return Ok(Got::Skip);
    }
    let Some(entries) = list_all(folder, &dir, meter, report)? else {
        return Ok(Got::Halt);
    };
    let index_path = format!("{dir}/index.json");
    let Some(index_entry) = entries.iter().find(|e| e.name == index_path) else {
        report
            .set_aside
            .push((index_path, "not in storage yet".into()));
        return Ok(Got::Skip);
    };
    // SP7, SY28: the index is read within its slot's limit (1 MB).
    let Some(bytes) = read_bytes(folder, index_entry, report)? else {
        note.tries += 1;
        note.refused |= note.tries >= MAX_TRIES;
        return Ok(Got::Skip);
    };
    if let Some(stop) = meter.check(report) {
        report.stopped = Some(stop);
        return Ok(Got::Halt);
    }
    let (index, hash, _) = match view::open_index(&bytes, &device.lib.id, peer, key) {
        Ok(opened) => opened,
        Err(e) => return Ok(fail(note, report, index_path, e)),
    };
    if hash != note.hash {
        return Ok(fail(
            note,
            report,
            index_path,
            "not the index the device file names",
        ));
    }
    note.through = Some(index.through.s);
    if !finish && index.through.s <= have {
        return Ok(Got::Skip);
    }
    // SP15, SY28: the declared sizes are added up and held to the budget before any
    // part is read.
    let refs: Vec<(Hash32, u64)> = index
        .shards
        .iter()
        .map(|s| (s.hash, s.size))
        .chain(index.spine.iter().map(|s| (s.hash, s.size)))
        .chain([(index.open.hash, index.open.size)])
        .collect();
    let declared = refs
        .iter()
        .try_fold(0u64, |total, (_, size)| total.checked_add(*size))
        .filter(|d| *d <= view::MAX_VIEW_BYTES);
    let Some(declared) = declared else {
        return Ok(fail(
            note,
            report,
            index_path,
            "its parts are larger than SP15 allows",
        ));
    };
    let used = report.bytes_read + report.bytes_written;
    if declared > meter.budgets().bytes {
        // No pass can read it. Which prefix of a chain or view a device may keep
        // across passes is the owner's (docs/pending-approvals.md, item 126).
        note.refused = true;
        report
            .set_aside
            .push((dir, "larger than one pass's byte budget".into()));
        return Ok(Got::Skip);
    }
    if used + declared > meter.budgets().bytes {
        report.stopped = Some(Stopped::Bytes);
        return Ok(Got::Halt);
    }
    // SY28: a part that storage hasn't synced yet is waited for, not read.
    let listed: HashMap<&str, &Listed<F::Token>> =
        entries.iter().map(|e| (e.name.as_str(), e)).collect();
    let sizes: HashMap<String, u64> = refs
        .iter()
        .map(|(hash, size)| (format!("{dir}/{hash}.json"), *size))
        .collect();
    if let Some(path) = sizes
        .keys()
        .filter(|p| !listed.contains_key(p.as_str()))
        .min()
    {
        report
            .set_aside
            .push((path.clone(), "not in storage yet".into()));
        return Ok(Got::Skip);
    }
    let parts = Cell::new(0u64);
    let halted = Cell::new(None);
    let over = RefCell::new(None::<String>);
    let read_part = |path: &str| -> Option<Vec<u8>> {
        // PS6: each part's read is budgeted before it is made.
        if let Some(stop) = meter.reading(used + parts.get()) {
            halted.set(Some(stop));
            return None;
        }
        let entry = listed.get(path)?;
        let size = *sizes.get(path)?;
        match files::read_file_within(folder, entry, size) {
            Ok(Read::Bytes(bytes)) => {
                parts.set(parts.get() + bytes.len() as u64);
                Some(bytes)
            }
            Ok(Read::TooLarge) => {
                *over.borrow_mut() = Some("over the size its index declares".into());
                None
            }
            _ => None,
        }
    };
    let opened = view::open(&bytes, &device.lib.id, peer, key, &read_part);
    report.bytes_read += parts.get();
    if let Some(stop) = halted.get() {
        report.stopped = Some(stop);
        return Ok(Got::Halt);
    }
    let view = match opened {
        Ok(view) => view,
        Err(ViewError::Missing(path)) => {
            return Ok(match over.take() {
                Some(why) => fail(note, report, path, why),
                None => {
                    report.set_aside.push((path, "can't be read now".into()));
                    Got::Skip
                }
            });
        }
        Err(e) => return Ok(fail(note, report, dir, e)),
    };
    // SY28: the bodies of the records this device doesn't hold, within the run cap;
    // the rest wait for step 5.
    // SP17: finishing a view that was applied needs its claims, not its bodies.
    if finish {
        return Ok(Got::Ready(Box::new(Opened {
            view,
            bodies: Vec::new(),
        })));
    }
    let fresh = newer_keys(&view, have);
    let mut seen = HashSet::new();
    let mut held = 0u64;
    let mut bodies = Vec::new();
    let mut stop = meter.check(report);
    for record in view.shards.iter().flat_map(|s| &s.records) {
        if stop.is_some() {
            break;
        }
        if !fresh.contains(&record.key.spine_hash()) {
            continue;
        }
        let Some(body) = record.version["body"]["hash"]
            .as_str()
            .and_then(Hash32::parse)
        else {
            continue;
        };
        let size = record.version["body"]["size"].as_u64().unwrap_or(0);
        if !seen.insert(body) || held.saturating_add(size) > meter.budgets().run_bytes {
            continue;
        }
        match pass::fetch_body(folder, meter, report, blobs, naming, &record.version)? {
            pass::Body::Skip => continue,
            pass::Body::Halt => break,
            pass::Body::Got(bytes) => {
                held += bytes.len() as u64;
                bodies.push(bytes);
            }
        }
        stop = meter.check(report);
    }
    Ok(Got::Ready(Box::new(Opened { view, bodies })))
}

/// SP17: the keys the view's spine says changed after `have`, by their 16-byte
/// hashes: the records whose versions this device doesn't hold.
fn newer_keys(view: &View, have: u64) -> HashSet<[u8; 16]> {
    view.spine
        .iter()
        .filter(|e| e.s > have)
        .filter_map(|e| b64url_decode(&e.v))
        .flat_map(|v| {
            v.chunks_exact(48)
                .map(|c| <[u8; 16]>::try_from(&c[..16]).expect("16 bytes"))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// SP18: apply an opened view with the change files after it. Before anything is
/// applied the view's `week` claims are taken (SP17: they only add protection) and
/// the view is marked as landing, both saved (PS5). Once it has applied, the peer's
/// floor takes its generation (SP18), the claims are proven, and the mark is cleared
/// in the same save. A stop anywhere between the first save and the last leaves the
/// mark, and `finish_landings` finishes it on a later pass. One the floor or the chain
/// refuses is set aside and isn't read again until the peer names another.
#[allow(clippy::too_many_arguments)]
pub(super) fn receive(
    device: &mut Device<'_>,
    report: &mut Report,
    naming: &[u8; 32],
    peer: &DeviceId,
    key: &[u8; 32],
    opened: Opened,
    files: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
    now_ms: i64,
) -> Result<(), Error> {
    let Opened { view, bodies } = opened;
    let lib = device.lib;
    device
        .state
        .weeks
        .receive_view(device.conn, &lib, naming, *peer, &view)?;
    // PS5: the mark is saved before the apply commits, never after.
    device.state.views.landing.insert(
        *peer,
        Landing {
            hash: view.hash,
            gen: view.gen,
            through: view.through.s,
            tries: 0,
        },
    );
    save_state(device)?;
    let floor = device
        .store
        .library(&lib.id)
        .and_then(|l| l.floors.get(&peer.to_string()))
        .cloned()
        .unwrap_or_default();
    match read::receive_view(
        device.conn,
        &lib,
        &view,
        peer,
        key,
        &floor,
        false,
        bodies,
        files,
        now_ms,
    ) {
        Ok(got) => {
            if matches!(got.received.first(), Some(Received::Applied { .. })) {
                device_file::raise_view_generation(
                    device.ks,
                    device.store,
                    &lib.id,
                    peer,
                    view.gen,
                )?;
                device
                    .state
                    .weeks
                    .receive_view(device.conn, &lib, naming, *peer, &view)?;
                device.state.views.landing.remove(peer);
                report.viewed.push(*peer);
            } else if apply::seen(device.conn, &lib, *peer)? < view.through.s {
                // Nothing of it landed (it waits, or the reader is asked first).
                device.state.views.landing.remove(peer);
            }
            save_state(device)?;
        }
        // This device's own database or store failed: not the peer's doing. The mark
        // stays, and the next pass finishes whatever the apply did commit.
        Err(e @ (ChainError::Journal(_) | ChainError::Store(_))) => return Err(Error::Chain(e)),
        Err(e) => {
            if let Some(note) = device.state.views.peers.get_mut(peer) {
                note.refused = true;
            }
            device.state.views.landing.remove(peer);
            report
                .set_aside
                .push((format!("state/{peer}/{}", view.hash), e.to_string()));
            save_state(device)?;
        }
    }
    Ok(())
}

/// PS5, SP18, SP17: finish the views an earlier pass was landing, whether or not the
/// peer is ahead now, before anything is read of any peer. A view that hadn't applied
/// (`have` below its `through`) loses its mark: the normal road takes it again. One
/// that had has its generation raised on the peer's floor first, from what this device
/// holds (no read), and then, for peers read at all, its claims are proven from the
/// view read again within the budgets (no bodies, nothing applied). If it can't be
/// read again (gone, damaged, a part not synced), the claims stay as they are, unproven,
/// which only keeps content longer (SP17); the mark is dropped after `MAX_TRIES` such
/// passes so that it costs a pass its reads at most that many times. `true` when a
/// budget ran out.
pub(super) fn finish_landings<F: Folder>(
    device: &mut Device<'_>,
    folder: &F,
    meter: &Meter<'_>,
    report: &mut Report,
    membership: &super::membership::Membership,
    naming: &[u8; 32],
) -> Result<bool, Error> {
    let lib = device.lib;
    let marks: Vec<(DeviceId, Landing)> = device
        .state
        .views
        .landing
        .iter()
        .map(|(p, l)| (*p, l.clone()))
        .collect();
    for (peer, landing) in marks {
        let trusted = report.trusted.contains(&peer);
        if !trusted || apply::seen(device.conn, &lib, peer)? < landing.through {
            device.state.views.landing.remove(&peer);
            save_state(device)?;
            continue;
        }
        device_file::raise_view_generation(device.ks, device.store, &lib.id, &peer, landing.gen)?;
        let key = membership.key(&peer);
        // SP30: nothing is read of a quarantined peer, nor of one this device has
        // stopped receiving from.
        let skip = device.state.membership.quarantined.contains_key(&peer)
            || apply::stopped(device.conn, &lib, peer)?;
        let (Some(key), false) = (key, skip) else {
            device.state.views.landing.remove(&peer);
            save_state(device)?;
            continue;
        };
        let mut note = Noted {
            hash: landing.hash,
            through: Some(landing.through),
            tries: 0,
            refused: false,
        };
        let got = fetch(
            device,
            folder,
            meter,
            report,
            &mut None,
            naming,
            &peer,
            &key,
            &mut note,
            landing.through,
            true,
        )?;
        match got {
            Got::Halt => return Ok(true),
            Got::Ready(opened) => {
                device
                    .state
                    .weeks
                    .receive_view(device.conn, &lib, naming, peer, &opened.view)?;
                device.state.views.landing.remove(&peer);
            }
            Got::Skip => {
                let gave_up = landing.tries + 1 >= MAX_TRIES;
                if gave_up {
                    device.state.views.landing.remove(&peer);
                } else if let Some(l) = device.state.views.landing.get_mut(&peer) {
                    l.tries += 1;
                }
            }
        }
        save_state(device)?;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AnnotationRow;
    use crate::sync::apply::{
        self,
        tests::{highlight, ANNO},
    };
    use crate::sync::chain;
    use crate::sync::crypto::blob_name;
    use crate::sync::device_store::{Keystore, Memory, StoreError};
    use crate::sync::folder::{self, Page};
    use crate::sync::frontier;
    use crate::sync::pass::{Budgets, Readings, State};
    use crate::sync::testkit::{self, Kit, KitDevice, NAMING};
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Past the real clock, as in pass.rs's tests: SY10 holds a version whose clock
    // is ahead of the reader's.
    const READ: Readings = Readings {
        mono_ms: 1_000,
        wall_ms: 1_900_000_000_000,
        clock_ahead: false,
        same_boot: false,
    };

    fn pass_on<F: Folder>(
        kit: &mut Kit,
        i: usize,
        folder: &mut F,
        ks: Option<&dyn Keystore>,
        budgets: &Budgets,
        now: &dyn Fn() -> u64,
    ) -> Result<(Report, State), Error> {
        let kd = &mut kit.devices[i];
        let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
        let mut device = Device {
            conn: kd.db.conn(),
            lib: kd.lib,
            store: &mut kd.store,
            ks: ks.unwrap_or(&kd.ks),
            state,
            name: "Test Mac",
        };
        let report = pass::run(&mut device, folder, READ, budgets, now)?;
        Ok((report, device.state))
    }

    fn reader(kit: &Kit, i: usize) -> String {
        kit.devices[i].lib.device.to_string()
    }

    fn pass_within(kit: &mut Kit, i: usize, budgets: &Budgets) -> (Report, State) {
        let mut folder = kit.folder.view(&reader(kit, i));
        pass_on(kit, i, &mut folder, None, budgets, &|| 0).unwrap()
    }

    fn pass(kit: &mut Kit, i: usize) -> (Report, State) {
        pass_within(kit, i, &Budgets::sy2())
    }

    fn annotation(id: &str, cfi: &str, note: &str) -> AnnotationRow {
        AnnotationRow {
            id: id.into(),
            cfi_range: cfi.into(),
            ..highlight("book", Some(note), 1_760_000_000_500)
        }
    }

    /// A device's edit of the one note the tests change: the next change.
    fn edit(kd: &mut KitDevice, text: &str, n: i64) {
        kd.db
            .save_annotation(&highlight("book", Some(text), 1_760_000_001_000 + n))
            .unwrap();
        kd.publish_pending().unwrap();
    }

    /// Device 1 writes `total` changes: 30 highlights besides the note in the first,
    /// the note changed in each, and a view through change `through` (none for 0),
    /// published before the changes after it.
    fn rig(devices: usize, through: u64, total: u64, text: &str) -> Kit {
        let mut kit = testkit::library(devices);
        let kd = &mut kit.devices[1];
        for i in 0..30 {
            let id = format!("{i:08x}-6f7a-4b1c-9d2e-4f5a6b7c8d9e");
            let cfi = format!("epubcfi(/6/4!/4/{},/1:0,/1:4)", 2 * i + 10);
            kd.db
                .save_annotation(&annotation(&id, &cfi, "kept"))
                .unwrap();
        }
        for n in 1..=total {
            edit(kd, format!("note {n}{text}").as_str(), n as i64);
            if n == through {
                kd.publish_view(through);
            }
        }
        kit
    }

    fn peer_of(kit: &Kit) -> DeviceId {
        kit.devices[1].lib.device
    }

    fn change_at(kit: &Kit, s: u64) -> String {
        chain::change_path(&peer_of(kit), s)
    }

    fn view_dir(kit: &Kit) -> String {
        format!("state/{}/{}", peer_of(kit), kit.devices[1].view.unwrap())
    }

    fn files_in(kit: &Kit, dir: &str) -> Vec<String> {
        let mut names: Vec<String> = kit.devices[1]
            .folder
            .list(dir, None)
            .unwrap()
            .entries
            .into_iter()
            .map(|e| e.name)
            .collect();
        names.sort();
        names
    }

    /// The parts of device 1's view, not its index.
    fn parts(kit: &Kit) -> Vec<String> {
        files_in(kit, &view_dir(kit))
            .into_iter()
            .filter(|n| !n.ends_with("/index.json"))
            .collect()
    }

    fn get(kit: &Kit, path: &str) -> Vec<u8> {
        let dir = path.rsplit_once('/').unwrap().0;
        let kd = &kit.devices[1];
        let entry = kd
            .folder
            .list(dir, None)
            .unwrap()
            .entries
            .into_iter()
            .find(|e| e.name == path)
            .unwrap();
        match files::read_file(&kd.folder, &entry).unwrap() {
            Read::Bytes(b) => b,
            other => panic!("{other:?}"),
        }
    }

    fn put(kit: &mut Kit, path: &str, bytes: &[u8]) {
        folder::Folder::write(&mut kit.devices[1].folder, path, bytes).unwrap();
    }

    fn reads(kit: &Kit, path: &str) -> usize {
        kit.folder.read_count(&reader(kit, 0), path)
    }

    /// The reader's (device 0's) note text of the one note the tests change, if any.
    fn note(kit: &Kit) -> Option<Option<String>> {
        kit.devices[0]
            .db
            .annotations("book")
            .unwrap()
            .into_iter()
            .find(|a| a.id == ANNO)
            .map(|a| a.note)
    }

    fn held(kit: &Kit) -> u64 {
        let kd = &kit.devices[0];
        apply::seen(kd.db.conn(), &kd.lib, peer_of(kit)).unwrap()
    }

    fn gen_of(kit: &Kit) -> u64 {
        let kd = &kit.devices[0];
        kd.store.library(&kd.lib.id).unwrap().floors[&peer_of(kit).to_string()].gen
    }

    /// Without any change file before the view (storage no longer holds them).
    fn vanish_changes(kit: &Kit, through: u64) {
        for s in 1..=through {
            kit.folder.vanish(&change_at(kit, s));
        }
    }

    fn used(report: &Report, path_prefix: &str) -> bool {
        report
            .set_aside
            .iter()
            .any(|(p, _)| p.starts_with(path_prefix))
    }

    #[test]
    fn a_device_behind_resumes_from_the_view_and_the_changes_after_it() {
        // C27 end to end, SP18: what is held is older than the view's `through`, so
        // the view is taken, then the change files after it.
        for prior in [0u64, 2] {
            let mut kit = testkit::library(2);
            let peer = peer_of(&kit);
            let kd = &mut kit.devices[1];
            kd.db
                .save_annotation(&annotation(
                    "2b3c4d5e-6f7a-4b1c-9d2e-4f5a6b7c8d9e",
                    "epubcfi(/6/4!/4/4,/1:0,/1:4)",
                    "kept",
                ))
                .unwrap();
            for n in 1..=2 {
                edit(kd, &format!("note {n}"), n);
            }
            if prior == 2 {
                let (report, _) = pass(&mut kit, 0);
                assert_eq!(report.received, vec![(peer, 2)]);
                assert!(report.viewed.is_empty());
            }
            for n in 3..=4 {
                edit(&mut kit.devices[1], &format!("note {n}"), n);
            }
            let index = kit.devices[1].publish_view(4);
            for n in 5..=6 {
                edit(&mut kit.devices[1], &format!("note {n}"), n);
            }
            let (report, state) = pass(&mut kit, 0);
            assert_eq!(report.viewed, vec![peer], "{report:?}");
            assert_eq!(report.received, vec![(peer, 6)], "{report:?}");
            assert!(report.behind.is_empty());
            assert_eq!(held(&kit), 6);
            assert_eq!(note(&kit), Some(Some("note 6".into())));
            assert_eq!(kit.devices[0].db.annotations("book").unwrap().len(), 2);
            // The changes the view covers were not read one by one.
            for s in 1..=6 {
                let expected = usize::from(s <= prior || s > 4);
                assert_eq!(reads(&kit, &change_at(&kit, s)), expected, "change {s}");
            }
            assert_eq!(
                gen_of(&kit),
                1,
                "SP18: the floor takes the view's generation"
            );
            assert_eq!(state.views.peers[&peer].through, Some(4));
            // A record this device already holds isn't fetched again: the other
            // highlight's body was read once, by whichever pass first took it.
            let first = chain::open_change(
                &get(&kit, &change_at(&kit, 1)),
                &kit.devices[0].lib.id,
                &peer,
                &kit.devices[1].key.public(),
            )
            .unwrap();
            let named = first
                .versions
                .iter()
                .find(|v| {
                    v["key"]
                        .as_str()
                        .unwrap()
                        .starts_with("annotation:2b3c4d5e")
                })
                .unwrap();
            let body = named["body"]["hash"]
                .as_str()
                .and_then(Hash32::parse)
                .unwrap();
            let path = format!("blobs/{}", blob_name(&NAMING, &body));
            assert_eq!(reads(&kit, &path), 1, "prior {prior}");
            // Nothing new: nothing is read again.
            let (again, _) = pass(&mut kit, 0);
            assert!(again.received.is_empty() && again.viewed.is_empty());
            // A deletion after the view arrives from the change file, and the view's
            // index isn't read again: this device is past it.
            let id = ANNO.to_string();
            kit.devices[1].db.delete_annotation(&id).unwrap();
            assert_eq!(kit.devices[1].publish_pending(), Some(7));
            let (report, _) = pass(&mut kit, 0);
            assert_eq!(report.received, vec![(peer, 7)], "{report:?}");
            assert!(report.viewed.is_empty());
            assert_eq!(note(&kit), None);
            let index_path = format!("state/{peer}/{index}/index.json");
            assert_eq!(reads(&kit, &index_path), 1);
        }
    }

    #[test]
    fn a_chain_past_a_passes_byte_budget_arrives_through_a_view() {
        // #208, SP18: the first floor checkpoint a device hasn't passed (the peer's
        // latest change) is more than one pass's budget away by change files, so a
        // run keeps nothing and every pass reads the same prefix. With the view it is
        // a few KB away.
        let text = format!(" {}", "x".repeat(50_000));
        // What the steps before step (4) read, from a kit with nothing to read, and
        // room for the view, a change and the bodies of both: 8 changes won't fit.
        let (probe, _) = pass(&mut testkit::library(2), 0);
        let mut b = Budgets::sy2();
        b.bytes = probe.bytes_read + 220_000;
        let mut without = rig(2, 0, 8, &text);
        for _ in 0..3 {
            let (report, _) = pass_within(&mut without, 0, &b);
            assert_eq!(report.stopped, Some(Stopped::Bytes), "{report:?}");
            assert!(report.received.is_empty() && report.viewed.is_empty());
            assert_eq!(report.behind, vec![peer_of(&without)]);
            assert_eq!(
                held(&without),
                0,
                "the same prefix is read again, for nothing"
            );
        }
        let mut with = rig(2, 7, 8, &text);
        let (report, _) = pass_within(&mut with, 0, &b);
        assert_eq!(report.stopped, None, "{report:?}");
        assert_eq!(report.viewed, vec![peer_of(&with)]);
        assert_eq!(report.received, vec![(peer_of(&with), 8)]);
        assert_eq!(note(&with), Some(Some(format!("note 8{text}"))));
        assert!(report.behind.is_empty());
    }

    #[test]
    fn a_view_vouches_for_its_last_change_in_the_frontier() {
        // SP16: a view alone has no change file for its last change; its spine entry
        // carries that change's own signature, so the frontier entry can be made.
        let mut kit = rig(2, 4, 4, "");
        let (report, _) = pass(&mut kit, 0);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        let kd = &kit.devices[0];
        let path = device_file::device_path(&kd.lib.device);
        let entry = kd
            .folder
            .list("devices", None)
            .unwrap()
            .entries
            .into_iter()
            .find(|e| e.name == path)
            .unwrap();
        let Read::Bytes(bytes) = files::read_file(&kd.folder, &entry).unwrap() else {
            panic!("no device file")
        };
        let file = device_file::open(&bytes, &kd.lib.id, &kd.lib.device, &kd.key.public()).unwrap();
        let entries = frontier::entries(&file.fields.lists["frontier"], true);
        let last = kit.devices[1].own_changes(4).pop().unwrap();
        assert_eq!(entries.len(), 1, "{:?}", file.fields.lists);
        assert_eq!(
            (entries[0].d, entries[0].s, entries[0].h),
            (peer_of(&kit), 4, last.hash)
        );
    }

    #[test]
    fn a_views_week_claims_are_kept() {
        // SP17, #176: a view adds protection for what its writer retired, and the
        // claims are saved with the pass's state.
        let mut kit = rig(2, 0, 4, "");
        pass(&mut kit, 0);
        edit(&mut kit.devices[1], "note 5", 5);
        kit.devices[1].publish_view(5);
        let (report, state) = pass(&mut kit, 0);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        let weeks = state.weeks.weeks();
        assert!(!weeks.is_empty());
        // The retirement by change 5 comes only with the view, and is proven once the
        // view has landed: the version before it is held from the change files.
        assert!(weeks
            .iter()
            .any(|w| w.retiring.seq == 5 && state.weeks.may_release(w.name, &w.retiring)));
        let saved = State::load(kit.devices[0].db.conn(), &kit.devices[0].lib.id).unwrap();
        assert_eq!(saved.weeks.weeks().len(), weeks.len());
    }

    /// A folder that sets `hit` when a part of a view is read, to move a clock.
    struct Spy<'a, F: Folder> {
        inner: F,
        hit: &'a std::cell::Cell<u64>,
    }

    impl<F: Folder> Folder for Spy<'_, F> {
        type Token = F::Token;
        fn list(&self, dir: &str, cursor: Option<&str>) -> io::Result<Page<F::Token>> {
            self.inner.list(dir, cursor)
        }
        fn stat(&self, token: &F::Token) -> io::Result<Option<u64>> {
            self.inner.stat(token)
        }
        fn read(&self, token: &F::Token, limit: Option<u64>) -> io::Result<folder::Read> {
            let path = format!("{token:?}");
            if path.contains("state/") && !path.contains("index.json") {
                self.hit.set(120_000);
            }
            self.inner.read(token, limit)
        }
        fn write(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
            self.inner.write(path, bytes)
        }
        fn remove(&mut self, token: &F::Token) -> io::Result<()> {
            self.inner.remove(token)
        }
    }

    fn count(kit: &Kit, dir: &str) -> u64 {
        kit.devices[0].folder.list(dir, None).unwrap().entries.len() as u64
    }

    #[test]
    fn each_budget_stops_the_view_and_the_next_pass_takes_it_up() {
        // PS6, SP37: a budget that runs out stops the pass where it is; nothing of the
        // view is applied half-way, and the next pass takes the same road.
        let finish = |kit: &mut Kit| {
            let (report, _) = pass(kit, 0);
            assert_eq!(report.viewed, vec![peer_of(kit)], "{report:?}");
            assert_eq!(report.received, vec![(peer_of(kit), 6)]);
            assert_eq!(note(kit), Some(Some("note 6".into())));
        };
        // The name budget, at the listing of the view's directory.
        let mut kit = rig(2, 4, 6, "");
        let mut b = Budgets::sy2();
        b.names = count(&kit, "libraries") + count(&kit, "trust") + count(&kit, "devices");
        let (report, _) = pass_within(&mut kit, 0, &b);
        assert_eq!(report.stopped, Some(Stopped::Names));
        let before_view = report.bytes_read;
        assert!(report.viewed.is_empty() && held(&kit) == 0);
        finish(&mut kit);
        // The byte budget: the parts' declared sizes don't fit in what is left, so
        // none is read.
        let mut kit = rig(2, 4, 6, "");
        let index = chain_free_index(&kit);
        let declared: u64 = {
            let (idx, _, _) = view::open_index(
                &index,
                &kit.devices[0].lib.id,
                &peer_of(&kit),
                &kit.devices[1].key.public(),
            )
            .unwrap();
            idx.shards
                .iter()
                .map(|s| s.size)
                .chain(idx.spine.iter().map(|s| s.size))
                .sum::<u64>()
                + idx.open.size
        };
        assert!(before_view / 2 <= declared, "{before_view} {declared}");
        let mut b = Budgets::sy2();
        b.bytes = declared + before_view / 2;
        let (report, _) = pass_within(&mut kit, 0, &b);
        assert_eq!(report.stopped, Some(Stopped::Bytes), "{report:?}");
        assert!(report.viewed.is_empty() && held(&kit) == 0);
        for part in parts(&kit) {
            assert_eq!(reads(&kit, &part), 0, "{part} was read past the budget");
        }
        finish(&mut kit);
        // Time, after the first part.
        let mut kit = rig(2, 4, 6, "");
        let clock = std::cell::Cell::new(0);
        let mut spy = Spy {
            inner: kit.folder.view(&reader(&kit, 0)),
            hit: &clock,
        };
        let (report, _) = pass_on(&mut kit, 0, &mut spy, None, &Budgets::sy2(), &|| {
            clock.get()
        })
        .unwrap();
        assert_eq!(report.stopped, Some(Stopped::Time), "{report:?}");
        assert!(report.viewed.is_empty() && held(&kit) == 0);
        finish(&mut kit);
    }

    fn chain_free_index(kit: &Kit) -> Vec<u8> {
        get(kit, &format!("{}/index.json", view_dir(kit)))
    }

    #[test]
    fn a_view_that_fails_its_check_is_set_aside_and_every_peer_is_still_read() {
        // SY28, SP18: a broken view stops nothing: the peer's change files are read
        // as if it had none, and a healthy peer is read as before.
        let mut kit = rig(3, 4, 6, "");
        let peer = peer_of(&kit);
        let healthy = kit.devices[2].lib.device;
        kit.devices[2]
            .db
            .save_annotation(&annotation(
                "3c4d5e6f-7a8b-4c1d-8e2f-5a6b7c8d9e0f",
                "epubcfi(/6/4!/4/90,/1:0,/1:4)",
                "from the third",
            ))
            .unwrap();
        kit.devices[2].publish_pending().unwrap();
        let index_path = format!("{}/index.json", view_dir(&kit));
        put(&mut kit, &index_path, b"{}");
        let (report, state) = pass(&mut kit, 0);
        assert!(used(&report, &index_path), "{:?}", report.set_aside);
        assert!(report.viewed.is_empty());
        assert!(report.received.contains(&(peer, 6)), "{report:?}");
        assert!(report.received.contains(&(healthy, 1)), "{report:?}");
        assert_eq!(state.views.peers[&peer].tries, 1);
        assert_eq!(note(&kit), Some(Some("note 6".into())));
    }

    #[test]
    fn a_failed_views_note_is_saved_when_the_pass_stops_straight_after() {
        // PS5: the note is saved on every way out of the view step.
        let mut kit = rig(2, 4, 6, "");
        let index_path = format!("{}/index.json", view_dir(&kit));
        put(&mut kit, &index_path, b"{}");
        let mut b = Budgets::sy2();
        b.names = count(&kit, "libraries")
            + count(&kit, "trust")
            + count(&kit, "devices")
            + files_in(&kit, &view_dir(&kit)).len() as u64;
        let (report, _) = pass_within(&mut kit, 0, &b);
        assert_eq!(report.stopped, Some(Stopped::Names), "{report:?}");
        let saved = State::load(kit.devices[0].db.conn(), &kit.devices[0].lib.id).unwrap();
        assert_eq!(saved.views.peers[&peer_of(&kit)].tries, 1);
    }

    #[test]
    fn a_part_not_in_storage_yet_is_waited_for() {
        // SY28: the view waits for a part storage hasn't synced; nothing is read of
        // it, and it isn't counted against the view.
        let mut kit = rig(2, 4, 6, "");
        vanish_changes(&kit, 4);
        let part = parts(&kit).remove(0);
        let bytes = get(&kit, &part);
        kit.folder.vanish(&part);
        let (report, state) = pass(&mut kit, 0);
        assert!(used(&report, &part), "{:?}", report.set_aside);
        assert!(report.received.is_empty() && held(&kit) == 0);
        assert_eq!(report.behind, vec![peer_of(&kit)]);
        assert_eq!(state.views.peers[&peer_of(&kit)].tries, 0);
        for other in parts(&kit) {
            assert_eq!(
                reads(&kit, &other),
                0,
                "no part is read while one is missing"
            );
        }
        put(&mut kit, &part, &bytes);
        let (report, _) = pass(&mut kit, 0);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        assert_eq!(report.received, vec![(peer_of(&kit), 6)]);
        assert_eq!(note(&kit), Some(Some("note 6".into())));
    }

    #[test]
    fn a_half_synced_part_is_read_again_on_the_next_pass() {
        // SP7: wrong bytes for a while fail the part's hash; nothing is applied, and
        // the next pass takes the view.
        let mut kit = rig(2, 4, 6, "");
        vanish_changes(&kit, 4);
        let part = parts(&kit).remove(0);
        kit.folder.half_synced(&reader(&kit, 0), &part, b"junk", 1);
        let (report, state) = pass(&mut kit, 0);
        assert!(used(&report, &view_dir(&kit)), "{:?}", report.set_aside);
        assert!(report.viewed.is_empty() && held(&kit) == 0);
        assert_eq!(state.views.peers[&peer_of(&kit)].tries, 1);
        let (report, _) = pass(&mut kit, 0);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        assert_eq!(held(&kit), 6);
    }

    #[test]
    fn a_view_that_keeps_failing_is_not_read_again_until_another_is_named() {
        let mut kit = rig(2, 4, 6, "");
        vanish_changes(&kit, 4);
        let part = parts(&kit).remove(0);
        put(&mut kit, &part, b"not the part");
        let index_path = format!("{}/index.json", view_dir(&kit));
        for _ in 0..MAX_TRIES {
            let (report, _) = pass(&mut kit, 0);
            assert!(report.viewed.is_empty() && held(&kit) == 0);
        }
        assert_eq!(reads(&kit, &index_path), MAX_TRIES as usize);
        let (report, state) = pass(&mut kit, 0);
        assert_eq!(
            reads(&kit, &index_path),
            MAX_TRIES as usize,
            "not read again"
        );
        assert!(state.views.peers[&peer_of(&kit)].refused);
        assert!(report
            .set_aside
            .iter()
            .any(|(_, why)| why.contains("refused on an earlier pass")));
        // The peer names another view (its index isn't in storage yet): a new note.
        let named = Hash32([9; 32]);
        kit.devices[1].view = Some(named);
        kit.devices[1].publish_device_file();
        let (report, state) = pass(&mut kit, 0);
        let note = &state.views.peers[&peer_of(&kit)];
        assert_eq!((note.hash, note.tries, note.refused), (named, 0, false));
        assert!(report
            .set_aside
            .iter()
            .any(|(p, why)| p.ends_with("index.json") && why.contains("not in storage")));
    }

    #[test]
    fn an_index_or_a_part_over_its_limit_is_set_aside_unread() {
        // SY28, SP8: the index is at most 1 MB, and a part no more than its index
        // declares; neither is read first and measured after.
        let mut kit = rig(2, 4, 6, "");
        vanish_changes(&kit, 4);
        let part = parts(&kit).remove(0);
        let declared = get(&kit, &part).len();
        put(&mut kit, &part, &vec![b'x'; declared + 1]);
        let (report, state) = pass(&mut kit, 0);
        assert!(used(&report, &part), "{:?}", report.set_aside);
        assert_eq!(kit.folder.bytes_read(&reader(&kit, 0), &part), 0);
        assert_eq!(state.views.peers[&peer_of(&kit)].tries, 1);
        let index_path = format!("{}/index.json", view_dir(&kit));
        let before = kit.folder.bytes_read(&reader(&kit, 0), &index_path);
        put(&mut kit, &index_path, &vec![b'x'; (1 << 20) + 1]);
        let (report, _) = pass(&mut kit, 0);
        assert!(used(&report, &index_path), "{:?}", report.set_aside);
        assert_eq!(kit.folder.bytes_read(&reader(&kit, 0), &index_path), before);
        assert!(report.viewed.is_empty() && held(&kit) == 0);
    }

    #[test]
    fn a_view_bigger_than_one_passes_byte_budget_is_refused_unread() {
        // SP15, #208: no pass could read it. It is set aside, with a pending item for
        // what a device may keep across passes (docs/pending-approvals.md, 126).
        let mut kit = rig(2, 4, 6, "");
        vanish_changes(&kit, 4);
        let mut b = Budgets::sy2();
        b.bytes = parts(&kit)
            .iter()
            .map(|p| get(&kit, p).len() as u64)
            .sum::<u64>()
            - 1;
        let (report, state) = pass_within(&mut kit, 0, &b);
        assert!(report
            .set_aside
            .iter()
            .any(|(_, why)| why.contains("larger than one pass's byte budget")));
        assert!(state.views.peers[&peer_of(&kit)].refused);
        for part in parts(&kit) {
            assert_eq!(reads(&kit, &part), 0);
        }
    }

    #[test]
    fn an_index_that_isnt_the_one_the_device_file_names_is_refused() {
        // SP17: a directory is named by its index's hash; a valid index of the peer's
        // under another name (an older one served as the newer) isn't taken.
        let mut kit = rig(2, 4, 6, "");
        vanish_changes(&kit, 4);
        let bytes = get(&kit, &format!("{}/index.json", view_dir(&kit)));
        let named = Hash32([9; 32]);
        kit.devices[1].view = Some(named);
        kit.devices[1].publish_device_file();
        let path = format!("state/{}/{named}/index.json", peer_of(&kit));
        put(&mut kit, &path, &bytes);
        let (report, state) = pass(&mut kit, 0);
        assert!(report
            .set_aside
            .iter()
            .any(|(_, why)| why.contains("not the index the device file names")));
        assert!(report.viewed.is_empty() && held(&kit) == 0);
        assert_eq!(state.views.peers[&peer_of(&kit)].tries, 1);
    }

    #[test]
    fn a_view_older_than_the_floor_is_refused() {
        // SP18: a floor refuses a view older than its generation.
        let mut kit = rig(2, 4, 6, "");
        vanish_changes(&kit, 4);
        let peer = peer_of(&kit);
        let lib = kit.devices[0].lib.id;
        let kd = &mut kit.devices[0];
        kd.store
            .library_mut(&lib)
            .unwrap()
            .floors
            .entry(peer.to_string())
            .or_default()
            .gen = 5;
        let (report, state) = pass(&mut kit, 0);
        assert!(report
            .set_aside
            .iter()
            .any(|(_, why)| why.contains("older than the floor")));
        assert!(state.views.peers[&peer].refused);
        assert_eq!(held(&kit), 0);
        assert!(report.viewed.is_empty());
    }

    #[test]
    fn a_conflict_copy_of_a_part_is_never_read() {
        // SP7: a copy storage made is a candidate for the file it copies, not the view.
        let mut kit = rig(2, 4, 6, "");
        let part = parts(&kit).remove(0);
        let copy = part.replace(".json", "-conflict.json");
        put(&mut kit, &copy, b"not the part");
        let (report, _) = pass(&mut kit, 0);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        assert_eq!(reads(&kit, &copy), 0);
        assert_eq!(held(&kit), 6);
    }

    #[test]
    fn bodies_storage_lacks_leave_their_versions_waiting_not_half_applied() {
        // SY28: the view lands; a version whose body isn't in storage waits for it.
        let mut kit = rig(2, 4, 6, "");
        for blob in files_in(&kit, "blobs") {
            kit.folder.vanish(&blob);
        }
        let (report, _) = pass(&mut kit, 0);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        assert_eq!(held(&kit), 6);
        assert_eq!(note(&kit), None);
        assert!(kit.devices[0].db.annotations("book").unwrap().is_empty());
    }

    #[test]
    fn bodies_past_the_run_cap_are_left_for_step_five() {
        // SP15 (the 32 MB cap): the view lands, and a body that would take it past
        // the cap isn't read; its version waits.
        let mut kit = rig(2, 4, 6, "");
        let mut b = Budgets::sy2();
        b.run_bytes = 1;
        let (report, _) = pass_within(&mut kit, 0, &b);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        assert_eq!(note(&kit), None);
        // Only the bodies of the two later changes were read, not the view's 62.
        let total: usize = files_in(&kit, "blobs").iter().map(|b| reads(&kit, b)).sum();
        assert!(total <= 4, "{total} bodies read");
    }

    /// A device-store keystore whose `n`th save fails, as a crash between two writes.
    struct Flaky {
        inner: Memory,
        saves: AtomicUsize,
        fail: usize,
    }

    impl Keystore for Flaky {
        fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
            self.inner.load()
        }
        fn save(&self, bytes: &[u8]) -> Result<(), StoreError> {
            if self.saves.fetch_add(1, Ordering::SeqCst) == self.fail {
                return Err(StoreError::Unreadable);
            }
            self.inner.save(bytes)
        }
    }

    fn versions(kit: &Kit) -> i64 {
        kit.devices[0]
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM sync_versions", [], |r| r.get(0))
            .unwrap()
    }

    /// Device 0 holds changes 1 to 4; the view through 5 retires versions it holds,
    /// so its claims can be proven; change 6 follows the view.
    fn landing_rig() -> Kit {
        let mut kit = rig(2, 0, 4, "");
        pass(&mut kit, 0);
        edit(&mut kit.devices[1], "note 5", 5);
        kit.devices[1].publish_view(5);
        edit(&mut kit.devices[1], "note 6", 6);
        kit
    }

    /// What device 0's saved claims say, and which of them are proven.
    fn proofs(kit: &Kit) -> Vec<(String, bool)> {
        let state = State::load(kit.devices[0].db.conn(), &kit.devices[0].lib.id).unwrap();
        let mut all: Vec<_> = state
            .weeks
            .weeks()
            .iter()
            .map(|w| {
                (
                    format!("{:?} at {}", w.name, w.retiring.seq),
                    state.weeks.may_release(w.name, &w.retiring),
                )
            })
            .collect();
        all.sort();
        all
    }

    fn landing_of(kit: &Kit) -> usize {
        State::load(kit.devices[0].db.conn(), &kit.devices[0].lib.id)
            .unwrap()
            .views
            .landing
            .len()
    }

    /// The mark a pass saves before it applies a view, and (when `keep_claims` is
    /// false) without the claims it had saved.
    fn mark_landing(kit: &mut Kit, keep_claims: bool) {
        let (peer, hash) = (peer_of(kit), kit.devices[1].view.unwrap());
        let lib = kit.devices[0].lib.id;
        let kd = &mut kit.devices[0];
        let mut state = State::load(kd.db.conn(), &lib).unwrap();
        if !keep_claims {
            state.weeks = Default::default();
        }
        state.views.landing.insert(
            peer,
            Landing {
                hash,
                gen: 1,
                through: 5,
                tries: 0,
            },
        );
        let tx = kd.db.conn().unchecked_transaction().unwrap();
        state.save(&tx, &lib).unwrap();
        tx.commit().unwrap();
    }

    /// A pass that stopped after the apply: the view is applied, the floor's generation
    /// and the proofs are as they were before it, and the mark is saved.
    fn interrupt_after_apply(kit: &mut Kit, keep_claims: bool) {
        mark_landing(kit, keep_claims);
        let (peer, lib) = (peer_of(kit), kit.devices[0].lib.id);
        kit.devices[0]
            .store
            .library_mut(&lib)
            .unwrap()
            .floors
            .get_mut(&peer.to_string())
            .unwrap()
            .gen = 0;
    }

    #[test]
    fn a_failure_at_any_save_of_a_pass_that_takes_a_view_is_finished_by_the_next_pass() {
        // PS5: a failed save of the device store, wherever it falls, leaves what the
        // next pass completes: no version applied twice, the floor at the view's
        // generation, the claims proven as in a pass that wasn't interrupted.
        let mut reference = landing_rig();
        pass(&mut reference, 0);
        let expected = versions(&reference);
        let expected_proofs = proofs(&reference);
        assert!(expected_proofs.iter().any(|(_, proven)| *proven));
        assert_eq!(gen_of(&reference), 1);
        let mut n = 0;
        loop {
            let mut kit = landing_rig();
            let flaky = Flaky {
                inner: Memory::default(),
                saves: AtomicUsize::new(0),
                fail: n,
            };
            let mut folder = kit.folder.view(&reader(&kit, 0));
            let first = pass_on(
                &mut kit,
                0,
                &mut folder,
                Some(&flaky),
                &Budgets::sy2(),
                &|| 0,
            );
            // A failed save isn't always an error of the pass: the floor's (SP18) is
            // set aside as a device file that couldn't be accepted, and read again.
            let _ = first;
            let reached = flaky.saves.load(Ordering::SeqCst) > n;
            let (report, _) = pass(&mut kit, 0);
            assert_eq!(held(&kit), 6, "save {n}: {report:?}");
            assert_eq!(note(&kit), Some(Some("note 6".into())), "save {n}");
            assert_eq!(versions(&kit), expected, "save {n}: nothing applied twice");
            assert_eq!(
                gen_of(&kit),
                1,
                "save {n}: the floor takes the view's generation"
            );
            assert_eq!(proofs(&kit), expected_proofs, "save {n}: the claims");
            assert_eq!(landing_of(&kit), 0, "save {n}: nothing left landing");
            assert!(report.behind.is_empty(), "save {n}");
            if !reached {
                break;
            }
            n += 1;
            assert!(n < 60, "the pass never completes");
        }
        assert!(
            n > 2,
            "a pass that takes a view saves the device store more than once: {n}"
        );
    }

    #[test]
    fn a_view_applied_but_not_finished_is_finished_though_the_peer_is_not_ahead() {
        // PS5, SP18, SP17: a stop right after the apply commits (or after the floor's
        // generation was raised) leaves the peer with nothing ahead of this device, and
        // the generation and the proofs are still made.
        for keep_claims in [true, false] {
            let mut kit = landing_rig();
            pass(&mut kit, 0);
            let expected = proofs(&kit);
            interrupt_after_apply(&mut kit, keep_claims);
            assert_eq!(gen_of(&kit), 0);
            let (report, _) = pass(&mut kit, 0);
            assert!(
                report.viewed.is_empty() && report.received.is_empty(),
                "{report:?}"
            );
            assert!(report.behind.is_empty());
            assert_eq!(gen_of(&kit), 1, "the generation is raised");
            assert_eq!(proofs(&kit), expected, "the claims are proven again");
            assert_eq!(landing_of(&kit), 0);
            assert_eq!(held(&kit), 6);
        }
    }

    #[test]
    fn a_mark_whose_view_never_applied_is_cleared_and_the_view_is_taken() {
        // PS5: the stop came after the mark and before the apply.
        let mut kit = landing_rig();
        mark_landing(&mut kit, true);
        let (report, _) = pass(&mut kit, 0);
        assert_eq!(report.viewed, vec![peer_of(&kit)], "{report:?}");
        assert_eq!(held(&kit), 6);
        assert_eq!(gen_of(&kit), 1);
        assert_eq!(landing_of(&kit), 0);
    }

    #[test]
    fn a_view_that_did_not_land_leaves_no_mark_and_raises_no_generation() {
        // PS5, SP18: the view waits (the change file that reaches the floor's
        // checkpoint is missing), so nothing of it is applied; whether the mark was
        // cleared by the receive or by the replay of one an earlier pass left, the
        // floor keeps its generation.
        let mut kit = landing_rig();
        kit.folder.vanish(&change_at(&kit, 6));
        let (report, _) = pass(&mut kit, 0);
        assert!(report.viewed.is_empty(), "{report:?}");
        assert_eq!((held(&kit), gen_of(&kit), landing_of(&kit)), (4, 0, 0));
        mark_landing(&mut kit, true);
        let (report, _) = pass(&mut kit, 0);
        assert!(report.viewed.is_empty(), "{report:?}");
        assert_eq!((held(&kit), gen_of(&kit), landing_of(&kit)), (4, 0, 0));
    }

    #[test]
    fn a_view_that_cant_be_read_again_still_has_its_generation_and_costs_a_bounded_number_of_reads()
    {
        // SP18, SP17: the floor's generation comes from what this device holds; the
        // proof needs the view, and gives up after MAX_TRIES passes (the claims stay,
        // unproven, which only keeps content longer).
        let mut kit = landing_rig();
        pass(&mut kit, 0);
        interrupt_after_apply(&mut kit, true);
        let part = parts(&kit).remove(0);
        kit.folder.vanish(&part);
        let index_path = format!("{}/index.json", view_dir(&kit));
        let before = reads(&kit, &index_path);
        for n in 1..=MAX_TRIES + 2 {
            let (report, _) = pass(&mut kit, 0);
            assert!(
                report.stopped.is_none() && report.behind.is_empty(),
                "{report:?}"
            );
            assert_eq!(gen_of(&kit), 1, "pass {n}");
            assert_eq!(landing_of(&kit), usize::from(n < MAX_TRIES), "pass {n}");
        }
        assert_eq!(reads(&kit, &index_path) - before, MAX_TRIES as usize);
        assert!(!proofs(&kit).is_empty(), "the claims are kept");
    }
}
