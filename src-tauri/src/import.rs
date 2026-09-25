//! Import pipeline (plan Phase 1, B3, E3, E4).
//!
//! validate (zip limits, XML entities, DRM) → hash → duplicate or updated-file
//! check (B3) → copy into the library atomically → metadata and cover → store.

use crate::epub::{self, Rejection};
use crate::store::{now_ms, Store, StoreError};
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Covers larger than this are not extracted; the generated cover is used instead.
const MAX_COVER_BYTES: u64 = 10 << 20;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportOutcome {
    /// A new book. `damaged` counts unreadable spine items (E3).
    Imported {
        book_id: String,
        title: String,
        damaged: usize,
    },
    /// The same file is already in the library (B3): nothing copied.
    AlreadyInLibrary { book_id: String },
    /// A changed file of a book already in the library (same OPF identifier, B3):
    /// the copy was replaced and its annotations are queued for re-anchoring.
    Replaced {
        book_id: String,
        title: String,
        damaged: usize,
    },
    /// Not imported. `reason` is shown on the E3 card or as the hostile-file message.
    Rejected {
        reason: String,
        hostile: bool,
        drm: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not write to the library: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Library {
    /// ~/Library/Application Support/app.linen.reader/Books (B3)
    pub books_dir: PathBuf,
    pub covers_dir: PathBuf,
}

impl Library {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let lib = Library {
            books_dir: root.join("Books"),
            covers_dir: root.join("Covers"),
        };
        std::fs::create_dir_all(&lib.books_dir)?;
        std::fs::create_dir_all(&lib.covers_dir)?;
        Ok(lib)
    }
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn hostile(r: &Rejection) -> bool {
    matches!(
        r,
        Rejection::TooManyEntries
            | Rejection::TooLarge
            | Rejection::CompressionRatio(_)
            | Rejection::UnsafePath(_)
            | Rejection::Symlink(_)
            | Rejection::XmlEntities(_)
    )
}

/// Write `data` to `dest` atomically: a temporary file in the same folder, synced, then renamed (E5).
fn write_atomic(dest: &Path, mut data: impl Read) -> std::io::Result<()> {
    let tmp = dest.with_extension("partial");
    {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::copy(&mut data, &mut f)?;
        f.flush()?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, dest)
}

/// A stable tint for the generated typographic cover (E4), derived from the title.
pub fn cover_tint(title: &str) -> String {
    // Screen 01's cover tints (the same list as src/lib/library/tint.ts, whose test
    // keeps the lettering above 7:1, X1); chosen by a hash of the title.
    const TINTS: [&str; 14] = [
        "#B9C6CC", "#D8C8A8", "#C9BED6", "#BCC9A8", "#E0C4BE", "#A9B4A4", "#D6CFC0", "#CDB592",
        "#D9B8A0", "#C8D3B6", "#C9A9A6", "#B8B3A6", "#D8C3CF", "#CFC39A",
    ];
    let h = title
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    TINTS[(h as usize) % TINTS.len()].to_string()
}

fn same_title(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn file_stem_title(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().replace(['_', '-'], " "))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Untitled".into())
}

pub fn import_book(
    store: &mut Store,
    lib: &Library,
    source: &Path,
) -> Result<ImportOutcome, ImportError> {
    let hash = sha256_file(source)?;
    if let Some(book_id) = store.book_by_hash(&hash)? {
        // A book removed this session comes back as it was (G4, provisional).
        store.restore_book(&book_id)?;
        return Ok(ImportOutcome::AlreadyInLibrary { book_id });
    }
    // Parsing untrusted files must never take the app down: a panic here (a parser
    // bug) rejects this one file. It could not unwind through the WebView's native
    // callback, so uncaught it would abort the process.
    let parsed = std::panic::catch_unwind(|| epub::open(source))
        .unwrap_or_else(|_| Err(Rejection::BadPackage));
    let (mut archive, package) = match parsed {
        Ok(x) => x,
        Err(r) => {
            return Ok(ImportOutcome::Rejected {
                hostile: hostile(&r),
                drm: matches!(r, Rejection::Drm(_)),
                reason: r.to_string(),
            })
        }
    };
    if package.spine.is_empty() {
        return Ok(ImportOutcome::Rejected {
            hostile: false,
            drm: false,
            reason: "none of the book's chapters could be opened".into(),
        });
    }

    // E4: title from the package, else the first heading, else the file name.
    let md = &package.metadata;
    let (title, title_source) = match (&md.title, &package.first_heading) {
        (Some(t), _) => (t.clone(), "package"),
        (None, Some(h)) => (h.clone(), "heading"),
        (None, None) => (file_stem_title(source), "filename"),
    };

    // B3: the same OPF identifier with different bytes is an updated file. Guard
    // (assumption, docs/decisions.md): identifiers are sometimes reused or left as
    // placeholders, so the package titles must also match; otherwise it is a new book.
    let existing = match (md.package_identifier.as_deref(), md.title.as_deref()) {
        (Some(ident), Some(package_title)) => store
            .books_by_package_identifier(ident)?
            .into_iter()
            .find(|(_, _, t)| same_title(t, package_title))
            .map(|(id, path, _)| (id, path)),
        _ => None,
    };
    let book_id = existing
        .as_ref()
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let dest = lib.books_dir.join(format!("{book_id}.epub"));
    write_atomic(&dest, std::fs::File::open(source)?)?;

    let cover_path = match &package.cover {
        Some(href) => extract_cover(&mut archive, href, &lib.covers_dir, &book_id)?,
        None => None,
    };
    let tint = cover_path.is_none().then(|| cover_tint(&title));
    let authors = serde_json::to_string(&md.authors).unwrap_or_else(|_| "[]".into());
    let a11y = serde_json::to_string(&md.a11y).unwrap_or_else(|_| "[]".into());
    let now = now_ms();

    let tx = store.conn_mut().transaction()?;
    if existing.is_some() {
        tx.execute(
            "UPDATE books SET content_hash = ?2, file_path = ?3, title = ?4, title_source = ?5, authors = ?6,
                 language = ?7, page_direction = ?8, layout = ?9, has_page_list = ?10, a11y_metadata = ?11,
                 cover_path = ?12, generated_cover_tint = ?13, replaced_at = ?14, removed_at = NULL
             WHERE id = ?1",
            params![
                book_id, hash, dest.to_string_lossy(), title, title_source, authors, md.language,
                md.page_direction, md.layout, md.has_page_list as i64, a11y, cover_path, tint, now
            ],
        )?;
        // Annotations were anchored to the old file; re-anchoring runs when the book opens (B3, Phase 5).
        tx.execute(
            "UPDATE annotations SET anchor_status = 'reanchor' WHERE book_id = ?1 AND anchored_content_hash != ?2",
            params![book_id, hash],
        )?;
        tx.execute("DELETE FROM book_damage WHERE book_id = ?1", [&book_id])?;
    } else {
        tx.execute(
            "INSERT INTO books (id, content_hash, package_identifier, file_path, title, title_source, authors,
                 language, page_direction, layout, has_page_list, a11y_metadata, cover_path, generated_cover_tint,
                 added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                book_id, hash, md.package_identifier, dest.to_string_lossy(), title, title_source, authors,
                md.language, md.page_direction, md.layout, md.has_page_list as i64, a11y, cover_path, tint, now
            ],
        )?;
    }
    for d in &package.damage {
        tx.execute(
            "INSERT OR REPLACE INTO book_damage (book_id, item_href, error_kind) VALUES (?1, ?2, ?3)",
            params![book_id, d.item_href, d.error_kind],
        )?;
    }
    tx.commit()?;

    let damaged = package.damage.len();
    Ok(if existing.is_some() {
        ImportOutcome::Replaced {
            book_id,
            title,
            damaged,
        }
    } else {
        ImportOutcome::Imported {
            book_id,
            title,
            damaged,
        }
    })
}

fn extract_cover<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    href: &str,
    dir: &Path,
    book_id: &str,
) -> std::io::Result<Option<String>> {
    let Ok(entry) = archive.by_name(href) else {
        return Ok(None);
    };
    if entry.size() > MAX_COVER_BYTES {
        return Ok(None);
    }
    let ext = Path::new(href)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| {
            ["jpg", "jpeg", "png", "gif", "webp", "svg"].contains(&e.to_ascii_lowercase().as_str())
        })
        .unwrap_or("img")
        .to_ascii_lowercase();
    let dest = dir.join(format!("{book_id}.{ext}"));
    write_atomic(&dest, entry.take(MAX_COVER_BYTES))?;
    Ok(Some(dest.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_tints_match_the_ui_list() {
        // src/lib/library/tint.ts holds the same list, with the contrast test.
        let ts = include_str!("../../src/lib/library/tint.ts");
        for n in 0..40 {
            let tint = cover_tint(&format!("Title {n}"));
            assert!(
                ts.contains(&format!("'{tint}'")),
                "{tint} is not in tint.ts"
            );
        }
    }

    fn corpus(name: &str) -> Option<PathBuf> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("corpus");
        [
            root.join("cache").join(name),
            root.join("generated").join(name),
        ]
        .into_iter()
        .find(|p| p.exists())
    }

    fn setup() -> (tempfile::TempDir, Store, Library) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("linen.db")).unwrap();
        let lib = Library::new(dir.path()).unwrap();
        (dir, store, lib)
    }

    /// Every corpus file gives the expected outcome (plan Phase 1 Done-when).
    /// Skipped when the corpus has not been fetched and generated.
    #[test]
    fn corpus_outcomes() {
        let expected: &[(&str, &str)] = &[
            ("standardebooks-moby-dick.epub", "imported"),
            ("gutenberg-2701-moby-dick-epub2.epub", "imported"),
            ("idpf-wasteland-otf-obf.epub", "imported"),
            ("idpf-page-blanche.epub", "imported"),
            ("idpf-regime-anticancer-arabic.epub", "imported"),
            ("hostile-content.epub", "imported"), // content is neutralised at render time (Spike E)
            ("broken-missing-items.epub", "damaged"),
            ("broken-no-metadata.epub", "imported"),
            ("broken-no-toc.epub", "imported"),
            ("broken-bad-css-and-font.epub", "imported"),
            ("broken-truncated.epub", "rejected"),
            ("broken-bad-opf.epub", "rejected"),
            ("zip-traversal.epub", "hostile"),
            ("zip-absolute.epub", "hostile"),
            ("zip-symlink.epub", "hostile"),
            ("zip-bomb.epub", "hostile"),
            ("zip-100k-entries.epub", "hostile"),
            ("xml-billion-laughs.epub", "hostile"),
            ("xml-external-entity.epub", "hostile"),
        ];
        let (_dir, mut store, lib) = setup();
        let mut checked = 0;
        for (name, want) in expected {
            let Some(path) = corpus(name) else { continue };
            let got = match import_book(&mut store, &lib, &path).unwrap() {
                ImportOutcome::Imported { damaged: 0, .. } => "imported",
                ImportOutcome::Imported { .. } => "damaged",
                ImportOutcome::Rejected { hostile: true, .. } => "hostile",
                ImportOutcome::Rejected { .. } => "rejected",
                other => panic!("{name}: unexpected {other:?}"),
            };
            assert_eq!(got, *want, "{name}");
            checked += 1;
        }
        eprintln!(
            "corpus_outcomes: {checked}/{} corpus files present and checked",
            expected.len()
        );
    }

    /// Every corpus file imports or is refused; none takes the importer down
    /// (a Japanese book once panicked the entity check at a character boundary).
    #[test]
    fn every_corpus_file_imports_or_is_refused() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../corpus");
        let (_dir, mut store, lib) = setup();
        let mut seen = 0;
        for dir in ["cache", "generated"] {
            let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "epub") {
                    continue;
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    import_book(&mut store, &lib, &path)
                }));
                assert!(outcome.is_ok(), "{} panicked", path.display());
                seen += 1;
            }
        }
        eprintln!("every_corpus_file_imports_or_is_refused: {seen} files");
    }

    #[test]
    fn missing_metadata_falls_back_to_the_first_heading_and_a_generated_cover() {
        let Some(path) = corpus("broken-no-metadata.epub") else {
            return;
        };
        let (_dir, mut store, lib) = setup();
        import_book(&mut store, &lib, &path).unwrap();
        let book = &store.books().unwrap()[0];
        assert_eq!(book.title_source, "heading");
        assert!(book.cover_path.is_none());
        assert!(book.generated_cover_tint.is_some());
    }

    #[test]
    fn duplicates_and_updated_files_follow_b3() {
        let Some(path) = corpus("standardebooks-moby-dick.epub") else {
            return;
        };
        let (dir, mut store, lib) = setup();
        let first = import_book(&mut store, &lib, &path).unwrap();
        let ImportOutcome::Imported { book_id, .. } = first else {
            panic!("{first:?}")
        };

        // Same bytes: already in the library, nothing copied.
        assert_eq!(
            import_book(&mut store, &lib, &path).unwrap(),
            ImportOutcome::AlreadyInLibrary {
                book_id: book_id.clone()
            }
        );

        // Same OPF identifier, different bytes: an updated file replaces the copy.
        store
            .conn()
            .execute(
                "INSERT INTO annotations (id, book_id, anchored_content_hash, color, cfi_range, quote_exact, created_at, updated_at)
                 VALUES ('a1', ?1, (SELECT content_hash FROM books WHERE id = ?1), 'yellow', 'epubcfi(/6/4!/4/2,/1:0,/1:4)', 'Call', 0, 0)",
                [&book_id],
            )
            .unwrap();
        let changed = dir.path().join("moby-changed.epub");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"trailing bytes change the hash, not the archive");
        std::fs::write(&changed, bytes).unwrap();
        let replaced = import_book(&mut store, &lib, &changed).unwrap();
        assert!(
            matches!(&replaced, ImportOutcome::Replaced { book_id: id, .. } if *id == book_id),
            "{replaced:?}"
        );
        assert_eq!(store.books().unwrap().len(), 1);
        let status: String = store
            .conn()
            .query_row(
                "SELECT anchor_status FROM annotations WHERE id = 'a1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "reanchor");
        assert!(std::fs::read_dir(&lib.books_dir).unwrap().count() == 1);
    }

    #[test]
    fn a_reused_identifier_with_a_different_title_is_a_new_book() {
        let (Some(a), Some(b)) = (corpus("broken-no-toc.epub"), corpus("code-heavy.epub")) else {
            return;
        };
        let (_dir, mut store, lib) = setup();
        // Give both the same identifier.
        let ident = "urn:uuid:11111111-1111-4111-8111-111111111111";
        let retag = |src: &Path, dst: &Path| {
            let mut input = zip::ZipArchive::new(std::fs::File::open(src).unwrap()).unwrap();
            let mut out = zip::ZipWriter::new(std::fs::File::create(dst).unwrap());
            for i in 0..input.len() {
                let mut e = input.by_index(i).unwrap();
                let name = e.name().to_string();
                let mut data = Vec::new();
                e.read_to_end(&mut data).unwrap();
                if name.ends_with(".opf") {
                    let text = String::from_utf8(data).unwrap();
                    let start = text.find("<dc:identifier id=\"uid\">").unwrap() + 24;
                    let end = start + text[start..].find('<').unwrap();
                    data = format!("{}{}{}", &text[..start], ident, &text[end..]).into_bytes();
                }
                out.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                out.write_all(&data).unwrap();
            }
            out.finish().unwrap();
        };
        let (a2, b2) = (_dir.path().join("a.epub"), _dir.path().join("b.epub"));
        retag(&a, &a2);
        retag(&b, &b2);
        assert!(matches!(
            import_book(&mut store, &lib, &a2).unwrap(),
            ImportOutcome::Imported { .. }
        ));
        assert!(matches!(
            import_book(&mut store, &lib, &b2).unwrap(),
            ImportOutcome::Imported { .. }
        ));
        assert_eq!(store.books().unwrap().len(), 2);
    }

    /// B9: DRM-protected files are detected and not added.
    #[test]
    fn drm_protected_books_are_rejected_as_drm() {
        let Some(src) = corpus("broken-no-toc.epub") else {
            return;
        };
        let (dir, mut store, lib) = setup();
        let dst = dir.path().join("drm.epub");
        let mut input = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
        let mut out = zip::ZipWriter::new(std::fs::File::create(&dst).unwrap());
        for i in 0..input.len() {
            let mut e = input.by_index(i).unwrap();
            let name = e.name().to_string();
            let mut data = Vec::new();
            e.read_to_end(&mut data).unwrap();
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(&data).unwrap();
        }
        out.start_file(
            "META-INF/encryption.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        out.write_all(br#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/><enc:CipherData><enc:CipherReference URI="OEBPS/chapter-1.xhtml"/></enc:CipherData></enc:EncryptedData></encryption>"#).unwrap();
        out.finish().unwrap();
        let outcome = import_book(&mut store, &lib, &dst).unwrap();
        assert!(
            matches!(
                outcome,
                ImportOutcome::Rejected {
                    drm: true,
                    hostile: false,
                    ..
                }
            ),
            "{outcome:?}"
        );
        assert!(store.books().unwrap().is_empty());
    }

    #[test]
    fn tints_are_stable() {
        assert_eq!(cover_tint("Moby-Dick"), cover_tint("Moby-Dick"));
    }
}
