//! Installed extensions (P5): inspect and install from a `.linenext` file (D4,
//! provisional), enable, disable, remove, crash tracking for the watchdog (P6),
//! private storage capped at 10 MB, and `net.fetch` for declared hosts (§7.2).

use super::manifest::{self, Consent, Manifest};
use crate::store::{now_ms, Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// P3: private storage per extension.
pub const STORAGE_QUOTA: usize = 10 << 20;
/// A package's files, unpacked.
const MAX_PACKAGE: u64 = 20 << 20;
const MAX_MANIFEST: u64 = 64 << 10;
/// net.fetch responses (the body goes to the extension as text).
const MAX_RESPONSE: u64 = 5 << 20;
/// P6: three crashes in ten minutes suspend an extension.
const CRASH_WINDOW_MS: i64 = 10 * 60 * 1000;
const CRASHES_TO_SUSPEND: usize = 3;
/// Built-in extensions ship with the app and cannot be removed, only turned off (G7).
pub const BUILTIN_PREFIX: &str = "app.linen.";
/// Package files the host can serve (see `serve`); nothing else is unpacked.
const PACKAGE_TYPES: &[&str] = &["js", "mjs", "html", "css", "json", "png", "svg", "woff2"];

#[derive(Debug, thiserror::Error)]
pub enum ExtError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct Permission {
    pub permission: String,
    pub consent: Consent,
}

/// What the install sheet shows (G1): the manifest, its permissions with how each
/// is consented to, and for an update, what it newly asks for.
#[derive(Debug, Clone, Serialize)]
pub struct Inspection {
    pub manifest: Manifest,
    pub permissions: Vec<Permission>,
    pub update_from: Option<String>,
    pub new_permissions: Vec<String>,
    pub incompatible: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub enabled: bool,
    pub granted: Vec<String>,
    pub builtin: bool,
    /// P5: why it cannot run on this build.
    pub incompatible: Option<String>,
    /// P6: suspended after repeated crashes, until the reader restarts it.
    pub suspended: bool,
    pub crashes: Vec<i64>,
}

#[derive(Debug, Default, serde::Deserialize, Serialize)]
struct CrashLog {
    #[serde(default)]
    crashes: Vec<i64>,
    #[serde(default)]
    suspended: bool,
    /// Removed with its data kept (P5): hidden, and restored on reinstall.
    #[serde(default)]
    removed: bool,
}

fn crash_log(json: &str) -> CrashLog {
    // Schema v1 stored a plain array of crash times.
    serde_json::from_str::<CrashLog>(json)
        .or_else(|_| {
            serde_json::from_str::<Vec<i64>>(json).map(|crashes| CrashLog {
                crashes,
                ..Default::default()
            })
        })
        .unwrap_or_default()
}

/// Read and check a package: its manifest, and every entry's path and type.
fn read_package(path: &Path) -> Result<(Manifest, zip::ZipArchive<std::fs::File>), ExtError> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|_| ExtError::Invalid("this is not an extension package (.linenext)".into()))?;
    crate::epub::validate_archive(&mut zip)
        .map_err(|e| ExtError::Invalid(format!("the package is unsafe: {e:?}")))?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip
            .by_index_raw(i)
            .map_err(|e| ExtError::Invalid(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        total += entry.size();
        let ext = entry
            .name()
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !PACKAGE_TYPES.contains(&ext.as_str()) {
            return Err(ExtError::Invalid(format!(
                "the package has a file Linen does not load: {}",
                entry.name()
            )));
        }
    }
    if total > MAX_PACKAGE {
        return Err(ExtError::Invalid("the package is larger than 20 MB".into()));
    }
    let mut json = String::new();
    zip.by_name("manifest.json")
        .map_err(|_| ExtError::Invalid("the package has no manifest.json".into()))?
        .take(MAX_MANIFEST)
        .read_to_string(&mut json)
        .map_err(|_| ExtError::Invalid("manifest.json is not text".into()))?;
    let m = manifest::parse(&json).map_err(|e| ExtError::Invalid(e.join("; ")))?;
    if m.id.starts_with(BUILTIN_PREFIX) {
        return Err(ExtError::Refused(
            "this id belongs to a built-in extension".into(),
        ));
    }
    if let Some(main) = &m.main {
        if zip.by_name(main).is_err() {
            return Err(ExtError::Invalid(format!("the package has no {main}")));
        }
    }
    for tab in &m.contributes.navigator_tabs {
        if zip.by_name(&tab.page).is_err() {
            return Err(ExtError::Invalid(format!(
                "the package has no {}",
                tab.page
            )));
        }
    }
    Ok((m, zip))
}

