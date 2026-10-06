//! Sync (next-steps plan, Phase 13; decisions 2026-10-06, item 48): reading
//! positions, highlights and notes shared between devices through a folder the
//! reader chooses, such as one in iCloud Drive. No server and no account.
//!
//! Each device appends its changes to its own log in the folder and reads the
//! others'. Records merge by a hybrid logical clock (HLC): the newest change to a
//! record wins, except that two devices editing the same note at once keep both
//! texts. Applying a remote change never counts as a local one, so two devices
//! with the same book open don't bounce a position between them.
//!
//! The folder is reached through [`Adapter`], so extension adapters (WebDAV and
//! others) can plug in later; their Host API waits (decisions 2026-10-06).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The record format this build writes. Records from a newer format are kept as
/// they are and passed on, never dropped or rewritten in an older form.
pub const FORMAT: u32 = 1;

/// Remote clocks further ahead of this device's clock than this are set aside,
/// so one device with a wrong clock can't win every merge for years.
pub const MAX_AHEAD_MS: u64 = 24 * 60 * 60 * 1000;

/// A hybrid logical clock reading: wall time, a counter for events within one
/// millisecond, and the device as the final tie-break. Ordered in that order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hlc {
    pub ms: u64,
    pub n: u32,
    pub device: String,
}

/// This device's clock.
#[derive(Debug, Clone)]
pub struct Clock {
    device: String,
    last: (u64, u32),
}

impl Clock {
    pub fn new(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            last: (0, 0),
        }
    }

    /// A reading for a local change at wall time `wall_ms`, always after every
    /// reading this clock has made or seen.
    pub fn tick(&mut self, wall_ms: u64) -> Hlc {
        self.last = if wall_ms > self.last.0 {
            (wall_ms, 0)
        } else {
            (self.last.0, self.last.1 + 1)
        };
        Hlc {
            ms: self.last.0,
            n: self.last.1,
            device: self.device.clone(),
        }
    }

    /// Take a remote reading into account. Returns false, and learns nothing,
    /// when it is more than [`MAX_AHEAD_MS`] ahead of `wall_ms`.
    pub fn observe(&mut self, remote: &Hlc, wall_ms: u64) -> bool {
        if remote.ms > wall_ms.saturating_add(MAX_AHEAD_MS) {
            return false;
        }
        if (remote.ms, remote.n) > self.last {
            self.last = (remote.ms, remote.n);
        }
        true
    }
}

/// Which book a record belongs to, the same on every device: the EPUB's package
/// identifier when it has one, otherwise the file's content hash. Book UUIDs differ
/// between devices that imported the same book separately, so they aren't used.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BookKey(pub String);

impl BookKey {
    pub fn of(package_identifier: Option<&str>, content_hash: &str) -> Self {
        match package_identifier.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => BookKey(format!("id:{id}")),
            None => BookKey(format!("hash:{content_hash}")),
        }
    }
}

/// A highlight or note as it travels. `deleted` is the tombstone: a deletion is a
/// change like any other, so a later undo (a newer change) brings the record back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub book: BookKey,
    pub anchored_content_hash: String,
    pub color: String,
    pub cfi_range: String,
    pub quote_exact: String,
    pub quote_prefix: String,
    pub quote_suffix: String,
    pub note: Option<String>,
    pub created_at: i64,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub book: BookKey,
    pub cfi: String,
    pub fraction: f64,
}

// Records are short-lived and few at a time; boxing would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Body {
    Position(Position),
    Annotation(Annotation),
}

/// One change. `prev` is the clock of the version this change was made from, so a
/// merge can tell an edit that saw the other device's version from one made at the
/// same time without it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub v: u32,
    pub hlc: Hlc,
    #[serde(default)]
    pub prev: Option<Hlc>,
    pub body: Body,
}

impl Change {
    /// The record this change is a version of.
    pub fn key(&self) -> String {
        match &self.body {
            Body::Position(p) => format!("pos:{}", p.book.0),
            Body::Annotation(a) => format!("ann:{}", a.id),
        }
    }
}

