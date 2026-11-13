//! SP37 (9): publishing. Reservations and holds first, then uploads, then the
//! naming change (SP15, SP33, SP34). PS4: this module orders the writes; each
//! decision stays in the module that has it (`journal` numbers, `chain` signs and
//! keeps the signed bytes, `hold` limits and gates, `files` writes through SP7).
//!
//! The order of writes, and what stopping between any two leaves:
//!
//! 1. Changes signed on an earlier pass and not yet seen in storage are uploaded
//!    again from the device store, oldest first, never signed again (SP15).
//! 2. A change `R` with the ready versions and the new `reserve` and `hold` lists is
//!    numbered, signed and kept in the device store (SP15 (2)–(3)), the ledger
//!    notes it and is saved, and only then is it uploaded (SP15 (4)). Versions whose
//!    body is not in storage yet are withheld from it.
//! 3. The bodies those versions name are uploaded, each to its keyed name (SP9).
//! 4. A change `C` naming them is numbered only now that they are in storage, so
//!    no change file can name a body that was never written.
//!
//! A stop (a budget, a failed write, a crash) between any two of these leaves the
//! rows, the device store and the ledger so that the next pass takes the same
//! road: a signed change not yet uploaded is in the store; a reservation made is in
//! the ledger, so it isn't made twice; a body already in storage is listed and not
//! written again. A peer reading in between sees a reservation or a body that
//! nothing names, which it ignores (SP33), never a version without its body.

use super::chain::{self, Change};
use super::crypto::blob_name;
use super::device_file::{self, DeviceFile, Fields};
use super::files::{self, Read};
use super::folder::{Folder, Listed};
use super::frontier::{self, Held};
use super::hold::{Hold, Kind as Object, Progress, Reserve};
use super::ids::{identity_hash, DeviceId, Hash32};
use super::journal;
use super::pass::{list_all, save_state, Blocked, Device, Error, Meter, Report};
use super::record::{version_hash, Content, Entry, Key, Kind, Version};
use super::trust::Mark;
use std::collections::{BTreeMap, BTreeSet, HashSet};

type Blobs<T> = Option<BTreeMap<String, Listed<T>>>;

/// SP15 (1): what this device's own files in storage allow.
enum Own<T> {
    /// Nothing newer than its marks: its device file as storage has it, and the
    /// copies of it that a conflict made, which it may remove (SP7).
    Clear {
        file: Option<Box<DeviceFile>>,
        copies: Vec<T>,
    },
    /// A file that can't be read or checked now (half-synced): publish nothing
    /// this pass, and read it again on a later one (SP7).
    Wait,
    Blocked(Blocked),
    /// A budget ran out.
    Stopped,
}

/// SP37 (9)–(10). Nothing happens, and nothing is read, when there is nothing to
/// publish and the device file says what it said.
pub(super) fn publish<F: Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    naming: &[u8; 32],
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    devices: &[Listed<F::Token>],
) -> Result<(), Error> {
    let lib = device.lib;
    // This device's own device file as storage has it, once checked this pass.
    let mut checked = None;
    if anything_to_publish(device)? {
        let go = publish_waiting(
            device,
            folder,
            naming,
            meter,
            report,
            blobs,
            devices,
            &mut checked,
        )?;
        // Whatever stopped it, what is still waiting is said.
        report.unsent = journal::unsent(device.conn, &lib)?.len() as u64;
        if !go || report.stopped.is_some() {
            return Ok(());
        }
    }
    // SP37 (10): after the changes are in storage, never before.
    device_file_step(device, folder, meter, report, devices, checked)
}

/// A change signed and not yet in storage, one numbered and never signed, or a
/// local version not numbered yet.
fn anything_to_publish(device: &Device<'_>) -> Result<bool, Error> {
    let lib = device.lib;
    let last = device.store.library(&lib.id).ok_or(Error::NoIdentity)?.seq;
    Ok(!chain::pending(device.store, &lib.id)?.is_empty()
        || journal::numbered_unsigned(device.conn, &lib, last)?
        || !journal::unsent(device.conn, &lib)?.is_empty())
}

/// `false` when publication can't go on this pass: storage is ahead of this device,
/// its own device file can't be read yet, or a budget ran out.
#[allow(clippy::too_many_arguments)]
fn publish_waiting<F: Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    naming: &[u8; 32],
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    devices: &[Listed<F::Token>],
    checked: &mut Option<Option<Box<DeviceFile>>>,
) -> Result<bool, Error> {
    let lib = device.lib;
    // SP15 (1): this device's own files, as this pass found them, against its marks.
    if !settle_own(device, folder, devices, meter, report, true, checked)? {
        return Ok(false);
    }
    // SP33, SP34: this device's own reservations that lapsed, and holds that ended,
    // leave the ledger, so they no longer count against its limits.
    {
        let marks =
            frontier::stored_marks(device.store.library(&lib.id).ok_or(Error::NoIdentity)?)?;
        let held = |d: &DeviceId, s: u64| held_of(device.conn, &lib, d, s);
        let (none, mine) = (BTreeSet::new(), BTreeSet::from([lib.device]));
        let progress = Progress {
            accepted: &marks,
            held: &held,
            removed: &none,
            all_listed_read: &mine,
            late_decided: &none,
            choices: &[],
        };
        device
            .state
            .ledger
            .advance(&device.state.vouched, &progress);
        save_state(device)?;
    }
    // SP15 (4): changes signed on an earlier pass are uploaded again, never signed
    // again; then the rounds.
    let mut written = BTreeSet::new();
    for (path, bytes) in chain::pending(device.store, &lib.id)? {
        if !send(
            device,
            folder,
            naming,
            meter,
            report,
            blobs,
            &mut written,
            &path,
            &bytes,
        )? {
            return Ok(false);
        }
    }
    rounds(device, folder, naming, meter, report, blobs, &mut written)?;
    Ok(report.stopped.is_none())
}

/// SP15 (1), SP7: checks this device's own files, and removes the copies of its
/// device file that a conflict made, which are its own at or below its counter and
/// have nothing to merge. `false` when nothing may be written this pass; what it
/// found of the device file is kept in `checked`.
fn settle_own<F: Folder>(
    device: &Device<'_>,
    folder: &mut F,
    devices: &[Listed<F::Token>],
    meter: &Meter<'_>,
    report: &mut Report,
    changes: bool,
    checked: &mut Option<Option<Box<DeviceFile>>>,
) -> Result<bool, Error> {
    if checked.is_some() {
        return Ok(true);
    }
    match own_files(device, folder, devices, meter, report, changes)? {
        Own::Clear { file, copies } => {
            for token in copies {
                if let Some(stop) = meter.check(report) {
                    report.stopped = Some(stop);
                    return Ok(false);
                }
                folder.remove(&token)?;
            }
            *checked = Some(file);
            Ok(true)
        }
        Own::Blocked(why) => {
            report.blocked = Some(why);
            Ok(false)
        }
        Own::Wait | Own::Stopped => Ok(false),
    }
}