fn row(store: &Store, id: &str) -> Result<Option<(String, bool, Vec<String>, String)>, ExtError> {
    Ok(store
        .conn()
        .query_row(
            "SELECT version, enabled, granted_permissions, crash_log FROM extensions WHERE id = ?1",
            [id],
            |r| {
                let granted: String = r.get(2)?;
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)? != 0,
                    serde_json::from_str(&granted).unwrap_or_default(),
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?)
}

pub fn inspect(store: &Store, path: &Path) -> Result<Inspection, ExtError> {
    let (m, _) = read_package(path)?;
    let existing = row(store, &m.id)?.filter(|r| !crash_log(&r.3).removed);
    let granted: BTreeSet<String> = existing
        .as_ref()
        .map(|r| r.2.iter().cloned().collect())
        .unwrap_or_default();
    Ok(Inspection {
        permissions: m
            .permissions
            .iter()
            .map(|p| Permission {
                permission: p.clone(),
                consent: manifest::permission_consent(p).unwrap(),
            })
            .collect(),
        new_permissions: if existing.is_some() {
            m.permissions
                .iter()
                .filter(|p| !granted.contains(*p))
                .cloned()
                .collect()
        } else {
            vec![]
        },
        update_from: existing.map(|r| r.0),
        incompatible: manifest::incompatibility(&m),
        manifest: m,
    })
}

/// Unpack into `<extensions>/<id>`, replacing an older version only once the new one is out.
fn unpack(zip: &mut zip::ZipArchive<std::fs::File>, dir: &Path, id: &str) -> Result<(), ExtError> {
    let staging = dir.join(format!("{id}.installing"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| ExtError::Invalid(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let dest = staging.join(entry.name());
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut (&mut entry).take(MAX_PACKAGE), &mut out)?;
    }
    let target = dir.join(id);
    let old = dir.join(format!("{id}.old"));
    let _ = std::fs::remove_dir_all(&old);
    if target.exists() {
        std::fs::rename(&target, &old)?;
    }
    std::fs::rename(&staging, &target)?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Install after consent (G1): `granted` must be exactly what the manifest asks for.
pub fn install(
    store: &mut Store,
    dir: &Path,
    path: &Path,
    granted: &[String],
) -> Result<Manifest, ExtError> {
    let (m, mut zip) = read_package(path)?;
    if let Some(why) = manifest::incompatibility(&m) {
        return Err(ExtError::Refused(format!(
            "“{}” can't be installed: {why}",
            m.name
        )));
    }
    let asked: BTreeSet<&String> = m.permissions.iter().collect();
    let given: BTreeSet<&String> = granted.iter().collect();
    if asked != given {
        return Err(ExtError::Refused(
            "the permissions agreed to are not the ones the extension asks for".into(),
        ));
    }
    unpack(&mut zip, dir, &m.id)?;
    store.conn().execute(
        "INSERT INTO extensions (id, version, enabled, granted_permissions, installed_at, crash_log)
         VALUES (?1, ?2, 1, ?3, ?4, '{}')
         ON CONFLICT(id) DO UPDATE SET version = excluded.version, enabled = 1,
           granted_permissions = excluded.granted_permissions, crash_log = '{}'",
        params![m.id, m.version, serde_json::to_string(granted).unwrap(), now_ms()],
    )?;
    Ok(m)
}

fn read_manifest(dir: &Path, id: &str) -> Option<Manifest> {
    let json = std::fs::read_to_string(dir.join(id).join("manifest.json")).ok()?;
    manifest::parse(&json).ok()
}

pub fn list(store: &Store, dir: &Path) -> Result<Vec<Installed>, ExtError> {
    let mut stmt = store.conn().prepare(
        "SELECT id, enabled, granted_permissions, crash_log FROM extensions ORDER BY installed_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)? != 0,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = vec![];
    for r in rows {
        let (id, enabled, granted, log) = r?;
        let log = crash_log(&log);
        if log.removed {
            continue;
        }
        let Some(m) = read_manifest(dir, &id) else {
            continue;
        };
        out.push(Installed {
            incompatible: manifest::incompatibility(&m),
            builtin: id.starts_with(BUILTIN_PREFIX),
            manifest: m,
            enabled,
            granted: serde_json::from_str(&granted).unwrap_or_default(),
            suspended: log.suspended,
            crashes: log.crashes,
        });
    }
    Ok(out)
}

pub fn set_enabled(store: &Store, id: &str, enabled: bool) -> Result<(), ExtError> {
    store.conn().execute(
        "UPDATE extensions SET enabled = ?2 WHERE id = ?1",
        params![id, enabled as i64],
    )?;
    Ok(())
}

/// P5: remove, deleting its data only when the reader says so. Built-ins are only turned off.
pub fn remove(store: &Store, dir: &Path, id: &str, delete_data: bool) -> Result<(), ExtError> {
    if id.starts_with(BUILTIN_PREFIX) {
        return Err(ExtError::Refused(
            "built-in extensions can be turned off, not removed".into(),
        ));
    }
    if !super::valid_id(id) {
        return Err(ExtError::Invalid(format!("bad id {id}")));
    }
    let _ = std::fs::remove_dir_all(dir.join(id));
    if delete_data {
        store
            .conn()
            .execute("DELETE FROM extensions WHERE id = ?1", [id])?;
    } else {
        let log = CrashLog {
            removed: true,
            ..Default::default()
        };
        store.conn().execute(
            "UPDATE extensions SET enabled = 0, crash_log = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(&log).unwrap()],
        )?;
    }
    Ok(())
}

/// P6: record a crash (or a timeout); returns whether the extension is now suspended.
pub fn crashed(store: &Store, id: &str) -> Result<bool, ExtError> {
    let Some((_, _, _, log)) = row(store, id)? else {
        return Ok(false);
    };
    let mut log = crash_log(&log);
    let now = now_ms();
    log.crashes.retain(|&t| now - t < CRASH_WINDOW_MS);
    log.crashes.push(now);
    if log.crashes.len() >= CRASHES_TO_SUSPEND {
        log.suspended = true;
    }
    store.conn().execute(
        "UPDATE extensions SET crash_log = ?2 WHERE id = ?1",
        params![id, serde_json::to_string(&log).unwrap()],
    )?;
    Ok(log.suspended)
}

/// “Restart” in the slot's line or in Settings: the crash count starts again.
pub fn restart(store: &Store, id: &str) -> Result<(), ExtError> {
    store
        .conn()
        .execute("UPDATE extensions SET crash_log = '{}' WHERE id = ?1", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- storage (P3: 10 MB, private)

pub fn storage_get(store: &Store, id: &str, key: &str) -> Result<Option<Vec<u8>>, ExtError> {
    Ok(store
        .conn()
        .query_row(
            "SELECT value FROM extension_storage WHERE ext_id = ?1 AND key = ?2",
            [id, key],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn storage_keys(store: &Store, id: &str) -> Result<Vec<String>, ExtError> {
    let mut stmt = store
        .conn()
        .prepare("SELECT key FROM extension_storage WHERE ext_id = ?1 ORDER BY key")?;
    let keys = stmt
        .query_map([id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(keys)
}

pub fn storage_set(store: &mut Store, id: &str, key: &str, value: &[u8]) -> Result<(), ExtError> {
    if key.is_empty() || key.len() > 256 {
        return Err(ExtError::Invalid(
            "storage keys are 1–256 characters".into(),
        ));
    }
    let tx = store.conn_mut().transaction()?;
    let used: i64 = tx.query_row(
        "SELECT COALESCE(SUM(LENGTH(key) + LENGTH(value)), 0) FROM extension_storage WHERE ext_id = ?1 AND key != ?2",
        [id, key],
        |r| r.get(0),
    )?;
    if used as usize + key.len() + value.len() > STORAGE_QUOTA {
        return Err(ExtError::Refused(
            "the extension's storage is full (10 MB)".into(),
        ));
    }
    tx.execute(
        "INSERT INTO extension_storage (ext_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(ext_id, key) DO UPDATE SET value = excluded.value",
        params![id, key, value],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn storage_delete(store: &Store, id: &str, key: &str) -> Result<(), ExtError> {
    store.conn().execute(
        "DELETE FROM extension_storage WHERE ext_id = ?1 AND key = ?2",
        [id, key],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- net.fetch (§7.2)

#[derive(Debug, Serialize)]
pub struct FetchResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
}

/// Fetch for an extension, to hosts it declared and was granted only. Redirects
/// are not followed (a redirect could lead anywhere); the extension sees the 3xx.
pub fn net_fetch(
    granted: &[String],
    url: &str,
    method: &str,
    body: Option<&str>,
) -> Result<FetchResponse, ExtError> {
    if !manifest::network_allows(granted, url) {
        return Err(ExtError::Refused(format!(
            "not allowed to connect to {url}"
        )));
    }
    if method != "GET" && method != "POST" {
        return Err(ExtError::Invalid("net.fetch supports GET and POST".into()));
    }
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Linen")
        .build();
    let request = agent.request(method, url);
    let result = match body {
        Some(b) => request.send_string(b),
        None => request.call(),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(ExtError::Refused(format!("the request failed: {e}"))),
    };
    let status = response.status();
    let content_type = response.header("content-type").map(String::from);
    let mut text = String::new();
    response
        .into_reader()
        .take(MAX_RESPONSE)
        .read_to_string(&mut text)
        .map_err(|_| ExtError::Refused("the response is not text".into()))?;
    Ok(FetchResponse {
        status,
        content_type,
        body: text,
    })
}

// ---------------------------------------------------------------- built-in extensions

pub struct Builtin {
    pub id: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

/// Keep the built-in extensions installed at the shipped version. A reader who
/// turned one off keeps it off.
pub fn ensure_builtins(store: &Store, dir: &Path, builtins: &[Builtin]) -> Result<(), ExtError> {
    for b in builtins {
        let (_, json) = b
            .files
            .iter()
            .find(|(n, _)| *n == "manifest.json")
            .expect("built-in manifest");
        let m = manifest::parse(json).map_err(|e| ExtError::Invalid(e.join("; ")))?;
        let target: PathBuf = dir.join(b.id);
        let _ = std::fs::remove_dir_all(&target);
        for (name, contents) in b.files {
            let path = target.join(name);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, contents)?;
        }
        store.conn().execute(
            "INSERT INTO extensions (id, version, enabled, granted_permissions, installed_at, crash_log)
             VALUES (?1, ?2, 1, ?3, 0, '{}')
             ON CONFLICT(id) DO UPDATE SET version = excluded.version,
               granted_permissions = excluded.granted_permissions",
            params![m.id, m.version, serde_json::to_string(&m.permissions).unwrap()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn package(dir: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let path = dir.join(name);
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (n, c) in files {
            w.start_file(*n, SimpleFileOptions::default()).unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        path
    }

    fn manifest(version: &str, perms: &str) -> String {
        format!(
            r#"{{ "id": "org.example.dictionary", "version": "{version}", "name": "Dictionary",
            "engines": {{ "linen": "^1.0" }}, "main": "main.js",
            "activation": ["onCommand:define"],
            "contributes": {{ "commands": [{{ "id": "define", "title": "Define" }}] }},
            "permissions": [{perms}] }}"#
        )
    }

    #[test]
    fn install_update_with_new_permissions_and_remove_keeping_data() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Extensions");
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let v1 = package(
            tmp.path(),
            "v1.linenext",
            &[
                ("manifest.json", &manifest("1.0.0", r#""book.selection""#)),
                ("main.js", "1"),
            ],
        );
        let i = inspect(&store, &v1).unwrap();
        assert_eq!(i.update_from, None);
        assert_eq!(i.permissions[0].permission, "book.selection");
        assert!(
            install(&mut store, &dir, &v1, &[]).is_err(),
            "consent must cover every permission"
        );
        install(&mut store, &dir, &v1, &["book.selection".into()]).unwrap();
        storage_set(&mut store, "org.example.dictionary", "k", b"v").unwrap();

        let v2 = package(
            tmp.path(),
            "v2.linenext",
            &[
                (
                    "manifest.json",
                    &manifest(
                        "1.1.0",
                        r#""book.selection", "network:api.dictionaryapi.dev""#,
                    ),
                ),
                ("main.js", "2"),
            ],
        );
        let i = inspect(&store, &v2).unwrap();
        assert_eq!(i.update_from.as_deref(), Some("1.0.0"));
        assert_eq!(i.new_permissions, vec!["network:api.dictionaryapi.dev"]);
        install(
            &mut store,
            &dir,
            &v2,
            &[
                "book.selection".into(),
                "network:api.dictionaryapi.dev".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("org.example.dictionary/main.js")).unwrap(),
            "2"
        );

        remove(&store, &dir, "org.example.dictionary", false).unwrap();
        assert!(list(&store, &dir).unwrap().is_empty());
        assert_eq!(
            storage_get(&store, "org.example.dictionary", "k")
                .unwrap()
                .as_deref(),
            Some(&b"v"[..]),
            "data kept"
        );
        install(
            &mut store,
            &dir,
            &v2,
            &[
                "book.selection".into(),
                "network:api.dictionaryapi.dev".into(),
            ],
        )
        .unwrap();
        assert_eq!(list(&store, &dir).unwrap().len(), 1);
        remove(&store, &dir, "org.example.dictionary", true).unwrap();
        assert_eq!(
            storage_get(&store, "org.example.dictionary", "k").unwrap(),
            None,
            "data deleted"
        );
    }

    #[test]
    fn packages_are_checked_before_anything_is_unpacked() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let exe = package(
            tmp.path(),
            "a.linenext",
            &[
                ("manifest.json", &manifest("1.0.0", "")),
                ("main.js", "1"),
                ("x.dylib", ""),
            ],
        );
        assert!(inspect(&store, &exe)
            .unwrap_err()
            .to_string()
            .contains("x.dylib"));
        let nomain = package(
            tmp.path(),
            "b.linenext",
            &[("manifest.json", &manifest("1.0.0", ""))],
        );
        assert!(inspect(&store, &nomain)
            .unwrap_err()
            .to_string()
            .contains("no main.js"));
        let builtin = package(
            tmp.path(),
            "c.linenext",
            &[
                (
                    "manifest.json",
                    &manifest("1.0.0", "").replace("org.example.dictionary", "app.linen.fake"),
                ),
                ("main.js", ""),
            ],
        );
        assert!(inspect(&store, &builtin).is_err());
        let future = package(
            tmp.path(),
            "d.linenext",
            &[
                (
                    "manifest.json",
                    &manifest("1.0.0", "").replace("^1.0", "^2.0"),
                ),
                ("main.js", ""),
            ],
        );
        assert!(inspect(&store, &future).unwrap().incompatible.is_some());
    }

    #[test]
    fn storage_is_private_and_capped_at_10_mb() {
        let mut store = Store::open_in_memory().unwrap();
        for id in ["a.one", "b.two"] {
            store
                .conn()
                .execute(
                    "INSERT INTO extensions (id, version, installed_at) VALUES (?1, '1.0.0', 0)",
                    [id],
                )
                .unwrap();
        }
        let chunk = vec![7u8; 4 << 20];
        storage_set(&mut store, "a.one", "x", &chunk).unwrap();
        storage_set(&mut store, "a.one", "y", &chunk).unwrap();
        assert!(storage_set(&mut store, "a.one", "z", &chunk)
            .unwrap_err()
            .to_string()
            .contains("full"));
        storage_set(&mut store, "a.one", "x", b"small").unwrap(); // replacing frees space
        storage_set(&mut store, "b.two", "x", &chunk).unwrap(); // the quota is per extension
        assert_eq!(storage_get(&store, "b.two", "y").unwrap(), None);
        assert_eq!(storage_keys(&store, "a.one").unwrap(), vec!["x", "y"]);
    }

    #[test]
    fn three_crashes_in_ten_minutes_suspend() {
        let store = Store::open_in_memory().unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO extensions (id, version, installed_at) VALUES ('a.one', '1.0.0', 0)",
                [],
            )
            .unwrap();
        assert!(!crashed(&store, "a.one").unwrap());
        assert!(!crashed(&store, "a.one").unwrap());
        assert!(crashed(&store, "a.one").unwrap());
        restart(&store, "a.one").unwrap();
        assert!(!crashed(&store, "a.one").unwrap());
    }

    #[test]
    fn net_fetch_refuses_undeclared_hosts_before_connecting() {
        let granted = vec!["network:api.dictionaryapi.dev".to_string()];
        assert!(net_fetch(&granted, "http://127.0.0.1:9/x", "GET", None)
            .unwrap_err()
            .to_string()
            .contains("not allowed"));
        assert!(net_fetch(&[], "https://api.dictionaryapi.dev/", "GET", None).is_err());
    }
}
