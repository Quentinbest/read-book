//! SY7, SY8, SP13: which device this is, and which book is which.
//!
//! SY7 keeps a device's ID in its own store (the login keychain, item 112) with the
//! Mac's hardware UUID, a writer token and high-water marks. What the device finds at
//! start decides whether it goes on as itself, joins as a separate device, or rotates
//! to a successor. SY8 matches books across libraries by their content hashes, or by
//! their OPF identifier and folded title together (B3, B3-guard).

use super::ids::{derived_hash, Hash32, Id16};
use std::collections::BTreeSet;

/// What a device finds when it starts, for one library (SY7).
#[derive(Debug, Default, Clone)]
pub struct Found {
    /// The hardware UUID stored with the ID differs from this Mac's: a Mac set up by
    /// Migration Assistant.
    pub other_hardware: bool,
    /// Storage holds files under this ID carrying another writer token: an ID copied
    /// some other way.
    pub other_writer: bool,
    /// Storage holds changes under this ID newer than the device store's mark: the
    /// same Mac restored from a backup, store and library together.
    pub storage_ahead_of_store: bool,
    /// The library is behind the device store's marks: an older library restored.
    pub library_behind_store: bool,
}

/// What the device does about it (SY7).
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Go on as this device.
    Continue,
    /// Make a new key and enrol it as a separate device, signed in once by the
    /// copied key (an enrollment, not a rotation), then delete the copied key. Its
    /// writes wait until then, and it never replaces another writer's file.
    EnrolAsNewDevice,
    /// Rotate: the old key signs the new one in as its successor, and it stays enrolled.
    Rotate {
        /// The library was restored from an older copy: sync starts paused and Resume
        /// asks which should win (SY27).
        paused: bool,
    },
}

/// SY7: what a device does at start. The hardware and writer checks come first,
/// since a migrated or copied device must never write as the original.
pub fn on_start(found: &Found) -> Action {
    if found.other_hardware || found.other_writer {
        Action::EnrolAsNewDevice
    } else if found.library_behind_store {
        Action::Rotate { paused: true }
    } else if found.storage_ahead_of_store {
        Action::Rotate { paused: false }
    } else {
        Action::Continue
    }
}

/// SP13: a book's identity without its text: the plain hashes of its files and a
/// hash of its identifier with its folded title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookIdentity {
    pub hashes: BTreeSet<Hash32>,
    pub id_title: Option<Hash32>,
}

impl BookIdentity {
    pub fn new(
        hashes: impl IntoIterator<Item = Hash32>,
        identifier: Option<&str>,
        title: Option<&str>,
    ) -> Self {
        let id_title = match (identifier, title) {
            (Some(id), Some(t)) if !id.trim().is_empty() => Some(id_title_hash(id, t)),
            _ => None,
        };
        BookIdentity {
            hashes: hashes.into_iter().collect(),
            id_title,
        }
    }

    /// SY8: the same book when a content hash matches, or the identifier and the
    /// folded title both match.
    pub fn same_book(&self, other: &BookIdentity) -> bool {
        !self.hashes.is_disjoint(&other.hashes)
            || matches!((self.id_title, other.id_title), (Some(a), Some(b)) if a == b)
    }
}

/// SP13, SP4: `H("linen-sync/idtitle/v1" ‖ identifier ‖ folded title)`, each field
/// length-prefixed, with the title folded as B3-guard folds it.
pub fn id_title_hash(identifier: &str, title: &str) -> Hash32 {
    let folded = crate::import::fold_title(title);
    derived_hash(
        "linen-sync/idtitle/v1",
        &[],
        &[identifier.trim().as_bytes(), folded.as_bytes()],
    )
}

/// SY8: two sync IDs found to be one book merge into the smaller, on every device;
/// the other becomes its alias. Returns (kept, alias).
pub fn merged_sync_ids(a: uuid::Uuid, b: uuid::Uuid) -> (uuid::Uuid, uuid::Uuid) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// SY7: a fresh writer token, kept with the ID and carried in every file it writes.
pub fn new_writer_token() -> Id16 {
    Id16(*uuid::Uuid::new_v4().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_decisions_follow_sy7() {
        let base = Found::default();
        assert_eq!(on_start(&base), Action::Continue);
        // A Mac set up by Migration Assistant joins as a separate device, not a successor.
        let migrated = Found {
            other_hardware: true,
            storage_ahead_of_store: true,
            ..base.clone()
        };
        assert_eq!(on_start(&migrated), Action::EnrolAsNewDevice);
        // Two library folders with one ID and different writer tokens.
        let copied = Found {
            other_writer: true,
            ..base.clone()
        };
        assert_eq!(on_start(&copied), Action::EnrolAsNewDevice);
        // A Time Machine restore of the same Mac: store and library rolled back together.
        let restored = Found {
            storage_ahead_of_store: true,
            ..base.clone()
        };
        assert_eq!(on_start(&restored), Action::Rotate { paused: false });
        // An older library restored behind the store's marks starts paused (SY27).
        let old_library = Found {
            library_behind_store: true,
            ..base
        };
        assert_eq!(on_start(&old_library), Action::Rotate { paused: true });
    }

    #[test]
    fn books_match_by_hash_or_by_identifier_and_title() {
        let h1 = Hash32([1; 32]);
        let h2 = Hash32([2; 32]);
        let a = BookIdentity::new([h1], Some("urn:isbn:1"), Some("Moby  Dick"));
        let same_file = BookIdentity::new([h1], None, None);
        let new_edition = BookIdentity::new([h2], Some("urn:isbn:1"), Some("moby dick"));
        // B3-guard: a reused identifier with another title stays a different book.
        let reused = BookIdentity::new([h2], Some("urn:isbn:1"), Some("Typee"));
        let unrelated = BookIdentity::new([h2], Some("urn:isbn:2"), Some("Moby Dick"));
        assert!(a.same_book(&same_file));
        assert!(a.same_book(&new_edition), "case and spacing ignored");
        assert!(!a.same_book(&reused));
        assert!(!a.same_book(&unrelated));
        // A tombstone's identity holds no text, only hashes.
        assert_ne!(a.id_title, Some(Hash32([0; 32])));
    }

    #[test]
    fn the_smaller_sync_id_wins_on_every_device() {
        let x = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let y = uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
        assert_eq!(merged_sync_ids(x, y), (x, y));
        assert_eq!(merged_sync_ids(y, x), (x, y));
        assert_ne!(new_writer_token(), new_writer_token());
    }
}