/// SP15 (1), SY7: lists this device's `changes/` and reads its device file (and any
/// copies of it). A change newer than the marks, a device file with a higher counter
/// or another writer token means the device store is behind storage or the ID was
/// copied: nothing is signed. A file that fails its check may be half-synced, so
/// the pass waits and reads it again.
fn own_files<F: Folder>(
    device: &Device<'_>,
    folder: &F,
    devices: &[Listed<F::Token>],
    meter: &Meter<'_>,
    report: &mut Report,
    changes: bool,
) -> Result<Own<F::Token>, Error> {
    let me = device.lib.device;
    let state = device
        .store
        .library(&device.lib.id)
        .ok_or(Error::NoIdentity)?;
    let (mark, counter, writer, key) = (
        state.seq,
        state.counter,
        state.writer_id()?,
        state.key()?.public(),
    );
    // Only a pass that numbers a change needs its own `changes/` listed.
    if changes {
        let Some(listing) = list_all(folder, &format!("changes/{me}"), meter, report)? else {
            return Ok(Own::Stopped);
        };
        let newest = listing
            .iter()
            .filter(|e| files::layout(&e.name).is_some())
            .filter_map(|e| {
                e.name
                    .rsplit('/')
                    .next()?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        if newest > mark {
            return Ok(Own::Blocked(Blocked::StorageAhead));
        }
    }
    let path = device_file::device_path(&me);
    let (mut file, mut copies) = (None, Vec::new());
    for entry in devices
        .iter()
        .filter(|e| e.name == path || files::candidate_of(&e.name).is_some_and(|of| of == path))
    {
        let copy = entry.name != path;
        let read = if copy {
            files::read_candidate(folder, entry)?
        } else {
            files::read_file(folder, entry)?
        };
        let Read::Bytes(bytes) = read else {
            report
                .set_aside
                .push((entry.name.clone(), "unreadable".into()));
            if copy {
                continue;
            }
            return Ok(Own::Wait);
        };
        report.bytes_read += bytes.len() as u64;
        if let Some(stop) = meter.check(report) {
            report.stopped = Some(stop);
            return Ok(Own::Stopped);
        }
        match device_file::open(&bytes, &device.lib.id, &me, &key) {
            Ok(f) if f.writer != writer => return Ok(Own::Blocked(Blocked::OtherWriter)),
            Ok(f) if f.counter > counter => return Ok(Own::Blocked(Blocked::StorageAhead)),
            Ok(_) if copy => copies.push(entry.token.clone()),
            Ok(f) => file = Some(Box::new(f)),
            Err(e) => {
                report.set_aside.push((entry.name.clone(), format!("{e}")));
                if !copy {
                    return Ok(Own::Wait);
                }
            }
        }
    }
    Ok(Own::Clear { file, copies })
}

/// SP33: the ledger takes a change this device signed, once. A change it has
/// already taken (a retry after a crash between signing and saving) changes nothing.
fn note_own(device: &mut Device<'_>, bytes: &[u8], naming: &[u8; 32]) -> Result<Change, Error> {
    let lib = device.lib;
    let key = device
        .store
        .library(&lib.id)
        .ok_or(Error::NoIdentity)?
        .key()?
        .public();
    let change = chain::open_change(bytes, &lib.id, &lib.device, &key)?;
    if device
        .state
        .ledger
        .last_seq(lib.device)
        .is_some_and(|last| last >= change.seq)
    {
        return Ok(change);
    }
    let named: Vec<_> = change
        .versions
        .iter()
        .filter_map(|v| {
            let body = Hash32::parse(v["body"]["hash"].as_str()?)?;
            Some(super::hold::Named {
                kind: Object::Blob,
                name: Hash32::parse(&blob_name(naming, &body))?,
                key: Key::parse(v["key"].as_str()?)?.0,
                version: Entry {
                    device: lib.device,
                    seq: change.seq,
                    hash: version_hash(v),
                },
            })
        })
        .collect();
    device
        .state
        .ledger
        .receive(&change, &named, false, &device.state.vouched)?;
    // PS5: `chain::publish_with` commits its own transaction and the device store
    // is outside the database, so the ledger is saved right after. A crash between
    // them is closed by the store's copy of the signed bytes: the next pass notes
    // the change again before it uploads it.
    save_state(device)?;
    Ok(change)
}

/// PS6, SP7: one write through the folder's temporary name and rename, counted
/// against the budgets. `false` when a budget had already run out, with nothing written.
fn put<F: Folder>(
    folder: &mut F,
    path: &str,
    bytes: &[u8],
    meter: &Meter<'_>,
    report: &mut Report,
) -> Result<bool, Error> {
    if let Some(stop) = meter.check(report) {
        report.stopped = Some(stop);
        return Ok(false);
    }
    files::write_file(folder, path, bytes)?;
    report.bytes_written += bytes.len() as u64;
    Ok(true)
}

/// SP15 (4), SP33: one signed change of this device, written to storage. The
/// ledger takes it first. Every body it names that storage lacks is written
/// before it, whatever put the change in this state (a retry, a store saved but
/// not uploaded): no change file ever names a body that was never written. `false`
/// when a budget stopped it.
#[allow(clippy::too_many_arguments)]
fn send<F: Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    naming: &[u8; 32],
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    written: &mut BTreeSet<String>,
    path: &str,
    bytes: &[u8],
) -> Result<bool, Error> {
    let change = note_own(device, bytes, naming)?;
    for version in &change.versions {
        let Some(hash) = version["body"]["hash"].as_str().and_then(Hash32::parse) else {
            continue;
        };
        let object = format!("blobs/{}", blob_name(naming, &hash));
        if !list_blobs(folder, meter, report, blobs)? {
            return Ok(false);
        }
        if is_there(blobs, written, &object) {
            continue;
        }
        let body = journal::body(device.conn, &hash)?.filter(|b| identity_hash(b) == hash);
        let Some(body) = body else {
            // Nothing to write; a peer waits for the body (SY28).
            report
                .set_aside
                .push((object, "its body is not kept here".into()));
            continue;
        };
        if !put(folder, &object, &body, meter, report)? {
            return Ok(false);
        }
        written.insert(object);
        report.published.blobs += 1;
    }
    if !put(folder, path, bytes, meter, report)? {
        return Ok(false);
    }
    chain::uploaded(device.ks, device.store, &device.lib.id, path)?;
    report.published.changes.push(change.seq);
    report.published.reserved += change.reserve.len() as u64;
    report.published.held += change.hold.len() as u64;
    Ok(true)
}

/// What a local version needs before the change that names it may be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// It names nothing in storage, or what it names is there to rely on.
    Ready,
    /// New content: a reservation first (SP33).
    Reserve,
    /// Content no live version names: a hold first (SP34).
    Hold,
    /// Reserved or held, and not in storage: the body is uploaded first.
    Upload,
    /// Held, and waiting for every device's frontier (SP34); or not built.
    Waiting,
}

struct Blob {
    name: String,
    hash: Hash32,
    size: u64,
}

struct Item {
    version: Version,
    class: Class,
    blob: Option<Blob>,
}

/// SP33: whether some live version names the body: a head of a record that
/// carries it. A deleted record's body, or one an edit replaced, is not live.
fn live_named(device: &Device<'_>, hash: &Hash32) -> Result<bool, Error> {
    let keys: Vec<String> = device
        .conn
        .prepare(
            "SELECT DISTINCT key FROM sync_versions
             WHERE library = ?1 AND seq IS NOT NULL AND applied = 1
               AND json_extract(raw, '$.body.hash') = ?2",
        )
        .map_err(journal::JournalError::from)?
        .query_map(
            rusqlite::params![device.lib.id.to_string(), hash.to_string()],
            |r| r.get(0),
        )
        .map_err(journal::JournalError::from)?
        .collect::<Result<_, _>>()
        .map_err(journal::JournalError::from)?;
    for key in keys {
        let Some((key, _)) = Key::parse(&key) else {
            continue;
        };
        let record = journal::record_of(device.conn, &device.lib, &key, journal::Which::Applied)?;
        if record
            .heads(&HashSet::new())
            .iter()
            .any(|p| matches!(&p.version.content, Content::Body { hash: h, .. } if h == hash))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// SP33: whether any version this device has numbered or received carries the body,
/// live or not: content it has named before.
fn known(device: &Device<'_>, hash: &Hash32) -> Result<bool, Error> {
    Ok(device
        .conn
        .query_row(
            "SELECT 1 FROM sync_versions WHERE library = ?1 AND seq IS NOT NULL
             AND json_extract(raw, '$.body.hash') = ?2 LIMIT 1",
            rusqlite::params![device.lib.id.to_string(), hash.to_string()],
            |_| Ok(()),
        )
        .map(|_| true)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            e => Err(e),
        })
        .map_err(journal::JournalError::from)?)
}

/// The change this device holds of `d` at `s`. `hold::included_by_all` reads only
/// its hash, so the other fields are not filled in.
fn held_of(
    conn: &rusqlite::Connection,
    lib: &journal::Library,
    d: &DeviceId,
    s: u64,
) -> Option<Held> {
    chain::held_hash(conn, lib, d, s)
        .ok()
        .flatten()
        .map(|h| Held {
            h,
            prev: None,
            vroot: h,
        })
}

