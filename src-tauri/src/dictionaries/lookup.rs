//! DX4, DX5: finding a selection's entries in one generation, and its resources.

use super::fold::suffix_candidates;
use super::generation;
use super::index::{Hit, Index, IndexError};
use super::mdx::{Mdx, MdxError};
use std::path::Path;

/// `@@@LINK=` redirects followed at most this many times (DX5).
pub const MAX_HOPS: usize = 8;

/// An open generation: its dictionary, its resource files and its index.
pub struct Opened {
    pub generation: String,
    mdx: Mdx,
    mdds: Vec<Mdx>,
    index: Index,
}

#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error("the dictionary's files are missing")]
    Missing,
    #[error(transparent)]
    Mdx(#[from] MdxError),
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// One entry, as stored: its HTML is the dictionary's own and is sanitised before it is shown.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Entry {
    pub headword: String,
    pub html: String,
    /// DX4: the selection the entry was found for by a suffix rule (“for ‘invalidates’”).
    pub for_word: Option<String>,
}

impl Opened {
    pub fn open(dir: &Path, generation: &str) -> Result<Opened, LookupError> {
        let (mdx_path, mdd_paths) =
            generation::files(dir, generation).ok_or(LookupError::Missing)?;
        let g = generation::generation_dir(dir, generation).ok_or(LookupError::Missing)?;
        Ok(Opened {
            generation: generation.to_string(),
            mdx: Mdx::open(&mdx_path)?,
            mdds: mdd_paths
                .iter()
                .map(|p| Mdx::open(p))
                .collect::<Result<_, _>>()?,
            index: Index::open(&g.join(super::index::FILE))?,
        })
    }

    pub fn title(&self) -> &str {
        &self.mdx.header.title
    }

    /// Follow redirects from a hit to an entry's text; None for a cycle or too many hops.
    fn resolve(&self, hit: &Hit) -> Result<Option<(String, String, u64)>, LookupError> {
        let mut seen = std::collections::HashSet::new();
        let mut cur = hit.clone();
        for _ in 0..=MAX_HOPS {
            if !seen.insert(cur.start) {
                return Ok(None);
            }
            let text = self.mdx.entry_text(cur.start, cur.end)?;
            let trimmed = text.trim_start();
            if let Some(target) = trimmed.strip_prefix("@@@LINK=") {
                let target = target.lines().next().unwrap_or("").trim();
                match self.index.find(target)?.into_iter().next() {
                    Some(next) => {
                        cur = next;
                        continue;
                    }
                    None => return Ok(None),
                }
            }
            return Ok(Some((cur.key, text, cur.start)));
        }
        Ok(None)
    }

    /// DX4: exact, folded, then suffix rules; redirects followed (DX5); one entry per record.
    pub fn look_up(&self, query: &str) -> Result<Vec<Entry>, LookupError> {
        let mut hits = self.index.find(query)?;
        let mut for_word = None;
        if hits.is_empty() {
            for base in suffix_candidates(query) {
                let found = self.index.find(&base)?;
                if !found.is_empty() {
                    hits = found;
                    for_word = Some(query.trim().to_string());
                    break;
                }
            }
        }
        let mut out = vec![];
        let mut records = std::collections::HashSet::new();
        for hit in &hits {
            if let Some((headword, html, start)) = self.resolve(hit)? {
                if records.insert(start) {
                    out.push(Entry {
                        headword,
                        html,
                        for_word: for_word.clone(),
                    });
                }
            }
        }
        Ok(out)
    }

    /// Whether the dictionary has anything for the query (for the provider menu, DX11).
    pub fn has(&self, query: &str) -> Result<bool, LookupError> {
        Ok(!self.look_up(query)?.is_empty())
    }

    /// A resource from the MDD files, by the path an entry names (`dot.png`, `\img\a.png`).
    pub fn resource(&self, path: &str) -> Result<Option<Vec<u8>>, LookupError> {
        let Some(hit) = self.index.resource(path)? else {
            return Ok(None);
        };
        let Some(mdd) = self.mdds.get((hit.file - 1).max(0) as usize) else {
            return Ok(None);
        };
        Ok(Some(mdd.record(hit.start, hit.end)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    fn opened(name: &str) -> (tempfile::TempDir, Opened) {
        let tmp = tempfile::tempdir().unwrap();
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mdx")
            .join(name);
        let built = generation::build(
            tmp.path(),
            &generation::sources(&src).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let o = Opened::open(tmp.path(), &built.generation).unwrap();
        (tmp, o)
    }

    #[test]
    fn exact_then_folded_with_duplicates_in_source_order() {
        let (_t, o) = opened("basic.mdx");
        let e = o.look_up("Invalidate").unwrap();
        assert_eq!(e.len(), 1, "exact hits only");
        let e = o.look_up("INVALIDATE").unwrap();
        assert_eq!(e.len(), 2, "both folded hits");
        assert!(e[0].html.contains("in·val·i·date") || e[1].html.contains("in·val·i·date"));
        assert_eq!(o.look_up("it's").unwrap()[0].headword, "it\u{2019}s");
        assert_eq!(o.look_up("cafe\u{301}").unwrap()[0].headword, "caf\u{e9}");
    }

    #[test]
    fn redirects_resolve_and_cycles_stop() {
        let (_t, o) = opened("basic.mdx");
        let e = o.look_up("caches").unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].headword, "cache");
        assert_eq!(e[0].for_word, None, "a redirect is not a suffix rule");
        assert!(o.look_up("cycle-a").unwrap().is_empty());
    }

    #[test]
    fn suffix_rules_are_labelled_and_never_invent() {
        let (_t, o) = opened("basic.mdx");
        let e = o.look_up("invalidates").unwrap();
        assert!(!e.is_empty());
        assert_eq!(e[0].for_word.as_deref(), Some("invalidates"));
        assert_eq!(o.look_up("news").unwrap()[0].headword, "news");
        assert!(o.look_up("bus").unwrap().is_empty());
        assert_eq!(o.look_up("data").unwrap()[0].for_word, None);
    }

    #[test]
    fn resources_come_from_the_mdd_files() {
        let (_t, o) = opened("basic.mdx");
        assert!(o.resource("test.css").unwrap().unwrap().starts_with(b".hw"));
        assert!(o
            .resource("\\dot.png")
            .unwrap()
            .unwrap()
            .starts_with(b"\x89PNG"));
        assert!(o.resource("missing.png").unwrap().is_none());
    }
}
