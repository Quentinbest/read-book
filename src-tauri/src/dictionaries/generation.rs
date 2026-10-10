//! DX1, DX2: importing a dictionary as a new, immutable generation.
//!
//! An import copies the `.mdx` and the MDD files that share its base name (`name.mdd`,
//! `name.1.mdd`, `name.2.mdd` …) into `Dictionaries/.staging-<id>/`, checks that no
//! source changed while it was copied, validates every file and builds the index, and
//! only then renames the folder to `Dictionaries/<id>/` and marks it complete. A failure,
//! a cancel or a full disk leaves the old generation working; a kill at any step leaves
//! a staging folder or an unreferenced generation, which `recover` removes at the next
//! launch. One folder stays the whole backup (D2): stored names are file names inside it.

use super::index::{self, IndexError};
use super::mdx::{Mdx, MdxError};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub const COMPLETE: &str = ".complete";
const STAGING: &str = ".staging-";

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Mdx(#[from] MdxError),
    #[error("{0}")]
    Index(String),
    #[error("could not write to the library: {0}")]
    Io(#[from] std::io::Error),
    #[error("the file changed while it was being added")]
    Changed,
    #[error("the import was cancelled")]
    Cancelled,
    #[error("this isn't an .mdx file")]
    NotMdx,
}

impl From<IndexError> for ImportError {
    fn from(e: IndexError) -> Self {
        match e {
            IndexError::Mdx(m) => ImportError::Mdx(m),
            IndexError::Cancelled => ImportError::Cancelled,
            IndexError::Sqlite(s) => ImportError::Index(s.to_string()),
        }
    }
}

/// The files of one dictionary, as picked: the MDX and its MDDs in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Sources {
    pub mdx: PathBuf,
    pub mdds: Vec<PathBuf>,
}

/// DX1: the MDD files that come with an `.mdx`: same folder, same base name.
pub fn sources(mdx: &Path) -> Result<Sources, ImportError> {
    let ext = mdx.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !ext.eq_ignore_ascii_case("mdx") {
        return Err(ImportError::NotMdx);
    }
    let stem = mdx
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(ImportError::NotMdx)?;
    let dir = mdx.parent().unwrap_or(Path::new("."));
    let mut mdds = vec![];
    let first = dir.join(format!("{stem}.mdd"));
    if first.is_file() {
        mdds.push(first);
    }
    for n in 1..100 {
        let p = dir.join(format!("{stem}.{n}.mdd"));
        if !p.is_file() {
            break;
        }
        mdds.push(p);
    }
    Ok(Sources {
        mdx: mdx.to_path_buf(),
        mdds,
    })
}

/// What a finished import built.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub generation: String,
    pub name: String,
    pub title: String,
    pub source_hash: String,
    pub entries: u64,
    pub resources: u64,
    pub files: Vec<String>,
}

fn stamp(p: &Path) -> std::io::Result<(u64, Option<std::time::SystemTime>)> {
    let m = std::fs::metadata(p)?;
    Ok((m.len(), m.modified().ok()))
}

