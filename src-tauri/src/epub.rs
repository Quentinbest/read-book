//! EPUB archive validation and package parsing for import (plan §7.1, E1–E4).
//!
//! Everything here treats the book as hostile: zip entries are checked against
//! the approved limits before anything is read, and every XML document is
//! refused if it declares entities (Spike E finding 2: entity expansion hangs
//! WebKit's DOMParser, so such XML must never reach the WebView).

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};
use std::path::Path;

/// Approved limits (plan §7.1, 2026-09-24).
pub const MAX_ENTRIES: usize = 10_000;
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 1 << 30;
pub const MAX_RATIO: u64 = 100;
/// Entries smaller than this are exempt from the ratio check: tiny files such as
/// a run of spaces legitimately compress far better than 100:1.
const RATIO_EXEMPT_BELOW: u64 = 64 * 1024;
/// XML documents are parsed in memory; anything larger is not a real OPF or nav.
const MAX_XML_BYTES: u64 = 16 << 20;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Rejection {
    #[error("not a zip archive or the archive is truncated")]
    NotZip,
    #[error("more than {MAX_ENTRIES} entries")]
    TooManyEntries,
    #[error("uncompressed size over 1 GB")]
    TooLarge,
    #[error("entry {0} compresses more than {MAX_RATIO}:1")]
    CompressionRatio(String),
    #[error("unsafe entry path {0}")]
    UnsafePath(String),
    #[error("symbolic link entry {0}")]
    Symlink(String),
    #[error("no container.xml or no package document")]
    NoPackage,
    #[error("the package document is not well-formed XML")]
    BadPackage,
    #[error("{0} declares XML entities")]
    XmlEntities(String),
    #[error("encrypted content (DRM) in {0}")]
    Drm(String),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Metadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    /// The OPF unique-identifier value (B3 duplicate detection).
    pub package_identifier: Option<String>,
    /// `ltr`, `rtl` or `default`, from the spine's page-progression-direction.
    pub page_direction: String,
    /// `reflowable` or `fixed`.
    pub layout: String,
    pub has_page_list: bool,
    /// EPUB accessibility metadata (E10): schema:accessMode, accessibilityFeature, …
    pub a11y: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Damage {
    pub item_href: String,
    pub error_kind: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Package {
    pub opf_path: String,
    pub metadata: Metadata,
    /// Spine items in reading order (zip paths).
    pub spine: Vec<String>,
    /// Zip path of the cover image, if the package names one.
    pub cover: Option<String>,
    /// First heading of the first spine document, for E4's title fallback.
    pub first_heading: Option<String>,
    pub damage: Vec<Damage>,
    pub has_toc: bool,
}

/// Check the archive against the §7.1 limits without decompressing anything.
pub fn validate_archive<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> Result<(), Rejection> {
    if archive.len() > MAX_ENTRIES {
        return Err(Rejection::TooManyEntries);
    }
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|_| Rejection::NotZip)?;
        let name = entry.name().to_string();
        if !is_safe_path(&name) {
            return Err(Rejection::UnsafePath(name));
        }
        if entry.is_symlink() {
            return Err(Rejection::Symlink(name));
        }
        let size = entry.size();
        total = total.saturating_add(size);
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(Rejection::TooLarge);
        }
        let compressed = entry.compressed_size().max(1);
        if size >= RATIO_EXEMPT_BELOW && size / compressed > MAX_RATIO {
            return Err(Rejection::CompressionRatio(name));
        }
    }
    Ok(())
}

/// Relative, normalised paths only: no absolute paths, drive letters, `..`,
/// backslashes or NUL.
pub fn is_safe_path(name: &str) -> bool {
    let drive_letter = name.len() > 1 && name.as_bytes()[1] == b':';
    let unsafe_path = name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || drive_letter
        || name.split('/').any(|seg| seg == "..");
    !unsafe_path
}

