//! SP28, SY7: the device store, one JSON document in the login-keychain item
//! `app.linen.reader/sync-device`, outside the library, so restoring the library
//! can't move what it holds back.
//!
//! Per library this device joined: its own ID, key and writer token, and the marks
//! a restored library is caught by (the last sequence and hash it signed, and,
//! filled by later parts of Stage 1c, its counters, frontiers, floors, membership
//! head and accepted-by-all marks), with the signed bytes of files not yet
//! confirmed in storage (SP15). Fields this Linen doesn't know are kept as they
//! were, so a newer Linen's marks survive an older one writing the document.

use super::crypto::{self, CryptoError, Keypair};
use super::ids::{b64url_decode, b64url_encode, DeviceId, Hash32, Id16};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// SP28: the keychain item's service and account.
pub const SERVICE: &str = "app.linen.reader";
pub const ACCOUNT: &str = "sync-device";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("the Keychain refused ({0})")]
    Keychain(i32),
    #[error("the device store isn't readable")]
    Unreadable,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

/// Where the document is kept: the login keychain in the app, memory in tests.
pub trait Keystore: Send + Sync {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError>;
    fn save(&self, bytes: &[u8]) -> Result<(), StoreError>;
}

/// SY7 (item 112): the login keychain. Linen's ad-hoc builds can't use the
/// data-protection keychain's *ThisDeviceOnly* class (`docs/spikes/sync-folders.md`).
/// The item's service is `SERVICE`; tests use another.
pub struct Keychain(pub &'static str);

/// The device store's own item.
pub const DEVICE_KEYCHAIN: Keychain = Keychain(SERVICE);

/// errSecItemNotFound.
const NOT_FOUND: i32 = -25300;

impl Keystore for Keychain {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        match security_framework::passwords::get_generic_password(self.0, ACCOUNT) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(StoreError::Keychain(e.code())),
        }
    }

    fn save(&self, bytes: &[u8]) -> Result<(), StoreError> {
        security_framework::passwords::set_generic_password(self.0, ACCOUNT, bytes)
            .map_err(|e| StoreError::Keychain(e.code()))
    }
}

/// A keystore in memory, for tests and the in-memory storage of Stage 1.
#[derive(Default)]
pub struct Memory(Mutex<Option<Vec<u8>>>);

impl Keystore for Memory {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.lock().expect("not poisoned").clone())
    }

    fn save(&self, bytes: &[u8]) -> Result<(), StoreError> {
        *self.0.lock().expect("not poisoned") = Some(bytes.to_vec());
        Ok(())
    }
}

/// SP15: a change signed and kept here, by its storage path, until storage is seen
/// to hold it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unsent {
    pub path: String,
    /// The signed bytes, base64url.
    pub bytes: String,
}

impl Unsent {
    pub fn bytes(&self) -> Option<Vec<u8>> {
        b64url_decode(&self.bytes)
    }
}

/// One library's state on this device (SP28).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryState {
    pub device: String,
    pub writer: String,
    /// The Ed25519 private seed, base64url.
    pub seed: String,
    /// SP15: the last change this device signed: its sequence (0 before the first)
    /// and payload hash.
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub unsent: Vec<Unsent>,
    /// Everything else SP28 lists (`retired`, `genesis`, `head`, `removals`,
    /// `rotations`, `counter`, `gen`, `frontier`, `floors`, `acceptedByAll`), written
    /// by later parts of Stage 1c, and fields a newer Linen added: kept as they are.
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl LibraryState {
    /// SP1, SY7: a new identity in a library: a fresh key, the ID it names, and a
    /// fresh writer token.
    pub fn new() -> Result<Self, StoreError> {
        let key = Keypair::generate()?;
        let writer: [u8; 16] = crypto::random()?;
        Ok(LibraryState {
            device: key.device().to_string(),
            writer: Id16(writer).to_string(),
            seed: b64url_encode(&key.seed()),
            seq: 0,
            hash: None,
            unsent: Vec::new(),
            rest: Map::new(),
        })
    }

    /// The key, checked against the ID it must name (SP1).
    pub fn key(&self) -> Result<Keypair, StoreError> {
        let seed: [u8; 32] = b64url_decode(&self.seed)
            .and_then(|s| s.try_into().ok())
            .ok_or(StoreError::Unreadable)?;
        let key = Keypair::from_seed(&seed);
        if Some(key.device()) != Id16::parse(&self.device) {
            return Err(StoreError::Unreadable);
        }
        Ok(key)
    }

    pub fn device_id(&self) -> Result<DeviceId, StoreError> {
        Id16::parse(&self.device).ok_or(StoreError::Unreadable)
    }

    pub fn writer_id(&self) -> Result<Id16, StoreError> {
        Id16::parse(&self.writer).ok_or(StoreError::Unreadable)
    }

    pub fn last_hash(&self) -> Result<Option<Hash32>, StoreError> {
        self.hash
            .as_deref()
            .map(|h| Hash32::parse(h).ok_or(StoreError::Unreadable))
            .transpose()
    }
}