/// A line read from a log: a change this build understands, or one from a newer
/// format, kept verbatim.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Known(Change),
    Unknown(String),
}

pub fn parse_line(line: &str) -> Option<Line> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let v = value.get("v")?.as_u64()?;
    if v > u64::from(FORMAT) {
        return Some(Line::Unknown(line.to_string()));
    }
    serde_json::from_value(value).ok().map(Line::Known)
}

/// What a merge tells the store to do. None of these is a local change: the store
/// applies them without writing to this device's log.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Apply {
    /// Replace this record with the remote version.
    Take(Change),
    /// Two devices edited one note at once: keep the winner and add the loser's
    /// text as a new note on the same passage, so neither text is lost (A8).
    ConflictCopy { winner: Change, copy: Change },
}

/// Why a remote change was not applied.
#[derive(Debug, Clone, PartialEq)]
pub enum Skip {
    /// Not newer than what this device has.
    Older,
    /// From a clock too far in the future; kept aside, never applied silently.
    FromTheFuture,
}

/// Merge one remote change into the local version of its record (if any).
pub fn merge(local: Option<&Change>, remote: &Change) -> Result<Apply, Skip> {
    let Some(local) = local else {
        return Ok(Apply::Take(remote.clone()));
    };
    if remote.hlc <= local.hlc {
        return Err(Skip::Older);
    }
    if let (Body::Annotation(l), Body::Annotation(r)) = (&local.body, &remote.body) {
        // Concurrent: neither was made from the other, and both are live notes
        // with different text. The newer wins; the other's text is kept as a copy.
        let concurrent = remote.prev.as_ref() != Some(&local.hlc)
            && local.prev.as_ref() != Some(&remote.hlc)
            && remote.prev.as_ref().is_some_and(|p| *p < local.hlc);
        let both_notes = !l.deleted
            && !r.deleted
            && l.note.as_deref().is_some_and(|n| !n.is_empty())
            && l.note != r.note;
        if concurrent && both_notes {
            let mut copy = local.clone();
            if let Body::Annotation(a) = &mut copy.body {
                // Named after the losing version, so every version that loses gets
                // its own copy, and every device names it the same.
                a.id = format!(
                    "{}~{}.{}.{}",
                    a.id, local.hlc.device, local.hlc.ms, local.hlc.n
                );
            }
            copy.prev = None;
            return Ok(Apply::ConflictCopy {
                winner: remote.clone(),
                copy,
            });
        }
    }
    Ok(Apply::Take(remote.clone()))
}

/// The state of every record, as a device holds it. The store keeps the real one;
/// this in-memory form serves the merge tests and the simulation.
#[derive(Debug, Default, Clone)]
pub struct Replica {
    pub records: BTreeMap<String, Change>,
    pub unknown: Vec<String>,
}

impl Replica {
    /// Apply remote changes in any order. Changes already seen are skipped, so
    /// repeats are harmless. Returns how many were set aside as from the future, and
    /// the conflict copies made here: this device's note lost to a concurrent edit,
    /// so this device (and only this one) logs the copy, and it reaches the others.
    pub fn absorb(&mut self, clock: &mut Clock, wall_ms: u64, lines: &[Line]) -> Absorbed {
        let mut out = Absorbed::default();
        let mut known: Vec<&Change> = lines
            .iter()
            .filter_map(|l| match l {
                Line::Known(c) => Some(c),
                Line::Unknown(raw) => {
                    if !self.unknown.contains(raw) {
                        self.unknown.push(raw.clone());
                    }
                    None
                }
            })
            .collect();
        known.sort_by(|a, b| a.hlc.cmp(&b.hlc));
        for change in known {
            if !clock.observe(&change.hlc, wall_ms) {
                out.from_the_future += 1;
                continue;
            }
            let key = change.key();
            match merge(self.records.get(&key), change) {
                Ok(Apply::Take(c)) => {
                    self.records.insert(key, c);
                }
                Ok(Apply::ConflictCopy { winner, copy }) => {
                    if let std::collections::btree_map::Entry::Vacant(e) =
                        self.records.entry(copy.key())
                    {
                        // A local change with its own clock reading, to be logged.
                        let mut copy = copy;
                        copy.hlc = clock.tick(wall_ms);
                        e.insert(copy.clone());
                        out.copies.push(copy);
                    }
                    self.records.insert(key, winner);
                }
                Err(_) => {}
            }
        }
        out
    }
}

