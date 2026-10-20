//! SP4: the one cryptographic suite, and every derivation SP6, SP9, SP22 and SP24
//! name, with their exact inputs.
//!
//! SP4's exact inputs: fixed-length fields (IDs 16 bytes; hashes, keys and `root`
//! 32; nonces as stated; sequences 8 bytes big-endian) are joined as they are; a
//! domain string or any other variable-length field is preceded by its length as 4
//! bytes big-endian. HKDF's `info` is the domain string's bytes, as given, with a
//! zero-length salt where none is stated. `tests/fixtures/sync-vectors.json`, made
//! by `scripts/sync-vectors.py` from implementations other than these (libsodium,
//! the reference Argon2, Python's hashlib), checks each derivation.
//!
//! PROVISIONAL: where the protocol leaves the bytes open, the reading here is
//! Delegated to Claude, not approved (`docs/decisions.md`, “SP4's exact inputs and
//! the envelope”): each such place says so.

use super::ids::{device_id, identity_hash, DeviceId, Hash32, Id16};
use chacha20poly1305::{aead::Aead, aead::Payload, KeyInit, XChaCha20Poly1305};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("the system's random source failed")]
    Random,
}

/// SP1: bytes from the OS's CSPRNG.
pub fn random<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| CryptoError::Random)?;
    Ok(out)
}

/// SP4's exact inputs, built field by field.
#[derive(Default)]
pub struct Input(Vec<u8>);

impl Input {
    /// A domain string, length-prefixed.
    pub fn domain(domain: &str) -> Self {
        Input::default().var(domain.as_bytes())
    }

    /// A fixed-length field (an ID, a hash, a key, a nonce), joined as it is.
    pub fn fixed(mut self, bytes: &[u8]) -> Self {
        self.0.extend_from_slice(bytes);
        self
    }

    /// A variable-length field, preceded by its length as 4 bytes big-endian.
    pub fn var(mut self, bytes: &[u8]) -> Self {
        self.0
            .extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        self.0.extend_from_slice(bytes);
        self
    }

    /// A sequence or counter, 8 bytes big-endian.
    pub fn seq(self, n: u64) -> Self {
        self.fixed(&n.to_be_bytes())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn hash(&self) -> Hash32 {
        Hash32(Sha256::digest(&self.0).into())
    }
}

/// An Ed25519 key pair: a device's (SP1) or a recovery code's (SP22).
pub struct Keypair(SigningKey);

impl Keypair {
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Keypair(SigningKey::from_bytes(seed))
    }

    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self::from_seed(&random()?))
    }

    pub fn seed(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn public(&self) -> [u8; 32] {
        self.0.verifying_key().to_bytes()
    }

    /// SP1: the device ID this key names.
    pub fn device(&self) -> DeviceId {
        device_id(&self.public())
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.0.sign(message).to_bytes()
    }
}

/// SP4: Ed25519, verified strictly: a canonical `S`, and a public key of small
/// order refused, as is one that isn't a point.
pub fn verify(public: &[u8; 32], message: &[u8], sig: &[u8; 64]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(public) else {
        return false;
    };
    !key.is_weak()
        && key
            .verify_strict(message, &Signature::from_bytes(sig))
            .is_ok()
}

/// Crockford's base32 (SP4, SP22): digits and upper-case letters without I, L, O, U.
pub const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The first `nbits` (a multiple of 5) of `data`, in Crockford's base32.
fn crockford(data: &[u8], nbits: usize) -> String {
    (0..nbits / 5)
        .map(|i| {
            let mut v = 0usize;
            for b in i * 5..i * 5 + 5 {
                v = v << 1 | (data[b / 8] >> (7 - b % 8) & 1) as usize;
            }
            CROCKFORD[v] as char
        })
        .collect()
}

/// SP4: the fingerprint shown to the reader, the first 10 bytes of
/// `H("linen-sync/fpr/v1" ‖ key)` as 4 groups of 4 Crockford characters.
/// PROVISIONAL: the groups separated by spaces (decisions.md, (4)).
pub fn fingerprint(public: &[u8; 32]) -> String {
    let h = Input::domain("linen-sync/fpr/v1").fixed(public).hash();
    groups(&crockford(&h.0[..10], 80), ' ')
}

