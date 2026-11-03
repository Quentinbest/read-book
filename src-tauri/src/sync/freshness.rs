//! SP31 (c), SP32, item 113 (a), #148: membership freshness and waiting versions.
//!
//! The caller owns hearing history. All elapsed time is SY17's vouched-for time.

use super::ids::{DeviceId, Hash32};
use super::journal::{JournalError, Library};
use super::membership::State;
use super::merge::Placed;
use super::record::{Content, Kind, Version};
use super::retention::{Vouched, WEEK_MS};
use super::trust::Record;
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, JournalError>;

/// SP31 (c): trusted enrolled devices whose accepted head doesn't include the removal.
/// "Includes" follows the membership graph's links: predecessors and what a
/// `confirm` names (SP20 (4a)), as membership itself does.
/// The caller includes every enrolled device, with `None` when no file is held.
pub fn not_caught_up(
    records: &BTreeMap<Hash32, Record>,
    removal: &Hash32,
    heads: &BTreeMap<DeviceId, Option<Hash32>>,
    trusted: impl Fn(&DeviceId) -> bool,
) -> Vec<DeviceId> {
    heads
        .iter()
        .filter(|(device, _)| trusted(device))
        .filter(|(_, head)| {
            let mut pending: Vec<_> = head.iter().copied().collect();
            let mut seen = BTreeSet::new();
            while let Some(hash) = pending.pop() {
                if !seen.insert(hash) {
                    continue;
                }
                // SP31: a head not held here proves no inclusion.
                if let Some(record) = records.get(&hash) {
                    if hash == *removal {
                        return false;
                    }
                    // SP20 (4a): a `confirm` naming the removal counts as made
                    // on its own `prev`, even from another branch.
                    pending.extend(super::membership::links(record));
                }
            }
            true
        })
        .map(|(device, _)| *device)
        .collect()
}

/// SP32, SP27: exclude the writer's whole enrollment subtree, across rotations.
pub fn witnesses(state: &State, this: DeviceId, writer: DeviceId) -> BTreeSet<DeviceId> {
    let lineage = state.members.get(&writer).map_or(writer, |m| m.lineage);
    let mut excluded = BTreeSet::from([lineage]);
    loop {
        let before = excluded.len();
        // SP20: closed and removed identities still carry enrollment ancestry.
        for member in state.members.values() {
            if member.enroller.is_some_and(|d| excluded.contains(&d)) {
                excluded.insert(member.lineage);
            }
        }
        if excluded.len() == before {
            break;
        }
    }
    state
        .members
        .iter()
        .filter(|(device, member)| {
            **device != this
                && state.trusted(device).is_some()
                && !excluded.contains(&member.lineage)
        })
        .map(|(device, _)| *device)
        .collect()
}

/// SP32: caller-owned history of accepted rising counters and the reader's Apply.
#[derive(Debug, Default)]
pub struct Heard {
    counters: BTreeMap<DeviceId, (u64, u64)>,
    applied: BTreeMap<DeviceId, u64>,
}

impl Heard {
    /// SP16, SP32: a repeated or lower counter isn't hearing from a device.
    pub fn saw_counter(&mut self, device: DeviceId, counter: u64, clock: &Vouched) {
        let previous = self.counters.get(&device).map_or(0, |(n, _)| *n);
        if counter > previous {
            self.counters.insert(device, (counter, clock.now()));
        }
    }