/// Build a generation in `dir` (the library's `Dictionaries` folder) from `src`.
pub fn build(dir: &Path, src: &Sources, cancel: &AtomicBool) -> Result<Built, ImportError> {
    std::fs::create_dir_all(dir)?;
    let generation = uuid::Uuid::new_v4().simple().to_string();
    let staging = dir.join(format!("{STAGING}{generation}"));
    std::fs::create_dir(&staging)?;
    let result = build_in(&staging, src, cancel).and_then(|mut built| {
        std::fs::write(staging.join(COMPLETE), b"")?;
        std::fs::rename(&staging, dir.join(&generation))?;
        built.generation = generation.clone();
        Ok(built)
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn build_in(staging: &Path, src: &Sources, cancel: &AtomicBool) -> Result<Built, ImportError> {
    let name = src
        .mdx
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dictionary")
        .to_string();
    // Copy under fixed names, so nothing a file is called can reach outside the folder.
    let mut copies = vec![];
    for (i, from) in std::iter::once(&src.mdx).chain(&src.mdds).enumerate() {
        let before = stamp(from)?;
        let to = staging.join(if i == 0 {
            "dictionary.mdx".to_string()
        } else {
            format!("resources-{i}.mdd")
        });
        std::fs::copy(from, &to)?;
        if stamp(from)? != before || std::fs::metadata(&to)?.len() != before.0 {
            return Err(ImportError::Changed);
        }
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(ImportError::Cancelled);
        }
        copies.push(to);
    }
    let source_hash = crate::import::sha256_file(&copies[0])?;
    // Validate every file before anything is activated (DX3).
    let mdx = Mdx::open(&copies[0])?;
    if mdx.is_mdd {
        return Err(ImportError::NotMdx);
    }
    let mdds = copies[1..]
        .iter()
        .map(|p| Mdx::open(p))
        .collect::<Result<Vec<_>, _>>()?;
    let (entries, resources) = index::build(&staging.join(index::FILE), &mdx, &mdds, cancel)?;
    let title = match mdx.header.title.trim() {
        "" | "Title (No HTML code allowed)" => name.clone(),
        t => t.to_string(),
    };
    Ok(Built {
        generation: String::new(),
        name,
        title,
        source_hash,
        entries,
        resources,
        files: copies
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
            .collect(),
    })
}

/// The MDX of a generation, and its MDDs in order (resources-1.mdd …).
pub fn files(dir: &Path, generation: &str) -> Option<(PathBuf, Vec<PathBuf>)> {
    let g = generation_dir(dir, generation)?;
    if !g.join(COMPLETE).is_file() {
        return None;
    }
    let mdds = (1..100)
        .map(|i| g.join(format!("resources-{i}.mdd")))
        .take_while(|p| p.is_file())
        .collect();
    Some((g.join("dictionary.mdx"), mdds))
}

/// A generation's folder, if the id is one this module makes (32 hex digits) and it exists.
pub fn generation_dir(dir: &Path, generation: &str) -> Option<PathBuf> {
    let ok = generation.len() == 32 && generation.bytes().all(|b| b.is_ascii_hexdigit());
    let g = dir.join(generation);
    (ok && g.is_dir()).then_some(g)
}

/// Remove every generation that is neither active nor `kept` (an open entry still uses
/// it), and, at launch only (`staging`), the folders of imports a kill interrupted.
pub fn recover(
    dir: &Path,
    active: &[String],
    kept: &[String],
    staging: bool,
) -> std::io::Result<usize> {
    let mut removed = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let half_built = staging && name.starts_with(STAGING);
        let unused = generation_dir(dir, &name).is_some()
            && !active.contains(&name)
            && !kept.contains(&name);
        if half_built || unused {
            std::fs::remove_dir_all(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mdx")
    }

    #[test]
    fn an_mdx_brings_its_mdd_files() {
        let s = sources(&fixtures().join("basic.mdx")).unwrap();
        let names: Vec<_> = s
            .mdds
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["basic.mdd", "basic.1.mdd"]);
        assert!(sources(&fixtures().join("basic.mdd")).is_err());
    }

    #[test]
    fn a_good_import_activates_and_a_bad_one_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Dictionaries");
        let no = AtomicBool::new(false);
        let built = build(&dir, &sources(&fixtures().join("basic.mdx")).unwrap(), &no).unwrap();
        assert_eq!((built.entries, built.resources), (13, 3));
        assert_eq!(built.title, "Basic Test Dictionary");
        let (mdx, mdds) = files(&dir, &built.generation).unwrap();
        assert!(mdx.is_file() && mdds.len() == 2);
        for bad in ["registered.mdx", "truncated.mdx", "lzo.mdx", "counts.mdx"] {
            assert!(
                build(&dir, &sources(&fixtures().join(bad)).unwrap(), &no).is_err(),
                "{bad}"
            );
        }
        // Only the good generation is left: failures removed their staging folders.
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left.len(), 1);
        // A cancelled import leaves nothing either.
        let yes = AtomicBool::new(true);
        assert!(matches!(
            build(&dir, &sources(&fixtures().join("basic.mdx")).unwrap(), &yes),
            Err(ImportError::Cancelled)
        ));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn recovery_removes_half_built_and_unused_generations() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let no = AtomicBool::new(false);
        let a = build(dir, &sources(&fixtures().join("basic.mdx")).unwrap(), &no).unwrap();
        let b = build(dir, &sources(&fixtures().join("v1.mdx")).unwrap(), &no).unwrap();
        let c = build(dir, &sources(&fixtures().join("stored.mdx")).unwrap(), &no).unwrap();
        std::fs::create_dir(dir.join(".staging-killed")).unwrap();
        // a is active, b is still open in a peek, c was replaced.
        // While an import runs, its staging folder stays.
        assert_eq!(
            recover(
                dir,
                std::slice::from_ref(&a.generation),
                &[b.generation.clone(), c.generation.clone()],
                false
            )
            .unwrap(),
            0
        );
        let removed = recover(
            dir,
            std::slice::from_ref(&a.generation),
            std::slice::from_ref(&b.generation),
            true,
        )
        .unwrap();
        assert_eq!(removed, 2);
        assert!(files(dir, &a.generation).is_some());
        assert!(files(dir, &b.generation).is_some());
        assert!(files(dir, &c.generation).is_none());
        // Ids are checked: nothing outside the folder is named.
        assert!(generation_dir(dir, "../etc").is_none());
    }
}
