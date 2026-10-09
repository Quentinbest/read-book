//! Reading MDX and MDD dictionaries (Reading Lens DX3, DX10, DX13).
//!
//! New code from the format's published notes; no GPL or AGPL reader is used (DX12).
//! A file is as hostile as an EPUB: every length and count is checked against the
//! file's size and the caps below before anything is read or decompressed, and a
//! record over a cap is a data error, never shown or partly shown. Records are read
//! by offset, one block at a time; nothing loads whole (DX10).
//!
//! Supported: MDX and MDD 1.2 and 2.0; `Encrypted` 0 and 2 (the key index); zlib or
//! uncompressed blocks; records in UTF-8, UTF-16LE, GBK, GB18030 or Big5. Refused with
//! a specific error: registration-protected files (`Encrypted` 1), LZO blocks and any
//! other encoding.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// DX13 (prov.): sizes after decompression.
pub const MAX_HEADER: u64 = 1024 * 1024;
pub const MAX_BLOCK: u64 = 16 * 1024 * 1024;
pub const MAX_ENTRY: u64 = 4 * 1024 * 1024;
pub const MAX_RESOURCE: u64 = 16 * 1024 * 1024;
/// The key index of a whole dictionary, after decompression.
const MAX_KEY_INDEX: u64 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MdxError {
    #[error("the file could not be read: {0}")]
    Io(String),
    #[error("this isn't an MDX or MDD dictionary")]
    NotMdx,
    #[error("this dictionary is registration-protected")]
    Registered,
    #[error("this dictionary uses LZO compression, which Linen doesn't read")]
    Lzo,
    #[error("this dictionary's text is in {0}, which Linen doesn't read")]
    Encoding(String),
    #[error("this dictionary's format version {0} isn't supported")]
    Version(String),
    #[error("the dictionary is damaged: {0}")]
    Corrupt(&'static str),
    #[error("a part of the dictionary is larger than Linen allows: {0}")]
    TooLarge(&'static str),
}

impl From<std::io::Error> for MdxError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            MdxError::Corrupt("it ends early")
        } else {
            MdxError::Io(e.to_string())
        }
    }
}

type Result<T> = std::result::Result<T, MdxError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16,
    Gbk,
    Gb18030,
    Big5,
}

impl TextEncoding {
    fn parse(name: &str) -> Result<Self> {
        Ok(match name.trim().to_ascii_uppercase().as_str() {
            "" | "UTF-8" | "UTF8" => TextEncoding::Utf8,
            "UTF-16" | "UTF-16LE" | "UTF16" => TextEncoding::Utf16,
            "GBK" | "GB2312" => TextEncoding::Gbk,
            "GB18030" => TextEncoding::Gb18030,
            "BIG5" | "BIG-5" => TextEncoding::Big5,
            other => return Err(MdxError::Encoding(other.to_string())),
        })
    }

    fn wide(self) -> bool {
        self == TextEncoding::Utf16
    }

    /// Decode text; malformed bytes become U+FFFD rather than failing the entry.
    pub fn decode(self, bytes: &[u8]) -> String {
        let enc = match self {
            TextEncoding::Utf8 => encoding_rs::UTF_8,
            TextEncoding::Utf16 => encoding_rs::UTF_16LE,
            TextEncoding::Gbk => encoding_rs::GBK,
            TextEncoding::Gb18030 => encoding_rs::GB18030,
            TextEncoding::Big5 => encoding_rs::BIG5,
        };
        enc.decode_without_bom_handling(bytes).0.into_owned()
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub version: String,
    pub v2: bool,
    pub encrypted: u8,
    /// Key and record text; MDD keys are always UTF-16 and MDD records are bytes.
    pub encoding: TextEncoding,
    pub title: String,
    pub description: String,
    /// Style sheet substitutions (`StyleSheet`), kept as written.
    pub stylesheet: String,
}

/// One headword and where its record starts in the plain record data.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub text: String,
    pub offset: u64,
}

