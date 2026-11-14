//! PS5: caller-owned state saved alongside each completed step.
use super::ids::{DeviceId, Hash32, Id16};
use super::trust::Mark;
use super::{ages, freshness, hold, retention, weeks};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// PS6: per-pass limits. The SY2 values are PROVISIONAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budgets {
    pub names: u64,
    pub devices: u64,
    pub bytes: u64,
    pub elapsed_ms: u64,
    /// SP15: the most change-file bytes one peer's run holds in memory at once,
    /// past the lowest floor checkpoint the run has to reach (the peer cache's
    /// 32 MB per peer, PROVISIONAL).
    pub run_bytes: u64,
}

impl Budgets {
    pub fn sy2() -> Self {
        Self {
            names: 100_000,
            devices: 32,
            bytes: 256 * 1024 * 1024,
            elapsed_ms: 60_000,
            run_bytes: 32 << 20,
        }
    }
}

// PS5: JSON object keys cannot represent structured identifiers.
macro_rules! map_module {
    ($name:ident, $map:ident, $bound:path) => {
        pub(crate) mod $name {
            use serde::{Deserialize, Deserializer, Serialize, Serializer};
            use std::collections::$map;
            pub fn serialize<S: Serializer, K: Serialize + $bound, V: Serialize>(
                map: &$map<K, V>,
                s: S,
            ) -> Result<S::Ok, S::Error> {
                map.iter().collect::<Vec<_>>().serialize(s)
            }
            pub fn deserialize<
                'de,
                D: Deserializer<'de>,
                K: Deserialize<'de> + $bound,
                V: Deserialize<'de>,
            >(
                d: D,
            ) -> Result<$map<K, V>, D::Error> {
                Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
            }
        }
    };
}
map_module!(map, BTreeMap, Ord);
// HashMap also needs Eq, supplied by the Hash bound's companion trait below.
pub(crate) mod hash_map {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::{collections::HashMap, hash::Hash};
    pub fn serialize<S: Serializer, K: Serialize + Eq + Hash, V: Serialize>(
        map: &HashMap<K, V>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<
        'de,
        D: Deserializer<'de>,
        K: Deserialize<'de> + Eq + Hash,
        V: Deserialize<'de>,
    >(
        d: D,
    ) -> Result<HashMap<K, V>, D::Error> {
        Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
    }
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MembershipState {
    #[serde(with = "map")]
    pub first_seen: BTreeMap<Hash32, i64>,
    pub confirmed: BTreeSet<Hash32>,
    pub checkpoint: BTreeSet<Hash32>,
    #[serde(with = "map")]
    pub quarantined: BTreeMap<DeviceId, Option<Mark>>,
    #[serde(with = "map")]
    pub heads: BTreeMap<DeviceId, Option<Hash32>>,
}
/// SP16: a peer's change with its own signature, which a `frontier` entry carries
/// so that it proves itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedMark {
    pub s: u64,
    pub h: Hash32,
    /// The signature, base64url.
    pub sig: String,
}
/// What this device holds of a peer's signature: `candidate` is saved before the
/// peer's changes are applied, `confirmed` once this device has applied that change.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PeerSig {
    pub confirmed: Option<SignedMark>,
    pub candidate: Option<SignedMark>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Sigs {
    #[serde(with = "map")]
    pub peers: BTreeMap<DeviceId, PeerSig>,
}
#[derive(Debug)]
pub struct State {
    pub ledger: hold::Ledger,
    pub weeks: weeks::KnownWeeks,
    pub heard: freshness::Heard,
    pub marks: ages::Marks,
    pub ages: retention::Ages,
    pub vouched: retention::Vouched,
    pub membership: MembershipState,
    /// SP16: the signatures a frontier needs (PS5).
    pub sigs: Sigs,
    /// SP18: what this device knows of the view each peer's device file names.
    pub views: super::views::Notes,
    /// PS5, SY17: whether `vouched` was loaded from a saved row. A clock that
    /// was never saved starts at the current readings in `resume`, rather than
    /// counting everything since the Unix epoch as elapsed.
    started: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            ledger: Default::default(),
            weeks: Default::default(),
            heard: Default::default(),
            marks: Default::default(),
            ages: Default::default(),
            vouched: retention::Vouched::new(0),
            membership: Default::default(),
            sigs: Default::default(),
            views: Default::default(),
            started: false,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Folder(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] super::device_store::StoreError),
    /// SP37 (1): the device store doesn't hold a usable identity for this library.
    #[error("the device store has no usable identity for this library")]
    NoIdentity,
    /// SP20, SP21: membership can't be computed from what storage holds.
    #[error("membership can't be computed: {0:?}")]
    Membership(super::membership::NoMembership),
    #[error(transparent)]
    Journal(#[from] super::journal::JournalError),
    #[error(transparent)]
    Device(#[from] super::device_file::DeviceError),
    #[error(transparent)]
    Chain(#[from] super::chain::ChainError),
    #[error(transparent)]
    Hold(#[from] hold::HoldError),
}
fn read<T: DeserializeOwned + Default>(
    conn: &Connection,
    lib: &Id16,
    name: &str,
) -> Result<T, Error> {
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM sync_pass_state WHERE library = ?1 AND name = ?2",
            params![lib.to_string(), name],
            |r| r.get(0),
        )
        .optional()?;
    json.map(|s| serde_json::from_str(&s).map_err(Error::from))
        .unwrap_or_else(|| Ok(T::default()))
}
fn write<T: Serialize>(
    tx: &Transaction<'_>,
    lib: &Id16,
    name: &str,
    value: &T,
) -> Result<(), Error> {
    tx.execute("INSERT INTO sync_pass_state (library, name, json) VALUES (?1, ?2, ?3) ON CONFLICT(library, name) DO UPDATE SET json = excluded.json", params![lib.to_string(), name, serde_json::to_string(value)?])?;
    Ok(())
}
impl State {
    pub fn load(conn: &Connection, lib: &Id16) -> Result<Self, Error> {
        let json: Option<String> = conn
            .query_row(
                "SELECT json FROM sync_pass_state WHERE library = ?1 AND name = 'vouched'",
                [lib.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(Self {
            ledger: read(conn, lib, "ledger")?,
            weeks: read(conn, lib, "weeks")?,
            heard: read(conn, lib, "heard")?,
            marks: read(conn, lib, "marks")?,
            ages: read(conn, lib, "ages")?,
            started: json.is_some(),
            vouched: json
                .map(|s| serde_json::from_str(&s))
                .transpose()?
                .unwrap_or_else(|| retention::Vouched::new(0)),
            membership: read(conn, lib, "membership")?,
            sigs: read(conn, lib, "sigs")?,
            views: read(conn, lib, "views")?,
        })
    }
    pub fn save(&self, tx: &Transaction<'_>, lib: &Id16) -> Result<(), Error> {
        write(tx, lib, "ledger", &self.ledger)?;
        write(tx, lib, "weeks", &self.weeks)?;
        write(tx, lib, "heard", &self.heard)?;
        write(tx, lib, "marks", &self.marks)?;
        write(tx, lib, "ages", &self.ages)?;
        write(tx, lib, "vouched", &self.vouched)?;
        write(tx, lib, "membership", &self.membership)?;
        write(tx, lib, "sigs", &self.sigs)?;
        write(tx, lib, "views", &self.views)
    }
    /// SY17: resume the same vouched clock used by ages and hearing history.
    pub fn resume(&mut self, mono_ms: u64, wall_ms: u64, clock_ahead: bool, same_boot: bool) {
        if !self.started {
            // PS5, SY17: a library's first pass starts the clock now; a restart
            // from the default would add the time since the Unix epoch.
            self.vouched = retention::Vouched::new(wall_ms);
            self.started = true;
        }
        self.vouched
            .restart(mono_ms, wall_ms, clock_ahead, same_boot);
    }
}

/// PS7: the clock readings a pass starts from, passed in rather than read.
#[derive(Clone, Copy, Debug)]
pub struct Readings {
    pub mono_ms: u64,
    pub wall_ms: u64,
    pub clock_ahead: bool,
    pub same_boot: bool,
}

/// What one device holds for a pass (PS4).
pub struct Device<'a> {
    pub conn: &'a Connection,
    pub lib: super::journal::Library,
    pub store: &'a mut super::device_store::DeviceStore,
    pub ks: &'a dyn super::device_store::Keystore,
    pub state: State,
    /// SP16, SY7: the platform's name for this device (cut to 64 characters).
    pub name: &'a str,
}

/// PS6: which budget stopped a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopped {
    Names,
    Devices,
    Bytes,
    Time,
}

/// PS4: what a pass did and what it left.
#[derive(Debug, Default)]
pub struct Report {
    pub names_listed: u64,
    pub bytes_read: u64,
    /// Paths set aside, with why (SP7: outside the layout, too large, a copy).
    pub set_aside: Vec<(String, String)>,
    pub trusted: Vec<DeviceId>,
    /// Peers whose device file was accepted with a new counter (SP16, SP18).
    pub accepted: Vec<DeviceId>,
    /// SP37 (4): peers whose change files were received this pass, with how many
    /// of their changes this device holds now.
    pub received: Vec<(DeviceId, u64)>,
    /// SP18: peers whose view was taken this pass, so their changes up to its
    /// `through` were not read one by one.
    pub viewed: Vec<DeviceId>,
    /// SP37 (5): the later fetches of bodies that received versions waited for.
    pub late: super::bodies::Late,
    /// SP37 (4): peers still ahead of this device after the pass: a change file
    /// not in storage yet, a chain that waits, or one the pass left for a budget.
    pub behind: Vec<DeviceId>,
    pub stopped: Option<Stopped>,
    /// PS6: bytes written to storage; they count against `Budgets::bytes` with the
    /// bytes read.
    pub bytes_written: u64,
    /// SP37 (9)–(10): what this pass published.
    pub published: Published,
    /// SP37 (9): local versions still unsent after the pass: they wait for a
    /// budget, a reservation's room, a hold's acknowledgement, or a body.
    pub unsent: u64,
    /// SP15 (1): why this device published nothing, when it found storage that
    /// it can't write beside.
    pub blocked: Option<Blocked>,
    /// PS4: steps not built yet are named, never silently skipped.
    pub not_built: &'static str,
}

/// SP37 (9)–(10): what one pass wrote to storage.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Published {
    /// Change files uploaded (SP14), by sequence, in the order they were written.
    pub changes: Vec<u64>,
    /// SP33: reservations and SP34: holds listed in the changes made this pass.
    pub reserved: u64,
    pub held: u64,
    /// SP9: blobs uploaded.
    pub blobs: u64,
    /// SP16: the counter of the device file written this pass.
    pub device_file: Option<u64>,
}

/// SP15 (1), SY7: storage shows this device's own files in a state it must not
/// publish beside. Nothing is numbered or written until it is clear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// A change or device file of this ID newer than this device's marks: its
    /// device store was restored from an older copy. SY7 answers with a rotation
    /// (SP27), which isn't built yet.
    StorageAhead,
    /// A device file of this ID that carries another writer token (SY7).
    OtherWriter,
}

/// PS6: counts what a pass uses and says when a budget runs out.
pub(super) struct Meter<'b> {
    budgets: &'b Budgets,
    now: &'b dyn Fn() -> u64,
    start: u64,
    devices: u64,
}