/// SP33, SP34: each unsent version, and what it needs.
fn plan(
    device: &Device<'_>,
    naming: &[u8; 32],
    present: &dyn Fn(&str) -> bool,
) -> Result<Vec<Item>, Error> {
    let me = device.lib.device;
    let marks = frontier::stored_marks(
        device
            .store
            .library(&device.lib.id)
            .ok_or(Error::NoIdentity)?,
    )?;
    let held = |d: &DeviceId, s: u64| held_of(device.conn, &device.lib, d, s);
    let (none, mine) = (BTreeSet::new(), BTreeSet::from([me]));
    let progress = Progress {
        accepted: &marks,
        held: &held,
        removed: &none,
        all_listed_read: &mine,
        late_decided: &none,
        choices: &[],
    };
    let ledger = &device.state.ledger;
    let reserved: BTreeSet<Hash32> = ledger.open_reservations(me).map(|(_, r)| r.name).collect();
    let holds: Vec<_> = ledger.open_holds(me).collect();
    let mut out = Vec::new();
    for version in journal::unsent(device.conn, &device.lib)? {
        let (class, blob) = match &version.content {
            // Books arrive in Stage 4: a `bookFile` version, with its book, waits.
            _ if version.kind == Kind::BookFile => (Class::Waiting, None),
            Content::Body { hash, size } => {
                let name = blob_name(naming, hash);
                let key = Hash32::parse(&name);
                let there = present(&format!("blobs/{name}"));
                let hold = holds.iter().find(|(_, h)| {
                    Some(h.name) == key && h.hash == *hash && h.for_key == version.key
                });
                let class = if let Some((id, _)) = hold {
                    // SP34: named only once every device's frontier includes the
                    // hold and the content is in storage again.
                    if ledger.may_publish_naming(*id, there, &progress) {
                        Class::Ready
                    } else if there {
                        Class::Waiting
                    } else {
                        Class::Upload
                    }
                } else if key.is_some_and(|k| reserved.contains(&k)) {
                    if there {
                        Class::Ready
                    } else {
                        Class::Upload
                    }
                } else if live_named(device, hash)? {
                    // SP33: it may rely on what a live version names.
                    if there {
                        Class::Ready
                    } else {
                        Class::Reserve
                    }
                } else if there || known(device, hash)? {
                    // SP33, SP34: a stored object nothing live names, or content
                    // named before, is a Restore: a hold first.
                    Class::Hold
                } else {
                    Class::Reserve
                };
                (
                    class,
                    Some(Blob {
                        name,
                        hash: *hash,
                        size: *size,
                    }),
                )
            }
            Content::Deleted { .. } | Content::Inline(_) => (Class::Ready, None),
        };
        out.push(Item {
            version,
            class,
            blob,
        });
    }
    Ok(out)
}

/// SP33, SP34: the new `reserve` and `hold` lists that fit the limits.
fn lists(device: &Device<'_>, items: &[Item]) -> (Vec<Reserve>, Vec<Hold>) {
    let (mut reserve, mut hold): (Vec<Reserve>, Vec<Hold>) = (Vec::new(), Vec::new());
    let me = device.lib.device;
    for item in items {
        let (Some(blob), Some(name)) = (&item.blob, item.blob.as_ref().map(|b| &b.name)) else {
            continue;
        };
        let Some(name) = Hash32::parse(name) else {
            continue;
        };
        match item.class {
            Class::Reserve if !reserve.iter().any(|r| r.name == name) => {
                reserve.push(Reserve {
                    kind: Object::Blob,
                    name,
                    size: blob.size,
                });
                if device
                    .state
                    .ledger
                    .check_additions(me, &reserve, &hold)
                    .is_err()
                {
                    reserve.pop();
                }
            }
            Class::Hold
                if !hold
                    .iter()
                    .any(|h: &Hold| h.name == name && h.for_key == item.version.key) =>
            {
                hold.push(Hold {
                    kind: Object::Blob,
                    name,
                    hash: blob.hash,
                    size: blob.size,
                    for_key: item.version.key.clone(),
                });
                if device
                    .state
                    .ledger
                    .check_additions(me, &reserve, &hold)
                    .is_err()
                {
                    hold.pop();
                }
            }
            _ => {}
        }
    }
    (reserve, hold)
}

/// SP37 (9): rounds of reserve, upload, name, until nothing more can move.
fn rounds<F: Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    naming: &[u8; 32],
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
    written: &mut BTreeSet<String>,
) -> Result<bool, Error> {
    let lib = device.lib;
    loop {
        if let Some(stop) = meter.check(report) {
            report.stopped = Some(stop);
            return Ok(false);
        }
        let mut progress = false;
        if !anything_to_publish(device)? {
            return Ok(true);
        }
        // The listing says which bodies are in storage.
        if !list_blobs(folder, meter, report, blobs)? {
            return Ok(false);
        }
        // 2. Reservations and holds, with what is ready.
        let items = plan(device, naming, &|p| is_there(blobs, written, p))?;
        let (reserve, hold) = lists(device, &items);
        let withheld: BTreeSet<Key> = items
            .iter()
            .filter(|i| i.class != Class::Ready)
            .map(|i| i.version.key.clone())
            .collect();
        let signed = chain::publish_with(
            device.conn,
            &lib,
            device.ks,
            device.store,
            &reserve,
            &hold,
            &device.state.ledger,
            &withheld,
        )?;
        if let Some(signed) = signed {
            if !send(
                device,
                folder,
                naming,
                meter,
                report,
                blobs,
                written,
                &signed.path,
                &signed.bytes,
            )? {
                return Ok(false);
            }
            progress = true;
        }
        // 3. Bodies, each before the change that names it.
        let items = plan(device, naming, &|p| is_there(blobs, written, p))?;
        let mut seen = BTreeSet::new();
        for item in items.iter().filter(|i| i.class == Class::Upload) {
            let Some(blob) = &item.blob else { continue };
            if !seen.insert(blob.name.clone()) {
                continue;
            }
            let Some(body) = journal::body(device.conn, &blob.hash)? else {
                report.set_aside.push((
                    format!("blobs/{}", blob.name),
                    "its body is not kept here".into(),
                ));
                continue;
            };
            if identity_hash(&body) != blob.hash {
                report.set_aside.push((
                    format!("blobs/{}", blob.name),
                    "its body doesn't match its hash".into(),
                ));
                continue;
            }
            let path = format!("blobs/{}", blob.name);
            if !put(folder, &path, &body, meter, report)? {
                return Ok(false);
            }
            written.insert(path);
            report.published.blobs += 1;
            progress = true;
        }
        // 4. The naming change, now that its bodies are in storage.
        let items = plan(device, naming, &|p| is_there(blobs, written, p))?;
        let withheld: BTreeSet<Key> = items
            .iter()
            .filter(|i| i.class != Class::Ready)
            .map(|i| i.version.key.clone())
            .collect();
        if let Some(signed) = chain::publish_with(
            device.conn,
            &lib,
            device.ks,
            device.store,
            &[],
            &[],
            &device.state.ledger,
            &withheld,
        )? {
            if !send(
                device,
                folder,
                naming,
                meter,
                report,
                blobs,
                written,
                &signed.path,
                &signed.bytes,
            )? {
                return Ok(false);
            }
            progress = true;
        }
        if !progress {
            return Ok(true);
        }
    }
}

/// Whether `path` is in `blobs/` as this pass listed it, or written by this pass.
fn is_there<T>(blobs: &Blobs<T>, written: &BTreeSet<String>, path: &str) -> bool {
    blobs.as_ref().is_some_and(|b| b.contains_key(path)) || written.contains(path)
}

/// PS6: `blobs/` listed once a pass, shared with step (4).
fn list_blobs<F: Folder>(
    folder: &F,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Blobs<F::Token>,
) -> Result<bool, Error> {
    if blobs.is_none() {
        let Some(listed) = list_all(folder, "blobs", meter, report)? else {
            return Ok(false);
        };
        *blobs = Some(listed.into_iter().map(|e| (e.name.clone(), e)).collect());
    }
    Ok(true)
}