#[derive(Debug, Clone, Copy)]
struct RecordBlock {
    /// Where the block starts in the file.
    file_at: u64,
    comp: u64,
    plain: u64,
    /// Where its plain data starts in the whole record data.
    plain_at: u64,
}

#[derive(Debug, Clone, Copy)]
struct KeyBlock {
    file_at: u64,
    comp: u64,
    plain: u64,
}

/// An open dictionary: its header, where its key blocks are, and its record index.
/// Keys are read on demand (`keys`), records by offset (`record`).
pub struct Mdx {
    path: std::path::PathBuf,
    pub header: Header,
    pub is_mdd: bool,
    pub entries: u64,
    file_len: u64,
    key_blocks: Vec<KeyBlock>,
    record_blocks: Vec<RecordBlock>,
    /// The size of the whole plain record data.
    pub records_len: u64,
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |n, &b| (n << 8) | b as u64)
}

/// A reader over a byte slice that fails, rather than panics, at its end.
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or(MdxError::Corrupt("an index runs past its end"))?;
        let s = &self.data[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn num(&mut self, v2: bool) -> Result<u64> {
        Ok(be(self.take(if v2 { 8 } else { 4 })?))
    }
    fn done(&self) -> bool {
        self.at >= self.data.len()
    }
}

/// `Encrypted=2`: the key index's payload is keyed by its own checksum.
fn decrypt_key_index(block: &mut [u8]) {
    use ripemd::{Digest, Ripemd128};
    let mut h = Ripemd128::new();
    h.update(&block[4..8]);
    h.update(0x3695u32.to_le_bytes());
    let key = h.finalize();
    let mut previous = 0x36u8;
    for (i, c) in block[8..].iter_mut().enumerate() {
        let byte = *c;
        let t = byte.rotate_left(4) ^ previous ^ (i as u8) ^ key[i % key.len()];
        previous = byte;
        *c = t;
    }
}

/// A compressed block: type, adler32 of the plain data, payload. `plain` is what the
/// index says it holds; the result must be exactly that long and within `cap`.
fn unpack(block: &[u8], plain: u64, cap: u64, what: &'static str) -> Result<Vec<u8>> {
    if plain > cap {
        return Err(MdxError::TooLarge(what));
    }
    if block.len() < 8 {
        return Err(MdxError::Corrupt("a block is too short"));
    }
    let kind = u32::from_le_bytes(block[0..4].try_into().unwrap());
    let sum = u32::from_be_bytes(block[4..8].try_into().unwrap());
    let payload = &block[8..];
    let out = match kind {
        0 => payload.to_vec(),
        1 => return Err(MdxError::Lzo),
        2 => {
            // Read at most one byte past the stated size: a bomb stops there.
            let mut out = Vec::with_capacity(plain.min(cap) as usize);
            flate2::read::ZlibDecoder::new(payload)
                .take(plain + 1)
                .read_to_end(&mut out)
                .map_err(|_| MdxError::Corrupt("a block does not decompress"))?;
            out
        }
        _ => return Err(MdxError::Corrupt("a block has an unknown compression")),
    };
    if out.len() as u64 != plain {
        return Err(MdxError::Corrupt(
            "a block's size is not what its index says",
        ));
    }
    if adler32(&out) != sum {
        return Err(MdxError::Corrupt("a block's checksum does not match"));
    }
    Ok(out)
}