impl Meter<'_> {
    pub(super) fn check(&self, report: &Report) -> Option<Stopped> {
        if report.names_listed > self.budgets.names {
            Some(Stopped::Names)
        } else if self.devices > self.budgets.devices {
            Some(Stopped::Devices)
        } else {
            // PS6: what a pass writes counts with what it reads.
            self.reading(report.bytes_read + report.bytes_written)
        }
    }

    pub(super) fn budgets(&self) -> &Budgets {
        self.budgets
    }

    /// The budgets a read can use up: bytes and time.
    pub(super) fn reading(&self, bytes_read: u64) -> Option<Stopped> {
        if bytes_read > self.budgets.bytes {
            Some(Stopped::Bytes)
        } else if (self.now)().saturating_sub(self.start) > self.budgets.elapsed_ms {
            Some(Stopped::Time)
        } else {
            None
        }
    }
}

/// SP7, PS6: every page of one directory, each listing counted against the name
/// budget; `None` when a budget ran out part-way.
pub(super) fn list_all<F: super::folder::Folder>(
    folder: &F,
    dir: &str,
    meter: &Meter<'_>,
    report: &mut Report,
) -> Result<Option<Vec<super::folder::Listed<F::Token>>>, Error> {
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = folder.list(dir, cursor.as_deref())?;
        report.names_listed += page.entries.len() as u64;
        all.extend(page.entries);
        if let Some(stop) = meter.check(report) {
            report.stopped = Some(stop);
            return Ok(None);
        }
        match page.next {
            Some(next) if cursor.as_deref() != Some(next.as_str()) => cursor = Some(next),
            _ => return Ok(Some(all)),
        }
    }
}

/// SP7: a file's bytes through `files::read_file`, or why it was set aside.
pub(super) fn read_bytes<F: super::folder::Folder>(
    folder: &F,
    entry: &super::folder::Listed<F::Token>,
    report: &mut Report,
) -> Result<Option<Vec<u8>>, Error> {
    read_bytes_within(folder, entry, None, report)
}

/// `read_bytes` held to `limit` as well as the slot's, when one is given (SY28).
pub(super) fn read_bytes_within<F: super::folder::Folder>(
    folder: &F,
    entry: &super::folder::Listed<F::Token>,
    limit: Option<u64>,
    report: &mut Report,
) -> Result<Option<Vec<u8>>, Error> {
    use super::files::Read;
    let read = match limit {
        Some(limit) => super::files::read_file_within(folder, entry, limit)?,
        None => super::files::read_file(folder, entry)?,
    };
    let why = match read {
        Read::Bytes(bytes) => {
            report.bytes_read += bytes.len() as u64;
            return Ok(Some(bytes));
        }
        Read::Outside => "outside the layout",
        Read::Candidate { .. } => "a candidate copy",
        Read::TooLarge => "over its limit",
        Read::Gone => "gone",
    };
    report.set_aside.push((entry.name.clone(), why.into()));
    Ok(None)
}

/// SP20 (2): the code enrollments among `heads` and the records they build on.
fn code_enrollments_below(
    records: &BTreeMap<Hash32, super::trust::Record>,
    heads: impl Iterator<Item = Hash32>,
) -> BTreeSet<Hash32> {
    use super::trust::Body;
    let mut seen = BTreeSet::new();
    let mut found = BTreeSet::new();
    let mut todo: Vec<Hash32> = heads.collect();
    while let Some(h) = todo.pop() {
        if !seen.insert(h) {
            continue;
        }
        let Some(record) = records.get(&h) else {
            continue;
        };
        if matches!(record.body, Body::Enroll { by: None, .. }) {
            found.insert(h);
        }
        todo.extend(record.prevs());
    }
    found
}

pub(super) fn save_state(device: &Device<'_>) -> Result<(), Error> {
    let tx = device.conn.unchecked_transaction()?;
    device.state.save(&tx, &device.lib.id)?;
    tx.commit()?;
    Ok(())
}

/// SP37 (1)–(4) and (9)–(10), PS4–PS7: one pass. It reads membership before any
/// device file (#122's crowding risk), saves `State` after each step that changed
/// it (PS5), and stops where a budget runs out (PS6). What isn't built yet is named
/// in the report's `not_built`.
pub fn run<F: super::folder::Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    readings: Readings,
    budgets: &Budgets,
    now: &dyn Fn() -> u64,
) -> Result<Report, Error> {
    let mut report = steps(device, folder, readings, budgets, now)?;
    // SP37 (4): the peers still ahead, whichever way the steps ended.
    for &peer in &report.trusted {
        if peer == device.lib.device || device.state.membership.quarantined.contains_key(&peer) {
            continue;
        }
        let target = device
            .store
            .library(&device.lib.id)
            .and_then(|l| l.floors.get(&peer.to_string()))
            .map_or(0, |f| f.s);
        if super::apply::seen(device.conn, &device.lib, peer)? < target {
            report.behind.push(peer);
        }
    }
    Ok(report)
}

