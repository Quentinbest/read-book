//! P2: a shared in-memory library for whole-pass tests.

use super::crypto::Keypair;
use super::device_file::{self, Fields};
use super::device_store::{DeviceStore, Memory as Keystore};
use super::envelope::{self, Place, Signer};
use super::files::{self, Read};
use super::folder::{self, Folder};
use super::ids::{b64url_encode, Hash32, Id16};
use super::journal::{self, Library};
use super::{crypto, json, membership, trust};
use crate::store::Store;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

const LIB: Id16 = Id16([7; 16]);
const NOW: i64 = 1_760_000_000_000;
const NAMING: [u8; 32] = [42; 32];

// These fields are the kit's interface for later pass tests.
#[allow(dead_code)]
pub(crate) struct KitDevice {
    pub(crate) db: Store,
    pub(crate) lib: Library,
    pub(crate) store: DeviceStore,
    pub(crate) ks: Keystore,
    pub(crate) key: Keypair,
    pub(crate) folder: folder::View,
}

pub(crate) struct Kit {
    #[allow(dead_code)]
    pub(crate) folder: folder::Memory,
    pub(crate) devices: Vec<KitDevice>,
    genesis: Hash32,
}

/// Builds a library within the protocol's device limit. At least one device is needed.
pub(crate) fn library(n: usize) -> Kit {
    assert!((1..=membership::MAX_DEVICES).contains(&n));
    let folder = folder::Memory::default();
    let mut devices = Vec::new();
    for _ in 0..n {
        let ks = Keystore::default();
        let mut store = DeviceStore::default();
        let state = store.identity(&LIB).unwrap();
        state
            .rest
            .insert("namingKey".into(), json!(b64url_encode(&NAMING)));
        let key = state.key().unwrap();
        store.save(&ks).unwrap();
        let lib = Library {
            id: LIB,
            device: key.device(),
        };
        let db = Store::open_in_memory().unwrap();
        journal::join(db.conn(), &lib, "icloud-drive", NOW).unwrap();
        // One book, matched to the same sync ID on every device.
        db.conn()
            .execute(
                "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                 VALUES ('book', ?1, 'x.epub', 'Moby-Dick', 'package', 0)",
                [format!("{}0", super::apply::tests::HASH)],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO sync_books (library, sync_id, book_id) VALUES (?1, ?2, 'book')",
                rusqlite::params![LIB.to_string(), super::apply::tests::SYNC_BOOK],
            )
            .unwrap();
        let view = folder.view(&lib.device.to_string());
        devices.push(KitDevice {
            db,
            lib,
            store,
            ks,
            key,
            folder: view,
        });
    }
    let first = &devices[0];
    let code = Keypair::from_seed(&[99; 32]);
    let (bytes, genesis) = trust::sign(
        "genesis",
        &LIB,
        json!({
            "device": first.lib.device.to_string(), "key": b64url_encode(&first.key.public()),
            "label": "first", "clock": [NOW as u64, 0],
            "code": {"nonce": b64url_encode(&[5; 16]), "codeKey": b64url_encode(&code.public())}
        }),
        &[
            (&first.key, Signer::Device(first.lib.device), 0),
            (&code, Signer::Code, 0),
        ],
    );
    let mut head = genesis;
    files::write_file(&mut devices[0].folder, &trust::trust_path(&genesis), &bytes).unwrap();
    let first = &devices[0];
    let writer = first.store.library(&LIB).unwrap().writer_id().unwrap();
    let (claim, _) = trust::sign(
        "claim",
        &LIB,
        json!({
            "device": first.lib.device.to_string(), "writer": writer.to_string(),
            "key": b64url_encode(&first.key.public()), "genesis": genesis.to_string()
        }),
        &[(&first.key, Signer::Device(first.lib.device), 0)],
    );
    files::write_file(&mut devices[0].folder, &trust::claim_path(&LIB), &claim).unwrap();
    for i in 1..n {
        let by = &devices[i - 1].key;
        let new = &devices[i].key;
        let reveal = json::canonical(&json!({"type": "enroll-reveal", "library": LIB.to_string(),
            "applicant": new.device().to_string(), "enroller": by.device().to_string()}));
        let pop = new.sign(&envelope::message(
            "enroll-reveal",
            &LIB,
            &Signer::Device(new.device()),
            &Place::default(),
            &crypto::payload_hash(&reveal),
        ));
        let (bytes, next) = trust::sign(
            "enroll",
            &LIB,
            json!({
                "prev": head.to_string(), "device": new.device().to_string(),
                "key": b64url_encode(&new.public()), "label": "new", "clock": [NOW as u64, 1],
                "by": by.device().to_string(), "n": 1, "sas": "dd".repeat(32),
                "reveal": b64url_encode(&reveal), "pop": b64url_encode(&pop)
            }),
            &[(by, Signer::Device(by.device()), 1)],
        );
        files::write_file(
            &mut devices[i - 1].folder,
            &trust::trust_path(&next),
            &bytes,
        )
        .unwrap();
        head = next;
    }
    for device in &mut devices {
        let state = device.store.library_mut(&LIB).unwrap();
        state
            .rest
            .insert("genesis".into(), json!(genesis.to_string()));
        state.rest.insert("head".into(), json!(head.to_string()));
        state.counter = 1;
        let written = device_file::write(
            &device.key,
            &LIB,
            &state.writer_id().unwrap(),
            1,
            &Fields {
                head: Some(head),
                ..Fields::default()
            },
        )
        .unwrap();
        device.store.save(&device.ks).unwrap();
        files::write_file(&mut device.folder, &written.path, &written.bytes).unwrap();
    }
    Kit {
        folder,
        devices,
        genesis,
    }
}