/// The header's attributes: `<Dictionary A="…" B="…"/>`, decoded from UTF-16.
fn attributes(xml: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut rest = xml;
    while let Some(eq) = rest.find("=\"") {
        let name = rest[..eq]
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_string();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        let value = after[..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        out.push((name, value));
        rest = &after[end + 1..];
    }
    out
}

impl Mdx {
    pub fn open(path: &Path) -> Result<Mdx> {
        let is_mdd = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("mdd"));
        let mut f = File::open(path)?;
        let file_len = f.metadata()?.len();
        let mut four = [0u8; 4];
        f.read_exact(&mut four)?;
        let header_len = u32::from_be_bytes(four) as u64;
        if header_len < 4 || 4 + header_len + 4 > file_len {
            return Err(MdxError::NotMdx);
        }
        if header_len > MAX_HEADER {
            return Err(MdxError::TooLarge("the header"));
        }
        let mut header_bytes = vec![0u8; header_len as usize];
        f.read_exact(&mut header_bytes)?;
        f.read_exact(&mut four)?;
        if u32::from_le_bytes(four) != adler32(&header_bytes) {
            return Err(MdxError::NotMdx);
        }
        let units: Vec<u16> = header_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let xml = String::from_utf16_lossy(&units);
        let xml = xml.trim_end_matches('\0');
        if !xml.trim_start().starts_with("<Dictionary")
            && !xml.trim_start().starts_with("<Library_Data")
        {
            return Err(MdxError::NotMdx);
        }
        let attrs = attributes(xml);
        let get = |k: &str| {
            attrs
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let version = get("GeneratedByEngineVersion");
        let major = version.trim().split('.').next().unwrap_or("");
        let v2 = match major {
            // MDX 3 is a different layout: refused by its version, not misread.
            "2" => true,
            "1" => false,
            _ => return Err(MdxError::Version(version)),
        };
        let encrypted = match get("Encrypted").trim() {
            "" | "No" | "0" => 0,
            "Yes" | "1" => 1,
            "2" => 2,
            "3" => 3,
            _ => return Err(MdxError::Corrupt("the header's Encrypted is unknown")),
        };
        // DX3: a registration-protected dictionary (its keyword header is encrypted
        // with the buyer's registration) is refused before activation, as DRM books are.
        if encrypted & 1 == 1 {
            return Err(MdxError::Registered);
        }
        let encoding = if is_mdd {
            TextEncoding::Utf16
        } else {
            TextEncoding::parse(&get("Encoding"))?
        };
        let header = Header {
            version,
            v2,
            encrypted,
            encoding,
            title: get("Title"),
            description: get("Description"),
            stylesheet: get("StyleSheet"),
        };

        // Keyword section.
        let mut at = 4 + header_len + 4;
        let head_len = if v2 { 40 } else { 16 };
        let mut head = vec![0u8; head_len];
        f.read_exact(&mut head)?;
        at += head_len as u64;
        if v2 {
            f.read_exact(&mut four)?;
            at += 4;
            if u32::from_be_bytes(four) != adler32(&head) {
                return Err(MdxError::Corrupt(
                    "the key header's checksum does not match",
                ));
            }
        }
        let mut c = Cursor { data: &head, at: 0 };
        let num_key_blocks = c.num(v2)?;
        let entries = c.num(v2)?;
        let (index_plain, index_comp) = if v2 {
            let plain = c.num(true)?;
            (plain, c.num(true)?)
        } else {
            let n = c.num(false)?;
            (n, n)
        };
        let key_blocks_len = c.num(v2)?;
        // DX13: counts and lengths against what the file can hold.
        let left = file_len.saturating_sub(at);
        if index_comp > left || key_blocks_len > left - index_comp.min(left) {
            return Err(MdxError::Corrupt("the key index runs past the file's end"));
        }
        if index_plain > MAX_KEY_INDEX {
            return Err(MdxError::TooLarge("the key index"));
        }
        // Every entry takes at least a few bytes of its key block.
        if entries > key_blocks_len.saturating_mul(16) || num_key_blocks > entries.max(1) {
            return Err(MdxError::Corrupt(
                "the entry count is more than the file holds",
            ));
        }
        let mut index = vec![0u8; index_comp as usize];
        f.read_exact(&mut index)?;
        at += index_comp;
        let index = if v2 {
            if encrypted & 2 == 2 {
                if index.len() < 8 {
                    return Err(MdxError::Corrupt("the key index is too short"));
                }
                decrypt_key_index(&mut index);
            }
            unpack(&index, index_plain, MAX_KEY_INDEX, "the key index")?
        } else {
            index
        };
        let wide = encoding.wide();
        let mut c = Cursor {
            data: &index,
            at: 0,
        };
        let mut key_blocks = Vec::with_capacity(num_key_blocks.min(1 << 20) as usize);
        let mut block_at = at;
        let mut counted = 0u64;
        while !c.done() {
            let n = c.num(v2)?;
            counted = counted.saturating_add(n);
            for _ in 0..2 {
                let size = if v2 { be(c.take(2)?) } else { be(c.take(1)?) } as usize;
                let bytes = if wide { size * 2 } else { size };
                let term = if v2 {
                    if wide {
                        2
                    } else {
                        1
                    }
                } else {
                    0
                };
                c.take(bytes + term)?;
            }
            let comp = c.num(v2)?;
            let plain = c.num(v2)?;
            if plain > MAX_BLOCK {
                return Err(MdxError::TooLarge("a key block"));
            }
            key_blocks.push(KeyBlock {
                file_at: block_at,
                comp,
                plain,
            });
            block_at = block_at
                .checked_add(comp)
                .ok_or(MdxError::Corrupt("a key block's size overflows"))?;
        }
        if key_blocks.len() as u64 != num_key_blocks || counted != entries {
            return Err(MdxError::Corrupt("the key index does not match its header"));
        }
        if block_at != at + key_blocks_len {
            return Err(MdxError::Corrupt("the key blocks do not match their index"));
        }
        at += key_blocks_len;

        // Record section.
        f.seek(SeekFrom::Start(at))?;
        let mut head = vec![0u8; if v2 { 32 } else { 16 }];
        f.read_exact(&mut head)?;
        at += head.len() as u64;
        let mut c = Cursor { data: &head, at: 0 };
        let num_record_blocks = c.num(v2)?;
        let record_entries = c.num(v2)?;
        let record_index_len = c.num(v2)?;
        let record_blocks_len = c.num(v2)?;
        let pair = if v2 { 16 } else { 8 };
        if record_entries != entries
            || record_index_len != num_record_blocks.saturating_mul(pair)
            || record_index_len.saturating_add(record_blocks_len) > file_len.saturating_sub(at)
        {
            return Err(MdxError::Corrupt(
                "the record index runs past the file's end",
            ));
        }
        let mut rindex = vec![0u8; record_index_len as usize];
        f.read_exact(&mut rindex)?;
        at += record_index_len;
        let mut c = Cursor {
            data: &rindex,
            at: 0,
        };
        let mut record_blocks = Vec::with_capacity(num_record_blocks as usize);
        let (mut file_at, mut plain_at) = (at, 0u64);
        while !c.done() {
            let comp = c.num(v2)?;
            let plain = c.num(v2)?;
            record_blocks.push(RecordBlock {
                file_at,
                comp,
                plain,
                plain_at,
            });
            file_at = file_at
                .checked_add(comp)
                .ok_or(MdxError::Corrupt("a record block's size overflows"))?;
            plain_at = plain_at
                .checked_add(plain)
                .ok_or(MdxError::Corrupt("a record block's size overflows"))?;
        }
        if file_at != at + record_blocks_len || file_at > file_len {
            return Err(MdxError::Corrupt(
                "the record blocks do not match their index",
            ));
        }
        Ok(Mdx {
            path: path.to_path_buf(),
            header,
            is_mdd,
            entries,
            file_len,
            key_blocks,
            record_blocks,
            records_len: plain_at,
        })
    }

    fn read_at(&self, f: &mut File, at: u64, len: u64) -> Result<Vec<u8>> {
        if at.checked_add(len).map_or(true, |e| e > self.file_len) {
            return Err(MdxError::Corrupt("a block runs past the file's end"));
        }
        f.seek(SeekFrom::Start(at))?;
        let mut buf = vec![0u8; len as usize];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Every headword with its record offset, block by block (for indexing).
    pub fn keys(&self, mut each: impl FnMut(Key)) -> Result<()> {
        let mut f = File::open(&self.path)?;
        let (v2, wide) = (self.header.v2, self.header.encoding.wide());
        let id = if v2 { 8 } else { 4 };
        let mut seen = 0u64;
        for b in &self.key_blocks {
            let raw = self.read_at(&mut f, b.file_at, b.comp)?;
            let plain = unpack(&raw, b.plain, MAX_BLOCK, "a key block")?;
            let mut i = 0usize;
            while i < plain.len() {
                if i + id > plain.len() {
                    return Err(MdxError::Corrupt("a key block ends inside a key"));
                }
                let offset = be(&plain[i..i + id]);
                i += id;
                let start = i;
                let end = if wide {
                    let mut j = i;
                    while j + 1 < plain.len() && !(plain[j] == 0 && plain[j + 1] == 0) {
                        j += 2;
                    }
                    j
                } else {
                    plain[i..]
                        .iter()
                        .position(|&x| x == 0)
                        .map_or(plain.len(), |p| i + p)
                };
                if end >= plain.len() {
                    return Err(MdxError::Corrupt("a key has no end"));
                }
                if offset > self.records_len {
                    return Err(MdxError::Corrupt("a key points past the records"));
                }
                each(Key {
                    text: self.header.encoding.decode(&plain[start..end]),
                    offset,
                });
                seen += 1;
                i = end + if wide { 2 } else { 1 };
            }
        }
        if seen != self.entries {
            return Err(MdxError::Corrupt(
                "the key blocks hold a different number of keys",
            ));
        }
        Ok(())
    }

    /// The record data [start, end): one entry (MDX) or one resource (MDD), within the caps.
    pub fn record(&self, start: u64, end: u64) -> Result<Vec<u8>> {
        let cap = if self.is_mdd { MAX_RESOURCE } else { MAX_ENTRY };
        if end < start || end > self.records_len {
            return Err(MdxError::Corrupt("an entry's place is outside the records"));
        }
        if end - start > cap {
            return Err(MdxError::TooLarge(if self.is_mdd {
                "a resource"
            } else {
                "an entry"
            }));
        }
        let first = self
            .record_blocks
            .partition_point(|b| b.plain_at + b.plain <= start);
        let mut f = File::open(&self.path)?;
        let mut out = Vec::with_capacity((end - start) as usize);
        for b in &self.record_blocks[first.min(self.record_blocks.len())..] {
            if b.plain_at >= end && end > start {
                break;
            }
            let raw = self.read_at(&mut f, b.file_at, b.comp)?;
            let plain = unpack(&raw, b.plain, MAX_BLOCK, "a record block")?;
            let from = start.saturating_sub(b.plain_at) as usize;
            let to = (end.min(b.plain_at + b.plain) - b.plain_at) as usize;
            out.extend_from_slice(&plain[from.min(to)..to]);
            if b.plain_at + b.plain >= end {
                break;
            }
        }
        Ok(out)
    }

    /// An MDX entry's text, without its terminator.
    pub fn entry_text(&self, start: u64, end: u64) -> Result<String> {
        let bytes = self.record(start, end)?;
        let enc = self.header.encoding;
        let trimmed: &[u8] = if enc.wide() {
            let mut n = bytes.len() & !1;
            while n >= 2 && bytes[n - 2] == 0 && bytes[n - 1] == 0 {
                n -= 2;
            }
            &bytes[..n]
        } else {
            let mut n = bytes.len();
            while n > 0 && bytes[n - 1] == 0 {
                n -= 1;
            }
            &bytes[..n]
        };
        Ok(enc.decode(trimmed))
    }
}

/// Every key with the end of its record: the next distinct offset, or the records' end.
pub fn with_ends(mdx: &Mdx, mut keys: Vec<Key>) -> Vec<(Key, u64)> {
    let mut offsets: Vec<u64> = keys.iter().map(|k| k.offset).collect();
    offsets.sort_unstable();
    offsets.dedup();
    keys.sort_by(|a, b| a.offset.cmp(&b.offset));
    keys.into_iter()
        .map(|k| {
            let i = offsets.partition_point(|&o| o <= k.offset);
            let end = offsets.get(i).copied().unwrap_or(mdx.records_len);
            (k, end)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mdx")
            .join(name)
    }

    fn all(name: &str) -> Vec<(String, String)> {
        let m = Mdx::open(&fixture(name)).unwrap();
        let mut keys = vec![];
        m.keys(|k| keys.push(k)).unwrap();
        with_ends(&m, keys)
            .into_iter()
            .map(|(k, end)| (k.text.clone(), m.entry_text(k.offset, end).unwrap()))
            .collect()
    }

    fn entry(name: &str, word: &str) -> String {
        all(name)
            .into_iter()
            .find(|(k, _)| k == word)
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("{word} not in {name}"))
    }

    #[test]
    fn reads_an_encrypted_v2_dictionary_with_its_header() {
        let m = Mdx::open(&fixture("basic.mdx")).unwrap();
        assert!(m.header.v2);
        assert_eq!(m.header.encrypted, 2);
        assert_eq!(m.header.title, "Basic Test Dictionary");
        assert_eq!(m.entries, 12);
        let e = entry("basic.mdx", "invalidate");
        assert!(
            e.contains("in·val·i·date") && e.contains("证明……不成立"),
            "{e}"
        );
        assert_eq!(entry("basic.mdx", "caches"), "@@@LINK=cache");
    }

    #[test]
    fn keeps_duplicate_headwords_apart() {
        let entries = all("basic.mdx");
        let inval: Vec<_> = entries
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("invalidate"))
            .collect();
        assert_eq!(inval.len(), 2);
        assert_ne!(inval[0].1, inval[1].1);
    }

    #[test]
    fn reads_version_1_2_and_uncompressed_blocks() {
        for name in ["v1.mdx", "stored.mdx"] {
            assert!(entry(name, "cache").contains("a store of data"), "{name}");
            assert_eq!(all(name).len(), 12, "{name}");
        }
    }

    #[test]
    fn reads_every_supported_encoding_the_same() {
        let want = entry("basic.mdx", "cache");
        for name in [
            "enc-utf16.mdx",
            "enc-gbk.mdx",
            "enc-gb18030.mdx",
            "enc-big5.mdx",
        ] {
            assert_eq!(entry(name, "cache"), want, "{name}");
        }
        assert!(entry("enc-gbk.mdx", "缓存").contains("高速缓冲存储器"));
        assert!(entry("enc-gb18030.mdx", "缓存").contains("高速缓冲存储器"));
        assert!(entry("enc-big5.mdx", "緩存").contains("高速緩衝儲存器"));
        assert!(entry("enc-utf16.mdx", "café").contains("small restaurant"));
    }

    #[test]
    fn reads_mdd_resources_as_bytes() {
        let m = Mdx::open(&fixture("basic.1.mdd")).unwrap();
        assert!(m.is_mdd);
        let mut keys = vec![];
        m.keys(|k| keys.push(k)).unwrap();
        let (k, end) = with_ends(&m, keys).remove(0);
        assert_eq!(k.text, "\\dot.png");
        assert!(m.record(k.offset, end).unwrap().starts_with(b"\x89PNG"));
    }

    #[test]
    fn refuses_what_it_does_not_read_with_a_reason() {
        let open = |n: &str| Mdx::open(&fixture(n)).err();
        assert_eq!(open("registered.mdx"), Some(MdxError::Registered));
        assert_eq!(
            open("unknown-encoding.mdx"),
            Some(MdxError::Encoding("KOI8-R".into()))
        );
        assert!(matches!(open("truncated.mdx"), Some(MdxError::Corrupt(_))));
        assert!(matches!(open("counts.mdx"), Some(MdxError::Corrupt(_))));
        // LZO: the header and index are fine; the first block refuses.
        let m = Mdx::open(&fixture("lzo.mdx")).unwrap();
        assert_eq!(m.keys(|_| {}).err(), Some(MdxError::Lzo));
        // Not a dictionary at all.
        let epub = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(Mdx::open(&epub).err(), Some(MdxError::NotMdx));
    }

    #[test]
    fn caps_hold_after_decompression() {
        // A block that decompresses past 16 MB is refused before it is inflated.
        let m = Mdx::open(&fixture("bomb.mdx")).unwrap();
        let mut keys = vec![];
        m.keys(|k| keys.push(k)).unwrap();
        let (k, end) = with_ends(&m, keys).remove(0);
        assert_eq!(
            m.record(k.offset, end).err(),
            Some(MdxError::TooLarge("an entry"))
        );
        // An entry over 4 MB is an error; the one next to it still reads.
        let m = Mdx::open(&fixture("big-entry.mdx")).unwrap();
        let mut keys = vec![];
        m.keys(|k| keys.push(k)).unwrap();
        for (k, end) in with_ends(&m, keys) {
            let r = m.entry_text(k.offset, end);
            if k.text == "big" {
                assert_eq!(r.err(), Some(MdxError::TooLarge("an entry")));
            } else {
                assert_eq!(r.unwrap(), "ok");
            }
        }
    }

    /// Files from another writer: `LINEN_MDX_SAMPLES=<folder> cargo test -- --ignored external`.
    /// Each one opens and reads every entry, or is refused with a reason; none panics.
    #[test]
    #[ignore]
    fn reads_external_samples() {
        let Ok(dir) = std::env::var("LINEN_MDX_SAMPLES") else {
            return;
        };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
            if !ext.eq_ignore_ascii_case("mdx") && !ext.eq_ignore_ascii_case("mdd") {
                continue;
            }
            let outcome = Mdx::open(&p).and_then(|m| {
                let mut keys = vec![];
                m.keys(|k| keys.push(k))?;
                let n = keys.len();
                let mut first = String::new();
                for (k, end) in with_ends(&m, keys) {
                    let r = if m.is_mdd {
                        m.record(k.offset, end)
                            .map(|b| format!("{} bytes", b.len()))
                    } else {
                        m.entry_text(k.offset, end)
                    }?;
                    if first.is_empty() {
                        first = format!("{} → {}", k.text, r.chars().take(60).collect::<String>());
                    }
                }
                Ok(format!(
                    "v{} enc {} {:?}, {n} keys; {first}",
                    m.header.version, m.header.encrypted, m.header.encoding
                ))
            });
            println!(
                "{}: {:?}",
                p.file_name().unwrap().to_string_lossy(),
                outcome
            );
        }
    }

    #[test]
    fn a_damaged_block_is_an_error_not_a_panic() {
        let data = std::fs::read(fixture("basic.mdx")).unwrap();
        let dir = std::env::temp_dir().join(format!("linen-mdx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Flip bytes across the file: every outcome is Ok or an error, never a panic.
        for i in (0..data.len()).step_by(7) {
            let mut d = data.clone();
            d[i] ^= 0x5a;
            let p = dir.join("flip.mdx");
            std::fs::write(&p, &d).unwrap();
            if let Ok(m) = Mdx::open(&p) {
                let mut keys = vec![];
                if m.keys(|k| keys.push(k)).is_ok() {
                    for (k, end) in with_ends(&m, keys) {
                        let _ = m.entry_text(k.offset, end);
                    }
                }
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