fn steps<F: super::folder::Folder>(
    device: &mut Device<'_>,
    folder: &mut F,
    readings: Readings,
    budgets: &Budgets,
    now: &dyn Fn() -> u64,
) -> Result<Report, Error> {
    use super::device_file::{self, Accepted};
    use super::{membership, trust};
    let mut report = Report {
        not_built: "SP37 step (3)'s unknown identities and removed devices' tails, steps (6)-(8) and (11); within step (5): the repair of blobs (SY23) and its quarantine (SP16), which are B2b's second part, and the repair of books (Stage 4); within step (4): the open reservations and holds and the marks (SP35) a view carries, and a view found by listing when the device file names none (PS1 lists no directories); within steps (9)-(10): the accepted-by-all marks (SP29, so a hold's naming change waits), peers' reservations and holds in the ledger, books, SY7's rotation when storage is ahead (SP27), the device file's own view (compaction is P3b), its views and proofs lists, its refused, repairs and quarantined lists (B2b's second part), and the hour's re-upload of own and cached files (SP15)",
        ..Report::default()
    };
    let mut meter = Meter {
        budgets,
        now,
        start: now(),
        devices: 0,
    };
    device.state.resume(
        readings.mono_ms,
        readings.wall_ms,
        readings.clock_ahead,
        readings.same_boot,
    );
    let vouched_now = device.state.vouched.now() as i64;

    // (1) SP37, SY7, SP28: the device store holds this library's identity, the
    // writer token and readable marks.
    let library = device
        .store
        .library(&device.lib.id)
        .ok_or(Error::NoIdentity)?;
    if library.device_id()? != device.lib.device {
        return Err(Error::NoIdentity);
    }
    library.writer_id()?;
    super::frontier::stored_marks(library)?;
    let genesis = library
        .rest
        .get("genesis")
        .and_then(|v| v.as_str())
        .and_then(Hash32::parse)
        .ok_or(Error::NoIdentity)?;
    let own_head = library
        .rest
        .get("head")
        .and_then(|v| v.as_str())
        .and_then(Hash32::parse);
    let accepted = library.load_accepted()?;
    // SP9: bodies are named with the naming key, which the device store keeps.
    let naming = naming_key(library)?;
    save_state(device)?;

    // (2) SP37, SP20, SP21: libraries/ and trust/, then membership.
    if list_all(folder, "libraries", &meter, &mut report)?.is_none() {
        return Ok(report);
    }
    let Some(entries) = list_all(folder, "trust", &meter, &mut report)? else {
        return Ok(report);
    };
    // PS5, SY2: first-seen times are kept only for records still in trust/, so
    // records added and removed again can't grow the saved state without bound.
    // A record that comes back is timed afresh, which only makes it wait longer.
    let listed: BTreeSet<Hash32> = entries
        .iter()
        .filter_map(|e| {
            e.name
                .strip_prefix("trust/")
                .and_then(|n| n.strip_suffix(".json"))
                .and_then(Hash32::parse)
        })
        .collect();
    device
        .state
        .membership
        .first_seen
        .retain(|h, _| listed.contains(h));
    let mut records = BTreeMap::new();
    for entry in &entries {
        let Some(hash) = entry
            .name
            .strip_prefix("trust/")
            .and_then(|n| n.strip_suffix(".json"))
            .and_then(Hash32::parse)
        else {
            report
                .set_aside
                .push((entry.name.clone(), "not a trust record".into()));
            continue;
        };
        // PS5: first-seen times already recorded are saved on every way out, or
        // a later pass would time those records from later (SP20 (1)).
        let read = read_bytes(folder, entry, &mut report);
        let Some(bytes) = (match read {
            Ok(bytes) => bytes,
            Err(e) => {
                save_state(device)?;
                return Err(e);
            }
        }) else {
            continue;
        };
        if let Some(stop) = meter.check(&report) {
            report.stopped = Some(stop);
            save_state(device)?;
            return Ok(report);
        }
        match trust::open(&bytes, &device.lib.id, &hash) {
            Ok(record) => {
                // SP20 (1): a record is timed from when this device first saw it.
                device
                    .state
                    .membership
                    .first_seen
                    .entry(hash)
                    .or_insert(vouched_now);
                records.insert(hash, record);
            }
            Err(e) => report.set_aside.push((entry.name.clone(), format!("{e}"))),
        }
    }
    let mut heads = device.state.membership.heads.clone();
    heads.insert(device.lib.device, own_head);
    // SP20 (2): a code enrollment counts once a live enrolled device's file
    // names a head that includes it, so membership is computed again until the
    // trusted devices' heads confirm nothing new. Each round confirms at least
    // one more record, so it ends.
    let membership = loop {
        let ms = &device.state.membership;
        let membership = membership::compute(&membership::Inputs {
            genesis,
            records: &records,
            first_seen: &ms.first_seen,
            confirmed: &ms.confirmed,
            checkpoint: &ms.checkpoint,
            held: &|_, _| None,
            accepted: &accepted,
            now_ms: vouched_now,
            me: Some(device.lib.device),
            quarantined: &ms.quarantined,
            heads: &heads,
        });
        // PS5: the first-seen times read this pass are saved even when storage
        // is stale (SP21), or a later pass would time those records from later.
        let membership = match membership {
            Ok(membership) => membership,
            Err(e) => {
                save_state(device)?;
                return Err(Error::Membership(e));
            }
        };
        let named = heads
            .iter()
            .filter(|(d, _)| membership.trusted(d).is_some())
            .filter_map(|(_, h)| *h);
        // Only a record membership is waiting to have confirmed: a chain that
        // does not reach the genesis, or an enrollment that is refused, is not
        // kept (so a peer's head can't grow the saved set).
        let new: Vec<Hash32> = code_enrollments_below(&records, named)
            .into_iter()
            .filter(|h| !device.state.membership.confirmed.contains(h))
            .filter(|h| {
                membership.verdict(h)
                    == Some(membership::Verdict::Waits(membership::Wait::Unconfirmed))
            })
            .collect();
        if new.is_empty() {
            break membership;
        }
        device.state.membership.confirmed.extend(new);
    };
    report.trusted = membership
        .state
        .members
        .keys()
        .filter(|d| membership.trusted(d).is_some())
        .copied()
        .collect();
    {
        let mut next = device.store.clone();
        next.library_mut(&device.lib.id)
            .ok_or(Error::NoIdentity)?
            .save_accepted(&membership, &records);
        next.save(device.ks)?;
        *device.store = next;
    }
    save_state(device)?;

    // (3) SP37, SP16, SP18: each trusted peer's device file, at most 32.
    let Some(entries) = list_all(folder, "devices", &meter, &mut report)? else {
        return Ok(report);
    };
    // SP18: the view each peer's accepted device file names, for step (4).
    let mut named_views: BTreeMap<DeviceId, Hash32> = BTreeMap::new();
    for peer in report.trusted.clone() {
        // SP30: a quarantined device's head stays frozen, so no later file of
        // its is accepted.
        if peer == device.lib.device || device.state.membership.quarantined.contains_key(&peer) {
            continue;
        }
        let path = device_file::device_path(&peer);
        let Some(entry) = entries.iter().find(|e| e.name == path) else {
            continue;
        };
        meter.devices += 1;
        if let Some(stop) = meter.check(&report) {
            report.stopped = Some(stop);
            break;
        }
        let Some(bytes) = read_bytes(folder, entry, &mut report)? else {
            continue;
        };
        if let Some(stop) = meter.check(&report) {
            report.stopped = Some(stop);
            break;
        }
        let Some(key) = membership.key(&peer) else {
            continue;
        };
        let opened = match device_file::open(&bytes, &device.lib.id, &peer, &key) {
            Ok(opened) => opened,
            Err(e) => {
                report.set_aside.push((path, format!("{e}")));
                continue;
            }
        };
        let floor = device
            .store
            .library(&device.lib.id)
            .and_then(|l| l.floors.get(&peer.to_string()))
            .map(|f| f.counter);
        // SP16: overflow parts live in the peer's own directory, listed only
        // when a file newer than the floor names some.
        let parts = if opened.overflow.is_empty() || floor >= Some(opened.counter) {
            Vec::new()
        } else {
            match list_all(folder, &format!("devices/{peer}"), &meter, &mut report)? {
                Some(parts) => parts,
                None => break,
            }
        };
        // PS6: each part's read is budgeted before it's made, so a file naming
        // many parts stops the pass after at most one part past the budget.
        let part_bytes = std::cell::Cell::new(0u64);
        let halted = std::cell::Cell::new(None);
        let base = report.bytes_read;
        let fetch = |part: &str| -> Option<Vec<u8>> {
            if let Some(stop) = meter.reading(base + part_bytes.get()) {
                halted.set(Some(stop));
                return None;
            }
            let listed = parts.iter().find(|e| e.name == part)?;
            match super::files::read_file(folder, listed).ok()? {
                super::files::Read::Bytes(b) => {
                    part_bytes.set(part_bytes.get() + b.len() as u64);
                    Some(b)
                }
                _ => None,
            }
        };
        let accepted = device_file::accept(
            device.ks,
            device.store,
            &device.lib.id,
            &peer,
            &key,
            &bytes,
            &fetch,
        );
        report.bytes_read += part_bytes.get();
        if let Some(stop) = halted.get() {
            report.stopped = Some(stop);
            break;
        }
        match accepted {
            Ok(Accepted::New(file, _)) => {
                device
                    .state
                    .heard
                    .saw_counter(peer, file.counter, &device.state.vouched);
                device.state.membership.heads.insert(peer, file.fields.head);
                named_views.extend(file.fields.view.map(|v| (peer, v)));
                report.accepted.push(peer);
            }
            // PS5: a crash after `accept` saved the floor but before the state
            // was saved loses the head; the file at the floor's own counter is
            // the one accepted, so its head is taken again.
            Ok(Accepted::Stale) => {
                if floor == Some(opened.counter) {
                    device
                        .state
                        .membership
                        .heads
                        .insert(peer, opened.fields.head);
                    named_views.extend(opened.fields.view.map(|v| (peer, v)));
                }
            }
            Err(e) => report.set_aside.push((path, format!("{e}"))),
        }
        if let Some(stop) = meter.check(&report) {
            report.stopped = Some(stop);
            break;
        }
    }
    // PS5, SP32: catch Heard up with each peer's stored floor without
    // refreshing its time, so a crash after `accept` saved the floor but before
    // `Heard` was saved can only make this device out of touch sooner.
    if let Some(library) = device.store.library(&device.lib.id) {
        for (peer, floor) in &library.floors {
            if let Some(peer) = DeviceId::parse(peer) {
                device.state.heard.catch_up_counter(peer, floor.counter);
            }
        }
    }
    save_state(device)?;
    let mut blobs = None;
    if report.stopped.is_none() {
        receive_changes(
            device,
            folder,
            &membership,
            &naming,
            &named_views,
            readings.wall_ms as i64,
            &meter,
            &mut report,
            &mut blobs,
        )?;
    }
    // SP37 (5): the bodies received versions waited for, once storage has them.
    if report.stopped.is_none() {
        super::bodies::fetch_waiting(
            device,
            folder,
            &meter,
            &mut report,
            &mut blobs,
            &naming,
            readings.wall_ms as i64,
        )?;
    }
    // SP37 (9): a budget that ran out earlier stops the pass where it is.
    if report.stopped.is_none() {
        super::publish::publish(
            device,
            folder,
            &naming,
            &meter,
            &mut report,
            &mut blobs,
            &entries,
        )?;
    }
    Ok(report)
}

