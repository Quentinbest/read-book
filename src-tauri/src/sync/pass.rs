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
}

impl Budgets {
    pub fn sy2() -> Self {
        Self {
            names: 100_000,
            devices: 32,
            bytes: 256 * 1024 * 1024,
            elapsed_ms: 60_000,
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
#[derive(Debug)]
pub struct State {
    pub ledger: hold::Ledger,
    pub weeks: weeks::KnownWeeks,
    pub heard: freshness::Heard,
    pub marks: ages::Marks,
    pub ages: retention::Ages,
    pub vouched: retention::Vouched,
    pub membership: MembershipState,
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
        })
    }
    pub fn save(&self, tx: &Transaction<'_>, lib: &Id16) -> Result<(), Error> {
        write(tx, lib, "ledger", &self.ledger)?;
        write(tx, lib, "weeks", &self.weeks)?;
        write(tx, lib, "heard", &self.heard)?;
        write(tx, lib, "marks", &self.marks)?;
        write(tx, lib, "ages", &self.ages)?;
        write(tx, lib, "vouched", &self.vouched)?;
        write(tx, lib, "membership", &self.membership)
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
    pub stopped: Option<Stopped>,
    /// PS4: steps not built yet are named, never silently skipped.
    pub not_built: &'static str,
}

/// PS6: counts what a pass uses and says when a budget runs out.
struct Meter<'b> {
    budgets: &'b Budgets,
    now: &'b dyn Fn() -> u64,
    start: u64,
    devices: u64,
}

impl Meter<'_> {
    fn check(&self, report: &Report) -> Option<Stopped> {
        if report.names_listed > self.budgets.names {
            Some(Stopped::Names)
        } else if self.devices > self.budgets.devices {
            Some(Stopped::Devices)
        } else {
            self.reading(report.bytes_read)
        }
    }

    /// The budgets a read can use up: bytes and time.
    fn reading(&self, bytes_read: u64) -> Option<Stopped> {
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
fn list_all<F: super::folder::Folder>(
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
fn read_bytes<F: super::folder::Folder>(
    folder: &F,
    entry: &super::folder::Listed<F::Token>,
    report: &mut Report,
) -> Result<Option<Vec<u8>>, Error> {
    use super::files::Read;
    let why = match super::files::read_file(folder, entry)? {
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

fn save_state(device: &Device<'_>) -> Result<(), Error> {
    let tx = device.conn.unchecked_transaction()?;
    device.state.save(&tx, &device.lib.id)?;
    tx.commit()?;
    Ok(())
}

/// SP37 (1)–(3), PS4–PS7: one pass's first steps. It reads membership before any
/// device file (#122's crowding risk), saves `State` after each step that changed
/// it (PS5), and stops where a budget runs out (PS6). Steps (4)–(11) aren't built
/// yet, nor step (3)'s unknown identities and removed devices' tails; the report
/// says so.
pub fn run<F: super::folder::Folder>(
    device: &mut Device<'_>,
    folder: &F,
    readings: Readings,
    budgets: &Budgets,
    now: &dyn Fn() -> u64,
) -> Result<Report, Error> {
    use super::device_file::{self, Accepted};
    use super::{membership, trust};
    let mut report = Report {
        not_built:
            "SP37 step (3)'s unknown identities and removed devices' tails, and steps (4)-(11)",
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
    save_state(device)?;

    // (2) SP37, SP20, SP21: libraries/ and trust/, then membership.
    if list_all(folder, "libraries", &meter, &mut report)?.is_none() {
        return Ok(report);
    }
    let Some(entries) = list_all(folder, "trust", &meter, &mut report)? else {
        return Ok(report);
    };
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
        let new: Vec<Hash32> = code_enrollments_below(&records, named)
            .into_iter()
            .filter(|h| !device.state.membership.confirmed.contains(h))
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
    Ok(report)
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
        let state = State {
            ledger: serde_json::from_value(ledger.clone()).unwrap(),
            weeks: serde_json::from_value(weeks.clone()).unwrap(),
            heard: serde_json::from_value(heard.clone()).unwrap(),
            marks: serde_json::from_value(marks.clone()).unwrap(),
            ages: serde_json::from_value(ages.clone()).unwrap(),
            vouched: serde_json::from_value(vouched.clone()).unwrap(),
            membership: serde_json::from_value(membership.clone()).unwrap(),
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

        const READ: Readings = Readings {
            mono_ms: 1_000,
            wall_ms: 1_790_000_000_000,
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
            };
            let report = run(&mut device, &kd.folder, READ, budgets, &|| 0).unwrap();
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
            };
            let report = run(&mut device, &kd.folder, READ, &Budgets::sy2(), &clock).unwrap();
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
            };
            assert!(run(&mut device, &kd.folder, READ, &Budgets::sy2(), &|| 0).is_err());
            let saved = State::load(kd.db.conn(), &kd.lib.id).unwrap();
            assert_eq!(saved.membership.first_seen.len(), 2);
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