fn groups(s: &str, sep: char) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

fn hmac(key: &[u8; 32], input: &Input) -> Hash32 {
    let mut m = <Hmac<Sha256>>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(input.bytes());
    Hash32(m.finalize().into_bytes().into())
}

/// SP9: a body's keyed name in `blobs/`, from its plain hash.
pub fn blob_name(naming_key: &[u8; 32], body_hash: &Hash32) -> String {
    hmac(
        naming_key,
        &Input::domain("linen-sync/blob/v1").fixed(&body_hash.0),
    )
    .to_string()
}

/// SP9: a book's keyed name in `books/`, from its file's plain hash.
pub fn book_name(naming_key: &[u8; 32], file_hash: &Hash32) -> String {
    hmac(
        naming_key,
        &Input::domain("linen-sync/book/v1").fixed(&file_hash.0),
    )
    .to_string()
}

/// SP4: HKDF-SHA256, 32 bytes.
fn hkdf(salt: &[u8], ikm: &[u8], info: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    hkdf::Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info.as_bytes(), &mut out)
        .expect("32 bytes is within HKDF's limit");
    out
}

/// SP4: XChaCha20-Poly1305 under `key` with `nonce`; the ciphertext ends with its tag.
fn seal_with(key: &[u8; 32], nonce: &[u8; 24], plaintext: &[u8], ad: &[u8]) -> Vec<u8> {
    XChaCha20Poly1305::new(key.into())
        .encrypt(
            nonce.into(),
            Payload {
                msg: plaintext,
                aad: ad,
            },
        )
        .expect("a 32-byte message is within the AEAD's limits")
}

fn open_with(key: &[u8; 32], nonce: &[u8; 24], ciphertext: &[u8], ad: &[u8]) -> Option<Vec<u8>> {
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            nonce.into(),
            Payload {
                msg: ciphertext,
                aad: ad,
            },
        )
        .ok()
}

/// SP4: X25519, with an all-zero shared secret refused (a public key of small order).
fn x25519(secret: &[u8; 32], public: &[u8; 32]) -> Option<[u8; 32]> {
    let shared = x25519_dalek::StaticSecret::from(*secret)
        .diffie_hellman(&x25519_dalek::PublicKey::from(*public))
        .to_bytes();
    (shared != [0u8; 32]).then_some(shared)
}

/// SP24: an X25519 public key from its secret.
pub fn x25519_public(secret: &[u8; 32]) -> [u8; 32] {
    x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*secret)).to_bytes()
}

/// SP22: the recovery code, its 26 characters (130 bits), never stored.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCode([u8; 26]);

impl std::fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryCode(…)")
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodeError {
    #[error("a recovery code has 28 characters")]
    Length,
    #[error("a character that isn't in a recovery code")]
    Character,
    #[error("the code doesn't check: a typing error")]
    Check,
}

/// SP22: what one code gives: its signing key and its escrow key.
pub struct CodeKeys {
    pub signing: Keypair,
    pub escrow: [u8; 32],
}

impl RecoveryCode {
    /// 26 characters, 5 random bits each, from the CSPRNG.
    pub fn generate() -> Result<Self, CryptoError> {
        let bits: [u8; 17] = random()?;
        let s = crockford(&bits, 130);
        let mut chars = [0u8; 26];
        chars.copy_from_slice(s.as_bytes());
        Ok(RecoveryCode(chars))
    }

    /// The 2 check characters: the first 10 bits of
    /// `H("linen-sync/code-check/v1" ‖ the 26 characters)`. PROVISIONAL: the
    /// characters as ASCII, a fixed field with no length (decisions.md, (3)).
    fn check(chars: &[u8; 26]) -> String {
        let h = Input::domain("linen-sync/code-check/v1")
            .fixed(chars)
            .hash();
        crockford(&h.0[..2], 10)
    }

    /// As it is shown: 28 characters in 7 groups of 4.
    pub fn display(&self) -> String {
        let s = std::str::from_utf8(&self.0).expect("Crockford characters are ASCII");
        groups(&format!("{s}{}", Self::check(&self.0)), ' ')
    }