/// What [`Replica::absorb`] did besides applying changes.
#[derive(Debug, Default)]
pub struct Absorbed {
    pub from_the_future: usize,
    pub copies: Vec<Change>,
}

/// Where logs are kept: a folder, or (later) an extension's storage.
pub trait Adapter {
    /// Append lines to this device's log.
    fn append(&self, device: &str, lines: &[String]) -> std::io::Result<()>;
    /// Every log line from every device, this one included.
    fn read_all(&self) -> std::io::Result<Vec<(String, String)>>;
}

/// The built-in adapter: `<folder>/Linen Sync/v1/<device>.jsonl`, one log per
/// device. A device writes only its own file, so devices never write the same file.
pub struct FolderAdapter {
    root: PathBuf,
}

impl FolderAdapter {
    pub fn new(folder: &Path) -> Self {
        Self {
            root: folder.join("Linen Sync").join("v1"),
        }
    }

    fn log_path(&self, device: &str) -> PathBuf {
        self.root.join(format!("{device}.jsonl"))
    }
}

/// A file in the sync folder that holds a device's log: `<device>.jsonl`, or a
/// conflict copy a file service made of one (iCloud's "<device> 2.jsonl").
/// Temporary files, hidden files and iCloud placeholders are not logs yet.
fn log_device(name: &str) -> Option<String> {
    if name.starts_with('.') || name.ends_with(".tmp") || name.ends_with(".icloud") {
        return None;
    }
    let stem = name.strip_suffix(".jsonl")?;
    let device = match stem.rsplit_once(' ') {
        Some((d, n)) if n.chars().all(|c| c.is_ascii_digit()) => d,
        _ => stem,
    };
    (!device.is_empty()).then(|| device.to_string())
}

impl Adapter for FolderAdapter {
    fn append(&self, device: &str, lines: &[String]) -> std::io::Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.root)?;
        let path = self.log_path(device);
        // Write the whole new log under a temporary name, then rename it over the
        // old one: another device never reads a half-written file.
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        let tmp = self.root.join(format!(".{device}.jsonl.tmp"));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)
    }

    fn read_all(&self) -> std::io::Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(device) = log_device(&name) else {
                continue;
            };
            let text = std::fs::read_to_string(entry.path())?;
            out.extend(
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| (device.clone(), l.to_string())),
            );
        }
        Ok(out)
    }
}