/// SP9, SP28: the library's naming key, 32 bytes, kept in the device store.
fn naming_key(library: &super::device_store::LibraryState) -> Result<[u8; 32], Error> {
    library
        .rest
        .get("namingKey")
        .and_then(|v| v.as_str())
        .and_then(super::ids::b64url_decode)
        .and_then(|b| b.try_into().ok())
        .ok_or(Error::NoIdentity)
}

/// SY28: a version's record type limit on its body, when its key says which; and
/// whether the size it declares is past it, so the blob is never fetched.
pub(super) fn body_limit(version: &serde_json::Value) -> Option<u64> {
    version["key"]
        .as_str()
        .and_then(super::record::Key::parse)
        .map(|(key, _)| key.kind().body_limit())
}

pub(super) fn body_over_limit(version: &serde_json::Value) -> bool {
    body_limit(version)
        .zip(version["body"]["size"].as_u64())
        .is_some_and(|(limit, size)| size > limit)
}

/// SP37 (4), SP18, PS6: for each trusted peer whose accepted device file names a
/// change this device doesn't hold: its view when it is needed (`views`), then its
/// change files in order, each with the blobs its versions name that storage has
/// now (a body not there yet waits for step 5). `chain::receive_run` checks the run
/// against the peer's floor and applies it, after the view when there is one.
#[allow(clippy::too_many_arguments)]
fn receive_changes<F: super::folder::Folder>(
    device: &mut Device<'_>,
    folder: &F,
    membership: &super::membership::Membership,
    naming: &[u8; 32],
    named_views: &BTreeMap<DeviceId, Hash32>,
    now_ms: i64,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Option<BTreeMap<String, super::folder::Listed<F::Token>>>,
) -> Result<(), Error> {
    use super::{apply, chain, device_file, views};
    // PS5: a removed device's note of a view isn't kept.
    let trusted = report.trusted.clone();
    device.state.views.peers.retain(|d, _| trusted.contains(d));
    // PS5: a view an earlier pass was landing is finished first, ahead or not.
    if views::finish_landings(device, folder, meter, report, membership, naming)? {
        return Ok(());
    }
    for peer in trusted {
        if peer == device.lib.device || device.state.membership.quarantined.contains_key(&peer) {
            continue;
        }
        let Some(target) = device
            .store
            .library(&device.lib.id)
            .and_then(|l| l.floors.get(&peer.to_string()))
            .map(|f| f.s)
        else {
            continue;
        };
        let have = apply::seen(device.conn, &device.lib, peer)?;
        let Some(key) = membership.key(&peer).filter(|_| have < target) else {
            continue;
        };
        // SY28: a peer this device has stopped receiving from costs the pass
        // nothing; it stays in `behind`.
        if apply::stopped(device.conn, &device.lib, peer)? {
            continue;
        }
        let floor = device_file::floor_of(device.store, &device.lib.id, &peer)?;
        // SP18: the view, when what this device has applied is older than its
        // `through`. One that can't be used is set aside and the change files are
        // read as before.
        let mut taken = None;
        if let Some(named) = named_views.get(&peer) {
            match views::open(
                device, folder, meter, report, blobs, naming, &peer, &key, named, have,
            )? {
                views::Got::Ready(opened) => taken = Some(*opened),
                views::Got::Skip => {}
                // #229 (2): this peer waits for a pass with room; the next is read.
                views::Got::Wait => continue,
                views::Got::Halt => return Ok(()),
            }
        }
        let start = taken.as_ref().map_or(have, |o| o.view.through.s);
        // SP18: a run reaches the lowest checkpoint it has not passed, however
        // long that takes, or `receive_run` would wait on it for ever.
        let lowest = floor
            .iter()
            .map(|(s, _)| *s)
            .filter(|s| *s > have)
            .min()
            .unwrap_or(0);
        // #229 (1): a budget that ran out while the view was read stops the pass
        // after the view is received, when it alone reaches the checkpoint: the
        // bodies fetched so far come with it, and the rest wait for step 5 (SY28).
        // Otherwise the next pass would read the same view again, for ever.
        let stop = report.stopped.or_else(|| meter.check(report));
        let listing = match stop {
            Some(stop) => {
                report.stopped = Some(stop);
                None
            }
            None => list_all(folder, &format!("changes/{peer}"), meter, report)?,
        };
        let Some(listing) = listing else {
            if let Some(opened) = taken.filter(|o| o.view.through.s >= lowest) {
                keep_candidate(device, &peer, spine_mark(&opened))?;
                views::receive(
                    device,
                    report,
                    naming,
                    &peer,
                    &key,
                    opened,
                    Vec::new(),
                    now_ms,
                )?;
                settle(device, report, peer, have)?;
            }
            return Ok(());
        };
        let listing: BTreeMap<_, _> = listing.into_iter().map(|e| (e.name.clone(), e)).collect();
        let mut files = Vec::new();
        let mut last_sig = None;
        let mut run_bytes = 0u64;
        // A budget that runs out mid-run still lets what was read be received: a
        // body not fetched waits for step 5 (SY28), so the pass moves on.
        let mut halted = false;
        'run: for seq in start + 1..=target {
            let path = chain::change_path(&peer, seq);
            let Some(entry) = listing.get(&path) else {
                break;
            };
            let Some(bytes) = read_bytes(folder, entry, report)? else {
                break;
            };
            run_bytes += bytes.len() as u64;
            let change = match chain::open_change(&bytes, &device.lib.id, &peer, &key) {
                Ok(change) => change,
                Err(e) => {
                    report.set_aside.push((path, format!("{e}")));
                    break;
                }
            };
            let mut bodies = Vec::new();
            let mut stop = meter.check(report);
            for version in &change.versions {
                if stop.is_some() {
                    break;
                }
                match fetch_body(folder, meter, report, blobs, naming, version)? {
                    Body::Skip => continue,
                    Body::Halt => {
                        halted = true;
                        break;
                    }
                    Body::Got(body) => bodies.push(body),
                }
                stop = meter.check(report);
            }
            last_sig = Some(SignedMark {
                s: change.seq,
                h: change.hash,
                sig: super::ids::b64url_encode(&change.sig),
            });
            files.push((bytes, bodies));
            if halted || stop.is_some() {
                halted = true;
                report.stopped = report.stopped.or(stop);
                break 'run;
            }
            if run_bytes > meter.budgets().run_bytes && seq >= lowest {
                break;
            }
        }
        // SP16: a view alone is vouched for by its own last spine entry.
        if last_sig.is_none() {
            last_sig = taken.as_ref().and_then(spine_mark);
        }
        // SP16: the signature of the last change of the run is kept before the run is
        // applied, so a crash after it leaves the frontier entry to be made.
        keep_candidate(device, &peer, last_sig)?;
        if let Some(opened) = taken {
            views::receive(device, report, naming, &peer, &key, opened, files, now_ms)?;
        } else if !files.is_empty() {
            match chain::receive_run(device.conn, &device.lib, &peer, &key, &floor, files, now_ms) {
                Ok(_) => {}
                Err(e) => report
                    .set_aside
                    .push((format!("changes/{peer}"), format!("{e}"))),
            }
        }
        settle(device, report, peer, have)?;
        if halted {
            return Ok(());
        }
    }
    Ok(())
}

