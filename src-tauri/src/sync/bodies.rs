//! SP37 (5), SY28, SP9: the later fetches of bodies. A version whose body wasn't in
//! storage when its change file or view was read waits (SY28). On a later pass, once
//! storage has the body, it is fetched here under its keyed name, read no further than
//! its record type allows, checked, and the waiting versions are applied. PS4: this
//! module orders the reads, the checks and the saves; each decision stays where it
//! lives (`apply::receive_body` decides whether a waiting version still wants the
//! body and keeps it, `apply::apply_pending` applies what is ready).
//!
//! Order, and what a stop between two of them leaves:
//!
//! 1. The waiting bodies are read off the database: each distinct hash a received
//!    version still waits for (applied 0, -1, -2 or -7: the states `receive_body`
//!    keeps a body for) that `sync_bodies` doesn't hold. Nothing is kept for them
//!    between passes (PS5): the versions themselves are the record.
//! 2. Each is fetched in turn (`pass::fetch_body`: `blobs/` listed once a pass, the
//!    keyed name SP9, the size from `stat` against the type's limit before the read,
//!    and again while reading). The bytes' plain hash must be the hash the version
//!    names (SY23): bytes that aren't are set aside, never kept. Then `receive_body`
//!    keeps it, in its own transaction.
//! 3. What was kept is applied (`apply_pending`), also when a budget stopped step 2:
//!    a stop leaves no body kept but unapplied. If a crash comes between 2 and 3, the
//!    body is held and its version still pending, and the next pass applies it,
//!    because step 3 runs whenever `apply::can_move` says so: a pending version's body is held,
//!    or a version kept for this clock to catch up (SY10) has been reached by it.
//!
//! Nothing is applied before its body is held and has passed `apply`'s own check of it
//! against its record type, and a version is applied once (`apply_key` marks it).

use super::apply;
use super::folder::{Folder, Listed};
use super::guard;
use super::ids::{identity_hash, Hash32};
use super::pass::{self, Device, Error, Meter, Report};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// SP37 (5): what the later fetches of one pass came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Late {
    /// Bodies kept this pass that waiting versions named.
    pub fetched: u64,
    /// Records applied late: a version that had waited for its body.
    pub applied: usize,
    /// Distinct bodies a received version still waits for after the pass: not in
    /// storage, set aside as damaged, or left for a budget.
    pub waiting: u64,
}

/// SY28: one body to fetch, with the oldest version that names it, which gives the
/// limit it is read to (each version is held to its own type's limit when applied).
struct Want {
    hash: Hash32,
    version: Value,
}