/// This device's ID: bound to the machine, not stored in the library folder, so a
/// library restored on another Mac (README, “put the folder back”) gets a new ID
/// instead of overwriting the first Mac's changes. `library_id` is the library's own
/// random ID, so two libraries on one machine don't share an ID either.
pub fn device_id(machine_id: &str, library_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("linen-sync\0{machine_id}\0{library_id}"));
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The machine's stable identifier: IOPlatformUUID on macOS. Elsewhere, and if it
/// can't be read, None; the caller then keeps a random ID outside the library.
pub fn machine_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find(|l| l.contains("IOPlatformUUID"))
            .and_then(|l| l.split('"').nth(3))
            .map(str::to_string)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hlc(ms: u64, n: u32, device: &str) -> Hlc {
        Hlc {
            ms,
            n,
            device: device.into(),
        }
    }

    fn note(id: &str, text: Option<&str>, deleted: bool, at: Hlc, prev: Option<Hlc>) -> Change {
        Change {
            v: FORMAT,
            hlc: at,
            prev,
            body: Body::Annotation(Annotation {
                id: id.into(),
                book: BookKey("id:moby".into()),
                anchored_content_hash: "h".into(),
                color: "yellow".into(),
                cfi_range: "epubcfi(/6/4!/4/2,/1:0,/1:5)".into(),
                quote_exact: "Call me".into(),
                quote_prefix: String::new(),
                quote_suffix: String::new(),
                note: text.map(str::to_string),
                created_at: 1,
                deleted,
            }),
        }
    }

    fn pos(fraction: f64, at: Hlc) -> Change {
        Change {
            v: FORMAT,
            hlc: at,
            prev: None,
            body: Body::Position(Position {
                book: BookKey("id:moby".into()),
                cfi: format!("epubcfi(/6/{})", (fraction * 100.0) as u32),
                fraction,
            }),
        }
    }

    #[test]
    fn the_clock_moves_forward_even_when_the_wall_clock_does_not() {
        let mut c = Clock::new("a");
        let first = c.tick(1000);
        let second = c.tick(1000);
        let third = c.tick(900); // the wall clock went back
        assert!(first < second && second < third);
    }

    #[test]
    fn a_clock_far_in_the_future_is_set_aside() {
        let mut c = Clock::new("a");
        assert!(!c.observe(&hlc(1000 + MAX_AHEAD_MS + 1, 0, "b"), 1000));
        assert!(c.tick(1000).ms == 1000, "nothing was learnt from it");
        assert!(c.observe(&hlc(1000 + MAX_AHEAD_MS, 0, "b"), 1000));
        let mut r = Replica::default();
        let far = pos(0.9, hlc(10 * MAX_AHEAD_MS, 0, "b"));
        let set_aside = r.absorb(&mut Clock::new("a"), 1000, &[Line::Known(far)]);
        assert_eq!(set_aside.from_the_future, 1);
        assert!(r.records.is_empty());
    }

    #[test]
    fn the_newest_position_wins_even_when_it_is_earlier_in_the_book() {
        let newer_but_earlier = pos(0.2, hlc(2000, 0, "b"));
        let older_but_further = pos(0.8, hlc(1000, 0, "a"));
        assert_eq!(
            merge(Some(&older_but_further), &newer_but_earlier),
            Ok(Apply::Take(newer_but_earlier.clone()))
        );
        assert_eq!(
            merge(Some(&newer_but_earlier), &older_but_further),
            Err(Skip::Older)
        );
    }

    #[test]
    fn a_change_already_applied_is_not_applied_again() {
        let p = pos(0.5, hlc(1000, 0, "b"));
        assert_eq!(merge(Some(&p), &p), Err(Skip::Older));
    }

    #[test]
    fn concurrent_note_edits_keep_both_texts() {
        let base = hlc(1000, 0, "a");
        let mine = note(
            "n1",
            Some("whales"),
            false,
            hlc(2000, 0, "a"),
            Some(base.clone()),
        );
        let theirs = note("n1", Some("the sea"), false, hlc(2001, 0, "b"), Some(base));
        let Ok(Apply::ConflictCopy { winner, copy }) = merge(Some(&mine), &theirs) else {
            panic!("expected a conflict copy");
        };
        assert_eq!(winner, theirs);
        let Body::Annotation(a) = &copy.body else {
            panic!()
        };
        assert_eq!(a.note.as_deref(), Some("whales"));
        assert_eq!(a.id, "n1~a.2000.0");
    }

    #[test]
    fn an_edit_made_from_the_other_version_is_not_a_conflict() {
        let mine = note("n1", Some("whales"), false, hlc(2000, 0, "a"), None);
        let theirs = note(
            "n1",
            Some("whales!"),
            false,
            hlc(3000, 0, "b"),
            Some(mine.hlc.clone()),
        );
        assert_eq!(merge(Some(&mine), &theirs), Ok(Apply::Take(theirs.clone())));
    }

    #[test]
    fn a_deletion_wins_over_older_edits_and_an_undo_brings_it_back() {
        let made = note("n1", Some("x"), false, hlc(1000, 0, "a"), None);
        let deleted = note(
            "n1",
            Some("x"),
            true,
            hlc(2000, 0, "b"),
            Some(made.hlc.clone()),
        );
        let undone = note(
            "n1",
            Some("x"),
            false,
            hlc(3000, 0, "b"),
            Some(deleted.hlc.clone()),
        );
        assert_eq!(
            merge(Some(&made), &deleted),
            Ok(Apply::Take(deleted.clone()))
        );
        assert_eq!(merge(Some(&deleted), &made), Err(Skip::Older));
        assert_eq!(
            merge(Some(&deleted), &undone),
            Ok(Apply::Take(undone.clone()))
        );
    }

    #[test]
    fn records_from_a_newer_format_are_kept_as_they_are() {
        let raw = r#"{"v":2,"hlc":{"ms":1,"n":0,"device":"z"},"body":{"kind":"bookmark","x":1}}"#;
        assert_eq!(parse_line(raw), Some(Line::Unknown(raw.to_string())));
        let mut r = Replica::default();
        r.absorb(
            &mut Clock::new("a"),
            10,
            &[Line::Unknown(raw.into()), Line::Unknown(raw.into())],
        );
        assert_eq!(r.unknown, vec![raw.to_string()]);
        assert_eq!(parse_line("not json"), None);
    }

    #[test]
    fn a_book_is_the_same_book_on_every_device() {
        assert_eq!(
            BookKey::of(Some(" urn:uuid:1 "), "h1"),
            BookKey::of(Some("urn:uuid:1"), "h2")
        );
        assert_eq!(BookKey::of(None, "h1"), BookKey("hash:h1".into()));
        assert_eq!(BookKey::of(Some(""), "h1"), BookKey("hash:h1".into()));
    }

    #[test]
    fn a_restored_library_on_another_machine_gets_its_own_device_id() {
        let here = device_id("mac-1", "lib-1");
        assert_eq!(here, device_id("mac-1", "lib-1"));
        assert_ne!(here, device_id("mac-2", "lib-1"));
        assert_ne!(here, device_id("mac-1", "lib-2"));
        assert_eq!(here.len(), 16);
    }

    #[test]
    fn log_files_are_recognised_and_others_ignored() {
        assert_eq!(log_device("ab12.jsonl"), Some("ab12".into()));
        assert_eq!(log_device("ab12 2.jsonl"), Some("ab12".into()));
        assert_eq!(log_device(".ab12.jsonl.tmp"), None);
        assert_eq!(log_device(".ab12.jsonl.icloud"), None);
        assert_eq!(log_device("notes.txt"), None);
    }

    #[test]
    fn the_folder_adapter_round_trips_and_reads_conflict_copies() {
        let dir = tempfile::tempdir().unwrap();
        let folder = FolderAdapter::new(dir.path());
        assert!(
            folder.read_all().unwrap().is_empty(),
            "a missing folder is empty"
        );
        folder.append("a", &["1".into(), "2".into()]).unwrap();
        folder.append("a", &["3".into()]).unwrap();
        std::fs::write(folder.root.join("a 2.jsonl"), "4\n").unwrap();
        std::fs::write(folder.root.join(".b.jsonl.tmp"), "half").unwrap();
        let mut lines: Vec<String> = folder
            .read_all()
            .unwrap()
            .into_iter()
            .map(|(_, l)| l)
            .collect();
        lines.sort();
        assert_eq!(lines, ["1", "2", "3", "4"]);
    }

    /// The soak test (plan Phase 13): three devices make random changes, go offline,
    /// have skewed clocks, and receive each other's logs late, repeated and out of
    /// order. Afterwards every device holds the same records and no note text is lost.
    #[test]
    fn three_devices_converge_and_lose_no_note_text() {
        for seed in 1..=40u64 {
            soak(seed, 300);
        }
    }

    fn soak(seed: u64, steps: usize) {
        let mut state = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut rand = move |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % n
        };
        let names = ["a", "b", "c"];
        let skew = [0i64, 4000, -3000];
        let mut clocks: Vec<Clock> = names.iter().map(|d| Clock::new(*d)).collect();
        let mut replicas = vec![Replica::default(); 3];
        let mut logs: Vec<Vec<Change>> = vec![Vec::new(); 3];
        let mut written_notes: Vec<String> = Vec::new();
        let mut wall: u64 = 1_000_000;
        for _ in 0..steps {
            wall += rand(50);
            let d = rand(3) as usize;
            let now = (wall as i64 + skew[d]) as u64;
            match rand(5) {
                0 => {
                    let c = pos(rand(1000) as f64 / 1000.0, clocks[d].tick(now));
                    replicas[d].records.insert(c.key(), c.clone());
                    logs[d].push(c);
                }
                1..=3 => {
                    let id = format!("n{}", rand(4));
                    let key = format!("ann:{id}");
                    let prev = replicas[d].records.get(&key).map(|c| c.hlc.clone());
                    let deleted = rand(6) == 0;
                    let text = format!("{}-{}", names[d], rand(1_000_000));
                    if !deleted {
                        written_notes.push(text.clone());
                    }
                    let c = note(&id, Some(&text), deleted, clocks[d].tick(now), prev);
                    replicas[d].records.insert(key, c.clone());
                    logs[d].push(c);
                }
                _ => {
                    // Device d syncs a random, possibly stale and repeated, slice of
                    // one other device's log, shuffled.
                    let from = rand(3) as usize;
                    let upto = rand(logs[from].len() as u64 + 1) as usize;
                    let mut slice: Vec<Line> = logs[from][..upto]
                        .iter()
                        .cloned()
                        .map(Line::Known)
                        .collect();
                    for i in (1..slice.len()).rev() {
                        slice.swap(i, rand(i as u64 + 1) as usize);
                    }
                    let made = replicas[d].absorb(&mut clocks[d], now, &slice);
                    logs[d].extend(made.copies);
                }
            }
        }
        // Everyone exchanges every log until nothing new appears (copies made in
        // one round travel in the next), and their own again, harmlessly.
        for round in 0.. {
            assert!(round < 5, "seed {seed}: no settling");
            let all: Vec<Line> = logs.iter().flatten().cloned().map(Line::Known).collect();
            let mut new = 0;
            for d in 0..3 {
                let now = (wall as i64 + skew[d]) as u64 + 10_000;
                let made = replicas[d].absorb(&mut clocks[d], now, &all);
                new += made.copies.len();
                logs[d].extend(made.copies);
            }
            if new == 0 {
                break;
            }
        }
        let state = |r: &Replica| -> BTreeMap<String, Hlc> {
            r.records
                .iter()
                .map(|(k, c)| (k.clone(), c.hlc.clone()))
                .collect()
        };
        assert_eq!(
            state(&replicas[0]),
            state(&replicas[1]),
            "seed {seed}: a and b differ"
        );
        assert_eq!(
            state(&replicas[1]),
            state(&replicas[2]),
            "seed {seed}: b and c differ"
        );
        // No text is invented, and every concurrent loser's text is still present.
        let texts: Vec<String> = replicas[0]
            .records
            .values()
            .filter_map(|c| match &c.body {
                Body::Annotation(a) if !a.deleted => a.note.clone(),
                _ => None,
            })
            .collect();
        for t in &texts {
            assert!(written_notes.contains(t), "seed {seed}: invented text {t}");
        }
        for copy in logs.iter().flatten().filter(|c| c.key().contains('~')) {
            let Body::Annotation(a) = &copy.body else {
                unreachable!()
            };
            assert!(
                texts.contains(a.note.as_ref().unwrap()),
                "seed {seed}: lost copy"
            );
        }
    }
}