/// SP16: a view alone is vouched for by its own last spine entry.
fn spine_mark(opened: &super::views::Opened) -> Option<SignedMark> {
    opened.view.spine.last().map(|e| SignedMark {
        s: e.s,
        h: e.h,
        sig: super::ids::b64url_encode(&e.sig),
    })
}

/// SP16: the signature of the last change of what is about to be applied, kept
/// before the apply so a crash after it leaves the frontier entry to be made.
fn keep_candidate(
    device: &mut Device<'_>,
    peer: &DeviceId,
    mark: Option<SignedMark>,
) -> Result<(), Error> {
    if let Some(mark) = mark {
        device.state.sigs.peers.entry(*peer).or_default().candidate = Some(mark);
        save_state(device)?;
    }
    Ok(())
}

/// SP37 (4): what a peer's receive came to: the report, and the floor's checkpoints
/// that no later change need be kept for.
fn settle(
    device: &mut Device<'_>,
    report: &mut Report,
    peer: DeviceId,
    have: u64,
) -> Result<(), Error> {
    let now_held = super::apply::seen(device.conn, &device.lib, peer)?;
    if now_held > have {
        report.received.push((peer, now_held));
        super::device_file::prune_floors(device.ks, device.store, &device.lib.id, &peer, now_held)?;
    }
    Ok(())
}

/// SY28: what fetching one version's body came to.
pub(super) enum Body {
    /// Nothing to read: no body, past its type's limit, or not in storage yet.
    Skip,
    Got(Vec<u8>),
    /// A budget ran out while listing `blobs/`.
    Halt,
}

