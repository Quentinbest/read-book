//! SY11, SY23, SP2, SP7, SP9: shared-object conflict copies (#174).
//!
//! This rule is pure: the folder is the caller's. Stage 2 supplies the brokered
//! folder; nothing here opens storage or is wired into the app.

use super::crypto::{blob_name, book_name};
use super::ids::Hash32;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Area {
    Books,
    Blobs,
}

fn keyed(area: Area, naming: &[u8; 32], hash: &Hash32) -> String {
    match area {
        Area::Books => book_name(naming, hash),
        Area::Blobs => blob_name(naming, hash),
    }
}

/// SP2, SP7: PROVISIONAL until Stage 6 measures real conflict-copy names.
/// A wrong guess is harmless: SY11's hash rule decides by content.
pub fn copy_of(listed: &str) -> Option<&str> {
    let base = listed.get(..64)?;
    Hash32::parse(base)?;
    let first = *listed.as_bytes().get(64)?;
    (!first.is_ascii_hexdigit() && first != b'.').then_some(base)
}

/// SY2: tokens come only from listing; writes use object names, never tokens.
pub trait SharedFolder {
    type Token: Clone;

    fn list(&self, area: Area) -> Vec<(String, Self::Token)>;
    /// SP7, SP9: plain SHA-256, or unreadable now (e.g. still downloading).
    /// The real folder streams book files; this rule never needs their bytes.
    fn hash(&self, token: &Self::Token) -> Option<Hash32>;
    /// SY2: read the token's bytes, write a temporary name, then rename to `name`.
    /// SP9, SY23: the staged bytes must hash to `expected`, the hash `resolve`
    /// checked, or the rename is refused; storage can change a copy between the
    /// two reads. An error means the object may not hold those bytes, so nothing
    /// is removed.
    fn replace(&mut self, name: &str, from: &Self::Token, expected: &Hash32)
        -> std::io::Result<()>;
    fn remove(&mut self, token: &Self::Token);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step<T> {
    RemoveCopy(T),
    /// `hash` is the copy's checked plain hash, which `replace` must see again.
    Replace {
        with: T,
        hash: Hash32,
    },
    RemoveObject(T),
    Repair,
}

/// SY11, SY23: decide from plain hashes, checked against SP9's keyed name.
pub fn resolve<T: Clone>(
    area: Area,
    naming: &[u8; 32],
    name: &str,
    object: Option<(T, Option<Hash32>)>,
    copies: &[(T, Option<Hash32>)],
) -> Vec<Step<T>> {
    // SP7: wait for an unreadable object before deciding anything about it.
    if matches!(object, Some((_, None))) {
        return Vec::new();
    }
    let matches = |hash: &Hash32| keyed(area, naming, hash) == name;
    let object_matches = object
        .as_ref()
        .and_then(|(_, hash)| hash.as_ref())
        .is_some_and(matches);
    let good_copy = copies
        .iter()
        .find(|(_, hash)| hash.as_ref().is_some_and(matches));
    let mut steps = Vec::new();
    if !object_matches {
        if let Some((token, Some(hash))) = good_copy {
            // SY11: preserve the good bytes before removing any copies.
            steps.push(Step::Replace {
                with: token.clone(),
                hash: *hash,
            });
        } else {
            if object.is_none() && copies.iter().all(|(_, hash)| hash.is_none()) {
                return steps;
            }
            if let Some((token, _)) = object {
                steps.push(Step::RemoveObject(token));
            }
        }
    }
    // SY11: removing a bad copy beside a good object is Delegated to Claude
    // (docs/decisions.md): bytes that fail its content-fixed name aren't it.
    steps.extend(
        copies
            .iter()
            .filter(|(_, hash)| hash.is_some())
            .map(|(token, _)| Step::RemoveCopy(token.clone())),
    );
    if !object_matches && good_copy.is_none() {
        steps.push(Step::Repair);
    }
    steps
}

struct Group<T> {
    object: Option<T>,
    copies: Vec<T>,
}

/// SY11: tidy one listed snapshot. SY23 repair is returned for the caller. Each
/// group returns the steps actually taken: a failed replacement stops that group
/// before any removal, so the only good bytes are never lost.
pub fn tidy<F: SharedFolder>(
    folder: &mut F,
    area: Area,
    naming: &[u8; 32],
) -> Vec<(String, Vec<Step<F::Token>>)> {
    let mut groups: BTreeMap<String, Group<F::Token>> = BTreeMap::new();
    for (listed, token) in folder.list(area) {
        let (name, is_copy) = if Hash32::parse(&listed).is_some() {
            (listed.as_str(), false)
        } else if let Some(base) = copy_of(&listed) {
            (base, true)
        } else {
            continue;
        };
        let group = groups.entry(name.to_owned()).or_insert_with(|| Group {
            object: None,
            copies: Vec::new(),
        });
        if is_copy {
            group.copies.push(token);
        } else {
            group.object = Some(token);
        }
    }
    groups
        .into_iter()
        .map(|(name, group)| {
            let object = group.object.map(|token| {
                let hash = folder.hash(&token);
                (token, hash)
            });
            let copies: Vec<_> = group
                .copies
                .into_iter()
                .map(|token| {
                    let hash = folder.hash(&token);
                    (token, hash)
                })
                .collect();
            let mut taken = Vec::new();
            for step in resolve(area, naming, &name, object, &copies) {
                match &step {
                    Step::Replace { with, hash } => {
                        // SY11: the good copy's bytes must be in place before
                        // any copy goes; try again on a later pass.
                        if folder.replace(&name, with, hash).is_err() {
                            break;
                        }
                    }
                    Step::RemoveCopy(token) | Step::RemoveObject(token) => folder.remove(token),
                    Step::Repair => {} // SY23: the caller schedules a validated re-upload.
                }
                taken.push(step);
            }
            (name, taken)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::ids::identity_hash;
    use std::cell::RefCell;

    const NAMING: [u8; 32] = [17; 32];
    const GOOD: &[u8] = b"shared object bytes";
    const BAD: &[u8] = b"damaged bytes";

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Token(Area, String);

    #[derive(Default)]
    struct Folder {
        files: BTreeMap<Token, Option<Vec<u8>>>,
        hashed: RefCell<Vec<Token>>,
        failing: bool,
        changes_before_replace: Option<Vec<u8>>,
    }

    impl Folder {
        fn put(&mut self, area: Area, name: &str, bytes: Option<&[u8]>) -> Token {
            let token = Token(area, name.to_owned());
            self.files.insert(token.clone(), bytes.map(<[u8]>::to_vec));
            token
        }

        fn write_concurrently(&mut self, area: Area, name: &str, bytes: &[u8]) {
            let target = if self.files.contains_key(&Token(area, name.to_owned())) {
                format!("{name} 2")
            } else {
                name.to_owned()
            };
            self.put(area, &target, Some(bytes));
        }
    }

    impl SharedFolder for Folder {
        type Token = Token;

        fn list(&self, area: Area) -> Vec<(String, Token)> {
            self.files
                .keys()
                .filter(|token| token.0 == area)
                .map(|token| (token.1.clone(), token.clone()))
                .collect()
        }

        fn hash(&self, token: &Token) -> Option<Hash32> {
            self.hashed.borrow_mut().push(token.clone());
            self.files.get(token)?.as_deref().map(identity_hash)
        }

        fn replace(&mut self, name: &str, from: &Token, expected: &Hash32) -> std::io::Result<()> {
            if self.failing {
                return Err(std::io::Error::other("disk full"));
            }
            if let Some(changed) = self.changes_before_replace.take() {
                self.files.insert(from.clone(), Some(changed));
            }
            let bytes = self.files[from].clone().expect("readable copy");
            if identity_hash(&bytes) != *expected {
                return Err(std::io::Error::other(
                    "the copy changed since it was checked",
                ));
            }
            // SY2: model a temporary write followed by an atomic rename.
            let temporary = self.put(from.0, ".linen-tmp-test", Some(&bytes));
            let bytes = self.files.remove(&temporary).unwrap();
            self.files.insert(Token(from.0, name.to_owned()), bytes);
            Ok(())
        }

        fn remove(&mut self, token: &Token) {
            assert!(self.files.remove(token).is_some());
        }
    }

    fn name(area: Area) -> String {
        match area {
            Area::Books => book_name(&NAMING, &identity_hash(GOOD)),
            Area::Blobs => blob_name(&NAMING, &identity_hash(GOOD)),
        }
    }

    #[test]
    fn two_writers_of_one_object_leave_one_object_and_no_copy() {
        for area in [Area::Books, Area::Blobs] {
            let mut folder = Folder::default();
            let name = name(area);
            folder.write_concurrently(area, &name, GOOD);
            folder.write_concurrently(area, &name, GOOD);
            assert_eq!(folder.files.len(), 2, "#174: cloud left a copy");
            tidy(&mut folder, area, &NAMING);
            assert_eq!(folder.files.len(), 1);
            assert_eq!(folder.files[&Token(area, name)], Some(GOOD.to_vec()));
        }
    }

    #[test]
    fn a_good_copy_replaces_a_damaged_object() {
        for area in [Area::Books, Area::Blobs] {
            for damaged in [false, true] {
                let mut folder = Folder::default();
                let name = name(area);
                if damaged {
                    folder.put(area, &name, Some(BAD));
                }
                let bad = folder.put(area, &format!("{name} (0)"), Some(BAD));
                let good = folder.put(area, &format!("{name} (1)"), Some(GOOD));
                let second = folder.put(area, &format!("{name} (2)"), Some(GOOD));
                let result = tidy(&mut folder, area, &NAMING);
                assert_eq!(
                    result,
                    vec![(
                        name.clone(),
                        vec![
                            Step::Replace {
                                with: good.clone(),
                                hash: identity_hash(GOOD),
                            },
                            Step::RemoveCopy(bad),
                            Step::RemoveCopy(good),
                            Step::RemoveCopy(second),
                        ]
                    )]
                );
                assert_eq!(folder.files.len(), 1);
                assert_eq!(folder.files[&Token(area, name)], Some(GOOD.to_vec()));
            }
        }
    }

    #[test]
    fn a_bad_copy_beside_a_good_object_is_removed() {
        let area = Area::Books;
        let name = name(area);
        let mut folder = Folder::default();
        let object = folder.put(area, &name, Some(GOOD));
        let copy = folder.put(area, &format!("{name} 2"), Some(BAD));
        assert_eq!(
            tidy(&mut folder, area, &NAMING),
            vec![(name, vec![Step::RemoveCopy(copy)])]
        );
        assert_eq!(folder.files.len(), 1);
        assert_eq!(folder.files[&object], Some(GOOD.to_vec()));
    }

    #[test]
    fn when_neither_matches_both_go_and_repair_is_asked() {
        let area = Area::Blobs;
        let name = name(area);
        for present in [false, true] {
            let mut folder = Folder::default();
            let mut expected = Vec::new();
            if present {
                expected.push(Step::RemoveObject(folder.put(area, &name, Some(BAD))));
            }
            let copy = folder.put(area, &format!("{name}-conflict"), Some(BAD));
            expected.extend([Step::RemoveCopy(copy), Step::Repair]);
            assert_eq!(
                tidy(&mut folder, area, &NAMING),
                vec![(name.clone(), expected)]
            );
            assert!(folder.files.is_empty());
        }
        // SY23 also repairs a damaged object with no candidate copies.
        assert_eq!(
            resolve(
                area,
                &NAMING,
                &name,
                Some((0, Some(identity_hash(BAD)))),
                &[]
            ),
            vec![Step::RemoveObject(0), Step::Repair]
        );
    }

    #[test]
    fn an_unreadable_object_or_copy_waits() {
        let area = Area::Books;
        let name = name(area);
        let mut folder = Folder::default();
        let object = folder.put(area, &name, None);
        folder.put(area, &format!("{name} 2"), Some(GOOD));
        folder.put(area, &format!("{name} 3"), Some(BAD));
        let before = folder.files.clone();
        assert_eq!(
            tidy(&mut folder, area, &NAMING),
            vec![(name.clone(), vec![])]
        );
        assert_eq!(folder.files, before);
        folder.files.remove(&object);
        let waiting = folder.put(area, &format!("{name} 4"), None);
        tidy(&mut folder, area, &NAMING);
        assert_eq!(folder.files.len(), 2);
        assert_eq!(folder.files[&waiting], None);
        assert_eq!(folder.files[&object], Some(GOOD.to_vec()));
        assert_eq!(
            tidy(&mut folder, area, &NAMING),
            vec![(name.clone(), vec![])]
        );
        folder.files.remove(&object);
        assert_eq!(
            tidy(&mut folder, area, &NAMING),
            vec![(name.clone(), vec![])]
        );
        assert_eq!(folder.files.len(), 1);
        // An unreadable copy cannot save a damaged object or be removed with it.
        folder.put(area, &name, Some(BAD));
        assert_eq!(
            tidy(&mut folder, area, &NAMING),
            vec![(name, vec![Step::RemoveObject(object), Step::Repair])]
        );
        assert_eq!(folder.files.len(), 1);
        assert!(folder.files.contains_key(&waiting));
    }

    #[test]
    fn names_that_arent_objects_or_copies_are_left_alone() {
        let area = Area::Books;
        let name = name(area);
        let mut folder = Folder::default();
        for stranger in [
            "notes.txt".to_owned(),
            name.to_uppercase(),
            format!("{name}.tmp"),
            ".linen-tmp-1234".to_owned(),
        ] {
            folder.put(area, &stranger, Some(BAD));
        }
        // Areas are independent, including their tokens.
        folder.put(Area::Blobs, &name, Some(BAD));
        let before = folder.files.clone();
        assert!(tidy(&mut folder, area, &NAMING).is_empty());
        assert_eq!(folder.files, before);
        assert!(folder.hashed.borrow().is_empty());
    }

    #[test]
    fn copy_of_recognises_only_a_hex_name_with_a_suffix() {
        let name = "ab01".repeat(16);
        for suffix in [" 2", " (1)", "-conflict", "_copy", "副本"] {
            assert_eq!(copy_of(&format!("{name}{suffix}")), Some(name.as_str()));
        }
        for suffix in ["", ".tmp", "0", "a copy", "F copy"] {
            assert_eq!(copy_of(&format!("{name}{suffix}")), None);
        }
        for invalid in [
            "notes.txt".to_owned(),
            format!("{} 2", name.to_uppercase()),
            format!("{} 2", "g".repeat(64)),
            format!("{} 2", "a".repeat(63)),
            "副".repeat(64),
        ] {
            assert_eq!(copy_of(&invalid), None);
        }
    }

    #[test]
    fn a_failed_replacement_removes_nothing() {
        let mut folder = Folder {
            failing: true,
            ..Folder::default()
        };
        let name = name(Area::Blobs);
        folder.put(Area::Blobs, &name, Some(BAD));
        let copy = folder.put(Area::Blobs, &format!("{name} 2"), Some(GOOD));
        let before = folder.files.clone();
        let taken = tidy(&mut folder, Area::Blobs, &NAMING);
        assert_eq!(folder.files, before, "the only good bytes stay");
        assert_eq!(taken, vec![(name.clone(), vec![])]);
        folder.failing = false;
        let taken = tidy(&mut folder, Area::Blobs, &NAMING);
        assert_eq!(
            taken[0].1[0],
            Step::Replace {
                with: copy,
                hash: identity_hash(GOOD),
            }
        );
        assert_eq!(folder.files.len(), 1);
    }

    #[test]
    fn a_copy_that_changes_before_replacing_installs_nothing() {
        let mut folder = Folder {
            changes_before_replace: Some(BAD.to_vec()),
            ..Folder::default()
        };
        let name = name(Area::Books);
        let object = folder.put(Area::Books, &name, Some(b"wrong bytes"));
        folder.put(Area::Books, &format!("{name} 2"), Some(GOOD));
        let taken = tidy(&mut folder, Area::Books, &NAMING);
        assert_eq!(taken, vec![(name.clone(), vec![])]);
        assert_eq!(folder.files[&object].as_deref(), Some(&b"wrong bytes"[..]));
        assert_eq!(folder.files.len(), 2, "nothing is removed");
    }
}