/// Read a (small) XML entry, refusing entity declarations.
fn read_xml<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<String>, Rejection> {
    let Ok(entry) = archive.by_name(name) else {
        return Ok(None);
    };
    if entry.size() > MAX_XML_BYTES {
        return Err(Rejection::BadPackage);
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .take(MAX_XML_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|_| Rejection::NotZip)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    check_xml_entities(name, &text)?;
    Ok(Some(text))
}

/// Refuse a DOCTYPE that declares entities (internal subset). Plain `<!DOCTYPE html>`
/// and public XHTML doctypes without an internal subset are fine.
pub fn check_xml_entities(name: &str, text: &str) -> Result<(), Rejection> {
    // The first 64 KB, cut at a character boundary: a multi-byte character (CJK
    // text) can straddle the limit, and slicing inside it would panic.
    let mut cut = text.len().min(64 * 1024);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let head = &text[..cut];
    if let Some(start) = find_ci(head, "<!DOCTYPE") {
        let rest = &text[start..];
        let end = rest.find('>').unwrap_or(rest.len());
        let subset_start = rest[..end].find('[');
        if subset_start.is_some() || find_ci(rest, "<!ENTITY").is_some_and(|i| i < 64 * 1024) {
            return Err(Rejection::XmlEntities(name.to_string()));
        }
    }
    if find_ci(head, "<!ENTITY").is_some() {
        return Err(Rejection::XmlEntities(name.to_string()));
    }
    Ok(())
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_uppercase().find(&needle.to_ascii_uppercase())
}

/// Resolve `href` relative to the directory of `base` (both zip paths).
pub fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let href = percent_decode(href);
    let mut parts: Vec<&str> = match base.rfind('/') {
        Some(i) => base[..i].split('/').collect(),
        None => vec![],
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn local_name(raw: &str) -> String {
    raw.rsplit(':').next().unwrap_or("").to_string()
}

fn attrs(e: &quick_xml::events::BytesStart) -> HashMap<String, String> {
    e.attributes()
        .flatten()
        .map(|a| {
            let key = a.key.as_ref().to_string();
            let value = a
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| a.value.to_string());
            (key, value)
        })
        .collect()
}

fn container_rootfile(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local_name(e.name().as_ref()) == "rootfile" =>
            {
                let a = attrs(&e);
                let media = a
                    .get("media-type")
                    .map(String::as_str)
                    .unwrap_or("application/oebps-package+xml");
                if media == "application/oebps-package+xml" {
                    return a.get("full-path").cloned();
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

struct ManifestItem {
    href: String,
    media_type: String,
    properties: Vec<String>,
}

/// Parse the package: metadata, spine, cover, TOC presence and damaged items.
pub fn read_package<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<Package, Rejection> {
    if archive.by_name("META-INF/encryption.xml").is_ok() {
        // Font obfuscation also uses encryption.xml; only real encryption is DRM.
        let xml = read_xml(archive, "META-INF/encryption.xml")?.unwrap_or_default();
        if let Some(item) = drm_encrypted_item(&xml) {
            return Err(Rejection::Drm(item));
        }
    }
    let container = read_xml(archive, "META-INF/container.xml")?.ok_or(Rejection::NoPackage)?;
    let opf_path = container_rootfile(&container).ok_or(Rejection::NoPackage)?;
    let opf = read_xml(archive, &opf_path)?.ok_or(Rejection::NoPackage)?;

    let mut reader = Reader::from_str(&opf);
    let mut md = Metadata {
        page_direction: "default".into(),
        layout: "reflowable".into(),
        ..Default::default()
    };
    let mut unique_id_ref = None;
    let mut identifiers: Vec<(Option<String>, String)> = Vec::new();
    let mut manifest: HashMap<String, ManifestItem> = HashMap::new();
    let mut spine_ids: Vec<String> = Vec::new();
    let mut toc_ncx_id: Option<String> = None;
    let mut cover_meta_id: Option<String> = None;
    let mut in_metadata = false;
    let mut text_target: Option<(String, HashMap<String, String>)> = None;
    let mut text = String::new();
    let mut saw_package = false;

    loop {
        let event = reader.read_event().map_err(|_| Rejection::BadPackage)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = local_name(e.name().as_ref());
                let a = attrs(e);
                let empty = matches!(event, Event::Empty(_));
                match name.as_str() {
                    "package" => {
                        saw_package = true;
                        unique_id_ref = a.get("unique-identifier").cloned();
                    }
                    "metadata" => in_metadata = !empty,
                    "item" => {
                        if let (Some(id), Some(href)) = (a.get("id"), a.get("href")) {
                            manifest.insert(
                                id.clone(),
                                ManifestItem {
                                    href: resolve(&opf_path, href),
                                    media_type: a.get("media-type").cloned().unwrap_or_default(),
                                    properties: a
                                        .get("properties")
                                        .map(|p| p.split_whitespace().map(String::from).collect())
                                        .unwrap_or_default(),
                                },
                            );
                        }
                    }
                    "spine" => {
                        toc_ncx_id = a.get("toc").cloned();
                        if let Some(d) = a.get("page-progression-direction") {
                            md.page_direction = d.clone();
                        }
                    }
                    "itemref" => {
                        if let Some(id) = a.get("idref") {
                            spine_ids.push(id.clone());
                        }
                    }
                    "meta" if in_metadata => {
                        if a.get("name").map(String::as_str) == Some("cover") {
                            cover_meta_id = a.get("content").cloned();
                        }
                        if !empty {
                            text_target = Some(("meta".into(), a));
                            text.clear();
                        }
                    }
                    "title" | "creator" | "language" | "identifier" if in_metadata && !empty => {
                        text_target = Some((name.clone(), a));
                        text.clear();
                    }
                    _ => {}
                }
            }
            Event::Text(t) if text_target.is_some() => {
                text.push_str(&t.xml10_content());
            }
            Event::GeneralRef(r) if text_target.is_some() => {
                // Character references and the five predefined entities only;
                // documents declaring other entities were refused earlier.
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else {
                    text.push_str(match r.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(e) => {
                let name = local_name(e.name().as_ref());
                if name == "metadata" {
                    in_metadata = false;
                }
                if let Some((target, a)) = text_target.take() {
                    if target != name {
                        text_target = Some((target, a));
                        continue;
                    }
                    let value = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    match target.as_str() {
                        "title" if md.title.is_none() && !value.is_empty() => {
                            md.title = Some(value)
                        }
                        "creator" if !value.is_empty() => md.authors.push(value),
                        "language" if md.language.is_none() && !value.is_empty() => {
                            md.language = Some(value)
                        }
                        "identifier" => identifiers.push((a.get("id").cloned(), value)),
                        "meta" => {
                            let prop = a.get("property").cloned().unwrap_or_default();
                            if prop == "rendition:layout" && value == "pre-paginated" {
                                md.layout = "fixed".into();
                            } else if prop.starts_with("schema:access") {
                                md.a11y
                                    .push((prop.trim_start_matches("schema:").to_string(), value));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_package {
        return Err(Rejection::BadPackage);
    }

    md.package_identifier = identifiers
        .iter()
        .find(|(id, _)| id.is_some() && id == &unique_id_ref)
        .or(identifiers.first())
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty());

    let entries: HashSet<String> = archive.file_names().map(String::from).collect();
    let mut damage = Vec::new();
    let mut spine = Vec::new();
    for id in &spine_ids {
        match manifest.get(id) {
            Some(item) => {
                if entries.contains(&item.href) {
                    spine.push(item.href.clone());
                } else {
                    damage.push(Damage {
                        item_href: item.href.clone(),
                        error_kind: "missing".into(),
                    });
                }
            }
            None => damage.push(Damage {
                item_href: id.clone(),
                error_kind: "not-in-manifest".into(),
            }),
        }
    }
    // Refuse entity declarations in every XML document the WebView will parse.
    for item in manifest.values() {
        let xml = item.media_type.contains("xml") || item.href.ends_with(".ncx");
        if xml && entries.contains(&item.href) {
            if let Err(Rejection::XmlEntities(n)) = read_xml(archive, &item.href) {
                return Err(Rejection::XmlEntities(n));
            }
        }
    }

    let nav = manifest
        .values()
        .find(|i| i.properties.iter().any(|p| p == "nav"));
    let ncx = toc_ncx_id.as_ref().and_then(|id| manifest.get(id));
    let has_toc = nav.is_some_and(|n| entries.contains(&n.href))
        || ncx.is_some_and(|n| entries.contains(&n.href));
    if let Some(n) = nav {
        if let Ok(Some(xml)) = read_xml(archive, &n.href) {
            md.has_page_list = xml.contains("page-list");
        }
    }

    let cover = manifest
        .values()
        .find(|i| i.properties.iter().any(|p| p == "cover-image"))
        .or_else(|| cover_meta_id.as_ref().and_then(|id| manifest.get(id)))
        .filter(|i| i.media_type.starts_with("image/") && entries.contains(&i.href))
        .map(|i| i.href.clone());

    let first_heading = spine.first().and_then(|first| {
        let xml = read_xml(archive, first).ok().flatten()?;
        first_heading_text(&xml)
    });

    Ok(Package {
        opf_path,
        metadata: md,
        spine,
        cover,
        first_heading,
        damage,
        has_toc,
    })
}

/// An `EncryptedData` whose algorithm is not one of the two font-obfuscation
/// algorithms means the book is DRM-protected (B9, detection only).
fn drm_encrypted_item(xml: &str) -> Option<String> {
    const OBFUSCATION: [&str; 2] = [
        "http://www.idpf.org/2008/embedding",
        "http://ns.adobe.com/pdf/enc#RC",
    ];
    let mut reader = Reader::from_str(xml);
    let mut algorithm: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let a = attrs(&e);
                match local_name(e.name().as_ref()).as_str() {
                    "EncryptionMethod" => algorithm = a.get("Algorithm").cloned(),
                    "CipherReference" => {
                        let alg = algorithm.take().unwrap_or_default();
                        if !OBFUSCATION.contains(&alg.as_str()) {
                            return Some(a.get("URI").cloned().unwrap_or_default());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn first_heading_text(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let n = local_name(e.name().as_ref());
                if depth > 0 || matches!(n.as_str(), "h1" | "h2" | "h3") {
                    depth += 1;
                }
            }
            Ok(Event::End(_)) if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !t.is_empty() {
                        return Some(t);
                    }
                }
            }
            Ok(Event::Text(t)) if depth > 0 => text.push_str(&t.xml10_content()),
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// Open and validate a book file. Used by import; the WebView never sees a book
/// that fails here.
pub fn open(path: &Path) -> Result<(zip::ZipArchive<std::fs::File>, Package), Rejection> {
    let file = std::fs::File::open(path).map_err(|_| Rejection::NotZip)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| Rejection::NotZip)?;
    validate_archive(&mut archive)?;
    let package = read_package(&mut archive)?;
    Ok((archive, package))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn build(entries: &[(&str, &[u8])]) -> zip::ZipArchive<Cursor<Vec<u8>>> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        zip::ZipArchive::new(w.finish().unwrap()).unwrap()
    }

    #[test]
    fn a_multibyte_character_across_the_64_kb_scan_limit_does_not_panic() {
        // 襟 is three bytes; put it across byte 65536 (a Japanese book crashed the importer).
        let mut text = String::from("<?xml version=\"1.0\"?><html><body><p>");
        while text.len() < 64 * 1024 - 1 {
            text.push('a');
        }
        text.push_str("襟</p></body></html>");
        assert!(!text.is_char_boundary(64 * 1024));
        assert!(check_xml_entities("c.xhtml", &text).is_ok());
    }

    const CONTAINER: &[u8] = br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

    fn opf(meta: &str, manifest: &str, spine: &str) -> Vec<u8> {
        format!(r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata>{meta}</metadata><manifest>{manifest}</manifest><spine{spine}</spine></package>"#).into_bytes()
    }

    #[test]
    fn safe_paths() {
        assert!(is_safe_path("OEBPS/text/ch1.xhtml"));
        for bad in ["/etc/passwd", "../x", "a/../../b", "C:/x", "a\\b", ""] {
            assert!(!is_safe_path(bad), "{bad}");
        }
    }

    #[test]
    fn rejects_unsafe_entries() {
        let mut a = build(&[
            ("mimetype", b"application/epub+zip"),
            ("../../escaped.txt", b"x"),
        ]);
        assert_eq!(
            validate_archive(&mut a),
            Err(Rejection::UnsafePath("../../escaped.txt".into()))
        );
    }

    #[test]
    fn rejects_compression_bombs() {
        let zeros = vec![0u8; 4 << 20];
        let mut a = build(&[("OEBPS/bomb.bin", &zeros)]);
        assert_eq!(
            validate_archive(&mut a),
            Err(Rejection::CompressionRatio("OEBPS/bomb.bin".into()))
        );
    }

    #[test]
    fn small_compressible_entries_are_fine() {
        let spaces = vec![b' '; 32 * 1024];
        let mut a = build(&[("OEBPS/style.css", &spaces)]);
        assert_eq!(validate_archive(&mut a), Ok(()));
    }

    #[test]
    fn entity_declarations_are_refused() {
        assert!(check_xml_entities("x", "<!DOCTYPE html><html/>").is_ok());
        assert!(check_xml_entities(
            "x",
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd"><html/>"#
        )
        .is_ok());
        assert!(check_xml_entities("x", r#"<!DOCTYPE p [<!ENTITY a "aaaa">]><p>&a;</p>"#).is_err());
        assert!(check_xml_entities(
            "x",
            r#"<!DOCTYPE p [<!ENTITY x SYSTEM "file:///etc/hosts">]><p/>"#
        )
        .is_err());
    }

    #[test]
    fn resolves_hrefs() {
        assert_eq!(
            resolve("OEBPS/content.opf", "text/ch%201.xhtml#x"),
            "OEBPS/text/ch 1.xhtml"
        );
        assert_eq!(
            resolve("OEBPS/text/a.xhtml", "../img/c.jpg"),
            "OEBPS/img/c.jpg"
        );
        assert_eq!(resolve("content.opf", "a.xhtml"), "a.xhtml");
        assert_eq!(resolve("OEBPS/c.opf", "é%"), "OEBPS/é%");
    }

    #[test]
    fn reads_metadata_spine_cover_and_damage() {
        let package = opf(
            r#"<dc:identifier id="uid">urn:isbn:1</dc:identifier><dc:identifier>other</dc:identifier><dc:title>Moby-Dick</dc:title><dc:creator>Herman Melville</dc:creator><dc:language>en</dc:language><meta property="rendition:layout">pre-paginated</meta><meta property="schema:accessMode">textual</meta>"#,
            r#"<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/><item id="cov" href="img/cover.jpg" media-type="image/jpeg" properties="cover-image"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
            r#" page-progression-direction="rtl"><itemref idref="c1"/><itemref idref="c2"/><itemref idref="ghost"/>"#,
        );
        let mut a = build(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &package),
            (
                "OEBPS/c1.xhtml",
                b"<html><body><h1>Loomings</h1></body></html>",
            ),
            ("OEBPS/img/cover.jpg", b"jpeg"),
            ("OEBPS/nav.xhtml", b"<nav epub:type=\"page-list\"/>"),
        ]);
        let p = read_package(&mut a).unwrap();
        assert_eq!(p.metadata.title.as_deref(), Some("Moby-Dick"));
        assert_eq!(p.metadata.authors, vec!["Herman Melville"]);
        assert_eq!(p.metadata.package_identifier.as_deref(), Some("urn:isbn:1"));
        assert_eq!(p.metadata.page_direction, "rtl");
        assert_eq!(p.metadata.layout, "fixed");
        assert!(p.metadata.has_page_list);
        assert_eq!(
            p.metadata.a11y,
            vec![("accessMode".to_string(), "textual".to_string())]
        );
        assert_eq!(p.spine, vec!["OEBPS/c1.xhtml"]);
        assert_eq!(p.cover.as_deref(), Some("OEBPS/img/cover.jpg"));
        assert_eq!(p.first_heading.as_deref(), Some("Loomings"));
        assert!(p.has_toc);
        assert_eq!(
            p.damage,
            vec![
                Damage {
                    item_href: "OEBPS/c2.xhtml".into(),
                    error_kind: "missing".into()
                },
                Damage {
                    item_href: "ghost".into(),
                    error_kind: "not-in-manifest".into()
                },
            ]
        );
    }

    #[test]
    fn detects_drm_but_not_font_obfuscation() {
        let obf = br#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/><enc:CipherData><enc:CipherReference URI="OEBPS/font.otf"/></enc:CipherData></enc:EncryptedData></encryption>"#;
        let drm = br#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/><enc:CipherData><enc:CipherReference URI="OEBPS/c1.xhtml"/></enc:CipherData></enc:EncryptedData></encryption>"#;
        let base = opf("<dc:title>T</dc:title>", "", "><itemref idref=\"x\"/>");
        let mut ok = build(&[
            ("META-INF/encryption.xml", obf),
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &base),
        ]);
        assert!(read_package(&mut ok).is_ok());
        let mut locked = build(&[
            ("META-INF/encryption.xml", drm),
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &base),
        ]);
        assert_eq!(
            read_package(&mut locked),
            Err(Rejection::Drm("OEBPS/c1.xhtml".into()))
        );
    }

    #[test]
    fn refuses_entities_in_the_opf() {
        let evil =
            br#"<?xml version="1.0"?><!DOCTYPE package [<!ENTITY a "aaaaaaaaaa">]><package/>"#;
        let mut a = build(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", evil),
        ]);
        assert_eq!(
            read_package(&mut a),
            Err(Rejection::XmlEntities("OEBPS/content.opf".into()))
        );
    }
}