/// SP9, SY28: the body a version names, from `blobs/` under its keyed name, read no
/// further than its record type allows (whatever size the version declared). `blobs/`
/// is listed once per pass, when a body is first wanted.
pub(super) fn fetch_body<F: super::folder::Folder>(
    folder: &F,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Option<BTreeMap<String, super::folder::Listed<F::Token>>>,
    naming: &[u8; 32],
    version: &serde_json::Value,
) -> Result<Body, Error> {
    let Some(hash) = version["body"]["hash"].as_str().and_then(Hash32::parse) else {
        return Ok(Body::Skip);
    };
    // SY28: a body past its record type's limit is never fetched.
    if body_over_limit(version) {
        return Ok(Body::Skip);
    }
    if blobs.is_none() {
        let Some(listed) = list_all(folder, "blobs", meter, report)? else {
            return Ok(Body::Halt);
        };
        *blobs = Some(listed.into_iter().map(|e| (e.name.clone(), e)).collect());
    }
    // SP9: a body is stored under its keyed name.
    let name = format!("blobs/{}", super::crypto::blob_name(naming, &hash));
    let Some(blob) = blobs.as_ref().and_then(|b| b.get(&name)) else {
        return Ok(Body::Skip);
    };
    Ok(
        match read_bytes_within(folder, blob, body_limit(version), report)? {
            Some(body) => Body::Got(body),
            None => Body::Skip,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::record::Entry;
    use serde_json::{json, Value};

    fn setup() -> (Store, Id16) {
        (Store::open_in_memory().unwrap(), Id16([1; 16]))
    }
    fn save(store: &mut Store, lib: &Id16, state: &State) {
        // Store exposes a connection; an unchecked transaction uses the same database.
        let tx = store.conn().unchecked_transaction().unwrap();
        state.save(&tx, lib).unwrap();
        tx.commit().unwrap();
    }
    #[test]
    fn state_round_trips_through_the_database() {
        let (mut store, lib) = setup();
        let d = Id16([2; 16]);
        let h = Hash32([3; 32]);
        let e = Entry {
            device: d,
            seq: 7,
            hash: h,
        };
        let id = json!({"device": d, "seq": 7, "index": 1});
        let change = json!({"device": d, "seq": 7, "hash": h});
        let mark = json!({"s": 7, "h": h});
        let reservation =
            json!({"item":{"kind":"Blob","name":h,"size":9},"first_seen":10,"_claimed":true});
        let holding = json!({"item":{"kind":"Book","name":h,"hash":h,"size":11,"for_key":"setting:theme"},"claimed":true,"change":change,"accepted_at":12,"naming":[change,e]});
        let ledger =
            json!({"reservations":[[id,reservation]],"holds":[[id,holding]],"last":[[d,mark]]});
        let weeks = json!({"entries":[[e,[["setting:theme",[[{"kind":"Blob","name":h},true]]]]]]});
        let heard = json!({"counters":[[d,[3,4]]],"applied":[[d,5]]});
        let marks = json!({"own":[[e,6]],"in_view":[e],"reported":[[e,[d]]]});
        let ages = json!({"first_seen":[[e,8]],"reported":[[[e,d],[9,10]]]});
        let vouched =
            json!({"total_ms":100,"last_mono_ms":42,"last_wall_ms":300,"untrusted_wall":true});
        let membership = json!({"first_seen":[[h,-2]],"confirmed":[h],"checkpoint":[h],"quarantined":[[d,mark]],"heads":[[d,h]]});
        let views = json!({"peers":[[d,{"hash":h,"through":7,"tries":2,"refused":false}]],"landing":[[d,{"hash":h,"gen":3,"through":7,"tries":1}]]});
        let state = State {
            ledger: serde_json::from_value(ledger.clone()).unwrap(),
            weeks: serde_json::from_value(weeks.clone()).unwrap(),
            heard: serde_json::from_value(heard.clone()).unwrap(),
            marks: serde_json::from_value(marks.clone()).unwrap(),
            ages: serde_json::from_value(ages.clone()).unwrap(),
            vouched: serde_json::from_value(vouched.clone()).unwrap(),
            membership: serde_json::from_value(membership.clone()).unwrap(),
            sigs: Default::default(),
            views: serde_json::from_value(views.clone()).unwrap(),
            started: true,
        };
        save(&mut store, &lib, &state);
        let loaded = State::load(store.conn(), &lib).unwrap();
        for (actual, expected) in [
            (serde_json::to_value(loaded.ledger).unwrap(), ledger),
            (serde_json::to_value(loaded.weeks).unwrap(), weeks),
            (serde_json::to_value(loaded.heard).unwrap(), heard),
            (serde_json::to_value(loaded.marks).unwrap(), marks),
            (serde_json::to_value(loaded.ages).unwrap(), ages),
            (serde_json::to_value(loaded.vouched).unwrap(), vouched),
            (serde_json::to_value(loaded.membership).unwrap(), membership),
            (serde_json::to_value(loaded.views).unwrap(), views),
        ] {
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn a_restart_keeps_a_deletions_age_and_a_witness_timeout() {
        let (mut store, lib) = setup();
        let mut state = State::default();
        let d = Id16([2; 16]);
        let e = Entry {
            device: d,
            seq: 1,
            hash: Hash32([3; 32]),
        };
        state.vouched.tick(100, 1000);
        state.vouched.tick(200, 1100);
        state.ages.saw(e, state.vouched.now());
        state.heard.saw_counter(d, 1, &state.vouched);
        state.vouched.tick(300, 1200);
        save(&mut store, &lib, &state);
        for same_boot in [true, false] {
            let mut loaded = State::load(store.conn(), &lib).unwrap();
            loaded.resume(if same_boot { 400 } else { 10 }, 1300, false, same_boot);
            assert_eq!(loaded.ages.own_age(&e, loaded.vouched.now()), Some(200));
            let heard: Value = serde_json::to_value(&loaded.heard).unwrap();
            assert_eq!(heard["counters"][0][1][1], 100);
            assert_eq!(
                loaded.vouched.now() - heard["counters"][0][1][1].as_u64().unwrap(),
                200
            );
        }
    }
    #[test]
    fn membership_inputs_survive_a_restart() {
        let (mut store, lib) = setup();
        let mut state = State::default();
        let h = Hash32([9; 32]);
        state.membership.first_seen.insert(h, 123);
        save(&mut store, &lib, &state);
        assert_eq!(
            State::load(store.conn(), &lib)
                .unwrap()
                .membership
                .first_seen
                .get(&h),
            Some(&123)
        );
    }

    #[test]
    fn a_first_pass_starts_the_clock_now() {
        // PS5, SY17: no saved clock yet; resuming must not count the time since
        // the Unix epoch as elapsed, or every unheard witness looks silent for
        // decades and SP32 holds changes at once.
        let (store, lib) = setup();
        let mut state = State::load(store.conn(), &lib).unwrap();
        let wall = 1_790_000_000_000;
        state.resume(5_000, wall, false, false);
        assert_eq!(state.vouched.now(), 0);
        state.resume(5_000 + 60_000, wall + 60_000, false, true);
        assert_eq!(state.vouched.now(), 60_000);
    }

    mod run {
        use super::super::*;
        use crate::sync::folder::Folder;
        use crate::sync::testkit::{self, KitDevice};
        use std::cell::Cell;

        // Past the real clock: the kit's changes are stamped with it (SY10 holds a
        // version whose clock is ahead of the reader's).
        const READ: Readings = Readings {
            mono_ms: 1_000,
            wall_ms: 1_900_000_000_000,
            clock_ahead: false,
            same_boot: false,
        };

        fn pass(kd: &mut KitDevice, budgets: &Budgets) -> (Report, State) {
            let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            let mut device = Device {
                conn: kd.db.conn(),
                lib: kd.lib,
                store: &mut kd.store,
                ks: &kd.ks,
                state,
                name: "Test Mac",
            };
            let report = run(&mut device, &mut kd.folder, READ, budgets, &|| 0).unwrap();
            (report, device.state)
        }

        #[test]
        fn three_devices_read_each_others_device_files() {
            let mut kit = testkit::library(3);
            let peers: Vec<_> = kit.devices[1..].iter().map(|d| d.lib.device).collect();
            let (report, state) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert_eq!(report.stopped, None);
            assert_eq!(report.trusted.len(), 3);
            let mut accepted = report.accepted.clone();
            accepted.sort();
            let mut expected = peers.clone();
            expected.sort();
            assert_eq!(accepted, expected);
            for peer in &peers {
                assert!(state.heard.heard(peer).is_some_and(|(n, _)| n > 0));
            }
            // SP20 (1): every trust record is timed from when it was first seen.
            let trust = kit.devices[0]
                .folder
                .list("trust", None)
                .unwrap()
                .entries
                .len();
            assert_eq!(state.membership.first_seen.len(), trust);
            // Saved: a second pass loads it and finds nothing new.
            let (again, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(again.accepted.is_empty());
            assert_eq!(again.trusted.len(), 3);
        }

        #[test]
        fn each_budget_stops_the_pass() {
            let tight = |f: fn(&mut Budgets)| {
                let mut b = Budgets::sy2();
                f(&mut b);
                b
            };
            for (budgets, stop) in [
                (tight(|b| b.names = 1), Stopped::Names),
                (tight(|b| b.devices = 0), Stopped::Devices),
                (tight(|b| b.bytes = 10), Stopped::Bytes),
            ] {
                let mut kit = testkit::library(3);
                let (report, _) = pass(&mut kit.devices[0], &budgets);
                assert_eq!(report.stopped, Some(stop));
            }
            // Time: the injected clock jumps past the budget after the start.
            let mut kit = testkit::library(3);
            let kd = &mut kit.devices[0];
            let calls = Cell::new(0u64);
            let clock = || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    0
                } else {
                    120_000
                }
            };
            let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            let mut device = Device {
                conn: kd.db.conn(),
                lib: kd.lib,
                store: &mut kd.store,
                ks: &kd.ks,
                state,
                name: "Test Mac",
            };
            let report = run(&mut device, &mut kd.folder, READ, &Budgets::sy2(), &clock).unwrap();
            assert_eq!(report.stopped, Some(Stopped::Time));
        }

        #[test]
        fn membership_is_read_before_device_files() {
            let mut kit = testkit::library(3);
            // Enough names for libraries/ and trust/, none to spare for devices/.
            let kd = &kit.devices[0];
            let count = |dir| kd.folder.list(dir, None).unwrap().entries.len() as u64;
            let names = count("libraries") + count("trust");
            let mut budgets = Budgets::sy2();
            budgets.names = names;
            let (report, _) = pass(&mut kit.devices[0], &budgets);
            assert_eq!(report.stopped, Some(Stopped::Names));
            assert_eq!(report.trusted.len(), 3, "membership was computed");
            assert!(report.accepted.is_empty(), "no device file was read");
        }

        #[test]
        fn a_crash_before_saving_heard_is_caught_up_next_pass() {
            let mut kit = testkit::library(2);
            let peer = kit.devices[1].lib.device;
            pass(&mut kit.devices[0], &Budgets::sy2());
            // The crash: the floor survives in the device store, Heard doesn't.
            let kd = &mut kit.devices[0];
            let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            let counter = state.heard.heard(&peer).unwrap().0;
            state.heard = Default::default();
            let tx = kd.db.conn().unchecked_transaction().unwrap();
            state.save(&tx, &kd.lib.id).unwrap();
            tx.commit().unwrap();
            let (report, state) = pass(kd, &Budgets::sy2());
            assert!(report.accepted.is_empty(), "the file is stale now");
            assert_eq!(
                state.heard.heard(&peer),
                Some((counter, 0)),
                "counter, not time"
            );
        }

        fn file_bytes(kd: &KitDevice, dir: &str) -> Vec<u64> {
            kd.folder
                .list(dir, None)
                .unwrap()
                .entries
                .iter()
                .map(
                    |e| match crate::sync::files::read_file(&kd.folder, e).unwrap() {
                        crate::sync::files::Read::Bytes(b) => b.len() as u64,
                        _ => panic!("unreadable {}", e.name),
                    },
                )
                .collect()
        }

        fn save_raw(kd: &KitDevice, state: &State) {
            let tx = kd.db.conn().unchecked_transaction().unwrap();
            state.save(&tx, &kd.lib.id).unwrap();
            tx.commit().unwrap();
        }

        /// Gives `peer` a device file at counter 2 whose lists spill into
        /// overflow parts, one per list named.
        fn write_with_parts(
            peer: &mut KitDevice,
            lists: &[&str],
        ) -> crate::sync::device_file::Written {
            let lib = peer.store.library(&peer.lib.id).unwrap();
            let writer = lib.writer_id().unwrap();
            let head = lib
                .rest
                .get("head")
                .and_then(|v| v.as_str())
                .and_then(Hash32::parse);
            let frontier: Vec<serde_json::Value> = (0..4000)
                .map(|i| serde_json::json!({ "d": format!("{i:032x}"), "s": i, "h": "ab".repeat(32), "sig": "c".repeat(86) }))
                .collect();
            let mut fields = crate::sync::device_file::Fields {
                head,
                ..Default::default()
            };
            for list in lists {
                fields.lists.insert((*list).into(), frontier.clone());
            }
            let w = crate::sync::device_file::write(&peer.key, &peer.lib.id, &writer, 2, &fields)
                .unwrap();
            assert!(!w.parts.is_empty());
            for (path, bytes) in &w.parts {
                crate::sync::files::write_file(&mut peer.folder, path, bytes).unwrap();
            }
            crate::sync::files::write_file(&mut peer.folder, &w.path, &w.bytes).unwrap();
            w
        }

        #[test]
        fn a_device_file_with_overflow_parts_is_accepted() {
            // SP16: the parts are in devices/<peer>/, not in devices/ itself.
            let mut kit = testkit::library(2);
            let peer = &mut kit.devices[1];
            let w = write_with_parts(peer, &["frontier"]);
            let id = peer.lib.device;
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert_eq!(
                report.accepted,
                vec![id],
                "set aside: {:?}",
                report.set_aside
            );
            let parts: u64 = w.parts.iter().map(|(_, b)| b.len() as u64).sum();
            assert!(report.bytes_read > parts, "the parts' bytes are counted");
        }

        #[test]
        fn overflow_parts_are_read_within_the_byte_budget() {
            // PS6: a file naming several parts stops the pass after the first
            // part past the budget, before the rest are read.
            let mut kit = testkit::library(2);
            let w = write_with_parts(&mut kit.devices[1], &["frontier", "repairs"]);
            assert!(w.parts.len() >= 2);
            let kd = &kit.devices[0];
            let mut budgets = Budgets::sy2();
            budgets.bytes = file_bytes(kd, "trust").iter().sum::<u64>() + w.bytes.len() as u64;
            let (report, _) = pass(&mut kit.devices[0], &budgets);
            assert_eq!(report.stopped, Some(Stopped::Bytes));
            assert!(report.accepted.is_empty());
            let first = w.parts.iter().map(|(_, b)| b.len() as u64).max().unwrap();
            assert!(
                report.bytes_read <= budgets.bytes + first,
                "one part at most"
            );
        }

        #[test]
        fn a_quarantined_peers_device_file_is_not_accepted() {
            // SP30: its head stays where it was.
            let mut kit = testkit::library(2);
            let peer = kit.devices[1].lib.device;
            let kd = &mut kit.devices[0];
            let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            state.membership.quarantined.insert(peer, None);
            save_raw(kd, &state);
            let (report, state) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(!report.accepted.contains(&peer), "{report:?}");
            assert!(!state.membership.heads.contains_key(&peer));
        }

        #[test]
        fn a_code_enrollment_counts_once_a_trusted_peers_head_includes_it() {
            // SP20 (2): device 1's file names a head with C's code enrollment.
            use crate::sync::crypto::{CodeKeys, Keypair};
            let mut kit = testkit::library(2);
            let lib = kit.devices[0].lib.id;
            let genesis = kit.devices[0]
                .store
                .library(&lib)
                .unwrap()
                .rest
                .get("genesis")
                .and_then(|v| v.as_str())
                .and_then(Hash32::parse)
                .unwrap();
            let c = Keypair::from_seed(&[77; 32]);
            let code = CodeKeys {
                signing: Keypair::from_seed(&[99; 32]),
                escrow: [0; 32],
            };
            let (bytes, enroll) = crate::sync::recovery::enroll_by_code(
                &lib,
                &genesis,
                &code,
                &c,
                "C",
                crate::sync::clock::Hlc(1_790_000_000_000, 2),
            );
            crate::sync::files::write_file(
                &mut kit.devices[1].folder,
                &crate::sync::trust::trust_path(&enroll),
                &bytes,
            )
            .unwrap();
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(!report.trusted.contains(&c.device()), "waits for a confirm");
            let peer = &mut kit.devices[1];
            let writer = peer.store.library(&lib).unwrap().writer_id().unwrap();
            let fields = crate::sync::device_file::Fields {
                head: Some(enroll),
                ..Default::default()
            };
            let w = crate::sync::device_file::write(&peer.key, &lib, &writer, 2, &fields).unwrap();
            crate::sync::files::write_file(&mut peer.folder, &w.path, &w.bytes).unwrap();
            pass(&mut kit.devices[0], &Budgets::sy2());
            let (report, state) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(report.trusted.contains(&c.device()), "{report:?}");
            assert!(state.membership.confirmed.contains(&enroll));
        }

        #[test]
        fn first_seen_times_are_saved_when_storage_is_stale() {
            // PS5, SP21: the genesis this device accepted is missing for now.
            let mut kit = testkit::library(3);
            pass(&mut kit.devices[0], &Budgets::sy2());
            let kd = &mut kit.devices[0];
            let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            let genesis = kd
                .store
                .library(&kd.lib.id)
                .unwrap()
                .rest
                .get("genesis")
                .and_then(|v| v.as_str())
                .and_then(Hash32::parse)
                .unwrap();
            state.membership.first_seen.clear();
            save_raw(kd, &state);
            let path = crate::sync::trust::trust_path(&genesis);
            let listed = kd.folder.list("trust", None).unwrap().entries;
            let token = listed.into_iter().find(|e| e.name == path).unwrap().token;
            kd.folder.remove(&token).unwrap();
            let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            let mut device = Device {
                conn: kd.db.conn(),
                lib: kd.lib,
                store: &mut kd.store,
                ks: &kd.ks,
                state,
                name: "Test Mac",
            };
            assert!(run(&mut device, &mut kd.folder, READ, &Budgets::sy2(), &|| 0).is_err());
            let saved = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            assert_eq!(saved.membership.first_seen.len(), 2);
        }

        use crate::sync::apply::tests::{highlight, note_of};

        #[test]
        fn a_peers_note_and_deletion_are_received_in_order() {
            // SP37 (4), SP18: the change files follow the accepted device file.
            let mut kit = testkit::library(2);
            let peer = kit.devices[1].lib.device;
            kit.devices[1]
                .db
                .save_annotation(&highlight("book", Some("Call me"), 1_760_000_001_000))
                .unwrap();
            assert_eq!(kit.devices[1].publish_pending(), Some(1));
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert_eq!(report.received, vec![(peer, 1)], "{report:?}");
            assert!(report.behind.is_empty());
            assert_eq!(
                note_of(&kit.devices[0].db, "book"),
                Some(Some("Call me".into()))
            );
            // Nothing new: nothing is read again.
            let (again, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(again.received.is_empty());
            let id = kit.devices[1].db.annotations("book").unwrap()[0].id.clone();
            kit.devices[1].db.delete_annotation(&id).unwrap();
            assert_eq!(kit.devices[1].publish_pending(), Some(2));
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert_eq!(report.received, vec![(peer, 2)], "{report:?}");
            assert_eq!(note_of(&kit.devices[0].db, "book"), None);
        }

        #[test]
        fn a_change_file_not_in_storage_yet_leaves_the_peer_behind() {
            // SP14, SP18: the device file names change 1; its file hasn't synced.
            let mut kit = testkit::library(2);
            let peer = kit.devices[1].lib.device;
            kit.devices[1]
                .db
                .save_annotation(&highlight("book", Some("Call me"), 1_760_000_001_000))
                .unwrap();
            kit.devices[1].publish_pending();
            let kd = &mut kit.devices[1];
            let path = crate::sync::chain::change_path(&peer, 1);
            let entry = kd
                .folder
                .list(&format!("changes/{peer}"), None)
                .unwrap()
                .entries
                .into_iter()
                .find(|e| e.name == path)
                .unwrap();
            let bytes = match crate::sync::files::read_file(&kd.folder, &entry).unwrap() {
                crate::sync::files::Read::Bytes(b) => b,
                _ => panic!("unreadable"),
            };
            kd.folder.remove(&entry.token).unwrap();
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(report.received.is_empty());
            assert_eq!(report.behind, vec![peer]);
            assert_eq!(note_of(&kit.devices[0].db, "book"), None);
            // It syncs later: the next pass takes it, with no new device file.
            crate::sync::files::write_file(&mut kit.devices[1].folder, &path, &bytes).unwrap();
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert_eq!(report.received, vec![(peer, 1)], "{report:?}");
            assert!(report.behind.is_empty());
            assert_eq!(
                note_of(&kit.devices[0].db, "book"),
                Some(Some("Call me".into()))
            );
        }

        #[test]
        fn step_four_stops_at_the_name_budget() {
            // PS6: the listing of a peer's changes counts against the budget.
            let mut kit = testkit::library(2);
            kit.devices[1]
                .db
                .save_annotation(&highlight("book", Some("Call me"), 1_760_000_001_000))
                .unwrap();
            kit.devices[1].publish_pending();
            let kd = &kit.devices[0];
            let count = |dir| kd.folder.list(dir, None).unwrap().entries.len() as u64;
            let mut budgets = Budgets::sy2();
            budgets.names = count("libraries") + count("trust") + count("devices");
            let (report, _) = pass(&mut kit.devices[0], &budgets);
            assert_eq!(report.stopped, Some(Stopped::Names));
            assert!(report.received.is_empty());
            assert_eq!(note_of(&kit.devices[0].db, "book"), None);
            // The peer is still behind, however the pass ended.
            assert_eq!(report.behind, vec![kit.devices[1].lib.device]);
        }

        fn peer_with_note(kit: &mut testkit::Kit) {
            kit.devices[1]
                .db
                .save_annotation(&highlight("book", Some("Call me"), 1_760_000_001_000))
                .unwrap();
            kit.devices[1].publish_pending();
        }

        #[test]
        fn a_budget_that_runs_out_on_a_blob_still_receives_the_change() {
            // PS6, SY28: the change is received, and what was read with it; a
            // body not fetched waits for step 5. The next pass has less to do.
            let mut kit = testkit::library(2);
            peer_with_note(&mut kit);
            let peer = kit.devices[1].lib.device;
            let kd = &kit.devices[0];
            let mut budgets = Budgets::sy2();
            budgets.bytes = file_bytes(kd, "trust").iter().sum::<u64>()
                + file_bytes(kd, &format!("changes/{peer}"))
                    .iter()
                    .sum::<u64>()
                + file_bytes(kd, "devices")[..].iter().min().copied().unwrap();
            let (report, _) = pass(&mut kit.devices[0], &budgets);
            assert_eq!(report.stopped, Some(Stopped::Bytes), "{report:?}");
            assert_eq!(report.received, vec![(peer, 1)]);
            assert!(report.behind.is_empty());
        }

        #[test]
        fn a_run_reaches_the_floors_checkpoint_past_the_run_cap() {
            // SP18: with a checkpoint at 2 and the floor at 3, a run capped at one
            // change would never reach it and would wait for ever.
            let mut kit = testkit::library(2);
            let peer = kit.devices[1].lib.device;
            peer_with_note(&mut kit);
            let id = kit.devices[1].db.annotations("book").unwrap()[0].id.clone();
            kit.devices[1].db.delete_annotation(&id).unwrap();
            kit.devices[1].publish_pending();
            // The first pass accepts the device file (own = 2) but reads no change.
            let kd = &kit.devices[0];
            let count = |dir| kd.folder.list(dir, None).unwrap().entries.len() as u64;
            let mut stop = Budgets::sy2();
            stop.names = count("libraries") + count("trust") + count("devices");
            let (report, _) = pass(&mut kit.devices[0], &stop);
            assert_eq!(report.stopped, Some(Stopped::Names));
            kit.devices[1]
                .db
                .save_annotation(&highlight("book", Some("again"), 1_760_000_009_000))
                .unwrap();
            assert_eq!(kit.devices[1].publish_pending(), Some(3));
            let mut capped = Budgets::sy2();
            capped.run_bytes = 1;
            let (report, _) = pass(&mut kit.devices[0], &capped);
            assert_eq!(report.received, vec![(peer, 2)], "{report:?}");
            assert_eq!(report.behind, vec![peer]);
        }

        #[test]
        fn behind_is_reported_when_an_earlier_step_runs_out_of_budget() {
            // The floor is ahead from an earlier pass; this one stops in step (3).
            let mut kit = testkit::library(2);
            peer_with_note(&mut kit);
            let peer = kit.devices[1].lib.device;
            let kd = &kit.devices[0];
            let count = |dir| kd.folder.list(dir, None).unwrap().entries.len() as u64;
            let mut first = Budgets::sy2();
            first.names = count("libraries") + count("trust") + count("devices");
            pass(&mut kit.devices[0], &first);
            let mut second = Budgets::sy2();
            second.devices = 0;
            let (report, _) = pass(&mut kit.devices[0], &second);
            assert_eq!(report.stopped, Some(Stopped::Devices));
            assert_eq!(report.behind, vec![peer]);
        }

        #[test]
        fn a_peer_this_device_stopped_receiving_from_is_not_fetched() {
            // SY28: the pass spends nothing on it, and it stays behind.
            let mut kit = testkit::library(2);
            peer_with_note(&mut kit);
            let peer = kit.devices[1].lib.device;
            let kd = &kit.devices[0];
            crate::sync::apply::stop(kd.db.conn(), &kd.lib, peer, "test", 1).unwrap();
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(report.received.is_empty());
            assert_eq!(report.behind, vec![peer]);
            let trust: u64 = file_bytes(&kit.devices[0], "trust").iter().sum();
            // The peer's device file, and this device's own, which step (10) reads to
            // see whether it says anything new.
            let device_files: u64 = file_bytes(&kit.devices[0], "devices").iter().sum();
            assert!(
                report.bytes_read <= trust + device_files,
                "no change or blob was read"
            );
        }

        #[test]
        fn a_blob_bigger_than_its_types_limit_is_set_aside_unread() {
            // SY28: the version declares a small place; storage holds 9 KB (the
            // limit is 8 KB), under the generic 4 MB blob limit.
            let mut kit = testkit::library(2);
            kit.devices[1]
                .db
                .save_position("book", "epubcfi(/6/4!/4/2/1:0)", 0.3, Some("One"))
                .unwrap();
            kit.devices[1].publish_pending();
            let kd = &mut kit.devices[1];
            let blob = kd
                .folder
                .list("blobs", None)
                .unwrap()
                .entries
                .into_iter()
                .next()
                .unwrap();
            crate::sync::files::write_file(&mut kd.folder, &blob.name, &vec![b'x'; 9 << 10])
                .unwrap();
            let (report, _) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(
                report
                    .set_aside
                    .iter()
                    .any(|(path, why)| *path == blob.name && why == "over its limit"),
                "{report:?}"
            );
        }

        #[test]
        fn a_body_past_its_types_limit_is_not_fetched() {
            use serde_json::json;
            let over = |key: &str, size: u64| {
                body_over_limit(
                    &json!({ "key": key, "body": { "hash": "ab".repeat(32), "size": size } }),
                )
            };
            assert!(over("setting:fontPx", 64 * 1024 + 1));
            assert!(!over("setting:fontPx", 64 * 1024));
            assert!(!over("note:1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d", 4 << 20));
            assert!(over(
                "note:1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d",
                (4 << 20) + 1
            ));
            assert!(!body_over_limit(&json!({ "key": "setting:fontPx" })));
        }

        #[test]
        fn first_seen_times_are_pruned_even_when_a_budget_stops_the_pass() {
            // PS5, SY2: the listing is whole before any record is read.
            let mut kit = testkit::library(3);
            pass(&mut kit.devices[0], &Budgets::sy2());
            let kd = &mut kit.devices[0];
            let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            state.membership.first_seen.insert(Hash32([9; 32]), 5);
            save_raw(kd, &state);
            let mut budgets = Budgets::sy2();
            budgets.bytes = 1;
            let (report, state) = pass(kd, &budgets);
            assert_eq!(report.stopped, Some(Stopped::Bytes));
            assert!(!state.membership.first_seen.contains_key(&Hash32([9; 32])));
            let saved = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            assert!(!saved.membership.first_seen.contains_key(&Hash32([9; 32])));
        }

        #[test]
        fn a_head_naming_a_disconnected_enrollment_confirms_nothing() {
            // SP20 (2): only an enrollment membership is waiting on is confirmed.
            use crate::sync::crypto::{CodeKeys, Keypair};
            let mut kit = testkit::library(2);
            let lib = kit.devices[0].lib.id;
            let code = CodeKeys {
                signing: Keypair::from_seed(&[99; 32]),
                escrow: [0; 32],
            };
            let (bytes, bogus) = crate::sync::recovery::enroll_by_code(
                &lib,
                &Hash32([5; 32]),
                &code,
                &Keypair::from_seed(&[77; 32]),
                "C",
                crate::sync::clock::Hlc(1_790_000_000_000, 2),
            );
            crate::sync::files::write_file(
                &mut kit.devices[1].folder,
                &crate::sync::trust::trust_path(&bogus),
                &bytes,
            )
            .unwrap();
            let peer = &mut kit.devices[1];
            let writer = peer.store.library(&lib).unwrap().writer_id().unwrap();
            let fields = crate::sync::device_file::Fields {
                head: Some(bogus),
                ..Default::default()
            };
            let w = crate::sync::device_file::write(&peer.key, &lib, &writer, 2, &fields).unwrap();
            crate::sync::files::write_file(&mut peer.folder, &w.path, &w.bytes).unwrap();
            pass(&mut kit.devices[0], &Budgets::sy2());
            let (_, state) = pass(&mut kit.devices[0], &Budgets::sy2());
            assert!(state.membership.confirmed.is_empty());
        }

        #[test]
        fn first_seen_times_are_kept_only_for_records_in_trust() {
            // PS5, SY2: records added and removed again don't grow the state.
            let mut kit = testkit::library(2);
            pass(&mut kit.devices[0], &Budgets::sy2());
            let kd = &mut kit.devices[0];
            let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            let kept = state.membership.first_seen.clone();
            state.membership.first_seen.insert(Hash32([9; 32]), 5);
            save_raw(kd, &state);
            let (_, state) = pass(kd, &Budgets::sy2());
            assert_eq!(state.membership.first_seen, kept);
        }

        #[test]
        fn a_read_past_the_byte_budget_is_not_accepted() {
            // PS6: the budget is checked after each device file's read, not only
            // before the next peer's.
            let mut kit = testkit::library(3);
            let kd = &kit.devices[0];
            let mut budgets = Budgets::sy2();
            budgets.bytes = file_bytes(kd, "trust").iter().sum::<u64>();
            let (report, _) = pass(&mut kit.devices[0], &budgets);
            assert_eq!(report.stopped, Some(Stopped::Bytes));
            assert!(report.accepted.is_empty());
        }

        #[test]
        fn first_seen_times_are_saved_when_the_pass_stops_in_membership() {
            // PS5, SP20 (1): a record opened before the budget ran out keeps the
            // time it was first seen.
            let mut kit = testkit::library(3);
            let kd = &kit.devices[0];
            let sizes = file_bytes(kd, "trust");
            assert!(sizes.len() >= 2);
            let mut budgets = Budgets::sy2();
            budgets.bytes = sizes[0];
            let (report, _) = pass(&mut kit.devices[0], &budgets);
            assert_eq!(report.stopped, Some(Stopped::Bytes));
            let kd = &kit.devices[0];
            let saved = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            assert_eq!(saved.membership.first_seen.len(), 1);
        }

        #[test]
        fn a_crash_before_saving_a_peers_head_takes_it_again() {
            // PS5: the floor was saved with the accepted file, the head wasn't.
            let mut kit = testkit::library(2);
            let peer = kit.devices[1].lib.device;
            let (_, state) = pass(&mut kit.devices[0], &Budgets::sy2());
            let head = state.membership.heads[&peer];
            let kd = &mut kit.devices[0];
            let mut state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            state.membership.heads.remove(&peer);
            save_raw(kd, &state);
            let (report, state) = pass(kd, &Budgets::sy2());
            assert!(report.accepted.is_empty(), "the file is stale now");
            assert_eq!(state.membership.heads.get(&peer), Some(&head));
        }
    }
}