/// SP37 (10), SP16, SP15: this device's device file. A file signed on an earlier
/// pass and not yet in storage goes up as it was (parts first, then the file that
/// names them); otherwise a new one is signed, at the next counter and kept in
/// the device store, only when it would say something other than the file in
/// storage does, and uploaded. It names the newest change this device has in
/// storage, so it comes after step (9) and never before it.
fn device_file_step<F: Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    meter: &Meter<'_>,
    report: &mut Report,
    devices: &[Listed<F::Token>],
    mut checked: Option<Option<Box<DeviceFile>>>,
) -> Result<(), Error> {
    let lib = device.lib;
    if !chain::pending(device.store, &lib.id)?.is_empty() {
        return Ok(());
    }
    if device_file::pending(device.store, &lib.id)?.is_empty() {
        // SP15 (1), SP7: what storage has of this device's file, once.
        if !settle_own(device, folder, devices, meter, report, false, &mut checked)? {
            return Ok(());
        }
        let fields = device_fields(device, report)?;
        let same = checked
            .flatten()
            .is_some_and(|file| file.overflow.is_empty() && says_the_same(&fields, &file.fields));
        if same {
            // The ages and the signatures it promoted are kept all the same.
            save_state(device)?;
            return Ok(());
        }
        device_file::publish(device.ks, device.store, &lib.id, &fields)?;
        // PS5: the pending marks `ages::lists` moved are saved with the file's number.
        save_state(device)?;
    }
    for (path, bytes) in device_file::pending(device.store, &lib.id)? {
        if !put(folder, &path, &bytes, meter, report)? {
            return Ok(());
        }
        chain::uploaded(device.ks, device.store, &lib.id, &path)?;
    }
    report.published.device_file = device.store.library(&lib.id).map(|l| l.counter);
    Ok(())
}

/// Whether two device files carry the same words: everything but the clock, which
/// changes with every write, and the lists that are empty either way.
fn says_the_same(a: &Fields, b: &Fields) -> bool {
    let strip = |f: &Fields| {
        let mut f = f.clone();
        f.clock = (0, 0);
        f.lists.retain(|_, items| !items.is_empty());
        f
    };
    strip(a) == strip(b)
}