impl KitDevice {
    /// SP15: signs what was written locally as this device's next change and
    /// publishes it as a device would: the bodies it names in `blobs/`, the change
    /// file, and a device file naming the change. Returns the change's sequence, or
    /// none when nothing was waiting.
    pub(crate) fn publish_pending(&mut self) -> Option<u64> {
        let signed =
            super::chain::publish(self.db.conn(), &self.lib, &self.ks, &mut self.store).unwrap()?;
        let me = self.lib.device;
        let change =
            super::chain::open_change(&signed.bytes, &LIB, &me, &self.key.public()).unwrap();
        for version in &change.versions {
            if let Some(hash) = version["body"]["hash"].as_str().and_then(Hash32::parse) {
                let body = journal::body(self.db.conn(), &hash).unwrap().unwrap();
                files::write_file(&mut self.folder, &format!("blobs/{hash}"), &body).unwrap();
            }
        }
        files::write_file(&mut self.folder, &signed.path, &signed.bytes).unwrap();
        super::chain::uploaded(&self.ks, &mut self.store, &LIB, &signed.path).unwrap();
        let head = self
            .store
            .library(&LIB)
            .unwrap()
            .rest
            .get("head")
            .and_then(|v| v.as_str())
            .and_then(Hash32::parse);
        let written = device_file::publish(
            &self.ks,
            &mut self.store,
            &LIB,
            &Fields {
                head,
                own: Some((signed.seq, signed.hash)),
                ..Fields::default()
            },
        )
        .unwrap();
        files::write_file(&mut self.folder, &written.path, &written.bytes).unwrap();
        super::chain::uploaded(&self.ks, &mut self.store, &LIB, &written.path).unwrap();
        Some(signed.seq)
    }
}

impl Kit {
    pub(crate) fn membership(&self, i: usize) -> membership::Membership {
        let device = &self.devices[i];
        let mut records = BTreeMap::new();
        let mut cursor = None;
        loop {
            let page = device.folder.list("trust", cursor.as_deref()).unwrap();
            for entry in page.entries {
                let name = entry
                    .name
                    .strip_prefix("trust/")
                    .unwrap()
                    .strip_suffix(".json")
                    .unwrap();
                let hash = Hash32::parse(name).unwrap();
                let Read::Bytes(bytes) = files::read_file(&device.folder, &entry).unwrap() else {
                    panic!("missing trust record")
                };
                records.insert(hash, trust::open(&bytes, &LIB, &hash).unwrap());
            }
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        let head = device.store.library(&LIB).unwrap().rest["head"]
            .as_str()
            .and_then(Hash32::parse)
            .unwrap();
        membership::compute(&membership::Inputs {
            genesis: self.genesis,
            records: &records,
            first_seen: &BTreeMap::new(),
            confirmed: &BTreeSet::new(),
            checkpoint: &BTreeSet::new(),
            held: &|_, _| None,
            accepted: &BTreeSet::new(),
            now_ms: NOW,
            me: Some(device.lib.device),
            quarantined: &BTreeMap::new(),
            heads: &BTreeMap::from([(device.lib.device, Some(head))]),
        })
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kit_of_three_devices_trusts_all_three() {
        let kit = library(3);
        for i in 0..3 {
            let membership = kit.membership(i);
            for device in &kit.devices {
                assert!(membership.trusted(&device.lib.device).is_some());
            }
        }
    }

    #[test]
    fn each_device_file_is_in_the_layout_and_names_a_head() {
        let kit = library(3);
        for device in &kit.devices {
            let path = device_file::device_path(&device.lib.device);
            assert_eq!(files::layout(&path).unwrap().kind, files::Kind::Device);
            let entries = device.folder.list("devices", None).unwrap().entries;
            let entry = entries.iter().find(|e| e.name == path).unwrap();
            let Read::Bytes(bytes) = files::read_file(&device.folder, entry).unwrap() else {
                panic!("missing device file")
            };
            let file =
                device_file::open(&bytes, &LIB, &device.lib.device, &device.key.public()).unwrap();
            let head = file.fields.head.unwrap();
            let entries = device.folder.list("trust", None).unwrap().entries;
            let entry = entries
                .iter()
                .find(|e| e.name == trust::trust_path(&head))
                .unwrap();
            let Read::Bytes(bytes) = files::read_file(&device.folder, entry).unwrap() else {
                panic!("missing head")
            };
            trust::open(&bytes, &LIB, &head).unwrap();
        }
    }
}
