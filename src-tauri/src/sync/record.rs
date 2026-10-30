//! SY9, SY28, SP11–SP13: record keys, versions and the checks every one passes.
//!
//! A version is one record's state as one device wrote it. Storage is untrusted
//! (SY28): every version is checked here (format, key, writer, clock, history, size)
//! before anything uses it, and one that fails is set aside, never applied. A version
//! in a newer major format is kept aside whole and passed on unchanged (SY9).

use super::clock::{Hlc, MAX_MS};
use super::ids::{b64url_decode, derived_hash, identity_hash, DeviceId, Hash32, Id16};
use super::json::{self, Numbers};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fmt;

/// SY9: the major format this Linen reads and writes.
pub const FORMAT: u64 = 1;
/// SP1: a record key is at most 256 bytes.
pub const MAX_KEY_BYTES: usize = 256;
/// SP12: a choice's inline body holds no reader text and stays small.
pub const MAX_INLINE_BYTES: usize = 4 * 1024;

const KB: u64 = 1024;
const MB: u64 = 1024 * 1024;

/// What a record is (SY9, SP12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Place,
    Annotation,
    Note,
    Setting,
    BookSetting,
    Book,
    BookFile,
    BookOpened,
    BookFinished,
    Choice,
}

impl Kind {
    /// SY28 (prov., item 106): each body type's limit, checked from the size a
    /// version states before the body is read, and again while reading it.
    pub fn body_limit(self) -> u64 {
        match self {
            Kind::Place => 8 * KB,
            Kind::Annotation | Kind::Note => 4 * MB,
            Kind::Setting | Kind::BookSetting => 64 * KB,
            Kind::Book | Kind::BookFile => MB,
            Kind::BookOpened | Kind::BookFinished => KB,
            Kind::Choice => MAX_INLINE_BYTES as u64,
        }
    }
}

/// A record key (SY9), kept as the exact string it was written as.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(String);

impl Key {
    /// SY9, SP1, SP12: a key in one of the forms the plan lists. Book and
    /// annotation IDs are version-4 UUIDs written lowercase with hyphens.
    /// SY9: the key of a version in a newer major format, kept as it came, within
    /// the same size limit: this release doesn't interpret it, and such a version
    /// is never applied or rewritten. Only `as_str` and `spine_hash` are meaningful.
    pub(crate) fn opaque(s: &str) -> Option<Key> {
        (!s.is_empty() && s.len() <= MAX_KEY_BYTES).then(|| Key(s.to_owned()))
    }

    pub fn parse(s: &str) -> Option<(Key, Kind)> {
        if s.len() > MAX_KEY_BYTES {
            return None;
        }
        let (prefix, rest) = s.split_once(':')?;
        let kind = match prefix {
            "place" if is_uuid(rest) => Kind::Place,
            "annotation" if is_uuid(rest) => Kind::Annotation,
            "note" if is_uuid(rest) => Kind::Note,
            "book" if is_uuid(rest) => Kind::Book,
            "bookFile" if is_uuid(rest) => Kind::BookFile,
            "setting" if is_setting_name(rest) => Kind::Setting,
            "bookSetting" => {
                let (book, name) = rest.split_once(':')?;
                if !is_uuid(book) || !is_setting_name(name) {
                    return None;
                }
                Kind::BookSetting
            }
            "bookRead" => match rest.split_once(':')? {
                (book, "opened") if is_uuid(book) => Kind::BookOpened,
                (book, "finished") if is_uuid(book) => Kind::BookFinished,
                _ => return None,
            },
            "choice" if is_choice_target(rest) => Kind::Choice,
            _ => return None,
        };
        Some((Key(s.to_owned()), kind))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn kind(&self) -> Kind {
        Key::parse(&self.0).map(|(_, k)| k).unwrap_or(Kind::Choice)
    }

    /// The book or annotation UUID a key names, if any.
    fn subject(&self) -> Option<&str> {
        self.0.split(':').nth(1)
    }

    /// SP17: the 16-byte key hash a spine keeps beside each version hash: the
    /// first 16 bytes of plain `H(key)`.
    pub fn spine_hash(&self) -> [u8; 16] {
        let h = identity_hash(self.0.as_bytes());
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.0[..16]);
        out
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|u| {
        u.get_version_num() == 4 && u.get_variant() == uuid::Variant::RFC4122 && u.to_string() == s
    })
}

fn is_setting_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
}

/// SP1: a sequence from 1 to 2^53 − 1, written without leading zeros.
fn is_seq(s: &str) -> bool {
    !s.starts_with('0')
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u64>()
            .is_ok_and(|n| (1..=json::MAX_SAFE_INTEGER).contains(&n))
}

