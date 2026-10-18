//! SP3, SP11: the strict JSON sync reads, and RFC 8785's canonical form it hashes.
//!
//! Storage is untrusted input (SY28). Every protocol file is parsed with duplicate
//! keys refused, nesting held to 32 and integers only, within ±(2^53 − 1); bodies,
//! which may hold fractions (a place's position), are parsed the same way except
//! that they may hold any number. serde_json already refuses invalid UTF-8 and
//! unpaired surrogate escapes.

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

pub const MAX_DEPTH: usize = 32;
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JsonError {
    #[error("not valid JSON")]
    Syntax,
    #[error("starts with a byte-order mark")]
    ByteOrderMark,
    #[error("a key appears twice")]
    DuplicateKey,
    #[error("nested deeper than {MAX_DEPTH}")]
    TooDeep,
    #[error("a number that isn't a safe integer")]
    NotSafeInteger,
}

/// What numbers a document may hold.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Numbers {
    /// Protocol files (SP3): safe integers only.
    Integers,
    /// Bodies (SP12): any JSON number.
    Any,
}

const DUP: &str = "linen-sync:duplicate-key";
const DEEP: &str = "linen-sync:too-deep";
const NUM: &str = "linen-sync:not-safe-integer";

struct Strict {
    depth: usize,
    numbers: Numbers,
}

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        if self.numbers == Numbers::Integers && v.unsigned_abs() > MAX_SAFE_INTEGER {
            return Err(E::custom(NUM));
        }
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        if self.numbers == Numbers::Integers && v > MAX_SAFE_INTEGER {
            return Err(E::custom(NUM));
        }
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        if self.numbers == Numbers::Integers {
            return Err(E::custom(NUM));
        }
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom(NUM))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let inner = self.inner()?;
        let mut out = Vec::new();
        while let Some(v) = seq.next_element_seed(inner.clone_seed())? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let inner = self.inner()?;
        let mut out = Map::new();
        while let Some(k) = map.next_key::<String>()? {
            if out.contains_key(&k) {
                return Err(de::Error::custom(DUP));
            }
            let v = map.next_value_seed(inner.clone_seed())?;
            out.insert(k, v);
        }
        Ok(Value::Object(out))
    }
}

impl Strict {
    fn inner<E: de::Error>(&self) -> Result<Strict, E> {
        if self.depth + 1 > MAX_DEPTH {
            return Err(E::custom(DEEP));
        }
        Ok(Strict {
            depth: self.depth + 1,
            numbers: self.numbers,
        })
    }

    fn clone_seed(&self) -> Strict {
        Strict {
            depth: self.depth,
            numbers: self.numbers,
        }
    }
}

/// SP3: parse untrusted bytes. The caller has already checked their size (SY28).
pub fn parse(bytes: &[u8], numbers: Numbers) -> Result<Value, JsonError> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(JsonError::ByteOrderMark);
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let v = Strict { depth: 0, numbers }
        .deserialize(&mut d)
        .and_then(|v| d.end().map(|_| v))
        .map_err(|e| {
            let m = e.to_string();
            if m.starts_with(DUP) {
                JsonError::DuplicateKey
            } else if m.starts_with(DEEP) {
                JsonError::TooDeep
            } else if m.starts_with(NUM) {
                JsonError::NotSafeInteger
            } else {
                JsonError::Syntax
            }
        })?;
    Ok(v)
}

/// SP11: RFC 8785's canonical form of a protocol value. Protocol values hold only
/// integers (SP3), so no float formatting arises; a value parsed with
/// `Numbers::Integers` is always accepted.
pub fn canonical(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        // Integers print the same in RFC 8785 as in serde_json.
        Value::Number(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => write_string(s, out),
        Value::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(x, out);
            }
            out.push(b']');
        }
        Value::Object(m) => {
            // RFC 8785 §3.2.3: keys sorted by their UTF-16 code units.
            let mut keys: Vec<(&String, Vec<u16>)> =
                m.keys().map(|k| (k, k.encode_utf16().collect())).collect();
            keys.sort_by(|a, b| a.1.cmp(&b.1));
            out.push(b'{');
            for (i, (k, _)) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(k, out);
                out.push(b':');
                write_canonical(&m[k.as_str()], out);
            }
            out.push(b'}');
        }
    }
}

