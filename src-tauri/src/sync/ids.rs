//! SP1, SP2, SP4: identifiers, hashes and the way they are written.
//!
//! Every name and ID sync writes is lowercase hex, since both storages fold case
//! (SP2). Hashes are SHA-256; a derived hash starts with a length-prefixed domain
//! string, while an identity hash (a payload, a body, a book file) is plain SHA-256
//! of the exact bytes (SP4).

use sha2::{Digest, Sha256};
use std::fmt;

/// A SHA-256 value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash32(pub [u8; 32]);

/// A 16-byte ID: a library, a device or a writer token (SP1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id16(pub [u8; 16]);

/// A device's ID: the first 16 bytes of `H("linen-sync/device-id/v1" ‖ key)`, so an
/// ID names exactly one key (SP1).
pub type DeviceId = Id16;

/// Plain SHA-256 of exact bytes: a payload, a body or a book file (SP4).
pub fn identity_hash(bytes: &[u8]) -> Hash32 {
    Hash32(Sha256::digest(bytes).into())
}

/// SHA-256 of a domain string and parts, each preceded by its length as 4 bytes
/// big-endian, as SP4's exact inputs say for variable-length fields. Fixed-length
/// fields (IDs, hashes) go in `fixed` without a length.
pub fn derived_hash(domain: &str, fixed: &[&[u8]], variable: &[&[u8]]) -> Hash32 {
    let mut h = Sha256::new();
    h.update((domain.len() as u32).to_be_bytes());
    h.update(domain.as_bytes());
    for f in fixed {
        h.update(f);
    }
    for v in variable {
        h.update((v.len() as u32).to_be_bytes());
        h.update(v);
    }
    Hash32(h.finalize().into())
}

/// SP1: a device's ID from its Ed25519 public key.
pub fn device_id(public_key: &[u8; 32]) -> DeviceId {
    let h = derived_hash("linen-sync/device-id/v1", &[public_key], &[]);
    let mut id = [0u8; 16];
    id.copy_from_slice(&h.0[..16]);
    Id16(id)
}

fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for b in bytes {
        write!(f, "{b:02x}")?;
    }
    Ok(())
}

/// Lowercase hex only: an upper-case digit is a different name in storage that
/// folds to the same file (SP2), so it is refused, never normalised.
fn parse_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let b = s.as_bytes();
    if b.len() != N * 2 {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    for (i, pair) in b.chunks(2).enumerate() {
        out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(out)
}

impl Hash32 {
    pub fn parse(s: &str) -> Option<Self> {
        parse_hex::<32>(s).map(Hash32)
    }
}

impl Id16 {
    pub fn parse(s: &str) -> Option<Self> {
        parse_hex::<16>(s).map(Id16)
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

impl fmt::Display for Id16 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

impl fmt::Debug for Id16 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_refuses_upper_case() {
        let s = "00ff10a0b1c2d3e4f5061728394a5b6c";
        let id = Id16::parse(s).unwrap();
        assert_eq!(id.to_string(), s);
        // SP2: an upper-case name folds onto the lower-case one in storage.
        assert!(Id16::parse(&s.to_uppercase()).is_none());
        assert!(Id16::parse("00ff").is_none());
        assert!(Id16::parse("zz".repeat(16).as_str()).is_none());
    }

    #[test]
    fn identity_hash_is_plain_sha256() {
        // SP4: identity hashes carry no domain prefix.
        assert_eq!(
            identity_hash(b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn derived_hashes_separate_domains_and_lengths() {
        let a = derived_hash("linen-sync/a/v1", &[], &[b"xy", b"z"]);
        let b = derived_hash("linen-sync/a/v1", &[], &[b"x", b"yz"]);
        let c = derived_hash("linen-sync/b/v1", &[], &[b"xy", b"z"]);
        assert_ne!(a, b, "length prefixes keep part boundaries");
        assert_ne!(a, c, "domains keep purposes apart");
    }

    #[test]
    fn a_device_id_names_one_key() {
        let k1 = [1u8; 32];
        let k2 = [2u8; 32];
        assert_eq!(device_id(&k1), device_id(&k1));
        assert_ne!(device_id(&k1), device_id(&k2));
    }
}