/// The days in a month of the proleptic Gregorian calendar `toISOString` uses.
fn days_in(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// A9's timestamps, as `toISOString` writes them: `YYYY-MM-DDTHH:MM:SS.sssZ`.
fn is_iso_time(t: &str) -> bool {
    let b = t.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        t.get(r)?.bytes().try_fold(0u32, |n, c| {
            c.is_ascii_digit().then(|| n * 10 + (c - b'0') as u32)
        })
    };
    b.len() == 24
        && [
            (4, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'.'),
            (23, b'Z'),
        ]
        .iter()
        .all(|&(i, c)| b[i] == c)
        && num(0..4).is_some()
        && num(5..7).is_some_and(|m| (1..=12).contains(&m))
        && num(8..10).is_some_and(|d| {
            (1..=days_in(num(0..4).unwrap_or(0), num(5..7).unwrap_or(0))).contains(&d)
        })
        && num(11..13).is_some_and(|h| h < 24)
        && num(14..16).is_some_and(|m| m < 60)
        && num(17..19).is_some_and(|s| s < 60)
        && num(20..23).is_some()
}

/// SP12, A9: a highlight's Web Annotation JSON, as `fromW3C` reads it: its id, a
/// CFI fragment selector, a text-quote selector, a colour and an anchor status. Its
/// note is a record of its own (`note:`), so its `body` holds no text.
fn is_annotation(o: &Map<String, Value>, uuid: &str) -> bool {
    let s = |v: &Value, k: &str| v.get(k).is_some_and(Value::is_string);
    // SY8: the book is named by its sync ID, a lowercase UUID URN.
    let book = |t: &Value| {
        t.get("source")
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix("urn:uuid:"))
            .is_some_and(is_uuid)
    };
    let selectors = o
        .get("target")
        .filter(|t| book(t))
        .and_then(|t| t.get("selector"))
        .and_then(Value::as_array);
    // The first selector of a type is the one `fromW3C` reads. Its `anchor` field
    // must be non-empty: an empty CFI or quote can't place the highlight.
    let has = |kind: &str, anchor: &str, fields: &[&str]| {
        selectors.is_some_and(|sel| {
            sel.iter()
                .find(|x| x.get("type").and_then(Value::as_str) == Some(kind))
                .is_some_and(|x| {
                    fields.iter().all(|f| s(x, f))
                        && x.get(anchor)
                            .and_then(Value::as_str)
                            .is_some_and(|v| !v.is_empty())
                })
        })
    };
    o.get("type").and_then(Value::as_str) == Some("Annotation")
        && o.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.strip_prefix("urn:uuid:") == Some(uuid))
        && ["created", "modified"]
            .iter()
            .all(|k| o.get(*k).and_then(Value::as_str).is_some_and(is_iso_time))
        // The book file's SHA-256 the anchors belong to, as `import` writes it.
        && o.get("linen:anchoredContentHash")
            .and_then(Value::as_str)
            .and_then(Hash32::parse)
            .is_some()
        && o.get("body")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        && has("FragmentSelector", "value", &["value"])
        && has("TextQuoteSelector", "exact", &["exact", "prefix", "suffix"])
        && matches!(
            o.get("linen:color").and_then(Value::as_str),
            Some("yellow" | "green" | "blue" | "rose")
        )
        && matches!(
            o.get("linen:anchorStatus").and_then(Value::as_str),
            Some("anchored" | "reanchor" | "unplaced")
        )
}

/// SP12's choice keys: `‹d›.‹s›.‹h›`, `obj.‹name›`, `hold.‹d›.‹s›.‹i›`,
/// `reserve.‹d›.‹s›.‹i›` and `late.‹d›.‹s›` (`s` may be 0 there, a null cutoff).
fn is_choice_target(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    let index = |p: &str| p == "0" || is_seq(p);
    match parts.as_slice() {
        ["late", d, seq] => Id16::parse(d).is_some() && index(seq),
        ["obj", name] => Hash32::parse(name).is_some(),
        ["hold" | "reserve", d, seq, i] => Id16::parse(d).is_some() && is_seq(seq) && index(i),
        [d, seq, h] => Id16::parse(d).is_some() && is_seq(seq) && Hash32::parse(h).is_some(),
        _ => false,
    }
}