/// RFC 8785 §3.2.2.2: ECMAScript's string serialisation.
fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{9}' => out.extend_from_slice(b"\\t"),
            '\u{a}' => out.extend_from_slice(b"\\n"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\u{d}' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes())
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strict(s: &str) -> Result<Value, JsonError> {
        parse(s.as_bytes(), Numbers::Integers)
    }

    #[test]
    fn refuses_what_sp3_refuses() {
        assert_eq!(strict(r#"{"a":1,"a":2}"#), Err(JsonError::DuplicateKey));
        assert_eq!(
            strict(r#"{"a":{"b":1,"b":1}}"#),
            Err(JsonError::DuplicateKey)
        );
        assert_eq!(strict("1.5"), Err(JsonError::NotSafeInteger));
        assert_eq!(strict("1e3"), Err(JsonError::NotSafeInteger));
        assert_eq!(strict("9007199254740992"), Err(JsonError::NotSafeInteger));
        assert_eq!(strict("-9007199254740992"), Err(JsonError::NotSafeInteger));
        assert!(strict("9007199254740991").is_ok());
        assert_eq!(
            strict(r#""\ud800""#),
            Err(JsonError::Syntax),
            "unpaired surrogate"
        );
        assert_eq!(
            parse(b"\xef\xbb\xbf{}", Numbers::Integers),
            Err(JsonError::ByteOrderMark)
        );
        assert_eq!(strict("{} {}"), Err(JsonError::Syntax), "trailing data");
        assert_eq!(strict("{"), Err(JsonError::Syntax));
    }

    #[test]
    fn nesting_is_held_to_32() {
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(strict(&ok).is_ok());
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(strict(&deep), Err(JsonError::TooDeep));
    }

    #[test]
    fn bodies_may_hold_fractions_but_not_duplicates() {
        let v = parse(br#"{"fraction":0.25}"#, Numbers::Any).unwrap();
        assert_eq!(v["fraction"], 0.25);
        assert_eq!(
            parse(br#"{"a":0.1,"a":0.2}"#, Numbers::Any),
            Err(JsonError::DuplicateKey)
        );
    }

    #[test]
    fn canonical_form_follows_rfc_8785() {
        // RFC 8785 §3.2.3's sorting example (keys as UTF-16 code units).
        let v = strict(
            r#"{"€":"Euro Sign","\r":"Carriage Return","דּ":"Hebrew Letter Dalet With Dagesh","1":"One","😀":"Emoji: Grinning Face","\u0080":"Control","ö":"Latin Small Letter O With Diaeresis"}"#,
        )
        .unwrap();
        let out = String::from_utf8(canonical(&v)).unwrap();
        // The carriage return is written escaped, as `\r`.
        let order = [
            "\\r",
            "1",
            "\u{80}",
            "\u{f6}",
            "\u{20ac}",
            "\u{1f600}",
            "\u{fb33}",
        ];
        let mut at = 0;
        for k in order {
            let p = out[at..].find(&format!("\"{k}\":")).expect(k) + at;
            at = p + 1;
        }
        // §3.2.2.2's escapes, and no whitespace.
        let v = strict(r#"{"s":"€$\u000f\nA'B\"\\\\\"/","n":[1,-2,3]}"#).unwrap();
        assert_eq!(
            String::from_utf8(canonical(&v)).unwrap(),
            "{\"n\":[1,-2,3],\"s\":\"\u{20ac}$\\u000f\\nA'B\\\"\\\\\\\\\\\"/\"}"
        );
    }

    #[test]
    fn canonical_form_ignores_how_the_input_was_written() {
        let a = strict(r#"{ "b" : [ 1 , 2 ], "a" : "A" }"#).unwrap();
        let b = strict(r#"{"a":"A","b":[1,2]}"#).unwrap();
        assert_eq!(canonical(&a), canonical(&b));
    }
}