/// The bodies received versions wait for, oldest first. A version declaring a body
/// past its type's limit is never fetched (SY28), nor one whose key this release
/// can't read (SY9: it is never applied).
fn wanted(device: &Device<'_>) -> Result<Vec<Want>, Error> {
    let mut stmt = device.conn.prepare(
        "SELECT v.key, json_extract(v.raw, '$.body.hash'), json_extract(v.raw, '$.body.size')
         FROM sync_versions v
         WHERE v.library = ?1 AND v.seq IS NOT NULL AND v.applied IN (0, -1, -2, -7)
           AND json_extract(v.raw, '$.body.hash') IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM sync_bodies b
                           WHERE b.hash = json_extract(v.raw, '$.body.hash'))
         ORDER BY v.rowid",
    )?;
    let rows = stmt.query_map(params![device.lib.id.to_string()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let mut seen = HashSet::new();
    let mut wants = Vec::new();
    for row in rows {
        let (key, hash, size) = row?;
        let Some(parsed) = Hash32::parse(&hash) else {
            continue;
        };
        let version = json!({"key": key, "body": {"hash": hash, "size": size}});
        // SY28: past its type's limit, or a key this release can't read: not fetched.
        if pass::body_limit(&version).is_none()
            || pass::body_over_limit(&version)
            || !seen.insert(parsed)
        {
            continue;
        }
        wants.push(Want {
            hash: parsed,
            version,
        });
    }
    Ok(wants)
}

/// SP37 (5): fetch the bodies received versions wait for that storage has now, and
/// apply what that makes ready. A budget that runs out stops the pass after what was
/// fetched is applied (SY2).
#[allow(clippy::too_many_arguments)]
pub(super) fn fetch_waiting<F: Folder>(
    device: &mut Device<'_>,
    folder: &F,
    meter: &Meter<'_>,
    report: &mut Report,
    blobs: &mut Option<BTreeMap<String, Listed<F::Token>>>,
    naming: &[u8; 32],
    now_ms: i64,
) -> Result<(), Error> {
    let (conn, lib) = (device.conn, device.lib);
    // SY26, SY27: while the reader is asked or sync is paused no body is kept, so
    // none is read either; they are fetched after the answer.
    if guard::asking(conn, &lib)?.is_some() || guard::paused(conn, &lib)? {
        return Ok(());
    }
    let wants = wanted(device)?;
    let mut kept = 0u64;
    for want in &wants {
        if let Some(stop) = meter.check(report) {
            report.stopped = Some(stop);
            break;
        }
        let body = match pass::fetch_body(folder, meter, report, blobs, naming, &want.version)? {
            pass::Body::Skip => continue,
            pass::Body::Halt => break,
            pass::Body::Got(body) => body,
        };
        // SP9, SY23: the plain hash against the bytes. The keyed name was derived
        // from the hash the version names, so bytes that don't hash to it are a
        // damaged or swapped object: never kept, and the version keeps waiting.
        if identity_hash(&body) != want.hash {
            report.set_aside.push((
                format!("blobs/{}", super::crypto::blob_name(naming, &want.hash)),
                "its bytes aren't the body its name says".into(),
            ));
            continue;
        }
        if apply::receive_body(conn, &lib, &body)? {
            kept += 1;
        }
    }
    report.late.fetched = kept;
    report.late.waiting = wants.len() as u64 - kept;
    // A stop doesn't leave a kept body unapplied.
    if kept > 0 || apply::can_move(conn, &lib, now_ms)? {
        report.late.applied = apply::apply_pending(conn, &lib, now_ms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AnnotationRow;
    use crate::sync::apply::tests::{highlight, ANNO};
    use crate::sync::files::{self, Read};
    use crate::sync::folder::{self, Page};
    use crate::sync::pass::{run, Budgets, Readings, State, Stopped};
    use crate::sync::testkit::{self, Kit};
    use std::io;

    const READ: Readings = Readings {
        mono_ms: 1_000,
        wall_ms: 1_900_000_000_000,
        clock_ahead: false,
        same_boot: false,
    };
    const SECOND: &str = "2b3c4d5e-6f7a-4b1c-9d2e-4f5a6b7c8d9e";
    const THIRD: &str = "3c4d5e6f-7a8b-4c1d-8e2f-5a6b7c8d9e0f";

    fn pass_with<F: Folder>(
        kit: &mut Kit,
        readings: Readings,
        folder: &mut F,
        budgets: &Budgets,
        now: &dyn Fn() -> u64,
    ) -> Result<Report, Error> {
        let kd = &mut kit.devices[0];
        let state = State::load(kd.db.conn(), &kd.lib.id).unwrap();
        let mut device = Device {
            conn: kd.db.conn(),
            lib: kd.lib,
            store: &mut kd.store,
            ks: &kd.ks,
            state,
            name: "Test Mac",
        };
        run(&mut device, folder, readings, budgets, now)
    }

    fn pass_within(kit: &mut Kit, budgets: &Budgets) -> Report {
        let mut folder = kit.folder.view(&kit.devices[0].lib.device.to_string());
        pass_with(kit, READ, &mut folder, budgets, &|| 0).unwrap()
    }

    fn pass(kit: &mut Kit) -> Report {
        pass_within(kit, &Budgets::sy2())
    }

    /// Device 1 publishes a highlight with no note: one change with one body.
    fn highlight_change(kit: &mut Kit, id: &str, n: u8) {
        let row = AnnotationRow {
            id: id.into(),
            cfi_range: format!("epubcfi(/6/4!/4/{},/1:0,/1:4)", 2 * n),
            ..highlight("book", None, 1_760_000_000_500)
        };
        kit.devices[1].db.save_annotation(&row).unwrap();
        kit.devices[1].publish_pending().unwrap();
    }

    /// The bodies in `blobs/`, taken out of storage as a peer's upload not yet synced.
    fn take_blobs(kit: &Kit) -> Vec<(String, Vec<u8>)> {
        let kd = &kit.devices[1];
        let entries = kd.folder.list("blobs", None).unwrap().entries;
        entries
            .into_iter()
            .map(|e| {
                let Read::Bytes(bytes) = files::read_file(&kd.folder, &e).unwrap() else {
                    panic!("{}", e.name)
                };
                kit.folder.vanish(&e.name);
                (e.name, bytes)
            })
            .collect()
    }

    fn put(kit: &mut Kit, path: &str, bytes: &[u8]) {
        folder::Folder::write(&mut kit.devices[1].folder, path, bytes).unwrap();
    }

    fn reads(kit: &Kit, path: &str) -> usize {
        kit.folder
            .read_count(&kit.devices[0].lib.device.to_string(), path)
    }

    fn shown(kit: &Kit) -> Vec<String> {
        let mut ids: Vec<String> = kit.devices[0]
            .db
            .annotations("book")
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect();
        ids.sort();
        ids
    }

    /// Three changes by device 1, one highlight each, whose bodies are not in
    /// storage when device 0 reads them. Returns the bodies, in publishing order.
    fn missing(kit: &mut Kit) -> Vec<(String, Vec<u8>)> {
        let mut all = Vec::new();
        for (n, id) in [ANNO, SECOND, THIRD].into_iter().enumerate() {
            highlight_change(kit, id, n as u8 + 1);
            all.extend(take_blobs(kit));
        }
        assert_eq!(all.len(), 3);
        all
    }

    #[test]
    fn a_body_that_arrives_after_its_change_is_fetched_and_applied_once() {
        // SY28, SP37 (5): the change is received without a body; later versions of
        // other records are not held up by it; the body, once in storage, is fetched
        // under its keyed name and its version applied, on one pass and not again.
        let mut kit = testkit::library(2);
        let peer = kit.devices[1].lib.device;
        highlight_change(&mut kit, ANNO, 1);
        let held = take_blobs(&kit);
        assert_eq!(held.len(), 1);
        highlight_change(&mut kit, SECOND, 2);
        let report = pass(&mut kit);
        assert_eq!(report.received, vec![(peer, 2)], "{report:?}");
        assert_eq!(shown(&kit), vec![SECOND.to_string()]);
        assert_eq!(report.late.fetched, 0);
        assert_eq!(report.late.waiting, 1);
        // Still missing: it keeps waiting, and nothing else is disturbed.
        let report = pass(&mut kit);
        assert_eq!((report.late.fetched, report.late.waiting), (0, 1));
        assert_eq!(shown(&kit), vec![SECOND.to_string()]);
        assert_eq!(reads(&kit, &held[0].0), 0);
        // It arrives.
        put(&mut kit, &held[0].0, &held[0].1);
        let report = pass(&mut kit);
        assert_eq!(report.stopped, None, "{report:?}");
        assert_eq!(report.late.fetched, 1);
        assert_eq!(report.late.applied, 1);
        assert_eq!(report.late.waiting, 0);
        assert!(report.received.is_empty(), "no change was read again");
        let mut both = vec![ANNO.to_string(), SECOND.to_string()];
        both.sort();
        assert_eq!(shown(&kit), both);
        assert_eq!(reads(&kit, &held[0].0), 1);
        // Once: nothing is fetched or applied again.
        let report = pass(&mut kit);
        assert_eq!(report.late, Late::default(), "{report:?}");
        assert_eq!(reads(&kit, &held[0].0), 1);
    }

    #[test]
    fn a_body_that_fails_its_check_is_set_aside_and_its_version_keeps_waiting() {
        // SY23, SY28, SP9: wrong bytes under the right name, a body over the largest
        // limit, and one over its own type's limit (a place's is 8 KB) are never kept;
        // the healthy body of a later change is.
        let mut kit = testkit::library(2);
        let all = missing(&mut kit);
        kit.devices[1]
            .db
            .save_position("book", "epubcfi(/6/14!/4/2/1:0)", 0.31, Some("Loomings"))
            .unwrap();
        kit.devices[1].publish_pending().unwrap();
        // The change's other bodies are in storage; only the place's is taken.
        let change = kit.devices[1].own_changes(4).pop().unwrap();
        let hash = change
            .versions
            .iter()
            .find(|v| v["key"].as_str().unwrap().starts_with("place:"))
            .and_then(|v| v["body"]["hash"].as_str().and_then(Hash32::parse))
            .unwrap();
        let place_name = format!(
            "blobs/{}",
            super::super::crypto::blob_name(&testkit::NAMING, &hash)
        );
        let mut taken = take_blobs(&kit);
        let at = taken.iter().position(|(n, _)| *n == place_name).unwrap();
        let place = taken.remove(at);
        for (name, bytes) in &taken {
            put(&mut kit, name, bytes);
        }
        let (tampered, huge, healthy) = (&all[0], &all[1], &all[2]);
        pass(&mut kit);
        put(&mut kit, &tampered.0, b"not the body");
        put(&mut kit, &huge.0, &vec![0u8; (4 << 20) + 1]);
        put(&mut kit, &place.0, &vec![0u8; 9 << 10]);
        put(&mut kit, &healthy.0, &healthy.1);
        let report = pass(&mut kit);
        assert_eq!(report.stopped, None, "{report:?}");
        assert_eq!((report.late.fetched, report.late.waiting), (1, 3));
        assert_eq!(shown(&kit), vec![THIRD.to_string()]);
        for name in [&tampered.0, &huge.0, &place.0] {
            assert!(
                report.set_aside.iter().any(|(p, _)| p == name),
                "{:?}",
                report.set_aside
            );
        }
        // Neither oversize object is read: its size is checked first (SY28).
        assert_eq!(reads(&kit, &huge.0), 0);
        assert_eq!(reads(&kit, &place.0), 0);
        // Storage heals: the next pass takes all three.
        for (name, bytes) in [tampered, huge, &place] {
            put(&mut kit, name, bytes);
        }
        let report = pass(&mut kit);
        assert_eq!((report.late.fetched, report.late.waiting), (3, 0));
        assert_eq!(shown(&kit).len(), 3);
    }

    #[test]
    fn each_budget_stops_the_later_fetches_and_the_next_pass_takes_them_up() {
        // PS6, SP37 (5): names (listing `blobs/`), bytes and time each stop the pass
        // where it is; what was fetched is applied, and the next pass takes up the
        // rest without reading or applying any body twice.
        let finish = |kit: &mut Kit, bodies: &[(String, Vec<u8>)], fetched: u64| {
            let report = pass(kit);
            assert_eq!(report.stopped, None, "{report:?}");
            assert_eq!(report.late.fetched, fetched);
            assert_eq!(report.late.waiting, 0);
            assert_eq!(shown(kit).len(), 3);
            for (name, _) in bodies {
                assert_eq!(reads(kit, name), 1, "{name} was read once");
            }
            assert_eq!(pass(kit).late, Late::default());
        };
        // Names: the listing of `blobs/` doesn't fit.
        let mut kit = testkit::library(2);
        let all = missing(&mut kit);
        pass(&mut kit);
        let steady = pass(&mut kit);
        for (name, bytes) in &all {
            put(&mut kit, name, bytes);
        }
        let mut b = Budgets::sy2();
        b.names = steady.names_listed + all.len() as u64 - 1;
        let report = pass_within(&mut kit, &b);
        assert_eq!(report.stopped, Some(Stopped::Names), "{report:?}");
        assert_eq!((report.late.fetched, report.late.applied), (0, 0));
        // What the pass read before step 5, which the next case's budget builds on.
        let before_bodies = report.bytes_read;
        assert!(shown(&kit).is_empty());
        for (name, _) in &all {
            assert_eq!(reads(&kit, name), 0);
        }
        finish(&mut kit, &all, 3);
        // Bytes: the first body read takes the pass past its budget; it is kept and
        // applied, and the other two wait.
        let mut kit = testkit::library(2);
        let all = missing(&mut kit);
        pass(&mut kit);
        for (name, bytes) in &all {
            put(&mut kit, name, bytes);
        }
        let smallest = all.iter().map(|(_, b)| b.len() as u64).min().unwrap();
        let mut b = Budgets::sy2();
        b.bytes = before_bodies + smallest - 1;
        let report = pass_within(&mut kit, &b);
        assert_eq!(report.stopped, Some(Stopped::Bytes), "{report:?}");
        assert_eq!((report.late.fetched, report.late.applied), (1, 1));
        assert_eq!(shown(&kit).len(), 1);
        assert_eq!(all.iter().map(|(n, _)| reads(&kit, n)).sum::<usize>(), 1);
        finish(&mut kit, &all, 2);
        // Time: the clock moves past the budget when the first body is read.
        let mut kit = testkit::library(2);
        let all = missing(&mut kit);
        pass(&mut kit);
        for (name, bytes) in &all {
            put(&mut kit, name, bytes);
        }
        let clock = std::cell::Cell::new(0u64);
        let mut spy = Spy {
            inner: kit.folder.view(&kit.devices[0].lib.device.to_string()),
            clock: &clock,
        };
        let report = pass_with(&mut kit, READ, &mut spy, &Budgets::sy2(), &|| clock.get()).unwrap();
        assert_eq!(report.stopped, Some(Stopped::Time), "{report:?}");
        assert_eq!((report.late.fetched, report.late.applied), (1, 1));
        assert_eq!(shown(&kit).len(), 1);
        finish(&mut kit, &all, 2);
    }

    /// A folder that moves a clock past the time budget when a body is read.
    struct Spy<'a, F: Folder> {
        inner: F,
        clock: &'a std::cell::Cell<u64>,
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
            if format!("{token:?}").contains("blobs/") {
                self.clock.set(120_000);
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

    #[test]
    fn a_crash_between_the_fetch_and_the_apply_is_finished_by_the_next_pass() {
        // PS5: the body is kept before it is applied. A failure in the apply leaves it
        // held and its version pending; the next pass applies it without fetching it
        // again.
        let mut kit = testkit::library(2);
        let all = missing(&mut kit);
        pass(&mut kit);
        for (name, bytes) in &all {
            put(&mut kit, name, bytes);
        }
        let conn = kit.devices[0].db.conn();
        conn.execute_batch(
            "CREATE TRIGGER crash BEFORE UPDATE ON sync_versions
             BEGIN SELECT RAISE(ABORT, 'crash'); END",
        )
        .unwrap();
        let mut folder = kit.folder.view(&kit.devices[0].lib.device.to_string());
        let failed = pass_with(&mut kit, READ, &mut folder, &Budgets::sy2(), &|| 0);
        assert!(failed.is_err());
        let held: i64 = kit.devices[0]
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM sync_bodies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(held, 3);
        assert!(shown(&kit).is_empty());
        kit.devices[0]
            .db
            .conn()
            .execute_batch("DROP TRIGGER crash")
            .unwrap();
        let report = pass(&mut kit);
        assert_eq!(
            (report.late.fetched, report.late.applied),
            (0, 3),
            "{report:?}"
        );
        assert_eq!(shown(&kit).len(), 3);
        for (name, _) in &all {
            assert_eq!(reads(&kit, name), 1);
        }
    }

    #[test]
    fn a_body_for_a_version_kept_for_the_clock_is_applied_once_the_clock_catches_up() {
        // SY10, SP37 (5): the version's clock is more than a day ahead of this
        // device's, so it is kept (-2) without its body. The body arrives while the
        // clock is still behind: fetched and kept, not applied. A later pass with
        // nothing new in storage, once the clock has caught up, applies it, once.
        let mut kit = testkit::library(2);
        highlight_change(&mut kit, ANNO, 1);
        let held = take_blobs(&kit);
        let behind = Readings {
            wall_ms: crate::store::now_ms() as u64 - 3 * 24 * 3_600_000,
            ..READ
        };
        let at = |kit: &mut Kit, readings| {
            let mut folder = kit.folder.view(&kit.devices[0].lib.device.to_string());
            pass_with(kit, readings, &mut folder, &Budgets::sy2(), &|| 0).unwrap()
        };
        let report = at(&mut kit, behind);
        assert_eq!(report.received.len(), 1, "{report:?}");
        let waiting = |kit: &Kit| -> i64 {
            kit.devices[0]
                .db
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_versions WHERE applied = -2",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(waiting(&kit), 1);
        put(&mut kit, &held[0].0, &held[0].1);
        let report = at(&mut kit, behind);
        assert_eq!(
            (report.late.fetched, report.late.applied),
            (1, 0),
            "{report:?}"
        );
        assert!(shown(&kit).is_empty());
        assert_eq!(waiting(&kit), 1);
        // Nothing is new in storage, and the clock has caught up.
        let report = at(&mut kit, READ);
        assert_eq!(report.late.fetched, 0);
        assert_eq!(report.late.applied, 1, "{report:?}");
        assert_eq!(shown(&kit), vec![ANNO.to_string()]);
        assert_eq!(waiting(&kit), 0);
        let report = at(&mut kit, READ);
        assert_eq!(report.late, Late::default(), "{report:?}");
        assert_eq!(reads(&kit, &held[0].0), 1);
    }

    #[test]
    fn while_sync_is_paused_nothing_is_fetched() {
        // SY26, SY27: a paused library keeps no body, so none is read.
        let mut kit = testkit::library(2);
        let all = missing(&mut kit);
        pass(&mut kit);
        for (name, bytes) in &all {
            put(&mut kit, name, bytes);
        }
        let conn = kit.devices[0].db.conn();
        conn.execute("UPDATE sync_libraries SET state = 'paused'", [])
            .unwrap();
        let report = pass(&mut kit);
        assert_eq!(report.late, Late::default(), "{report:?}");
        for (name, _) in &all {
            assert_eq!(reads(&kit, name), 0);
        }
        kit.devices[0]
            .db
            .conn()
            .execute("UPDATE sync_libraries SET state = 'active'", [])
            .unwrap();
        assert_eq!(pass(&mut kit).late.fetched, 3);
    }
}