/// SP13: `{ "hashes": [plain file hashes], "idTitle": hash or null }`.
fn is_book_identity(v: &Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    o.len() == 2
        && o.get("hashes").and_then(Value::as_array).is_some_and(|a| {
            a.iter()
                .all(|h| h.as_str().and_then(Hash32::parse).is_some())
        })
        && o.get("idTitle")
            .is_some_and(|t| t.is_null() || t.as_str().and_then(Hash32::parse).is_some())
        // At least one way to match a copy that turns up later (SY8).
        && (o["hashes"].as_array().is_some_and(|a| !a.is_empty()) || !o["idTitle"].is_null())
}

/// SP12: each kind of choice key takes its own choices and fields.
fn is_choice_body(key: &str, v: &Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    let target = key.strip_prefix("choice:").unwrap_or_default();
    let choice = o.get("choice").and_then(Value::as_str).unwrap_or_default();
    let hash = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .and_then(Hash32::parse)
            .is_some()
    };
    let sig = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .and_then(b64url_decode)
            .is_some_and(|b| b.len() == 64)
    };
    match target.split('.').next().unwrap_or_default() {
        "obj" => {
            choice == "try-again"
                && o.get("clears").and_then(Value::as_array).is_some_and(|a| {
                    a.iter().all(|c| {
                        c.get("d")
                            .and_then(Value::as_str)
                            .and_then(Id16::parse)
                            .is_some()
                            && c.get("q")
                                .and_then(Value::as_u64)
                                .is_some_and(|q| (1..=json::MAX_SAFE_INTEGER).contains(&q))
                    })
                })
        }
        "hold" | "reserve" => choice == "stop-waiting",
        "late" => choice == "discard-all" && hash("removal") && hash("head") && sig("code"),
        // `‹d›.‹s›.‹h›`: a choice about one version. A Keep names its replacement.
        _ => match choice {
            "keep" => o.get("version").is_some_and(|e| {
                e.get("d")
                    .and_then(Value::as_str)
                    .and_then(Id16::parse)
                    .is_some()
                    && e.get("s").and_then(Value::as_u64).is_some_and(|s| s >= 1)
                    && e.get("h")
                        .and_then(Value::as_str)
                        .and_then(Hash32::parse)
                        .is_some()
            }),
            "discard" | "wrong-clock" | "stop-waiting" => true,
            _ => false,
        },
    }
}

/// A `{ "d", "s", "h" }` entry: a version ID in `history` (SP11), or a change
/// frontier in a tombstone's `covers` (SP13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub device: DeviceId,
    pub seq: u64,
    pub hash: Hash32,
}

/// What a version holds instead of, or as, its body.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    /// SP12: the body's plain hash and size; the body itself is a blob.
    Body { hash: Hash32, size: u64 },
    /// SP13: a tombstone, with its causal frontier and, for a book, its identity.
    Deleted {
        covers: Vec<Entry>,
        identity: Option<Value>,
    },
    /// SP12: a choice's small inline body.
    Inline(Value),
}

/// A version that passed SY28's checks.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub key: Key,
    pub kind: Kind,
    pub device: DeviceId,
    pub clock: Hlc,
    /// SP11: at most one entry per device.
    pub history: Vec<Entry>,
    pub content: Content,
    /// SP11: `H("linen-sync/version/v1" ‖ JCS(version))`.
    pub hash: Hash32,
    /// The version exactly as received, unknown fields and all (SY9).
    pub raw: Value,
}