    /// The reader's input: upper-cased, spaces and hyphens dropped, `O` read as `0`
    /// and `I`, `L` as `1`; a check that fails is a typing error, said at once.
    pub fn parse(input: &str) -> Result<Self, CodeError> {
        let mut s = Vec::with_capacity(28);
        for c in input.chars() {
            let c = match c.to_ascii_uppercase() {
                ' ' | '-' => continue,
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            if !c.is_ascii() || !CROCKFORD.contains(&(c as u8)) {
                return Err(CodeError::Character);
            }
            s.push(c as u8);
        }
        if s.len() != 28 {
            return Err(CodeError::Length);
        }
        let mut chars = [0u8; 26];
        chars.copy_from_slice(&s[..26]);
        if Self::check(&chars).as_bytes() != &s[26..] {
            return Err(CodeError::Check);
        }
        Ok(RecoveryCode(chars))
    }

    /// SP22: `root`, Argon2id over the 26 characters, salted with the library and
    /// the sealed head's 16-byte `nonce`. PROVISIONAL: the password is the 26
    /// characters as ASCII, without the check (decisions.md, (3)).
    pub fn root(&self, library: &Id16, nonce: &[u8; 16]) -> [u8; 32] {
        let salt = Input::domain("linen-sync/code-salt/v1")
            .fixed(&library.0)
            .fixed(nonce)
            .hash();
        let params = argon2::Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(32))
            .expect("SP4's parameters are valid");
        let mut root = [0u8; 32];
        argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password_into(&self.0, &salt.0, &mut root)
            .expect("the inputs fit Argon2's limits");
        root
    }

    /// SP22: the code's keys for one sealed head.
    pub fn keys(&self, library: &Id16, nonce: &[u8; 16]) -> CodeKeys {
        code_keys(&self.root(library, nonce))
    }
}

/// SP4 (prov.): Argon2id's memory in KiB (64 MiB), passes and lanes. PROVISIONAL.
pub const ARGON2_M_KIB: u32 = 64 * 1024;
pub const ARGON2_T: u32 = 3;
pub const ARGON2_P: u32 = 1;

/// SP22: from `root`, the code's signing seed and escrow key.
pub fn code_keys(root: &[u8; 32]) -> CodeKeys {
    CodeKeys {
        signing: Keypair::from_seed(&hkdf(&[], root, "linen-sync/code-sign/v1")),
        escrow: hkdf(&[], root, "linen-sync/escrow/v1"),
    }
}

fn escrow_ad(library: &Id16, head: &Hash32) -> Input {
    Input::domain("linen-sync/escrow/v1")
        .fixed(&library.0)
        .fixed(&head.0)
}

/// SP22: the naming key sealed for a head's escrow, under a random nonce.
pub fn seal_escrow(
    escrow_key: &[u8; 32],
    library: &Id16,
    head: &Hash32,
    naming_key: &[u8; 32],
) -> Result<([u8; 24], Vec<u8>), CryptoError> {
    let nonce = random()?;
    let ct = seal_with(
        escrow_key,
        &nonce,
        naming_key,
        escrow_ad(library, head).bytes(),
    );
    Ok((nonce, ct))
}

/// SP22: an escrow that doesn't open is ignored.
pub fn open_escrow(
    escrow_key: &[u8; 32],
    library: &Id16,
    head: &Hash32,
    nonce: &[u8; 24],
    ct: &[u8],
) -> Option<[u8; 32]> {
    open_with(escrow_key, nonce, ct, escrow_ad(library, head).bytes())?
        .try_into()
        .ok()
}

/// SP24 step 1: the applicant's commitment to `na`.
pub fn commit(
    library: &Id16,
    applicant: &DeviceId,
    key: &[u8; 32],
    kx: &[u8; 32],
    na: &[u8; 32],
) -> Hash32 {
    Input::domain("linen-sync/commit/v1")
        .fixed(&library.0)
        .fixed(&applicant.0)
        .fixed(key)
        .fixed(kx)
        .fixed(na)
        .hash()
}