/// SP28: the whole document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceStore {
    /// SY7: the Mac's hardware UUID (`IOPlatformUUID`), checked at each start.
    #[serde(default)]
    pub hardware: Option<String>,
    #[serde(default)]
    pub libraries: BTreeMap<String, LibraryState>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl DeviceStore {
    /// An absent item is an empty store; one that isn't a store is refused, never
    /// replaced, since its keys can't be made again.
    pub fn load(ks: &dyn Keystore) -> Result<Self, StoreError> {
        match ks.load()? {
            None => Ok(DeviceStore::default()),
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| StoreError::Unreadable),
        }
    }

    /// SP15 step 3: written and synced before anything it records is uploaded.
    pub fn save(&self, ks: &dyn Keystore) -> Result<(), StoreError> {
        ks.save(&serde_json::to_vec(self).expect("a store serialises"))
    }

    pub fn library(&self, id: &Id16) -> Option<&LibraryState> {
        self.libraries.get(&id.to_string())
    }

    pub fn library_mut(&mut self, id: &Id16) -> Option<&mut LibraryState> {
        self.libraries.get_mut(&id.to_string())
    }

    /// The library's identity, made on first use (SY7). Each library has its own,
    /// so a new ID for one never touches another (SP28).
    pub fn identity(&mut self, id: &Id16) -> Result<&mut LibraryState, StoreError> {
        let k = id.to_string();
        if !self.libraries.contains_key(&k) {
            self.libraries.insert(k.clone(), LibraryState::new()?);
        }
        Ok(self.libraries.get_mut(&k).expect("just inserted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_gets_its_own_identity_once_and_keeps_it() {
        let ks = Memory::default();
        let mut store = DeviceStore::load(&ks).unwrap();
        assert_eq!(store, DeviceStore::default());
        let (a, b) = (Id16([1; 16]), Id16([2; 16]));
        let first = store.identity(&a).unwrap().clone();
        assert_eq!(first.key().unwrap().device(), first.device_id().unwrap());
        // SP28: another library has another ID, key and writer token.
        let other = store.identity(&b).unwrap().clone();
        assert_ne!(first.device, other.device);
        assert_ne!(first.writer, other.writer);
        store.save(&ks).unwrap();
        let mut again = DeviceStore::load(&ks).unwrap();
        assert_eq!(again.identity(&a).unwrap(), &first);
    }

    #[test]
    fn fields_this_linen_doesnt_know_are_kept() {
        let ks = Memory::default();
        let mut store = DeviceStore::default();
        store.identity(&Id16([1; 16])).unwrap();
        let mut v = serde_json::to_value(&store).unwrap();
        v["future"] = serde_json::json!({ "x": 1 });
        let lib = Id16([1; 16]).to_string();
        v["libraries"][&lib]["acceptedByAll"] = serde_json::json!({ "d": 5 });
        ks.save(v.to_string().as_bytes()).unwrap();
        let mut store = DeviceStore::load(&ks).unwrap();
        store.library_mut(&Id16([1; 16])).unwrap().seq = 9;
        store.save(&ks).unwrap();
        let back: Value = serde_json::from_slice(&ks.load().unwrap().unwrap()).unwrap();
        assert_eq!(back["future"]["x"], 1);
        assert_eq!(back["libraries"][&lib]["acceptedByAll"]["d"], 5);
        assert_eq!(back["libraries"][&lib]["seq"], 9);
    }

    #[test]
    fn a_store_that_isnt_one_is_refused_never_replaced() {
        let ks = Memory::default();
        ks.save(b"not json").unwrap();
        assert_eq!(DeviceStore::load(&ks), Err(StoreError::Unreadable));
        // A seed that doesn't name the recorded ID is refused too (SP1).
        let mut store = DeviceStore::default();
        let state = store.identity(&Id16([1; 16])).unwrap();
        state.device = Id16([9; 16]).to_string();
        assert!(state.key().is_err());
    }

    /// Writes and deletes a test item in the login keychain; run by hand.
    #[test]
    #[ignore]
    fn the_keychain_round_trips() {
        const TEST_ITEM: Keychain = Keychain("app.linen.reader.test");
        let mut store = DeviceStore::default();
        store.identity(&Id16([1; 16])).unwrap();
        store.save(&TEST_ITEM).unwrap();
        assert_eq!(DeviceStore::load(&TEST_ITEM).unwrap(), store);
        security_framework::passwords::delete_generic_password("app.linen.reader.test", ACCOUNT)
            .unwrap();
    }
}