/// The outcome of checking one version.
#[derive(Debug, PartialEq)]
pub enum Checked {
    Version(Version),
    /// SY9: a newer major format, kept aside and passed on, never applied or rewritten.
    NewerFormat(Value),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RecordError {
    #[error("a version must be an object")]
    NotAnObject,
    #[error("field `{0}` is missing or malformed")]
    Field(&'static str),
    #[error("not a key sync knows")]
    Key,
    #[error("written by another device than its change file's")]
    WrongDevice,
    #[error("two history entries for one device")]
    DuplicateHistory,
    #[error("names more devices than a library has")]
    TooManyDevices,
    #[error("its change file holds another version of its key")]
    DuplicateKey,
    #[error("holds more than one of body, deleted and inline, or none")]
    Content,
    #[error("its body is over its type's limit")]
    TooLarge,
    #[error("an inline body on a record that isn't a choice")]
    InlineNotAllowed,
    #[error("body: {0}")]
    Json(#[from] json::JsonError),
    #[error("the body doesn't match its type")]
    Body,
}

fn hash_field(v: &Value, name: &'static str) -> Result<Hash32, RecordError> {
    v.get(name)
        .and_then(Value::as_str)
        .and_then(Hash32::parse)
        .ok_or(RecordError::Field(name))
}

/// SY2 (prov., item 106): a library has at most this many devices in a pass, so a
/// version's `history` or a tombstone's `covers` names no more. PROVISIONAL: Stage
/// 1c refuses entries for devices that aren't members.
pub const MAX_DEVICES: usize = 32;

fn entries(v: &Value, name: &'static str) -> Result<Vec<Entry>, RecordError> {
    let list = v.as_array().ok_or(RecordError::Field(name))?;
    if list.len() > MAX_DEVICES {
        return Err(RecordError::TooManyDevices);
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for e in list {
        let device = e
            .get("d")
            .and_then(Value::as_str)
            .and_then(Id16::parse)
            .ok_or(RecordError::Field(name))?;
        let seq = e
            .get("s")
            .and_then(Value::as_u64)
            .filter(|&s| s >= 1)
            .ok_or(RecordError::Field(name))?;
        let hash = hash_field(e, "h").map_err(|_| RecordError::Field(name))?;
        if !seen.insert(device) {
            return Err(RecordError::DuplicateHistory);
        }
        out.push(Entry { device, seq, hash });
    }
    Ok(out)
}

/// SP11: a version's hash.
pub fn version_hash(raw: &Value) -> Hash32 {
    derived_hash("linen-sync/version/v1", &[], &[&json::canonical(raw)])
}

/// SY28: check one version from device `writer`'s change file (SP11, SP12, SP13).
pub fn check_version(raw: Value, writer: DeviceId) -> Result<Checked, RecordError> {
    let obj: &Map<String, Value> = raw.as_object().ok_or(RecordError::NotAnObject)?;
    let format = obj
        .get("format")
        .and_then(Value::as_u64)
        .filter(|&f| f >= 1)
        .ok_or(RecordError::Field("format"))?;
    if format > FORMAT {
        return Ok(Checked::NewerFormat(raw));
    }
    let (key, kind) = obj
        .get("key")
        .and_then(Value::as_str)
        .ok_or(RecordError::Field("key"))
        .and_then(|k| Key::parse(k).ok_or(RecordError::Key))?;
    let device = obj
        .get("device")
        .and_then(Value::as_str)
        .and_then(Id16::parse)
        .ok_or(RecordError::Field("device"))?;
    if device != writer {
        return Err(RecordError::WrongDevice);
    }
    let clock = match obj
        .get("clock")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    {
        Some([ms, n]) => match (ms.as_u64(), n.as_u64()) {
            (Some(ms), Some(n)) if ms <= MAX_MS && n <= u16::MAX as u64 => Hlc(ms, n as u16),
            _ => return Err(RecordError::Field("clock")),
        },
        _ => return Err(RecordError::Field("clock")),
    };
    let history = entries(
        obj.get("history").ok_or(RecordError::Field("history"))?,
        "history",
    )?;

    let content = match (obj.get("body"), obj.get("deleted"), obj.get("inline")) {
        (Some(b), None, None) => {
            if kind == Kind::Choice {
                return Err(RecordError::Content);
            }
            let size = b
                .get("size")
                .and_then(Value::as_u64)
                .ok_or(RecordError::Field("body"))?;
            if size > kind.body_limit() {
                // SY28: refused from its stated size, before anything is fetched.
                return Err(RecordError::TooLarge);
            }
            Content::Body {
                hash: hash_field(b, "hash")?,
                size,
            }
        }
        (None, Some(d), None) => {
            let covers = entries(
                d.get("covers").ok_or(RecordError::Field("deleted"))?,
                "deleted",
            )?;
            let identity = d.get("identity").cloned();
            // SP13: a book's tombstone always carries its identity; nothing else does.
            match (&identity, kind) {
                (Some(id), Kind::Book) if is_book_identity(id) => {}
                (None, k) if k != Kind::Book => {}
                _ => return Err(RecordError::Field("deleted")),
            }
            Content::Deleted { covers, identity }
        }
        (None, None, Some(i)) => {
            if kind != Kind::Choice {
                return Err(RecordError::InlineNotAllowed);
            }
            if json::canonical(i).len() > MAX_INLINE_BYTES || !is_choice_body(key.as_str(), i) {
                return Err(RecordError::Field("inline"));
            }
            Content::Inline(i.clone())
        }
        _ => return Err(RecordError::Content),
    };
    let hash = version_hash(&raw);
    Ok(Checked::Version(Version {
        key,
        kind,
        device,
        clock,
        history,
        content,
        hash,
        raw,
    }))
}

/// SY28: check a body's bytes for its type, after its plain hash has matched.
/// A body that fails is set aside like a version that fails.
/// A body's own IDs must be its key's (SY9): an annotation's `id` and a book's
/// `syncId` name the record they belong to.
pub fn check_body(key: &Key, bytes: &[u8]) -> Result<Value, RecordError> {
    let kind = key.kind();
    let subject = key.subject().unwrap_or_default();
    if bytes.len() as u64 > kind.body_limit() {
        return Err(RecordError::TooLarge);
    }
    let v = json::parse(bytes, Numbers::Any)?;
    let o = v.as_object().ok_or(RecordError::Body)?;
    let str_field = |k: &str| o.get(k).is_some_and(Value::is_string);
    let filled = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.trim().is_empty())
    };
    let ok = match kind {
        Kind::Place => {
            filled("cfi")
                && o.get("fraction")
                    .and_then(Value::as_f64)
                    .is_some_and(|f| (0.0..=1.0).contains(&f))
                && o.get("chapter").map_or(true, Value::is_string)
        }
        Kind::Annotation => is_annotation(o, subject),
        // SY9: emptying a note is a deletion, sent as a tombstone, never as blank text.
        Kind::Note => filled("text"),
        // Settings are stored as strings. Each key's own schema (SY15, SY28) comes
        // with the list of keys that sync (item 103, Stage 3).
        Kind::Setting | Kind::BookSetting => o.get("value").is_some_and(Value::is_string),
        Kind::Book => {
            o.get("syncId").and_then(Value::as_str) == Some(subject)
                && o.get("aliases")
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().all(|x| x.as_str().is_some_and(is_uuid)))
        }
        Kind::BookFile => {
            o.get("hash")
                .and_then(Value::as_str)
                .and_then(Hash32::parse)
                .is_some()
                && o.get("size").and_then(Value::as_u64).is_some()
                && str_field("title")
                && str_field("layout")
                && ["language", "identifier"]
                    .iter()
                    .all(|k| o.get(*k).is_some_and(|v| v.is_string() || v.is_null()))
                && o.get("authors")
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().all(Value::is_string))
        }
        Kind::BookOpened => o.is_empty(),
        Kind::BookFinished => o.get("finished").is_some_and(Value::is_boolean),
        Kind::Choice => false,
    };
    if ok {
        Ok(v)
    } else {
        Err(RecordError::Body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BOOK: &str = "7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
    const D: &str = "a3f0a3f0a3f0a3f0a3f0a3f0a3f0a3f0";
    const E: &str = "51de51de51de51de51de51de51de51de";
    const H: &str = "9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c9b2c";

    fn dev(s: &str) -> DeviceId {
        Id16::parse(s).unwrap()
    }

    fn note(extra: Value) -> Value {
        let mut v = json!({
            "format": 1,
            "key": format!("note:{BOOK}"),
            "device": D,
            "clock": [1760702400123u64, 0],
            "history": [{ "d": E, "s": 7, "h": H }],
            "body": { "hash": H, "size": 312 },
        });
        for (k, x) in extra.as_object().unwrap() {
            if x.is_null() {
                v.as_object_mut().unwrap().remove(k);
            } else {
                v[k] = x.clone();
            }
        }
        v
    }

    fn body_of(kind: Kind, bytes: &[u8]) -> Result<Value, RecordError> {
        let key = match kind {
            Kind::Place => format!("place:{BOOK}"),
            Kind::Annotation => format!("annotation:{BOOK}"),
            Kind::Note => format!("note:{BOOK}"),
            Kind::Setting => "setting:readingPace".to_string(),
            Kind::Book => format!("book:{BOOK}"),
            Kind::BookFile => format!("bookFile:{BOOK}"),
            Kind::BookFinished => format!("bookRead:{BOOK}:finished"),
            _ => unreachable!(),
        };
        check_body(&Key::parse(&key).unwrap().0, bytes)
    }

    fn check(v: Value) -> Result<Checked, RecordError> {
        check_version(v, dev(D))
    }

    #[test]
    fn keys_take_the_forms_sy9_lists() {
        for (k, kind) in [
            (format!("place:{BOOK}"), Kind::Place),
            (
                format!("bookSetting:{BOOK}:simplifyStyles"),
                Kind::BookSetting,
            ),
            (format!("bookRead:{BOOK}:finished"), Kind::BookFinished),
            ("setting:fontPx".into(), Kind::Setting),
            (format!("choice:{D}.12.{H}"), Kind::Choice),
            (format!("choice:late.{D}.0"), Kind::Choice),
            (format!("choice:hold.{D}.3.0"), Kind::Choice),
            (format!("choice:obj.{H}"), Kind::Choice),
        ] {
            assert_eq!(Key::parse(&k).map(|(_, kind)| kind), Some(kind), "{k}");
        }
        for k in [
            format!("place:{}", BOOK.to_uppercase()),
            "place:not-a-uuid".into(),
            format!("bookRead:{BOOK}:started"),
            format!("choice:{D}.012.{H}"),
            "setting:".into(),
            format!("setting:{}", "x".repeat(65)),
            "highlight:x".into(),
            "book:11111111-1111-4111-0111-111111111111".into(),
            format!("choice:{D}.9999999999999999.{H}"),
        ] {
            assert!(Key::parse(&k).is_none(), "{k}");
        }
    }

    #[test]
    fn a_good_version_passes_and_keeps_unknown_fields() {
        let v = note(json!({ "future": { "x": 1 } }));
        let Checked::Version(c) = check(v.clone()).unwrap() else {
            panic!()
        };
        assert_eq!(c.kind, Kind::Note);
        assert_eq!(c.clock, Hlc(1760702400123, 0));
        assert_eq!(c.raw, v, "SY9: unknown fields are kept and passed on");
        assert_eq!(c.hash, version_hash(&v));
        // The unknown field is covered by the hash.
        let Checked::Version(d) = check(note(json!({ "future": { "x": 2 } }))).unwrap() else {
            panic!()
        };
        assert_ne!(c.hash, d.hash);
    }

    #[test]
    fn the_hash_ignores_how_a_version_was_written() {
        let a = json::parse(note(json!({})).to_string().as_bytes(), Numbers::Integers).unwrap();
        let pretty = serde_json::to_string_pretty(&note(json!({}))).unwrap();
        let b = json::parse(pretty.as_bytes(), Numbers::Integers).unwrap();
        assert_eq!(version_hash(&a), version_hash(&b));
    }

    #[test]
    fn checks_refuse_what_sy28_refuses() {
        assert_eq!(
            check_version(note(json!({})), dev(E)),
            Err(RecordError::WrongDevice)
        );
        assert_eq!(
            check(note(
                json!({ "history": [{ "d": E, "s": 1, "h": H }, { "d": E, "s": 2, "h": H }] })
            )),
            Err(RecordError::DuplicateHistory)
        );
        let many = |n: u8| -> Vec<Value> {
            (1..=n)
                .map(|i| json!({ "d": Id16([i; 16]).to_string(), "s": 1, "h": H }))
                .collect()
        };
        assert!(check(note(json!({ "history": many(32) }))).is_ok());
        assert_eq!(
            check(note(json!({ "history": many(33) }))),
            Err(RecordError::TooManyDevices)
        );
        assert_eq!(
            check(note(
                json!({ "body": null, "deleted": { "covers": many(33) } })
            )),
            Err(RecordError::TooManyDevices)
        );
        assert_eq!(
            check(note(json!({ "history": [{ "d": E, "s": 0, "h": H }] }))),
            Err(RecordError::Field("history"))
        );
        assert_eq!(
            check(note(json!({ "clock": [1, 70000] }))),
            Err(RecordError::Field("clock"))
        );
        assert_eq!(
            check(note(json!({ "deleted": { "covers": [] } }))),
            Err(RecordError::Content)
        );
        assert_eq!(
            check(note(json!({ "body": null }))),
            Err(RecordError::Content)
        );
        assert_eq!(
            check(note(json!({ "key": "note:x" }))),
            Err(RecordError::Key)
        );
        assert_eq!(
            check(note(
                json!({ "body": null, "inline": { "choice": "keep" } })
            )),
            Err(RecordError::InlineNotAllowed)
        );
        // A 900 MB note is refused from its stated size, before it is read (Stage 1 check).
        assert_eq!(
            check(note(json!({ "body": { "hash": H, "size": 900u64 * MB } }))),
            Err(RecordError::TooLarge)
        );
    }

    #[test]
    fn a_newer_format_is_kept_aside_whole() {
        let v = json!({ "format": 2, "anything": "goes" });
        assert_eq!(check(v.clone()), Ok(Checked::NewerFormat(v)));
    }

    #[test]
    fn tombstones_and_choices() {
        let t =
            note(json!({ "body": null, "deleted": { "covers": [{ "d": E, "s": 9, "h": H }] } }));
        let Checked::Version(c) = check(t).unwrap() else {
            panic!()
        };
        assert!(
            matches!(c.content, Content::Deleted { ref covers, identity: None } if covers.len() == 1)
        );
        // Only a book's tombstone carries an identity (SP13).
        let bad = note(json!({ "body": null, "deleted": { "covers": [], "identity": {} } }));
        assert_eq!(check(bad), Err(RecordError::Field("deleted")));
        let book_key = format!("book:{BOOK}");
        let good = json!({ "covers": [], "identity": { "hashes": [H], "idTitle": null } });
        assert!(check(note(
            json!({ "key": book_key, "body": null, "deleted": good })
        ))
        .is_ok());
        let missing = json!({ "covers": [] });
        assert_eq!(
            check(note(
                json!({ "key": book_key, "body": null, "deleted": missing })
            )),
            Err(RecordError::Field("deleted"))
        );
        let malformed = json!({ "covers": [], "identity": { "hashes": ["x"], "idTitle": null } });
        let empty = json!({ "covers": [], "identity": { "hashes": [], "idTitle": null } });
        assert_eq!(
            check(note(
                json!({ "key": book_key, "body": null, "deleted": empty })
            )),
            Err(RecordError::Field("deleted"))
        );
        assert_eq!(
            check(note(
                json!({ "key": book_key, "body": null, "deleted": malformed })
            )),
            Err(RecordError::Field("deleted"))
        );
        // SP12: each choice key takes its own body.
        let late = format!("choice:late.{D}.5");
        let bare =
            note(json!({ "key": late, "body": null, "inline": { "choice": "discard-all" } }));
        assert_eq!(
            check(bare),
            Err(RecordError::Field("inline")),
            "Discard all needs its removal, head and code"
        );
        let sig = "A".repeat(86);
        let full = note(
            json!({ "key": late, "body": null, "inline": { "choice": "discard-all", "removal": H, "head": H, "code": sig } }),
        );
        assert!(check(full).is_ok());
        let padbits = note(
            json!({ "key": late, "body": null, "inline": { "choice": "discard-all", "removal": H, "head": H, "code": format!("{}B", "A".repeat(85)) } }),
        );
        assert_eq!(
            check(padbits),
            Err(RecordError::Field("inline")),
            "not a 64-byte signature"
        );
        let keep = note(
            json!({ "key": format!("choice:{D}.12.{H}"), "body": null, "inline": { "choice": "keep" } }),
        );
        assert_eq!(
            check(keep),
            Err(RecordError::Field("inline")),
            "a Keep names its replacement"
        );
        let wrong = note(
            json!({ "key": format!("choice:hold.{D}.3.0"), "body": null, "inline": { "choice": "discard" } }),
        );
        assert_eq!(check(wrong), Err(RecordError::Field("inline")));
        let choice = note(
            json!({ "key": format!("choice:{D}.12.{H}"), "body": null, "inline": { "choice": "discard" } }),
        );
        assert!(matches!(
            check(choice),
            Ok(Checked::Version(Version {
                kind: Kind::Choice,
                ..
            }))
        ));
    }

    #[test]
    fn bodies_are_checked_for_their_type() {
        assert!(body_of(
            Kind::Place,
            br#"{"cfi":"epubcfi(/6/4!/4)","fraction":0.25,"chapter":"I"}"#
        )
        .is_ok());
        assert_eq!(
            body_of(Kind::Place, br#"{"cfi":"x","fraction":1.5}"#),
            Err(RecordError::Body)
        );
        assert!(body_of(Kind::Note, br#"{"text":"hello"}"#).is_ok());
        let anno = format!(
            r#"{{"type":"Annotation","id":"urn:uuid:{BOOK}","created":"2026-10-18T09:00:00.000Z","modified":"2026-10-18T09:00:00.000Z","body":[],
            "target":{{"source":"urn:uuid:{BOOK}","selector":[{{"type":"FragmentSelector","conformsTo":"x","value":"epubcfi(/6/4)"}},
            {{"type":"TextQuoteSelector","exact":"e","prefix":"","suffix":""}}]}},
            "linen:color":"rose","linen:anchorStatus":"anchored","linen:anchoredContentHash":"{H}"}}"#
        );
        assert!(body_of(Kind::Annotation, anno.as_bytes()).is_ok());
        for source in [
            "s",
            "urn:uuid:not-a-uuid",
            &format!("urn:uuid:{}", BOOK.to_uppercase()),
        ] {
            let bad = anno.replace(
                &format!("urn:uuid:{BOOK}\",\"selector"),
                &format!("{source}\",\"selector"),
            );
            assert_ne!(bad, anno);
            assert_eq!(
                body_of(Kind::Annotation, bad.as_bytes()),
                Err(RecordError::Body),
                "{source}"
            );
        }
        assert_eq!(
            body_of(Kind::Annotation, b"{}"),
            Err(RecordError::Body),
            "A9: anchors and a colour"
        );
        let shadowed = anno.replace(
            r#"[{"type":"FragmentSelector""#,
            r#"[{"type":"FragmentSelector","conformsTo":"x"},{"type":"FragmentSelector""#,
        );
        assert_ne!(shadowed, anno);
        assert_eq!(
            body_of(Kind::Annotation, shadowed.as_bytes()),
            Err(RecordError::Body),
            "the first FragmentSelector is the one read"
        );
        let undated = anno.replace(
            r#""created":"2026-10-18T09:00:00.000Z""#,
            r#""created":"invalid""#,
        );
        assert_eq!(
            body_of(Kind::Annotation, undated.as_bytes()),
            Err(RecordError::Body)
        );
        for (date, ok) in [
            ("2026-02-31", false),
            ("2026-02-28", true),
            ("2024-02-29", true),
            ("1900-02-29", false),
            ("2000-02-29", true),
            ("2026-04-31", false),
        ] {
            let t = anno.replace("2026-10-18", date);
            assert_eq!(
                body_of(Kind::Annotation, t.as_bytes()).is_ok(),
                ok,
                "{date}"
            );
        }
        for (from, to) in [
            (r#""value":"epubcfi(/6/4)""#, r#""value":"""#),
            (r#""exact":"e""#, r#""exact":"""#),
        ] {
            let empty = anno.replace(from, to);
            assert_ne!(empty, anno);
            assert_eq!(
                body_of(Kind::Annotation, empty.as_bytes()),
                Err(RecordError::Body),
                "{to}"
            );
        }
        assert_eq!(
            body_of(Kind::Note, br#"{"text":"  "}"#),
            Err(RecordError::Body)
        );
        assert_eq!(
            body_of(Kind::Place, br#"{"cfi":"","fraction":0.5}"#),
            Err(RecordError::Body)
        );
        let unhashed = anno.replace(
            &format!(r#""linen:anchoredContentHash":"{H}""#),
            r#""linen:anchoredContentHash":"""#,
        );
        assert_ne!(unhashed, anno);
        assert_eq!(
            body_of(Kind::Annotation, unhashed.as_bytes()),
            Err(RecordError::Body)
        );
        let purple = anno.replace("rose", "purple");
        assert_eq!(
            body_of(Kind::Annotation, purple.as_bytes()),
            Err(RecordError::Body)
        );
        assert_eq!(
            body_of(Kind::Note, br#"{"text":"a","text":"b"}"#),
            Err(RecordError::Json(json::JsonError::DuplicateKey))
        );
        // A setting value that isn't even JSON (`readingPace` = `{`) is set aside (Stage 1 check).
        assert_eq!(
            body_of(Kind::Setting, b"{"),
            Err(RecordError::Json(json::JsonError::Syntax))
        );
        assert!(body_of(Kind::BookFinished, br#"{"finished":true}"#).is_ok());
        assert!(body_of(Kind::Setting, br#"{"value":"18"}"#).is_ok());
        assert_eq!(
            body_of(Kind::Setting, br#"{"value":{}}"#),
            Err(RecordError::Body)
        );
        let file = format!(
            r#"{{"hash":"{H}","size":1,"title":"T","authors":[],"language":"en","identifier":null,"layout":"reflowable"}}"#
        );
        assert!(body_of(Kind::BookFile, file.as_bytes()).is_ok());
        let partial = format!(r#"{{"hash":"{H}","size":1,"title":"T","authors":[]}}"#);
        assert_eq!(
            body_of(Kind::BookFile, partial.as_bytes()),
            Err(RecordError::Body),
            "SP12: every field a file determines"
        );
        // A 9 MB note is over its limit (SY28).
        let big = format!(r#"{{"text":"{}"}}"#, "x".repeat(9 * MB as usize));
        assert_eq!(
            body_of(Kind::Note, big.as_bytes()),
            Err(RecordError::TooLarge)
        );
        let book = format!(r#"{{"syncId":"{BOOK}","aliases":["{BOOK}"]}}"#);
        assert!(body_of(Kind::Book, book.as_bytes()).is_ok());
        // SY9: a body's own IDs must be its key's.
        let other = "11111111-1111-4111-8111-111111111111";
        assert_eq!(
            body_of(Kind::Book, book.replace(BOOK, other).as_bytes()),
            Err(RecordError::Body)
        );
        assert_eq!(
            body_of(Kind::Annotation, anno.replace(BOOK, other).as_bytes()),
            Err(RecordError::Body)
        );
    }
}