/// SP24 step 4: what both devices hashed, from the bytes each kept.
pub struct Transcript<'a> {
    pub library: &'a Id16,
    pub request: &'a Hash32,
    pub answer: &'a Hash32,
    pub applicant: &'a DeviceId,
    pub key: &'a [u8; 32],
    pub kx: &'a [u8; 32],
    pub na: &'a [u8; 32],
    pub device: &'a DeviceId,
    pub device_key: &'a [u8; 32],
    pub ne: &'a [u8; 32],
}

impl Transcript<'_> {
    /// `t`.
    pub fn hash(&self) -> Hash32 {
        Input::domain("linen-sync/sas/v1")
            .fixed(&self.library.0)
            .fixed(&self.request.0)
            .fixed(&self.answer.0)
            .fixed(&self.applicant.0)
            .fixed(self.key)
            .fixed(self.kx)
            .fixed(self.na)
            .fixed(&self.device.0)
            .fixed(self.device_key)
            .fixed(self.ne)
            .hash()
    }
}

/// SP24 step 4: the code both screens show, `(the first 8 bytes of t) mod 1,000,000`.
pub fn sas(t: &Hash32) -> u32 {
    let mut first = [0u8; 8];
    first.copy_from_slice(&t.0[..8]);
    (u64::from_be_bytes(first) % 1_000_000) as u32
}

/// As it is shown: `472 913`. PROVISIONAL: zero-padded, two groups of three
/// (decisions.md, (4)).
pub fn sas_display(code: u32) -> String {
    format!("{:03} {:03}", code / 1000, code % 1000)
}

/// SP24 step 6: the naming key sealed to the applicant's `kx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedKey {
    pub epk: [u8; 32],
    pub nonce: [u8; 24],
    pub ct: Vec<u8>,
}

fn enroll_key(shared: &[u8; 32], t: &Hash32) -> [u8; 32] {
    hkdf(&t.0, shared, "linen-sync/enroll-seal/v1")
}

fn enroll_ad(t: &Hash32) -> Input {
    Input::domain("linen-sync/enroll-seal/v1").fixed(&t.0)
}

fn seal_to_with(
    kx: &[u8; 32],
    t: &Hash32,
    naming_key: &[u8; 32],
    esk: &[u8; 32],
    nonce: &[u8; 24],
) -> Option<SealedKey> {
    let shared = x25519(esk, kx)?;
    Some(SealedKey {
        epk: x25519_public(esk),
        nonce: *nonce,
        ct: seal_with(
            &enroll_key(&shared, t),
            nonce,
            naming_key,
            enroll_ad(t).bytes(),
        ),
    })
}

/// SP24 step 6, with a fresh ephemeral key and nonce. `None` when `kx` is of small
/// order (the shared secret would be zero).
pub fn seal_naming_key(
    kx: &[u8; 32],
    t: &Hash32,
    naming_key: &[u8; 32],
) -> Result<Option<SealedKey>, CryptoError> {
    Ok(seal_to_with(kx, t, naming_key, &random()?, &random()?))
}

/// SP24 step 7: the applicant opens the naming key with its `kx` secret.
pub fn open_naming_key(kx_secret: &[u8; 32], t: &Hash32, sealed: &SealedKey) -> Option<[u8; 32]> {
    let shared = x25519(kx_secret, &sealed.epk)?;
    open_with(
        &enroll_key(&shared, t),
        &sealed.nonce,
        &sealed.ct,
        enroll_ad(t).bytes(),
    )?
    .try_into()
    .ok()
}