    /// SP32: Apply is the reader's own assurance, for this writer only.
    pub fn applied(&mut self, writer: DeviceId, clock: &Vouched) {
        self.applied.insert(writer, clock.now());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    InTouch,
    NoWitness,
    OutOfTouch { witness: DeviceId, since_ms: u64 },
}

/// SP32: a week (PROVISIONAL) without a witness or this writer's Apply waits.
pub fn out_of_touch(
    state: &State,
    this: DeviceId,
    writer: DeviceId,
    heard: &Heard,
    clock: &Vouched,
) -> Touch {
    let witnesses = witnesses(state, this, writer);
    let Some(mut witness) = witnesses.first().copied() else {
        return Touch::NoWitness;
    };
    let mut latest = 0;
    // SP32: never heard means time zero. PROVISIONAL: ties name the smallest ID.
    for device in witnesses {
        let at = heard.counters.get(&device).map_or(0, |(_, at)| *at);
        if at > latest {
            witness = device;
            latest = at;
        }
    }
    latest = latest.max(heard.applied.get(&writer).copied().unwrap_or(0));
    let since_ms = clock.now().saturating_sub(latest);
    if since_ms >= WEEK_MS {
        Touch::OutOfTouch { witness, since_ms }
    } else {
        Touch::InTouch
    }
}

/// SP32: deletions of every kind wait; only these existing bodies' edits wait.
pub fn waits_when_out_of_touch(version: &Version, replaces_existing: bool) -> bool {
    if matches!(version.content, Content::Deleted { .. }) {
        return true;
    }
    match version.kind {
        // SP32: book-file replacements and edits of highlights and notes.
        Kind::BookFile | Kind::Annotation | Kind::Note => replaces_existing,
        // SP12: a choice that lets something go counts as a deletion of it, so
        // it waits too; a stolen key could otherwise end a hold or discard a
        // version with no word from the reader. `keep` proceeds.
        Kind::Choice => matches!(
            &version.content,
            Content::Inline(body) if matches!(
                body.get("choice").and_then(serde_json::Value::as_str),
                Some("discard" | "discard-all" | "wrong-clock" | "stop-waiting")
            )
        ),
        // SP32: places, settings, per-book settings and reading states proceed.
        // Book metadata isn't one of the held body edits either.
        Kind::Place
        | Kind::Setting
        | Kind::BookSetting
        | Kind::BookOpened
        | Kind::BookFinished
        | Kind::Book => false,
    }
}

/// SP32: store the caller's current decision. Leaving "out", whether back in
/// touch or to no witness at all, releases the writer's holds atomically: a
/// writer with no witness applies under SP32's 30-day undo keep instead (#148,
/// PR 2b), and holds left behind would never be asked about again.
pub fn record_touch(
    conn: &Connection,
    lib: &Library,
    writer: DeviceId,
    touch: Touch,
    now_ms: i64,
) -> Result<()> {
    let (state, witness, since_ms) = match touch {
        Touch::InTouch => ("in", None, None),
        Touch::NoWitness => ("none", None, None),
        Touch::OutOfTouch { witness, since_ms } => (
            "out",
            Some(witness.to_string()),
            Some(
                i64::try_from(since_ms)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            ),
        ),
    };
    let tx = conn.unchecked_transaction()?;
    if state != "out" {
        tx.execute(
            // SP32: released holds wait for their ancestry to be checked again
            // (-1), so a release never applies past a still-held ancestor.
            "UPDATE sync_versions SET applied = -1 WHERE library = ?1 AND device = ?2
             AND applied = -7 AND EXISTS (SELECT 1 FROM sync_touch
                WHERE library = ?1 AND device = ?2 AND state = 'out')",
            params![lib.id.to_string(), writer.to_string()],
        )?;
    }
    tx.execute(
        "INSERT INTO sync_touch (library, device, state, witness, since_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(library, device) DO UPDATE SET
            asked_at = CASE WHEN sync_touch.state != 'out' AND excluded.state = 'out'
                THEN NULL ELSE sync_touch.asked_at END,
            answered_at = CASE WHEN sync_touch.state != 'out' AND excluded.state = 'out'
                THEN NULL ELSE sync_touch.answered_at END,
            state = excluded.state, witness = excluded.witness, since_ms = excluded.since_ms",
        params![
            lib.id.to_string(),
            writer.to_string(),
            state,
            witness,
            since_ms
        ],
    )?;
    if state == "out" {
        // SP32: a version already pending when the writer goes out of touch (an
        // edit still waiting for its body, say) is held too, or it would apply
        // later without the reader's Apply. This runs after the upsert, which
        // sees the move into `out` and clears an earlier episode's question.
        let pending: Vec<(i64, String)> = tx
            .prepare(
                "SELECT seq, raw FROM sync_versions
                 WHERE library = ?1 AND device = ?2 AND seq IS NOT NULL AND applied = 0",
            )?
            .query_map(params![lib.id.to_string(), writer.to_string()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        let mut held_any = false;
        for (seq, raw) in pending {
            let Ok(value) = serde_json::from_str(&raw) else {
                continue;
            };
            if let Ok(super::record::Checked::Version(version)) =
                super::record::check_version(value, writer)
            {
                held_any |= hold(
                    &tx,
                    lib,
                    &Placed {
                        version,
                        seq: seq as u64,
                    },
                    now_ms,
                )?;
            }
        }
        if held_any {
            wait_for_held_ancestors(&tx, lib)?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// SP32: a pending version of any writer whose ancestry now names a held one
/// goes back to waiting for its ancestry (-1), where the recheck sees the hold
/// as unresolved; a pending version is applied without its ancestry being
/// checked again, so it would otherwise apply past the hold.
fn wait_for_held_ancestors(conn: &Connection, lib: &Library) -> Result<()> {
    let pending: Vec<(i64, String, i64, String)> = conn
        .prepare(
            "SELECT rowid, device, seq, raw FROM sync_versions
             WHERE library = ?1 AND seq IS NOT NULL AND applied = 0",
        )?
        .query_map([lib.id.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    for (rowid, device, seq, raw) in pending {
        let (Some(device), Ok(value)) = (DeviceId::parse(&device), serde_json::from_str(&raw))
        else {
            continue;
        };
        let Ok(super::record::Checked::Version(version)) =
            super::record::check_version(value, device)
        else {
            continue;
        };
        let placed = Placed {
            version,
            seq: seq as u64,
        };
        if super::apply::ancestry(conn, lib, &placed) == super::merge::HistoryCheck::Wait {
            conn.execute(
                "UPDATE sync_versions SET applied = -1 WHERE rowid = ?1",
                [rowid],
            )?;
        }
    }
    Ok(())
}

/// SP32: called after clock/ancestry checks, inside the receiving transaction.
/// A newly held version after Not now makes the question eligible once again.
pub(crate) fn hold(conn: &Connection, lib: &Library, p: &Placed, now_ms: i64) -> Result<bool> {
    let out: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_touch
         WHERE library = ?1 AND device = ?2 AND state = 'out')",
        params![lib.id.to_string(), p.version.device.to_string()],
        |r| r.get(0),
    )?;
    if !out {
        return Ok(false);
    }
    let existing: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_versions
         WHERE library = ?1 AND key = ?2 AND applied IN (1, 2))",
        params![lib.id.to_string(), p.version.key.as_str()],
        |r| r.get(0),
    )?;
    if !waits_when_out_of_touch(&p.version, existing) {
        return Ok(false);
    }
    conn.execute(
        "UPDATE sync_versions SET applied = -7
         WHERE library = ?1 AND device = ?2 AND seq = ?3 AND key = ?4",
        params![
            lib.id.to_string(),
            p.version.device.to_string(),
            p.seq as i64,
            p.version.key.as_str()
        ],
    )?;
    // SP32: the new question has no answer yet; retire the previous question's
    // timestamps together, so more holds before its answer don't ask again. This
    // hold follows the answer by transaction order, so any answer re-arms it; a
    // timestamp comparison would fail when the clock stalls or steps back.
    let _ = now_ms;
    conn.execute(
        "UPDATE sync_touch SET asked_at = NULL, answered_at = NULL
         WHERE library = ?1 AND device = ?2 AND answered_at IS NOT NULL",
        params![lib.id.to_string(), p.version.device.to_string()],
    )?;
    Ok(true)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Question {
    pub writer: DeviceId,
    pub witness: DeviceId,
    pub since_ms: u64,
    pub removed: usize,
    pub changed: usize,
}

/// SP32: return each question once; a fresh hold after Not now re-arms it.
/// Counts are versions, including SP12's letting-go choices as removals.
pub fn questions(conn: &Connection, lib: &Library, now_ms: i64) -> Result<Vec<Question>> {
    let tx = conn.unchecked_transaction()?;
    let rows: Vec<(String, String, i64, String)> = tx
        .prepare(
            "SELECT t.device, t.witness, t.since_ms, v.raw FROM sync_touch t
         JOIN sync_versions v ON v.library = t.library AND v.device = t.device
         WHERE t.library = ?1 AND t.state = 'out' AND t.asked_at IS NULL
            AND v.applied = -7 ORDER BY t.device, v.seq, v.key",
        )?
        .query_map([lib.id.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut questions: Vec<Question> = Vec::new();
    for (writer, witness, since_ms, raw) in rows {
        let writer = DeviceId::parse(&writer).ok_or(super::record::RecordError::Field("device"))?;
        let witness =
            DeviceId::parse(&witness).ok_or(super::record::RecordError::Field("witness"))?;
        let super::record::Checked::Version(v) = super::record::check_version(
            serde_json::from_str(&raw).map_err(|_| super::record::RecordError::Field("raw"))?,
            writer,
        )?
        else {
            continue;
        };
        if questions.last().map_or(true, |q| q.writer != writer) {
            questions.push(Question {
                writer,
                witness,
                since_ms: since_ms as u64,
                removed: 0,
                changed: 0,
            });
        }
        let q = questions.last_mut().expect("writer question");
        if waits_when_out_of_touch(&v, false) {
            q.removed += 1;
        } else {
            q.changed += 1;
        }
    }
    for q in &questions {
        tx.execute(
            "UPDATE sync_touch SET asked_at = ?3 WHERE library = ?1 AND device = ?2",
            params![lib.id.to_string(), q.writer.to_string(), now_ms],
        )?;
    }
    tx.commit()?;
    Ok(questions)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Apply,
    NotNow,
}

/// SP32: Apply releases holds, but never exempts them from SY26. Hearing is
/// updated only after the database transaction commits successfully.
pub fn answer(
    conn: &Connection,
    lib: &Library,
    writer: DeviceId,
    answer: Answer,
    heard: &mut Heard,
    clock: &Vouched,
    now_ms: i64,
) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE sync_touch SET answered_at = ?3, asked_at = COALESCE(asked_at, ?3),
            state = CASE WHEN ?4 THEN 'in' ELSE state END
         WHERE library = ?1 AND device = ?2",
        params![
            lib.id.to_string(),
            writer.to_string(),
            now_ms,
            answer == Answer::Apply
        ],
    )?;
    let released = if answer == Answer::Apply {
        tx.execute(
            // SP32: as on leaving `out`, released versions are rechecked first.
            "UPDATE sync_versions SET applied = -1 WHERE library = ?1 AND device = ?2 AND applied = -7",
            params![lib.id.to_string(), writer.to_string()],
        )?
    } else {
        0
    };
    tx.commit()?;
    if answer == Answer::Apply {
        heard.applied(writer, clock);
    }
    Ok(released)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::clock::Hlc;
    use crate::sync::crypto::Keypair;
    use crate::sync::envelope::Signer;
    use crate::sync::ids::{b64url_encode, Id16};
    use crate::sync::membership::Member;
    use crate::sync::record::Key;
    use crate::sync::trust;
    use serde_json::{json, Value};

    use crate::store::Store;
    use crate::sync::apply::tests::{
        device, highlight, note_of, publish_next, send, ANNO, SYNC_BOOK,
    };
    use crate::sync::apply::{self, Batch, JournalChains};
    use crate::sync::journal::{self, LocalContent};
    use crate::sync::late;
    use crate::sync::merge::{Chains, Lookup, Prior};
    use crate::sync::trust::Mark;

    struct Held {
        writer: Store,
        wl: Library,
        reader: Store,
        rl: Library,
        batch: Batch,
        now: i64,
    }

    fn out(conn: &Connection, lib: &Library, writer: DeviceId, now: i64) {
        record_touch(
            conn,
            lib,
            writer,
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS,
            },
            now,
        )
        .unwrap();
    }

    fn held() -> Held {
        let (mut writer, wl) = device(2, "writer");
        let (reader, rl) = device(1, "reader");
        let now = crate::store::now_ms();
        writer
            .save_annotation(&highlight("writer", Some("original note"), now))
            .unwrap();
        send(&writer, &wl, &reader, &rl).unwrap();
        out(reader.conn(), &rl, wl.device, now);
        let mut edit = highlight("writer", None, now + 1);
        edit.color = "blue".into();
        writer.save_annotation(&edit).unwrap();
        let mut addition = highlight("writer", None, now + 1);
        addition.id = "00000000-0000-4000-8000-000000000099".into();
        writer.save_annotation(&addition).unwrap();
        writer
            .save_position("writer", "epubcfi(/6/10)", 0.4, None)
            .unwrap();
        let key = Key::parse(&format!("choice:hold.{}.1.0", wl.device))
            .unwrap()
            .0;
        journal::record_local(
            writer.conn(),
            &wl,
            &key,
            LocalContent::Inline(json!({"choice": "stop-waiting"})),
            now as u64,
        )
        .unwrap();
        let batch = publish_next(writer.conn(), &wl).unwrap().unwrap();
        apply::receive(reader.conn(), &rl, &batch, now + 10).unwrap();
        Held {
            writer,
            wl,
            reader,
            rl,
            batch,
            now,
        }
    }

    fn state(h: &Held, key: &str, seq: u64) -> i64 {
        h.reader.conn().query_row(
            "SELECT applied FROM sync_versions WHERE library = ?1 AND device = ?2 AND key = ?3 AND seq = ?4",
            params![h.rl.id.to_string(), h.wl.device.to_string(), key, seq as i64], |r| r.get(0),
        ).unwrap()
    }

    #[test]
    fn an_out_of_touch_writers_deletion_waits_and_an_addition_applies() {
        let h = held();
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -7);
        assert_eq!(state(&h, &format!("annotation:{ANNO}"), h.batch.seq), -7);
        assert_eq!(
            state(&h, &format!("choice:hold.{}.1.0", h.wl.device), h.batch.seq),
            -7
        );
        assert_eq!(
            note_of(&h.reader, "reader").flatten().as_deref(),
            Some("original note")
        );
        let rows = h.reader.annotations("reader").unwrap();
        assert_eq!(rows.len(), 2, "the new highlight applies");
        assert_eq!(rows.iter().find(|r| r.id == ANNO).unwrap().color, "yellow");
        assert!(h.reader.position("reader").unwrap().is_some());
        assert_eq!(
            apply::apply_pending(h.reader.conn(), &h.rl, h.now + 20).unwrap(),
            0
        );
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -7);
        assert_eq!(state(&h, &format!("place:{SYNC_BOOK}"), h.batch.seq), 1);
    }

    #[test]
    fn apply_releases_held_versions_and_they_apply() {
        let h = held();
        let mut heard = Heard::default();
        let clock = at(WEEK_MS);
        assert_eq!(
            answer(
                h.reader.conn(),
                &h.rl,
                h.wl.device,
                Answer::Apply,
                &mut heard,
                &clock,
                h.now + 20
            )
            .unwrap(),
            3
        );
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -1);
        assert!(apply::apply_pending(h.reader.conn(), &h.rl, h.now + 20).unwrap() >= 2);
        let row = h
            .reader
            .annotations("reader")
            .unwrap()
            .into_iter()
            .find(|r| r.id == ANNO)
            .unwrap();
        assert_eq!(row.note, None);
        assert_eq!(row.color, "blue");
        let counted: i64 = h
            .reader
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_counted WHERE what = 'deleted' AND approved = 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(counted > 0, "SP32 Apply still counts under SY26");
        assert_eq!(
            out_of_touch(&family(), d(1), h.wl.device, &heard, &at(2 * WEEK_MS - 1)),
            Touch::InTouch
        );
        assert!(matches!(
            out_of_touch(&family(), d(1), h.wl.device, &heard, &at(2 * WEEK_MS)),
            Touch::OutOfTouch { .. }
        ));
    }

    #[test]
    fn back_in_touch_releases_held_versions() {
        let h = held();
        // Repeating OutOfTouch releases nothing.
        out(h.reader.conn(), &h.rl, h.wl.device, h.now + 20);
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -7);
        record_touch(
            h.reader.conn(),
            &h.rl,
            h.wl.device,
            Touch::InTouch,
            h.now + 30,
        )
        .unwrap();
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -1);
        assert!(apply::apply_pending(h.reader.conn(), &h.rl, h.now + 30).unwrap() >= 2);
        assert_eq!(
            h.reader
                .annotations("reader")
                .unwrap()
                .into_iter()
                .find(|r| r.id == ANNO)
                .unwrap()
                .note,
            None
        );
    }

    #[test]
    fn the_question_is_asked_once_and_not_now_waits_for_new_holds() {
        let h = held();
        assert_eq!(
            questions(h.reader.conn(), &h.rl, h.now).unwrap(),
            vec![Question {
                writer: h.wl.device,
                witness: d(4),
                since_ms: WEEK_MS,
                removed: 2,
                changed: 1,
            }]
        );
        assert!(questions(h.reader.conn(), &h.rl, h.now).unwrap().is_empty());
        let mut heard = Heard::default();
        assert_eq!(
            answer(
                h.reader.conn(),
                &h.rl,
                h.wl.device,
                Answer::NotNow,
                &mut heard,
                &at(WEEK_MS),
                h.now + 20
            )
            .unwrap(),
            0
        );
        out(h.reader.conn(), &h.rl, h.wl.device, h.now + 21);
        assert!(questions(h.reader.conn(), &h.rl, h.now).unwrap().is_empty());
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -7);
        // An ordinary place update doesn't re-arm the question.
        let key = Key::parse(&format!("place:{SYNC_BOOK}")).unwrap().0;
        journal::record_local(
            h.writer.conn(),
            &h.wl,
            &key,
            LocalContent::Body(br#"{"cfi":"epubcfi(/6/12)","fraction":500000}"#.to_vec()),
            h.now as u64,
        )
        .unwrap();
        let batch = publish_next(h.writer.conn(), &h.wl).unwrap().unwrap();
        apply::receive(h.reader.conn(), &h.rl, &batch, h.now + 30).unwrap();
        assert!(questions(h.reader.conn(), &h.rl, h.now).unwrap().is_empty());
        let key = Key::parse(&format!("choice:hold.{}.1.1", h.wl.device))
            .unwrap()
            .0;
        journal::record_local(
            h.writer.conn(),
            &h.wl,
            &key,
            LocalContent::Inline(json!({"choice": "stop-waiting"})),
            h.now as u64,
        )
        .unwrap();
        let batch = publish_next(h.writer.conn(), &h.wl).unwrap().unwrap();
        apply::receive(h.reader.conn(), &h.rl, &batch, h.now + 40).unwrap();
        assert_eq!(
            questions(h.reader.conn(), &h.rl, h.now).unwrap()[0].removed,
            3
        );
        assert!(questions(h.reader.conn(), &h.rl, h.now).unwrap().is_empty());
    }

    #[test]
    fn a_descendant_of_a_held_version_waits() {
        let h = held();
        let key = Key::parse(&format!("note:{ANNO}")).unwrap().0;
        let chains = JournalChains {
            conn: h.reader.conn(),
            lib: &h.rl,
        };
        assert_eq!(
            chains.version_at(h.wl.device, h.batch.seq, key.as_str()),
            Lookup::Unknown
        );
        assert_eq!(
            chains.prior_version(h.wl.device, key.as_str(), h.batch.seq + 1),
            Prior::Unknown
        );
        journal::record_local(
            h.writer.conn(),
            &h.wl,
            &key,
            LocalContent::Body(br#"{"text":"descendant"}"#.to_vec()),
            h.now as u64,
        )
        .unwrap();
        let batch = publish_next(h.writer.conn(), &h.wl).unwrap().unwrap();
        apply::receive(h.reader.conn(), &h.rl, &batch, h.now + 20).unwrap();
        assert_eq!(state(&h, key.as_str(), batch.seq), -1);
        assert_eq!(
            note_of(&h.reader, "reader").flatten().as_deref(),
            Some("original note")
        );
        record_touch(
            h.reader.conn(),
            &h.rl,
            h.wl.device,
            Touch::InTouch,
            h.now + 30,
        )
        .unwrap();
        apply::apply_pending(h.reader.conn(), &h.rl, h.now + 30).unwrap();
        assert_eq!(
            note_of(&h.reader, "reader").flatten().as_deref(),
            Some("descendant")
        );
    }

    #[test]
    fn a_removal_turns_held_versions_past_its_cutoff_into_late_ones() {
        let h = held();
        // A second independent held deletion provides a tail past the cutoff.
        let key = Key::parse(&format!("choice:hold.{}.1.1", h.wl.device))
            .unwrap()
            .0;
        journal::record_local(
            h.writer.conn(),
            &h.wl,
            &key,
            LocalContent::Inline(json!({"choice": "stop-waiting"})),
            h.now as u64,
        )
        .unwrap();
        let tail = publish_next(h.writer.conn(), &h.wl).unwrap().unwrap();
        apply::receive(h.reader.conn(), &h.rl, &tail, h.now + 20).unwrap();
        let result = late::revert_past(
            h.reader.conn(),
            &h.rl,
            &BTreeMap::from([(
                h.wl.device,
                Some(Mark {
                    s: h.batch.seq,
                    h: h.batch.hash,
                }),
            )]),
            &BTreeMap::new(),
            "removed",
            h.now + 30,
        )
        .unwrap();
        assert_eq!(result.listed, 1);
        assert_eq!(state(&h, key.as_str(), tail.seq), -5);
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), 0);
        // SP32: the removal takes over; the writer is no longer out of touch, so
        // nothing of its accepted prefix is held again.
        let touch: i64 = h
            .reader
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_touch WHERE device = ?1",
                [h.wl.device.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(touch, 0);
        let was: i64 = h
            .reader
            .conn()
            .query_row("SELECT was FROM sync_late_seen", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            was, 0,
            "late recovery restores pending, never a freshness hold"
        );
        assert!(questions(h.reader.conn(), &h.rl, h.now).unwrap().is_empty());
    }

    #[test]
    fn touch_fixture_has_the_current_schema_and_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("touch.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../../tests/fixtures/schema-v14.sql"))
            .unwrap();
        let columns = |conn: &Connection| -> Vec<(String, String, bool)> {
            conn.prepare("PRAGMA table_info(sync_touch)")
                .unwrap()
                .query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            columns(&conn),
            columns(Store::open_in_memory().unwrap().conn())
        );
        drop(conn);
        let store = Store::open(&path).unwrap();
        let row: (String, i64, i64) = store
            .conn()
            .query_row(
                "SELECT state, since_ms, asked_at FROM sync_touch",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("out".into(), WEEK_MS as i64, 100));
    }

    fn d(n: u8) -> DeviceId {
        Id16([n; 16])
    }

    fn h(n: u8) -> Hash32 {
        Hash32([n; 32])
    }

    fn member(state: &mut State, device: u8, enroller: Option<u8>, lineage: u8) {
        state.members.insert(
            d(device),
            Member {
                device: d(device),
                key: [device; 32],
                label: device.to_string(),
                record: h(device),
                enroller: enroller.map(d),
                lineage: d(lineage),
            },
        );
    }

    fn family() -> State {
        let mut state = State::default();
        member(&mut state, 1, None, 1); // A
        member(&mut state, 2, Some(1), 2); // A enrolls B
        member(&mut state, 3, Some(2), 3); // B enrolls C
        member(&mut state, 4, Some(1), 4); // A enrolls D
        state
    }

    fn at(ms: u64) -> Vouched {
        let mut clock = Vouched::new(0);
        clock.tick(0, 0);
        clock.tick(ms, ms);
        clock
    }

    fn add(records: &mut BTreeMap<Hash32, Record>, kind: &str, mut fields: Value) -> Hash32 {
        let device = Keypair::from_seed(&[1; 32]);
        let code = Keypair::from_seed(&[2; 32]);
        fields["code"] = json!({
            "nonce": b64url_encode(&[0; 16]), "codeKey": b64url_encode(&code.public())
        });
        if kind == "genesis" {
            fields["device"] = json!(device.device().to_string());
            fields["key"] = json!(b64url_encode(&device.public()));
            fields["label"] = json!("A");
        }
        let library = Id16([9; 16]);
        let (bytes, hash) = trust::sign(
            kind,
            &library,
            fields,
            &[
                (&device, Signer::Device(device.device()), 0),
                (&code, Signer::Code, 0),
            ],
        );
        records.insert(hash, trust::open(&bytes, &library, &hash).unwrap());
        hash
    }

    #[test]
    fn a_device_whose_head_doesnt_include_the_removal_hasnt_caught_up() {
        let mut records = BTreeMap::new();
        let old = add(&mut records, "genesis", json!({ "clock": [0, 0] }));
        let removal = add(
            &mut records,
            "remove",
            json!({ "prev": old.to_string(), "clock": [1, 0], "removes": [{
                "device": d(9).to_string(), "cutoff": null, "view": null, "cascade": false
            }] }),
        );
        let descendant = add(
            &mut records,
            "confirm",
            json!({ "prev": removal.to_string(), "confirms": removal.to_string() }),
        );
        let merge = add(
            &mut records,
            "merge",
            json!({ "prevs": [old.to_string(), removal.to_string()], "clock": [2, 0], "keeps": [] }),
        );
        let heads = BTreeMap::from([
            (d(1), Some(removal)),
            (d(2), Some(descendant)),
            (d(3), Some(old)),
            (d(4), Some(merge)),
            (d(5), None),
            (d(6), Some(h(99))), // Unknown head.
            (d(9), None),        // Removed device.
        ]);
        assert_eq!(
            not_caught_up(&records, &removal, &heads, |device| *device != d(9)),
            vec![d(3), d(5), d(6)]
        );
        // SP31: even a head equal to the removal must be held here.
        records.remove(&removal);
        assert_eq!(
            not_caught_up(&records, &removal, &heads, |device| *device != d(9)),
            vec![d(1), d(2), d(3), d(4), d(5), d(6)]
        );
    }

    #[test]
    fn a_confirm_on_another_branch_includes_the_removal() {
        // SP20 (4a): the removal is on one branch; a `confirm` on another names
        // it, and a head descending from that confirm has caught up.
        let mut records = BTreeMap::new();
        let old = add(&mut records, "genesis", json!({ "clock": [0, 0] }));
        let removal = add(
            &mut records,
            "remove",
            json!({ "prev": old.to_string(), "clock": [1, 0], "removes": [{
                "device": d(9).to_string(), "cutoff": null, "view": null, "cascade": false
            }] }),
        );
        // Another branch from the same head: a removal of a different device.
        let other = add(
            &mut records,
            "remove",
            json!({ "prev": old.to_string(), "clock": [1, 1], "removes": [{
                "device": d(8).to_string(), "cutoff": null, "view": null, "cascade": false
            }] }),
        );
        let confirm = add(
            &mut records,
            "confirm",
            json!({ "prev": other.to_string(), "confirms": removal.to_string() }),
        );
        let heads = BTreeMap::from([(d(1), Some(confirm)), (d(2), Some(other))]);
        assert_eq!(
            not_caught_up(&records, &removal, &heads, |_| true),
            vec![d(2)]
        );
    }

    #[test]
    fn witnesses_leave_out_the_writer_and_everything_it_enrolled() {
        let mut state = family();
        assert_eq!(witnesses(&state, d(1), d(2)), BTreeSet::from([d(4)]));
        member(&mut state, 5, Some(2), 3); // C rotates.
        state.rotated.insert(d(3), (d(5), None));
        member(&mut state, 6, Some(3), 6); // C's lineage enrolls another.
        member(&mut state, 7, Some(1), 2); // B rotates.
        state.rotated.insert(d(2), (d(7), None));
        member(&mut state, 8, Some(2), 8); // B's lineage enrolls another.
        assert_eq!(witnesses(&state, d(1), d(7)), BTreeSet::from([d(4)]));
        assert_eq!(witnesses(&state, d(1), d(2)), BTreeSet::from([d(4)]));
        state.removed.insert(d(4));
        assert!(witnesses(&state, d(1), d(7)).is_empty());
    }

    #[test]
    fn a_great_grandchild_listed_first_is_still_left_out() {
        // SP32: everything the writer enrolled "directly or through others",
        // whatever order the members happen to be listed in. Device 10 enrolls
        // 11, 11 enrolls 12, 12 enrolls 13; the deeper ones sort first.
        let mut state = State::default();
        member(&mut state, 20, None, 20); // this device
        member(&mut state, 21, Some(20), 21); // an independent witness
        member(&mut state, 10, Some(20), 10); // the writer
        member(&mut state, 3, Some(10), 3);
        member(&mut state, 2, Some(3), 2);
        member(&mut state, 1, Some(2), 1);
        assert_eq!(witnesses(&state, d(20), d(10)), BTreeSet::from([d(21)]));
    }

    #[test]
    fn two_devices_have_no_witness() {
        let mut state = State::default();
        member(&mut state, 1, None, 1);
        member(&mut state, 2, Some(1), 2);
        assert!(witnesses(&state, d(1), d(2)).is_empty());
        assert_eq!(
            out_of_touch(&state, d(1), d(2), &Heard::default(), &at(WEEK_MS)),
            Touch::NoWitness
        );
    }

    #[test]
    fn a_writer_that_enrolled_every_other_device_has_no_witness() {
        let state = family();
        assert!(witnesses(&state, d(4), d(1)).is_empty());
        assert_eq!(
            out_of_touch(&state, d(4), d(1), &Heard::default(), &at(WEEK_MS)),
            Touch::NoWitness
        );
    }

    #[test]
    fn a_week_without_a_rising_counter_from_a_witness_is_out_of_touch() {
        let mut state = family();
        let mut heard = Heard::default();
        let touch =
            |state: &State, heard: &Heard, ms| out_of_touch(state, d(1), d(2), heard, &at(ms));
        assert_eq!(touch(&state, &heard, WEEK_MS - 1), Touch::InTouch);
        assert_eq!(
            touch(&state, &heard, WEEK_MS),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS
            }
        );
        heard.saw_counter(d(4), 2, &at(100));
        heard.saw_counter(d(4), 2, &at(WEEK_MS));
        heard.saw_counter(d(4), 1, &at(WEEK_MS));
        heard.saw_counter(d(2), 50, &at(WEEK_MS + 100)); // Writer isn't a witness.
        assert_eq!(touch(&state, &heard, WEEK_MS + 99), Touch::InTouch);
        assert_eq!(
            touch(&state, &heard, WEEK_MS + 100),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS
            }
        );
        heard.applied(d(3), &at(WEEK_MS + 100)); // Another writer's Apply doesn't count.
        assert_eq!(
            touch(&state, &heard, WEEK_MS + 123),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS + 23
            }
        );
        heard.applied(d(2), &at(WEEK_MS + 123));
        assert_eq!(touch(&state, &heard, 2 * WEEK_MS + 122), Touch::InTouch);
        assert_eq!(
            touch(&state, &heard, 2 * WEEK_MS + 123),
            Touch::OutOfTouch {
                witness: d(4),
                since_ms: WEEK_MS
            }
        );
        heard.saw_counter(d(4), 3, &at(2 * WEEK_MS + 123));
        assert_eq!(touch(&state, &heard, 2 * WEEK_MS + 123), Touch::InTouch);
        member(&mut state, 5, Some(1), 5); // A second independent witness.
        heard.saw_counter(d(5), 1, &at(2 * WEEK_MS + 200));
        assert_eq!(
            touch(&state, &heard, 3 * WEEK_MS + 200),
            Touch::OutOfTouch {
                witness: d(5),
                since_ms: WEEK_MS
            }
        );
    }

    #[test]
    fn what_waits_when_out_of_touch() {
        let id = "12345678-1234-4123-8123-123456789abc";
        for (key, waits) in [
            (format!("place:{id}"), false),
            (format!("annotation:{id}"), true),
            (format!("note:{id}"), true),
            ("setting:font".into(), false),
            (format!("bookSetting:{id}:font"), false),
            (format!("book:{id}"), false),
            (format!("bookFile:{id}"), true),
            (format!("bookRead:{id}:opened"), false),
            (format!("bookRead:{id}:finished"), false),
            (format!("choice:obj.{}", h(1)), false),
        ] {
            let (key, kind) = Key::parse(&key).unwrap();
            let mut version = Version {
                key,
                kind,
                device: d(1),
                clock: Hlc(0, 0),
                history: Vec::new(),
                content: if kind == Kind::Choice {
                    Content::Inline(json!({ "choice": "try-again", "clears": [] }))
                } else {
                    Content::Body {
                        hash: h(1),
                        size: 1,
                    }
                },
                hash: h(2),
                raw: Value::Null,
            };
            assert!(
                !waits_when_out_of_touch(&version, false),
                "{kind:?} addition"
            );
            assert_eq!(
                waits_when_out_of_touch(&version, true),
                waits,
                "{kind:?} edit"
            );
            version.content = Content::Deleted {
                covers: Vec::new(),
                identity: None,
            };
            for replaces_existing in [false, true] {
                assert!(
                    waits_when_out_of_touch(&version, replaces_existing),
                    "{kind:?} deletion"
                );
            }
        }
    }

    #[test]
    fn a_choice_that_lets_something_go_waits() {
        // SP12: discard, discard-all, wrong-clock and stop-waiting count as
        // deletions of what they let go; keep and try-again proceed.
        for (choice, waits) in [
            ("discard", true),
            ("discard-all", true),
            ("wrong-clock", true),
            ("stop-waiting", true),
            ("keep", false),
            ("try-again", false),
        ] {
            let (key, kind) = Key::parse(&format!("choice:obj.{}", h(1))).unwrap();
            let version = Version {
                key,
                kind,
                device: d(1),
                clock: Hlc(0, 0),
                history: Vec::new(),
                content: Content::Inline(json!({ "choice": choice })),
                hash: h(2),
                raw: Value::Null,
            };
            for replaces_existing in [false, true] {
                assert_eq!(
                    waits_when_out_of_touch(&version, replaces_existing),
                    waits,
                    "{choice}"
                );
            }
        }
    }

    #[test]
    fn losing_every_witness_releases_held_versions() {
        // SP32: a writer with no witness applies (under PR 2b's undo keep); its
        // holds must not stay behind, never asked about again.
        let h = held();
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -7);
        record_touch(
            h.reader.conn(),
            &h.rl,
            h.wl.device,
            Touch::NoWitness,
            h.now + 20,
        )
        .unwrap();
        assert_eq!(state(&h, &format!("note:{ANNO}"), h.batch.seq), -1);
    }

    #[test]
    fn a_pending_edit_is_held_when_its_writer_goes_out_of_touch() {
        // SP32: an edit received in touch but still waiting for its body must not
        // apply after the writer goes out of touch without the reader's Apply.
        let (mut writer, wl) = device(2, "writer");
        let (reader, rl) = device(1, "reader");
        let now = crate::store::now_ms();
        writer
            .save_annotation(&highlight("writer", Some("original note"), now))
            .unwrap();
        send(&writer, &wl, &reader, &rl).unwrap();
        writer
            .save_annotation(&highlight("writer", Some("edited note"), now + 1))
            .unwrap();
        let mut batch = publish_next(writer.conn(), &wl).unwrap().unwrap();
        let bodies = std::mem::take(&mut batch.bodies);
        apply::receive(reader.conn(), &rl, &batch, now + 10).unwrap();
        let note_state =
            |r: &Store| -> i64 {
                r.conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = ?2 AND key = ?3",
                    params![wl.device.to_string(), batch.seq as i64, format!("note:{ANNO}")],
                    |r| r.get(0),
                )
                .unwrap()
            };
        assert_eq!(note_state(&reader), 0, "the edit waits for its body");
        out(reader.conn(), &rl, wl.device, now + 20);
        assert_eq!(note_state(&reader), -7, "going out of touch holds it");
        for body in bodies {
            apply::receive_body(reader.conn(), &rl, &body).unwrap();
        }
        apply::apply_pending(reader.conn(), &rl, now + 30).unwrap();
        assert_eq!(
            note_of(&reader, "reader"),
            Some(Some("original note".into()))
        );
    }

    #[test]
    fn a_late_body_over_the_budget_stops_the_writer_but_keeps_its_holds() {
        // SY28, SP32: a held version counts against the budget, but an
        // overflowing body never drops it; it stays for the question or a removal.
        let (mut writer, wl) = device(2, "writer");
        let (reader, rl) = device(1, "reader");
        let now = crate::store::now_ms();
        writer
            .save_annotation(&highlight("writer", Some("original note"), now))
            .unwrap();
        send(&writer, &wl, &reader, &rl).unwrap();
        out(reader.conn(), &rl, wl.device, now + 5);
        let big = "x".repeat(1_500_000);
        writer
            .save_annotation(&highlight("writer", Some(&big), now + 1))
            .unwrap();
        let mut batch = publish_next(writer.conn(), &wl).unwrap().unwrap();
        let bodies = std::mem::take(&mut batch.bodies);
        apply::receive(reader.conn(), &rl, &batch, now + 10).unwrap();
        let held = |r: &Store| -> i64 {
            r.conn()
                .query_row(
                    "SELECT COUNT(*) FROM sync_versions WHERE device = ?1 AND applied = -7",
                    [wl.device.to_string()],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let before = held(&reader);
        assert!(before > 0);
        for body in bodies {
            apply::receive_body(reader.conn(), &rl, &body).unwrap();
        }
        assert!(apply::stopped(reader.conn(), &rl, wl.device).unwrap());
        assert_eq!(held(&reader), before, "the holds are kept");
    }

    #[test]
    fn a_hold_after_not_now_re_arms_the_question_whatever_the_clock() {
        // SP32: a new hold after Not now re-arms the question even when the clock
        // shows the same moment as the answer, or an earlier one.
        let h = held();
        assert_eq!(questions(h.reader.conn(), &h.rl, h.now).unwrap().len(), 1);
        let mut heard = Heard::default();
        let clock = Vouched::new(0);
        answer(
            h.reader.conn(),
            &h.rl,
            h.wl.device,
            Answer::NotNow,
            &mut heard,
            &clock,
            h.now + 100,
        )
        .unwrap();
        assert!(questions(h.reader.conn(), &h.rl, h.now + 100)
            .unwrap()
            .is_empty());
        // A new hold: an edit of the highlight the fixture added (and applied).
        let mut writer = h.writer;
        let mut edit = highlight("writer", None, h.now + 2);
        edit.id = "00000000-0000-4000-8000-000000000099".into();
        edit.color = "green".into();
        writer.save_annotation(&edit).unwrap();
        let batch = publish_next(writer.conn(), &h.wl).unwrap().unwrap();
        // Received at an earlier clock than the answer.
        apply::receive(h.reader.conn(), &h.rl, &batch, h.now + 50).unwrap();
        assert_eq!(
            questions(h.reader.conn(), &h.rl, h.now + 60).unwrap().len(),
            1
        );
    }

    #[test]
    fn a_local_deletion_does_not_claim_to_have_seen_a_held_edit() {
        // SP13, SP32: a held change wasn't seen by the reader, so a deletion made
        // here stops its frontier before it, and stays in conflict with it.
        let h = held();
        let covers = crate::sync::local::covers(h.reader.conn(), &h.rl).unwrap();
        let writer = covers.iter().find(|e| e.device == h.wl.device);
        assert!(
            writer.map_or(true, |e| e.seq < h.batch.seq),
            "the frontier stops before the held change: {writer:?}"
        );
    }

    #[test]
    fn a_new_out_of_touch_episode_asks_again() {
        // SP32: asked but never answered, back in touch, then out of touch again
        // with a new hold: the new episode asks again.
        let h = held();
        assert_eq!(questions(h.reader.conn(), &h.rl, h.now).unwrap().len(), 1);
        record_touch(
            h.reader.conn(),
            &h.rl,
            h.wl.device,
            Touch::InTouch,
            h.now + 10,
        )
        .unwrap();
        apply::apply_pending(h.reader.conn(), &h.rl, h.now + 10).unwrap();
        out(h.reader.conn(), &h.rl, h.wl.device, h.now + 20);
        let mut writer = h.writer;
        let mut edit = highlight("writer", None, h.now + 2);
        edit.id = "00000000-0000-4000-8000-000000000099".into();
        edit.color = "green".into();
        writer.save_annotation(&edit).unwrap();
        let batch = publish_next(writer.conn(), &h.wl).unwrap().unwrap();
        apply::receive(h.reader.conn(), &h.rl, &batch, h.now + 30).unwrap();
        assert_eq!(
            questions(h.reader.conn(), &h.rl, h.now + 40).unwrap().len(),
            1
        );
    }

    #[test]
    fn a_pending_descendant_of_a_newly_held_version_waits() {
        // SP32: W's edit and V's edit made from it both wait for their bodies.
        // When W goes out of touch, W's is held, and V's must wait for it.
        let (mut w, wl) = device(2, "writer");
        let (mut v, vl) = device(3, "other");
        let (reader, rl) = device(1, "reader");
        let now = crate::store::now_ms();
        w.save_annotation(&highlight("writer", Some("original note"), now))
            .unwrap();
        let first = publish_next(w.conn(), &wl).unwrap().unwrap();
        apply::receive(reader.conn(), &rl, &first, now).unwrap();
        apply::receive(v.conn(), &vl, &first, now).unwrap();
        w.save_annotation(&highlight("writer", Some("w edit"), now + 1))
            .unwrap();
        let mut wb = publish_next(w.conn(), &wl).unwrap().unwrap();
        apply::receive(v.conn(), &vl, &wb, now + 2).unwrap();
        apply::apply_pending(v.conn(), &vl, now + 2).unwrap();
        v.save_annotation(&highlight("other", Some("v edit"), now + 3))
            .unwrap();
        let mut vb = publish_next(v.conn(), &vl).unwrap().unwrap();
        let w_bodies = std::mem::take(&mut wb.bodies);
        let v_bodies = std::mem::take(&mut vb.bodies);
        apply::receive(reader.conn(), &rl, &wb, now + 10).unwrap();
        apply::receive(reader.conn(), &rl, &vb, now + 11).unwrap();
        let state = |dev: DeviceId, seq: u64| -> i64 {
            reader
                .conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = ?2 AND key = ?3",
                    params![dev.to_string(), seq as i64, format!("note:{ANNO}")],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(state(vl.device, vb.seq), 0, "V's edit waits for its body");
        out(reader.conn(), &rl, wl.device, now + 20);
        assert_eq!(state(wl.device, wb.seq), -7);
        assert_eq!(state(vl.device, vb.seq), -1, "V's edit waits for W's");
        for b in w_bodies.iter().chain(&v_bodies) {
            apply::receive_body(reader.conn(), &rl, b).unwrap();
        }
        apply::apply_pending(reader.conn(), &rl, now + 30).unwrap();
        assert_eq!(
            note_of(&reader, "reader"),
            Some(Some("original note".into()))
        );
    }

    #[test]
    fn releasing_a_held_descendant_still_waits_for_its_held_ancestor() {
        // SP32: V's edit made from W's is held first (V out of touch), then W's
        // (W out of touch). Apply for V must not apply it past W's hold.
        let (mut w, wl) = device(2, "writer");
        let (mut v, vl) = device(3, "other");
        let (reader, rl) = device(1, "reader");
        let now = crate::store::now_ms();
        w.save_annotation(&highlight("writer", Some("original note"), now))
            .unwrap();
        let first = publish_next(w.conn(), &wl).unwrap().unwrap();
        apply::receive(reader.conn(), &rl, &first, now).unwrap();
        apply::receive(v.conn(), &vl, &first, now).unwrap();
        w.save_annotation(&highlight("writer", Some("w edit"), now + 1))
            .unwrap();
        let mut wb = publish_next(w.conn(), &wl).unwrap().unwrap();
        apply::receive(v.conn(), &vl, &wb, now + 2).unwrap();
        apply::apply_pending(v.conn(), &vl, now + 2).unwrap();
        v.save_annotation(&highlight("other", Some("v edit"), now + 3))
            .unwrap();
        let mut vb = publish_next(v.conn(), &vl).unwrap().unwrap();
        let bodies: Vec<_> = std::mem::take(&mut wb.bodies)
            .into_iter()
            .chain(std::mem::take(&mut vb.bodies))
            .collect();
        apply::receive(reader.conn(), &rl, &wb, now + 10).unwrap();
        apply::receive(reader.conn(), &rl, &vb, now + 11).unwrap();
        out(reader.conn(), &rl, vl.device, now + 20);
        out(reader.conn(), &rl, wl.device, now + 21);
        let state = |dev: DeviceId, seq: u64| -> i64 {
            reader
                .conn()
                .query_row(
                    "SELECT applied FROM sync_versions WHERE device = ?1 AND seq = ?2 AND key = ?3",
                    params![dev.to_string(), seq as i64, format!("note:{ANNO}")],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(state(vl.device, vb.seq), -7);
        assert_eq!(state(wl.device, wb.seq), -7);
        let mut heard = Heard::default();
        answer(
            reader.conn(),
            &rl,
            vl.device,
            Answer::Apply,
            &mut heard,
            &Vouched::new(0),
            now + 30,
        )
        .unwrap();
        for b in &bodies {
            apply::receive_body(reader.conn(), &rl, b).unwrap();
        }
        apply::apply_pending(reader.conn(), &rl, now + 40).unwrap();
        assert_eq!(state(vl.device, vb.seq), -1, "V's edit waits for W's hold");
        assert_eq!(
            note_of(&reader, "reader"),
            Some(Some("original note".into()))
        );
    }
}