/// SP16: what this device's device file says now, from the pass's state.
fn device_fields(device: &mut Device<'_>, report: &Report) -> Result<Fields, Error> {
    use super::ids::b64url_decode;
    let lib = device.lib;
    let me = lib.device;
    let state = device.store.library(&lib.id).ok_or(Error::NoIdentity)?;
    // Its latest change is the newest it has in storage: all it signed are (the
    // caller checked that none is waiting).
    let own = state.last_hash()?.map(|h| (state.seq, h));
    let head = state
        .rest
        .get("head")
        .and_then(|v| v.as_str())
        .and_then(Hash32::parse);
    let marks = frontier::stored_marks(state)?;
    let (ms, n) = journal::clock_of(device.conn, &lib)?;
    // SP16: for every other device whose changes it has applied, the latest, with
    // that device's own signature. A signature saved before the change was applied
    // vouches for nothing until it has been.
    let mut applied: BTreeMap<DeviceId, (Mark, [u8; 64])> = BTreeMap::new();
    for peer in &report.trusted {
        if *peer == me {
            continue;
        }
        let seen = super::apply::seen(device.conn, &lib, *peer)?;
        let held = chain::held_hash(device.conn, &lib, peer, seen)?;
        let Some(sigs) = device.state.sigs.peers.get_mut(peer) else {
            continue;
        };
        if let Some(c) = sigs.candidate.clone() {
            if c.s == seen && Some(c.h) == held {
                sigs.confirmed = Some(c);
                sigs.candidate = None;
            }
        }
        // It may vouch for less than it has applied, never for more.
        if let Some(c) = &sigs.confirmed {
            let still = chain::held_hash(device.conn, &lib, peer, c.s)?;
            if c.s <= seen && still == Some(c.h) {
                if let Some(sig) = b64url_decode(&c.sig).and_then(|b| b.try_into().ok()) {
                    applied.insert(*peer, (Mark { s: c.s, h: c.h }, sig));
                }
            }
        }
    }
    let mut lists = BTreeMap::new();
    lists.insert("frontier".to_owned(), frontier::frontier_list(&applied));
    lists.insert("inherited".to_owned(), frontier::inherited_list(&marks));
    // SP35: ages and marks; an untrusted clock publishes none.
    let retiring = device.state.ages.retiring();
    lists.extend(super::ages::lists(
        &device.state.ages,
        &mut device.state.marks,
        &retiring,
        &device.state.vouched,
    ));
    Ok(Fields {
        name: device.name.to_owned(),
        clock: (ms as u64, n as u16),
        ahead: device.state.vouched.untrusted_wall,
        head,
        own,
        view: None,
        lists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::apply::tests::{highlight, note_of};
    use crate::sync::folder::View;
    use crate::sync::pass::{self, Budgets, Readings, State, Stopped};
    use crate::sync::testkit::{self, Kit, KitDevice, NAMING};
    use std::cell::Cell;
    use std::io;

    // Past the real clock, as in pass.rs's tests: SY10 holds a version whose clock
    // is ahead of the reader's.
    const READ: Readings = Readings {
        mono_ms: 1_000,
        wall_ms: 1_900_000_000_000,
        clock_ahead: false,
        same_boot: false,
    };

    /// A device's folder that records every write, and can fail one: `crash` is
    /// `(n, after)`, the `n`th write (from 0) fails, having been made first when
    /// `after` is set (the process died after the rename, before it was noted).
    struct Log<F: Folder> {
        inner: F,
        writes: Vec<String>,
        crash: Option<(usize, bool)>,
    }

    impl<F: Folder> Folder for Log<F> {
        type Token = F::Token;
        fn list(
            &self,
            dir: &str,
            cursor: Option<&str>,
        ) -> io::Result<super::super::folder::Page<F::Token>> {
            self.inner.list(dir, cursor)
        }
        fn stat(&self, token: &F::Token) -> io::Result<Option<u64>> {
            self.inner.stat(token)
        }
        fn read(
            &self,
            token: &F::Token,
            limit: Option<u64>,
        ) -> io::Result<super::super::folder::Read> {
            self.inner.read(token, limit)
        }
        fn write(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
            let n = self.writes.len();
            match self.crash {
                Some((at, false)) if at == n => return Err(io::Error::other("injected crash")),
                Some((at, true)) if at == n => {
                    self.inner.write(path, bytes)?;
                    self.writes.push(path.to_owned());
                    return Err(io::Error::other("injected crash"));
                }
                _ => {}
            }
            self.inner.write(path, bytes)?;
            self.writes.push(path.to_owned());
            Ok(())
        }
        fn remove(&mut self, token: &F::Token) -> io::Result<()> {
            self.inner.remove(token)
        }
    }

    fn log_of(kit: &Kit, i: usize) -> Log<View> {
        Log {
            inner: kit.folder.view(&kit.devices[i].lib.device.to_string()),
            writes: Vec::new(),
            crash: None,
        }
    }

    fn run_on<F: Folder>(
        kd: &mut KitDevice,
        folder: &mut F,
        budgets: &Budgets,
        now: &dyn Fn() -> u64,
    ) -> Result<pass::Report, pass::Error> {
        run_at(kd, folder, READ, budgets, now)
    }

    fn run_at<F: Folder>(
        kd: &mut KitDevice,
        folder: &mut F,
        readings: Readings,
        budgets: &Budgets,
        now: &dyn Fn() -> u64,
    ) -> Result<pass::Report, pass::Error> {
        let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
        let mut device = pass::Device {
            conn: kd.db.conn(),
            lib: kd.lib,
            store: &mut kd.store,
            ks: &kd.ks,
            state,
            name: "Test Mac",
        };
        pass::run(&mut device, folder, readings, budgets, now)
    }

    /// One whole pass of device `i` with every budget at SY2's value.
    fn go(kit: &mut Kit, i: usize) -> pass::Report {
        let mut log = log_of(kit, i);
        run_on(&mut kit.devices[i], &mut log, &Budgets::sy2(), &|| 0).unwrap()
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

    /// Device `i`'s change files in storage, in sequence order.
    fn changes_of(kit: &Kit, i: usize) -> Vec<Change> {
        let kd = &kit.devices[i];
        let mut names: Vec<String> = kd
            .folder
            .list(&format!("changes/{}", kd.lib.device), None)
            .unwrap()
            .entries
            .into_iter()
            .map(|e| e.name)
            .filter(|n| files::layout(n).is_some())
            .collect();
        names.sort();
        names
            .iter()
            .map(|n| {
                chain::open_change(
                    &get(kd, n).unwrap(),
                    &kd.lib.id,
                    &kd.lib.device,
                    &kd.key.public(),
                )
                .unwrap()
            })
            .collect()
    }

    fn blob_path(body: &str) -> String {
        format!(
            "blobs/{}",
            blob_name(&NAMING, &Hash32::parse(body).unwrap())
        )
    }

    /// SP14, SP15, SP33: device `i`'s chain has no gap and no repeat, each body a
    /// change names is in storage, and no version is published twice. How many
    /// changes it takes depends on where a stop fell, so the caller counts them.
    fn assert_whole(kit: &Kit, i: usize) -> Vec<Change> {
        let changes = changes_of(kit, i);
        let mut keys = BTreeSet::new();
        for (n, c) in changes.iter().enumerate() {
            assert_eq!(c.seq, n as u64 + 1, "a gap or a repeat");
            assert_eq!(c.prev, n.checked_sub(1).map(|p| changes[p].hash));
            for v in &c.versions {
                assert!(keys.insert(version_hash(v)), "a version published twice");
                if let Some(h) = v["body"]["hash"].as_str() {
                    assert!(get(&kit.devices[i], &blob_path(h)).is_some(), "no body");
                }
            }
        }
        changes
    }

    fn with_note(kd: &mut KitDevice) {
        kd.db
            .save_annotation(&highlight("book", Some("Call me"), 1_760_000_001_000))
            .unwrap();
    }

    #[test]
    fn a_device_publishes_its_changes_and_a_peer_reads_them() {
        // SP37 (9), SP33, SP15: a reservation, then the bodies, then the change.
        let mut kit = testkit::library(2);
        let (a, b) = (1, 0);
        with_note(&mut kit.devices[a]);
        let me = kit.devices[a].lib.device;
        let mut log = log_of(&kit, a);
        let report = run_on(&mut kit.devices[a], &mut log, &Budgets::sy2(), &|| 0).unwrap();
        assert_eq!(report.published.changes, vec![1, 2], "{report:?}");
        assert_eq!(
            (report.published.reserved, report.published.blobs),
            (2, 2),
            "{report:?}"
        );
        assert_eq!(report.unsent, 0);
        // The order of the writes: the reservation, both bodies, the change naming them.
        let w = &log.writes;
        assert_eq!(w.len(), 5, "{w:?}");
        assert_eq!(w[0], chain::change_path(&me, 1));
        assert!(w[1].starts_with("blobs/") && w[2].starts_with("blobs/"));
        assert_eq!(w[3], chain::change_path(&me, 2));
        // SP37 (10): the device file, after the changes it names.
        assert_eq!(w[4], device_file::device_path(&me));
        let changes = assert_whole(&kit, a);
        assert!(changes[0].versions.is_empty() && changes[0].reserve.len() == 2);
        assert!(changes[1].versions.len() == 2 && changes[1].reserve.is_empty());
        // Named by the keyed name (SP9), which the reservation carries.
        for r in &changes[0].reserve {
            assert!(w.contains(&format!("blobs/{}", r["name"].as_str().unwrap())));
        }
        // The ledger took the reservations and dropped them when named (SP33).
        let state = State::load(kit.devices[a].db.conn(), &kit.devices[a].lib.id).unwrap();
        assert_eq!(state.ledger.reservation_count(me), 0);
        assert_eq!(state.ledger.last_seq(me), Some(2));
        // A peer reads them (SP37 (4)).
        let report = go(&mut kit, b);
        assert_eq!(report.received, vec![(me, 2)], "{report:?}");
        assert_eq!(
            note_of(&kit.devices[b].db, "book"),
            Some(Some("Call me".into()))
        );
    }

    #[test]
    fn a_second_pass_with_nothing_pending_writes_nothing() {
        let mut kit = testkit::library(2);
        with_note(&mut kit.devices[1]);
        go(&mut kit, 1);
        let mut log = log_of(&kit, 1);
        let report = run_on(&mut kit.devices[1], &mut log, &Budgets::sy2(), &|| 0).unwrap();
        assert!(log.writes.is_empty(), "{:?}", log.writes);
        assert_eq!(report.bytes_written, 0);
        assert_eq!(report.published, pass::Published::default());
    }

    #[test]
    fn reservations_past_the_limit_go_in_the_next_round() {
        // SP33: at most 20 reservations open at once; the rest follow, after the
        // first twenty are named.
        let mut kit = testkit::library(2);
        for i in 0..25u32 {
            let mut row = highlight("book", None, 1_760_000_001_000);
            row.id = format!("{i:08x}-0000-4000-8000-000000000000");
            kit.devices[1].db.save_annotation(&row).unwrap();
        }
        let report = go(&mut kit, 1);
        assert_eq!(report.published.changes, vec![1, 2, 3, 4], "{report:?}");
        assert_eq!(report.unsent, 0);
        let changes = assert_whole(&kit, 1);
        let sizes: Vec<_> = changes
            .iter()
            .map(|c| (c.reserve.len(), c.versions.len()))
            .collect();
        assert_eq!(sizes, vec![(20, 0), (0, 20), (5, 0), (0, 5)]);
    }

    #[test]
    fn every_stop_between_two_writes_is_finished_by_the_next_pass() {
        // SP15, SP33, PS5: a failure before or after each write of a pass (a rename
        // that failed, or a crash before it was noted). The next pass completes
        // the publication with no gap and no repeat, and a peer that reads in
        // between applies nothing wrong.
        for after in [false, true] {
            for at in 0..5 {
                let mut kit = testkit::library(2);
                let (a, b) = (1, 0);
                with_note(&mut kit.devices[a]);
                let me = kit.devices[a].lib.device;
                let mut log = log_of(&kit, a);
                log.crash = Some((at, after));
                let first = run_on(&mut kit.devices[a], &mut log, &Budgets::sy2(), &|| 0);
                assert!(first.is_err(), "write {at} (after: {after}) must fail");
                // A peer reads whatever storage holds now.
                go(&mut kit, b);
                let named = get(&kit.devices[a], &chain::change_path(&me, 2)).is_some();
                // It applies the note only when the change naming it is in storage (and
                // the device file naming that change is too).
                let applied = note_of(&kit.devices[b].db, "book").is_some();
                assert!(
                    !applied || named,
                    "a peer applied a note whose change isn't in storage ({at}, {after})"
                );
                let device_file_there = get(&kit.devices[a], &device_file::device_path(&me))
                    .and_then(|b| {
                        device_file::open(
                            &b,
                            &kit.devices[a].lib.id,
                            &me,
                            &kit.devices[a].key.public(),
                        )
                        .ok()
                    })
                    .is_some_and(|f| f.fields.own.is_some_and(|(s, _)| s == 2));
                assert_eq!(applied, named && device_file_there, "({at}, {after})");
                // The next pass finishes it.
                let report = go(&mut kit, a);
                assert_eq!(report.unsent, 0, "({at}, {after}) {report:?}");
                let changes = assert_whole(&kit, a);
                let versions: usize = changes.iter().map(|c| c.versions.len()).sum();
                assert_eq!(versions, 2, "({at}, {after})");
                let file = device_file_of(&kit, a);
                assert_eq!(file.counter, 2, "one counter, signed once ({at}, {after})");
                assert_eq!(
                    file.fields.own.map(|(s, _)| s),
                    Some(changes.last().unwrap().seq)
                );
                go(&mut kit, b);
                assert_eq!(
                    note_of(&kit.devices[b].db, "book"),
                    Some(Some("Call me".into())),
                    "({at}, {after})"
                );
            }
        }
    }

    #[test]
    fn a_budget_that_runs_out_anywhere_in_step_nine_is_taken_up_by_the_next_pass() {
        // PS6: bytes (reads and writes together) and time, stopped after every
        // possible amount of work.
        let mut stopped_in_nine = 0;
        for limit in 0..400u64 {
            let mut kit = testkit::library(2);
            with_note(&mut kit.devices[1]);
            let mut budgets = Budgets::sy2();
            budgets.bytes = limit * 40;
            let calls = Cell::new(0u64);
            let clock = || {
                calls.set(calls.get() + 1);
                if limit % 2 == 1 && calls.get() > limit / 2 + 20 {
                    120_000
                } else {
                    0
                }
            };
            let mut log = log_of(&kit, 1);
            let first = run_on(&mut kit.devices[1], &mut log, &budgets, &clock).unwrap();
            if first.stopped.is_some() && !log.writes.is_empty() {
                stopped_in_nine += 1;
            }
            let report = go(&mut kit, 1);
            assert_eq!(report.stopped, None);
            assert_eq!(report.unsent, 0, "limit {limit}: {first:?}");
            let versions: usize = assert_whole(&kit, 1).iter().map(|c| c.versions.len()).sum();
            assert_eq!(versions, 2, "limit {limit}");
        }
        assert!(stopped_in_nine > 10, "the sweep reached step nine's writes");
    }

    #[test]
    fn the_name_budget_stops_step_nine_at_the_blob_listing() {
        // PS6: listing blobs/ counts names. The first pass publishes; a second edit
        // is then listed against a budget that has room for everything before it.
        let mut kit = testkit::library(2);
        with_note(&mut kit.devices[1]);
        go(&mut kit, 1);
        let idle = go(&mut kit, 1).names_listed;
        let own = changes_of(&kit, 1).len() as u64;
        kit.devices[1]
            .db
            .save_position("book", "epubcfi(/6/4!/4/2/1:0)", 0.3, Some("One"))
            .unwrap();
        let mut budgets = Budgets::sy2();
        budgets.names = idle + own;
        let mut log = log_of(&kit, 1);
        let report = run_on(&mut kit.devices[1], &mut log, &budgets, &|| 0).unwrap();
        assert_eq!(report.stopped, Some(Stopped::Names), "{report:?}");
        assert!(log.writes.is_empty(), "nothing was written");
        assert!(report.unsent > 0);
        let report = go(&mut kit, 1);
        assert_eq!(report.unsent, 0);
        assert_eq!(assert_whole(&kit, 1).len(), 4);
    }

    #[test]
    fn a_rename_that_fails_leaves_the_signed_change_to_be_uploaded_as_it_was() {
        // PS2, SP7, SP15: the change is signed once, kept in the device store, and
        // uploaded again after the failed rename: the same bytes, the same sequence.
        let mut kit = testkit::library(2);
        with_note(&mut kit.devices[1]);
        let me = kit.devices[1].lib.device;
        let path = chain::change_path(&me, 1);
        kit.folder.fail_rename(&path);
        let mut log = log_of(&kit, 1);
        let err = run_on(&mut kit.devices[1], &mut log, &Budgets::sy2(), &|| 0);
        assert!(err.is_err());
        let kept = chain::pending(&kit.devices[1].store, &kit.devices[1].lib.id).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, path);
        go(&mut kit, 1);
        assert_eq!(get(&kit.devices[1], &path).unwrap(), kept[0].1);
        assert_eq!(assert_whole(&kit, 1).len(), 2);
    }

    #[test]
    fn a_half_synced_device_file_of_its_own_holds_publication_back() {
        // PS2, SP7: it can't be told from garbage, so nothing is numbered this pass.
        let mut kit = testkit::library(2);
        with_note(&mut kit.devices[1]);
        let me = kit.devices[1].lib.device;
        kit.folder
            .half_synced(&me.to_string(), &device_file::device_path(&me), b"junk", 1);
        let mut log = log_of(&kit, 1);
        let report = run_on(&mut kit.devices[1], &mut log, &Budgets::sy2(), &|| 0).unwrap();
        assert!(log.writes.is_empty());
        assert!(report.published.changes.is_empty());
        assert_eq!(report.unsent, 2, "{report:?}");
        assert_eq!(report.blocked, None);
        go(&mut kit, 1);
        assert_eq!(assert_whole(&kit, 1).len(), 2);
    }

    fn own_device_file_copy(kit: &mut Kit, counter_up: bool) {
        // A second view writing this device's file in the same instant: the
        // storage keeps both, one under a name it made (PS2).
        let me = kit.devices[1].lib.device;
        let path = device_file::device_path(&me);
        let mut bytes = get(&kit.devices[1], &path).unwrap();
        if counter_up {
            let state = kit.devices[1]
                .store
                .library(&kit.devices[1].lib.id)
                .unwrap();
            let fields = device_file::Fields::default();
            bytes = device_file::write(
                &state.key().unwrap(),
                &kit.devices[1].lib.id,
                &state.writer_id().unwrap(),
                9,
                &fields,
            )
            .unwrap()
            .bytes;
        }
        kit.folder.conflict_copies(true);
        files::write_file(&mut kit.devices[1].folder, &path, &bytes).unwrap();
        files::write_file(&mut kit.devices[0].folder, &path, &bytes).unwrap();
        kit.folder.conflict_copies(false);
    }

    #[test]
    fn a_conflict_copy_of_its_own_device_file_is_removed() {
        // SP7: a copy of a single-writer file is checked as that writer's, merged
        // and removed; this one is at the counter it has, so there is nothing to merge.
        let mut kit = testkit::library(2);
        own_device_file_copy(&mut kit, false);
        let me = kit.devices[1].lib.device;
        let copy = format!("devices/{me} 2.json");
        let listed = |kit: &Kit| {
            kit.devices[1]
                .folder
                .list("devices", None)
                .unwrap()
                .entries
                .iter()
                .any(|e| e.name == copy)
        };
        assert!(listed(&kit), "the storage made a copy");
        with_note(&mut kit.devices[1]);
        go(&mut kit, 1);
        assert!(!listed(&kit), "the copy was removed");
        assert_eq!(assert_whole(&kit, 1).len(), 2);
    }

    #[test]
    fn a_copy_of_its_own_device_file_with_a_higher_counter_blocks_publication() {
        // SY7, SP15 (1): the device store is behind storage.
        let mut kit = testkit::library(2);
        own_device_file_copy(&mut kit, true);
        with_note(&mut kit.devices[1]);
        let report = go(&mut kit, 1);
        assert_eq!(report.blocked, Some(Blocked::StorageAhead));
        assert!(report.published.changes.is_empty());
        assert!(changes_of(&kit, 1).is_empty());
        assert_eq!(report.unsent, 2);
    }

    #[test]
    fn a_change_of_its_own_newer_than_its_marks_blocks_publication() {
        // SY7, SP15 (1): storage holds a change this device never signed here.
        let mut kit = testkit::library(2);
        let me = kit.devices[1].lib.device;
        files::write_file(
            &mut kit.devices[1].folder,
            &chain::change_path(&me, 5),
            b"from a restored copy",
        )
        .unwrap();
        with_note(&mut kit.devices[1]);
        let report = go(&mut kit, 1);
        assert_eq!(report.blocked, Some(Blocked::StorageAhead));
        assert!(report.published.changes.is_empty());
        assert_eq!(
            kit.devices[1]
                .store
                .library(&kit.devices[1].lib.id)
                .unwrap()
                .seq,
            0
        );
    }

    #[test]
    fn a_device_file_with_another_writer_token_blocks_publication() {
        // SY7: its ID in storage with another token: an ID copied some other way.
        let mut kit = testkit::library(2);
        let me = kit.devices[1].lib.device;
        let state = kit.devices[1]
            .store
            .library(&kit.devices[1].lib.id)
            .unwrap();
        let stranger = crate::sync::ids::Id16([9; 16]);
        let written = device_file::write(
            &state.key().unwrap(),
            &kit.devices[1].lib.id,
            &stranger,
            1,
            &device_file::Fields::default(),
        )
        .unwrap();
        files::write_file(
            &mut kit.devices[1].folder,
            &device_file::device_path(&me),
            &written.bytes,
        )
        .unwrap();
        with_note(&mut kit.devices[1]);
        let report = go(&mut kit, 1);
        assert_eq!(report.blocked, Some(Blocked::OtherWriter));
        assert!(changes_of(&kit, 1).is_empty());
    }

    #[test]
    fn a_change_numbered_but_never_signed_is_signed_again_from_the_same_rows() {
        // SP15: a crash between numbering and signing leaves rows with a sequence
        // and no unsent version; the pass still has that change to publish.
        let mut kit = testkit::library(2);
        let kd = &mut kit.devices[1];
        kd.db
            .save_annotation(&highlight("book", None, 1_760_000_001_000))
            .unwrap();
        let numbered = journal::take_unsent_within_with(
            kd.db.conn(),
            &kd.lib,
            0,
            usize::MAX,
            u64::MAX,
            &[],
            &[],
            &BTreeSet::new(),
            &super::super::hold::Ledger::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(numbered.seq, 1);
        assert!(journal::unsent(kd.db.conn(), &kd.lib).unwrap().is_empty());
        let me = kit.devices[1].lib.device;
        let mut log = log_of(&kit, 1);
        let report = run_on(&mut kit.devices[1], &mut log, &Budgets::sy2(), &|| 0).unwrap();
        assert_eq!(report.published.changes, vec![1], "{report:?}");
        // Whatever numbered it, its body is in storage before the file that names it.
        assert_eq!(log.writes.len(), 3, "{:?}", log.writes);
        assert!(log.writes[0].starts_with("blobs/"));
        assert_eq!(log.writes[1], chain::change_path(&me, 1));
        assert_eq!(log.writes[2], device_file::device_path(&me));
        assert_eq!(assert_whole(&kit, 1)[0].versions.len(), 1);
    }

    #[test]
    fn a_change_signed_but_not_noted_is_noted_before_it_is_uploaded() {
        // SP15, SP33, PS5: signed and kept in the device store, then a crash before
        // the ledger was saved. The pass notes the reservation, uploads the change,
        // and does not reserve the same body again.
        let mut kit = testkit::library(2);
        with_note(&mut kit.devices[1]);
        let kd = &mut kit.devices[1];
        let versions = journal::unsent(kd.db.conn(), &kd.lib).unwrap();
        let reserve: Vec<Reserve> = versions
            .iter()
            .map(|v| match &v.content {
                Content::Body { hash, size } => Reserve {
                    kind: Object::Blob,
                    name: Hash32::parse(&blob_name(&NAMING, hash)).unwrap(),
                    size: *size,
                },
                _ => unreachable!(),
            })
            .collect();
        let withheld: BTreeSet<Key> = versions.iter().map(|v| v.key.clone()).collect();
        let signed = chain::publish_with(
            kd.db.conn(),
            &kd.lib,
            &kd.ks,
            &mut kd.store,
            &reserve,
            &[],
            &super::super::hold::Ledger::default(),
            &withheld,
        )
        .unwrap()
        .unwrap();
        assert_eq!(signed.seq, 1);
        let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
        assert_eq!(
            state.ledger.reservation_count(kd.lib.device),
            0,
            "not noted"
        );
        let report = go(&mut kit, 1);
        assert_eq!(report.unsent, 0, "{report:?}");
        let changes = assert_whole(&kit, 1);
        assert_eq!(changes.len(), 2, "one reservation, one naming change");
        assert_eq!(changes[0].reserve.len(), 2);
        assert!(changes[1].reserve.is_empty());
    }

    #[test]
    fn a_reservation_nothing_named_lapses_after_a_week() {
        // SP33: it counted against the 20 until then; 7 days on, by the time this
        // device can vouch for, it is gone and the room is free.
        let mut kit = testkit::library(2);
        let kd = &mut kit.devices[1];
        let me = kd.lib.device;
        let orphan = Reserve {
            kind: Object::Blob,
            name: Hash32([4; 32]),
            size: 10,
        };
        chain::publish_with(
            kd.db.conn(),
            &kd.lib,
            &kd.ks,
            &mut kd.store,
            &[orphan],
            &[],
            &super::super::hold::Ledger::default(),
            &BTreeSet::new(),
        )
        .unwrap()
        .unwrap();
        // The first pass notes and uploads it; the same pass also has the note.
        with_note(&mut kit.devices[1]);
        go(&mut kit, 1);
        let open = |kit: &Kit| {
            State::load(kit.devices[1].db.conn(), &kit.devices[1].lib.id)
                .unwrap()
                .ledger
                .reservation_count(me)
        };
        assert_eq!(open(&kit), 1, "still open the first day");
        kit.devices[1]
            .db
            .save_position("book", "epubcfi(/6/4!/4/2/1:0)", 0.3, Some("One"))
            .unwrap();
        let later = Readings {
            mono_ms: READ.mono_ms + 8 * 24 * 3600 * 1000,
            wall_ms: READ.wall_ms + 8 * 24 * 3600 * 1000,
            ..READ
        };
        let mut log = log_of(&kit, 1);
        run_at(&mut kit.devices[1], &mut log, later, &Budgets::sy2(), &|| 0).unwrap();
        assert_eq!(open(&kit), 0, "lapsed after a week");
    }

    #[test]
    fn a_body_a_live_version_names_that_storage_lost_is_reserved_and_sent_again() {
        // SP33: it may rely on a stored object a live version names, but not on one
        // that isn't stored: the same place saved again, after the body vanished.
        let mut kit = testkit::library(2);
        let a = 1;
        let save = |kit: &mut Kit| {
            kit.devices[a]
                .db
                .save_position("book", "epubcfi(/6/4!/4/2/1:0)", 0.3, Some("One"))
                .unwrap()
        };
        save(&mut kit);
        go(&mut kit, a);
        let body = crate::sync::local::place_body("epubcfi(/6/4!/4/2/1:0)", 0.3, Some("One"));
        let path = format!("blobs/{}", blob_name(&NAMING, &identity_hash(&body)));
        assert!(get(&kit.devices[a], &path).is_some());
        kit.folder.vanish(&path);
        save(&mut kit);
        let report = go(&mut kit, a);
        assert_eq!(report.unsent, 0, "{report:?}");
        assert!(get(&kit.devices[a], &path).is_some(), "sent again");
        assert_eq!(report.published.reserved, 1, "{report:?}");
        assert_whole(&kit, a);
    }

    /// Device `i`'s device file as storage has it.
    fn device_file_of(kit: &Kit, i: usize) -> DeviceFile {
        let kd = &kit.devices[i];
        let bytes = get(kd, &device_file::device_path(&kd.lib.device)).unwrap();
        device_file::open(&bytes, &kd.lib.id, &kd.lib.device, &kd.key.public()).unwrap()
    }

    #[test]
    fn the_device_file_says_what_sp16_requires_and_a_peer_accepts_it() {
        // SP37 (10), SP16, SP29: the latest change in storage, the head, the name,
        // the frontier with each peer's own signature on it.
        let mut kit = testkit::library(2);
        let (a, b) = (1, 0);
        let (da, db) = (kit.devices[a].lib.device, kit.devices[b].lib.device);
        with_note(&mut kit.devices[a]);
        let report = go(&mut kit, a);
        assert_eq!(report.published.device_file, Some(2), "{report:?}");
        let changes = assert_whole(&kit, a);
        let file = device_file_of(&kit, a);
        assert_eq!(file.counter, 2, "the kit wrote counter 1");
        let last = changes.last().unwrap();
        assert_eq!(file.fields.own, Some((last.seq, last.hash)));
        assert_eq!(file.fields.name, "Test Mac");
        assert_eq!(file.fields.view, None);
        let head = kit.devices[a]
            .store
            .library(&kit.devices[a].lib.id)
            .unwrap()
            .rest["head"]
            .as_str()
            .and_then(Hash32::parse);
        assert_eq!(file.fields.head, head);
        // A peer accepts it (step (3)) and reads the changes it names (step (4)).
        let report = go(&mut kit, b);
        assert_eq!(report.accepted, vec![da], "{report:?}");
        assert_eq!(report.received, vec![(da, 2)]);
        // Its own file now vouches for A's change, with A's signature.
        assert_eq!(go(&mut kit, b).published.device_file, None, "nothing new");
        let mut again = go(&mut kit, b);
        assert!(again.published.device_file.is_none(), "{again:?}");
        let theirs = device_file_of(&kit, b);
        let entries = frontier::entries(&theirs.fields.lists["frontier"], true);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            (entries[0].d, entries[0].s, entries[0].h),
            (da, 2, last.hash)
        );
        let kd = &kit.devices[a];
        let held = |d: &DeviceId, s: u64| {
            (*d == da && s == last.seq).then_some(Held {
                h: last.hash,
                prev: last.prev,
                vroot: last.vroot,
            })
        };
        let resolved = frontier::resolve(&entries[0], &kd.lib.id, Some(&kd.key.public()), &held);
        assert_eq!(resolved, frontier::Resolution::Resolved);
        // And A accepts B's file.
        again = go(&mut kit, a);
        assert_eq!(again.accepted, vec![db], "{again:?}");
    }

    #[test]
    fn a_signature_saved_before_its_change_is_applied_vouches_for_nothing() {
        // SP16, SP29: a frontier entry never claims more than was applied.
        let mut kit = testkit::library(2);
        let (a, b) = (1, 0);
        let (da, db) = (kit.devices[a].lib.device, kit.devices[b].lib.device);
        with_note(&mut kit.devices[a]);
        go(&mut kit, a);
        let last_hash = changes_of(&kit, a).last().unwrap().hash;
        // B has stopped receiving from A (SY28), so it applies none of it.
        let kd = &kit.devices[b];
        crate::sync::apply::stop(kd.db.conn(), &kd.lib, da, "test", 1).unwrap();
        let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
        state.sigs.peers.entry(da).or_default().candidate = Some(pass::SignedMark {
            s: 2,
            h: last_hash,
            sig: crate::sync::ids::b64url_encode(&[7u8; 64]),
        });
        let tx = kd.db.conn().unchecked_transaction().unwrap();
        state.save(&tx, &kd.lib.id).unwrap();
        tx.commit().unwrap();
        go(&mut kit, b);
        let file = device_file_of(&kit, b);
        assert!(file.fields.lists["frontier"].is_empty(), "{file:?}");
        assert_eq!(db, file.device);
    }

    #[test]
    fn the_device_file_is_signed_once_and_uploaded_again_after_a_failed_rename() {
        // PS2, SP15: counters follow the same order as change sequences: signed and
        // kept in the device store, then uploaded; a failure leaves those bytes to go.
        let mut kit = testkit::library(2);
        with_note(&mut kit.devices[1]);
        let me = kit.devices[1].lib.device;
        let path = device_file::device_path(&me);
        kit.folder.fail_rename(&path);
        let mut log = log_of(&kit, 1);
        assert!(run_on(&mut kit.devices[1], &mut log, &Budgets::sy2(), &|| 0).is_err());
        let kept = device_file::pending(&kit.devices[1].store, &kit.devices[1].lib.id).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, path);
        let report = go(&mut kit, 1);
        assert_eq!(report.published.device_file, Some(2));
        assert_eq!(get(&kit.devices[1], &path).unwrap(), kept[0].1);
        assert_eq!(device_file_of(&kit, 1).counter, 2, "no second counter");
    }

    #[test]
    fn a_half_synced_device_file_of_its_own_delays_the_next_one() {
        // PS2, SP7: with nothing else to publish, the device file waits too.
        let mut kit = testkit::library(2);
        let me = kit.devices[1].lib.device;
        let path = device_file::device_path(&me);
        kit.folder.half_synced(&me.to_string(), &path, b"junk", 1);
        let mut log = log_of(&kit, 1);
        let report = run_on(&mut kit.devices[1], &mut log, &Budgets::sy2(), &|| 0).unwrap();
        assert!(log.writes.is_empty(), "{:?}", log.writes);
        assert_eq!(report.published.device_file, None);
        assert!(!report.set_aside.is_empty());
        let report = go(&mut kit, 1);
        assert_eq!(report.published.device_file, Some(2));
    }

    #[test]
    fn a_conflict_copy_of_its_own_device_file_is_removed_even_when_nothing_else_is_published() {
        let mut kit = testkit::library(2);
        own_device_file_copy(&mut kit, false);
        let me = kit.devices[1].lib.device;
        let copy = format!("devices/{me} 2.json");
        go(&mut kit, 1);
        let there = kit.devices[1]
            .folder
            .list("devices", None)
            .unwrap()
            .entries
            .iter()
            .any(|e| e.name == copy);
        assert!(!there);
    }

    #[test]
    fn ages_are_published_and_a_device_running_ahead_publishes_none() {
        // SP35, SY17: a deletion's age goes in the device file; marked ahead, it says
        // so and sends no ages.
        let mut kit = testkit::library(2);
        let kd = &mut kit.devices[1];
        let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
        let e = Entry {
            device: Hash32([3; 32]).0[..16]
                .try_into()
                .map(crate::sync::ids::Id16)
                .unwrap(),
            seq: 4,
            hash: Hash32([4; 32]),
        };
        state.ages.saw(e, 0);
        let tx = kd.db.conn().unchecked_transaction().unwrap();
        state.save(&tx, &kd.lib.id).unwrap();
        tx.commit().unwrap();
        go(&mut kit, 1);
        let file = device_file_of(&kit, 1);
        assert_eq!(file.fields.lists["ages"].len(), 1, "{file:?}");
        assert!(!file.fields.ahead);
        let ahead = Readings {
            clock_ahead: true,
            mono_ms: READ.mono_ms + 1_000,
            wall_ms: READ.wall_ms + 1_000,
            ..READ
        };
        let mut log = log_of(&kit, 1);
        run_at(&mut kit.devices[1], &mut log, ahead, &Budgets::sy2(), &|| 0).unwrap();
        let file = device_file_of(&kit, 1);
        assert!(file.fields.ahead);
        assert!(file.fields.lists.get("ages").is_none_or(|a| a.is_empty()));
    }

    #[test]
    fn a_book_file_version_waits_for_books() {
        // SP37 (7): book files are fetched and sent in their own steps (Stage 4);
        // a bookFile version is not published, and the rest of the pass is.
        let mut kit = testkit::library(2);
        let kd = &mut kit.devices[1];
        let key = Key::parse("bookFile:7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6")
            .unwrap()
            .0;
        journal::record_local(
            kd.db.conn(),
            &kd.lib,
            &key,
            journal::LocalContent::Body(b"{}".to_vec()),
            1_760_000_001_000,
        )
        .unwrap();
        with_note(&mut kit.devices[1]);
        let report = go(&mut kit, 1);
        assert_eq!(report.unsent, 1, "{report:?}");
        let published: Vec<_> = assert_whole(&kit, 1)
            .iter()
            .flat_map(|c| {
                c.versions
                    .iter()
                    .map(|v| v["key"].as_str().unwrap().to_owned())
            })
            .collect();
        assert_eq!(published.len(), 2);
        assert!(!published.contains(&key.as_str().to_owned()));
    }

    #[test]
    fn content_no_live_version_names_is_held_before_it_is_named() {
        // SP34: a Restore names a body only a deleted version carried, so a hold
        // goes first and the naming version waits until every device's frontier
        // includes the hold.
        let mut kit = testkit::library(2);
        let a = 1;
        let me = kit.devices[a].lib.device;
        kit.devices[a]
            .db
            .save_annotation(&highlight("book", None, 1_760_000_001_000))
            .unwrap();
        go(&mut kit, a);
        let id = kit.devices[a].db.annotations("book").unwrap()[0].id.clone();
        kit.devices[a].db.delete_annotation(&id).unwrap();
        assert_eq!(go(&mut kit, a).unsent, 0);
        kit.devices[a].db.restore_annotation(&id).unwrap();
        let report = go(&mut kit, a);
        assert_eq!(report.published.held, 1, "{report:?}");
        assert_eq!(report.unsent, 1, "the restored version waits");
        let changes = assert_whole(&kit, a);
        let held = changes.last().unwrap();
        assert_eq!(held.hold.len(), 1);
        assert!(held.versions.is_empty());
        // Nothing more is written while the hold waits for the frontiers.
        let mut log = log_of(&kit, a);
        let again = run_on(&mut kit.devices[a], &mut log, &Budgets::sy2(), &|| 0).unwrap();
        assert!(log.writes.is_empty() && again.unsent == 1, "{again:?}");
        // Every device's frontier now includes the hold (SP29's marks).
        let mark = crate::sync::trust::Mark {
            s: held.seq,
            h: held.hash,
        };
        let kd = &mut kit.devices[a];
        frontier::store_marks(
            kd.store.library_mut(&kd.lib.id).unwrap(),
            &BTreeMap::from([(me, mark)]),
        )
        .unwrap();
        kd.store.save(&kd.ks).unwrap();
        let report = go(&mut kit, a);
        assert_eq!(report.unsent, 0, "{report:?}");
        let changes = assert_whole(&kit, a);
        assert_eq!(changes.last().unwrap().versions.len(), 1);
    }
}