/// SP5: the payload hash, plain SHA-256 of the payload's exact bytes.
pub fn payload_hash(payload: &[u8]) -> Hash32 {
    identity_hash(payload)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::Value;

    pub(crate) fn vectors() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/sync-vectors.json")).unwrap()
    }

    pub(crate) fn hex<const N: usize>(v: &Value) -> [u8; N] {
        let s = v.as_str().unwrap();
        let mut out = [0u8; N];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    fn hexv(v: &Value) -> Vec<u8> {
        let s = v.as_str().unwrap();
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn a_device_key_its_id_and_fingerprint_match_the_vectors() {
        let v = vectors();
        let k = Keypair::from_seed(&hex(&v["seed"]));
        assert_eq!(k.public(), hex::<32>(&v["public"]));
        assert_eq!(k.device(), Id16(hex(&v["device"])));
        let fpr = fingerprint(&k.public());
        assert_eq!(fpr.replace(' ', ""), v["fingerprint"].as_str().unwrap());
        assert_eq!(fpr.len(), 19, "4 groups of 4");
    }

    #[test]
    fn keyed_names_match_the_vectors() {
        let n = &vectors()["names"];
        let key = hex(&n["naming_key"]);
        let body = identity_hash(n["body"].as_str().unwrap().as_bytes());
        assert_eq!(blob_name(&key, &body), n["blob"].as_str().unwrap());
        let file = identity_hash(&hexv(&n["book_file"]));
        assert_eq!(book_name(&key, &file), n["book"].as_str().unwrap());
        // The two kinds never share a name for the same bytes.
        assert_ne!(blob_name(&key, &file), book_name(&key, &file));
    }

    #[test]
    fn signatures_are_verified_strictly() {
        let k = Keypair::from_seed(&[7; 32]);
        let sig = k.sign(b"m");
        assert!(verify(&k.public(), b"m", &sig));
        assert!(!verify(&k.public(), b"n", &sig));
        // S + L, the same signature in a non-canonical form, is refused.
        let mut s = [0u8; 32];
        s.copy_from_slice(&sig[32..]);
        let l: [u8; 32] = [
            0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9,
            0xde, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
        ];
        let mut carry = 0u16;
        for i in 0..32 {
            let t = s[i] as u16 + l[i] as u16 + carry;
            s[i] = t as u8;
            carry = t >> 8;
        }
        let mut malleated = sig;
        malleated[32..].copy_from_slice(&s);
        assert!(!verify(&k.public(), b"m", &malleated));
        // A key of small order (the identity point) is refused, whatever it signs.
        let mut identity = [0u8; 32];
        identity[0] = 1;
        assert!(!verify(&identity, b"m", &[0; 64]));
    }

    #[test]
    fn the_recovery_code_matches_the_vectors() {
        let c = &vectors()["code"];
        let chars = c["chars"].as_str().unwrap();
        let typed = format!("{chars}{}", c["check"].as_str().unwrap());
        let code = RecoveryCode::parse(&typed).unwrap();
        assert_eq!(code.display().replace(' ', ""), typed);
        assert_eq!(code.display().split(' ').count(), 7);
        let library = Id16(hex(&vectors()["library"]));
        let nonce = hex::<16>(&c["nonce"]);
        let root = code.root(&library, &nonce);
        assert_eq!(root, hex::<32>(&c["root"]));
        let keys = code_keys(&root);
        assert_eq!(keys.signing.seed(), hex::<32>(&c["sign_seed"]));
        assert_eq!(keys.signing.public(), hex::<32>(&c["code_key"]));
        assert_eq!(keys.escrow, hex::<32>(&c["escrow_key"]));
        let head = Hash32(hex(&c["head"]));
        let naming = hex::<32>(&vectors()["names"]["naming_key"]);
        let nonce24 = hex::<24>(&c["escrow_nonce"]);
        let ct = seal_with(
            &keys.escrow,
            &nonce24,
            &naming,
            escrow_ad(&library, &head).bytes(),
        );
        assert_eq!(ct, hexv(&c["escrow_ct"]));
        assert_eq!(
            open_escrow(&keys.escrow, &library, &head, &nonce24, &ct),
            Some(naming)
        );
        // An escrow for another head, or another library, doesn't open.
        assert_eq!(
            open_escrow(&keys.escrow, &library, &Hash32([0; 32]), &nonce24, &ct),
            None
        );
        assert_eq!(
            open_escrow(&keys.escrow, &Id16([0; 16]), &head, &nonce24, &ct),
            None
        );
    }

    #[test]
    fn a_typed_code_is_read_as_sp22_says() {
        let code = RecoveryCode::generate().unwrap();
        let shown = code.display();
        assert_eq!(shown.len(), 34);
        // Lower case, hyphens, and O, I, L for 0, 1, 1 are read the same.
        let typed = shown
            .to_lowercase()
            .replace(' ', "-")
            .replace('0', "o")
            .replace('1', "l");
        assert_eq!(RecoveryCode::parse(&typed), Ok(code.clone()));
        // One character wrong is a typing error, said at once.
        let mut wrong: Vec<char> = shown.replace(' ', "").chars().collect();
        wrong[3] = if wrong[3] == 'A' { 'B' } else { 'A' };
        let wrong: String = wrong.into_iter().collect();
        assert_eq!(RecoveryCode::parse(&wrong), Err(CodeError::Check));
        assert_eq!(RecoveryCode::parse("ABCD"), Err(CodeError::Length));
        assert_eq!(
            RecoveryCode::parse(&shown.replace(' ', "").replacen(|_| true, "U", 1)),
            Err(CodeError::Character)
        );
        // Two codes are different, and a code is never printed.
        assert_ne!(RecoveryCode::generate().unwrap(), code);
        assert_eq!(format!("{code:?}"), "RecoveryCode(…)");
    }

    #[test]
    fn enrollment_matches_the_vectors() {
        let v = vectors();
        let e = &v["enroll"];
        let library = Id16(hex(&v["library"]));
        let applicant_key = Keypair::from_seed(&hex(&e["applicant_seed"]));
        assert_eq!(applicant_key.public(), hex::<32>(&e["applicant_key"]));
        let applicant = applicant_key.device();
        assert_eq!(applicant, Id16(hex(&e["applicant"])));
        let kx_secret = hex::<32>(&e["kx_secret"]);
        let kx = x25519_public(&kx_secret);
        assert_eq!(kx, hex::<32>(&e["kx"]));
        let (na, ne) = (hex::<32>(&e["na"]), hex::<32>(&e["ne"]));
        let key = applicant_key.public();
        assert_eq!(
            commit(&library, &applicant, &key, &kx, &na),
            Hash32(hex(&e["commit"]))
        );
        let device = Keypair::from_seed(&hex(&v["seed"]));
        let t = Transcript {
            library: &library,
            request: &Hash32(hex(&e["request_hash"])),
            answer: &Hash32(hex(&e["answer_hash"])),
            applicant: &applicant,
            key: &key,
            kx: &kx,
            na: &na,
            device: &device.device(),
            device_key: &device.public(),
            ne: &ne,
        }
        .hash();
        assert_eq!(t, Hash32(hex(&e["t"])));
        assert_eq!(sas(&t), e["sas"].as_u64().unwrap() as u32);
        let naming = hex::<32>(&v["names"]["naming_key"]);
        let sealed =
            seal_to_with(&kx, &t, &naming, &hex(&e["esk"]), &hex(&e["seal_nonce"])).unwrap();
        assert_eq!(sealed.epk, hex::<32>(&e["epk"]));
        assert_eq!(sealed.ct, hexv(&e["seal_ct"]));
        assert_eq!(open_naming_key(&kx_secret, &t, &sealed), Some(naming));
        // Sealed for one transcript, it doesn't open under another.
        assert_eq!(open_naming_key(&kx_secret, &Hash32([0; 32]), &sealed), None);
    }

    #[test]
    fn a_fresh_seal_opens_and_a_small_order_key_is_refused() {
        let t = Hash32([9; 32]);
        let kx_secret = [3u8; 32];
        let sealed = seal_naming_key(&x25519_public(&kx_secret), &t, &[5; 32])
            .unwrap()
            .unwrap();
        assert_eq!(open_naming_key(&kx_secret, &t, &sealed), Some([5; 32]));
        // The zero point gives an all-zero shared secret: refused.
        assert_eq!(seal_naming_key(&[0; 32], &t, &[5; 32]).unwrap(), None);
    }

    #[test]
    fn the_short_code_is_shown_in_two_groups_of_three() {
        assert_eq!(sas_display(472_913), "472 913");
        assert_eq!(sas_display(7), "000 007");
    }
}
