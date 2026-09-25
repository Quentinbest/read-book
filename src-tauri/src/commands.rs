//! Typed IPC commands for the frontend (plan Phase 1 “Store: typed IPC”).
//!
//! Errors are returned as `CommandError` values the frontend can act on; a
//! failed write is never swallowed (E5).

use crate::import::{import_book, ImportOutcome, Library};
use crate::store::{BookRow, Store, StoreError};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub struct AppState {
    pub store: Mutex<Store>,
    pub library: Library,
    /// L17: the open book's validated archive, read entry by entry (one book open, B7).
    pub open_book: Mutex<Option<(String, zip::ZipArchive<std::fs::File>)>>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandError {
    /// A write did not reach the disk. The frontend keeps the write and offers Retry (E5).
    SaveFailed { message: String },
    /// Anything else, with a message for the log.
    Failed { message: String },
}

impl From<StoreError> for CommandError {
    fn from(e: StoreError) -> Self {
        match &e {
            StoreError::Sqlite(rusqlite::Error::SqliteFailure(f, _))
                if matches!(
                    f.code,
                    rusqlite::ErrorCode::DiskFull
                        | rusqlite::ErrorCode::ReadOnly
                        | rusqlite::ErrorCode::SystemIoFailure
                        | rusqlite::ErrorCode::CannotOpen
                        | rusqlite::ErrorCode::DatabaseBusy
                        | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                CommandError::SaveFailed {
                    message: e.to_string(),
                }
            }
            _ => CommandError::Failed {
                message: e.to_string(),
            },
        }
    }
}

impl From<crate::import::ImportError> for CommandError {
    fn from(e: crate::import::ImportError) -> Self {
        match e {
            crate::import::ImportError::Store(s) => s.into(),
            crate::import::ImportError::Sqlite(s) => StoreError::Sqlite(s).into(),
            crate::import::ImportError::Io(io) => CommandError::SaveFailed {
                message: io.to_string(),
            },
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

/// Open the store and library under the app data folder
/// (~/Library/Application Support/app.linen.reader, B3).
pub fn init<R: Runtime>(app: &tauri::App<R>) -> Result<AppState, Box<dyn std::error::Error>> {
    // Tests and spikes can point the app at a throwaway data folder.
    #[cfg(any(debug_assertions, feature = "spikes"))]
    let root = match std::env::var_os("LINEN_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => app.path().app_data_dir()?,
    };
    #[cfg(not(any(debug_assertions, feature = "spikes")))]
    let root = app.path().app_data_dir()?;
    std::fs::create_dir_all(&root)?;
    let store = Store::open(&root.join("linen.db"))?;
    let library = Library::new(&root)?;
    Ok(AppState {
        store: Mutex::new(store),
        library,
        open_book: Mutex::new(None),
    })
}

#[tauri::command]
pub fn library_list(state: State<AppState>) -> CmdResult<Vec<BookRow>> {
    Ok(state.store.lock().unwrap().books()?)
}

#[derive(Clone, serde::Serialize)]
pub struct ImportResult {
    pub path: String,
    pub outcome: ImportOutcome,
}

pub fn import_paths(state: &AppState, paths: &[PathBuf]) -> CmdResult<Vec<ImportResult>> {
    let mut results = Vec::new();
    {
        let mut store = state.store.lock().unwrap();
        for path in paths {
            let outcome = import_book(&mut store, &state.library, path)?;
            results.push(ImportResult {
                path: path.to_string_lossy().into_owned(),
                outcome,
            });
        }
    }
    // B3: a replaced book is read from its new file from now on, not from the archive
    // that was open. (Locked after the store: `with_open_book` takes them the other way.)
    let mut open = state.open_book.lock().unwrap();
    let replaced = |id: &str| {
        results.iter().any(
            |r| matches!(&r.outcome, ImportOutcome::Replaced { book_id, .. } if book_id == id),
        )
    };
    if open.as_ref().is_some_and(|(id, _)| replaced(id)) {
        *open = None;
    }
    Ok(results)
}

#[tauri::command]
pub fn library_import(state: State<AppState>, paths: Vec<String>) -> CmdResult<Vec<ImportResult>> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    import_paths(&state, &paths)
}

/// The book file inside the library, or an error for anything outside it.
fn library_book_path(state: &AppState, book_id: &str) -> CmdResult<PathBuf> {
    let failed = |e: String| CommandError::Failed { message: e };
    let path: String = state
        .store
        .lock()
        .unwrap()
        .conn()
        .query_row(
            "SELECT file_path FROM books WHERE id = ?1",
            [book_id],
            |r| r.get(0),
        )
        .map_err(|e| failed(e.to_string()))?;
    let books_dir = state
        .library
        .books_dir
        .canonicalize()
        .map_err(|e| failed(e.to_string()))?;
    let resolved = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| failed(e.to_string()))?;
    if !resolved.starts_with(&books_dir) {
        return Err(failed("book file is outside the library".into()));
    }
    Ok(resolved)
}

/// The whole book file (spikes and tests; the reader reads entries on demand, L17).
#[tauri::command]
pub fn book_bytes(state: State<AppState>, book_id: String) -> CmdResult<tauri::ipc::Response> {
    let path = library_book_path(&state, &book_id)?;
    std::fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })
}

/// Entries larger than this are never sent to the WebView (a single chapter or image).
const MAX_ENTRY_BYTES: u64 = 256 << 20;

fn with_open_book<T>(
    state: &AppState,
    book_id: &str,
    f: impl FnOnce(&mut zip::ZipArchive<std::fs::File>) -> CmdResult<T>,
) -> CmdResult<T> {
    let mut open = state.open_book.lock().unwrap();
    if open.as_ref().map(|(id, _)| id.as_str()) != Some(book_id) {
        let path = library_book_path(state, book_id)?;
        let file = std::fs::File::open(&path).map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })?;
        // The file was validated at import; check again in case it changed on disk.
        crate::epub::validate_archive(&mut archive).map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })?;
        *open = Some((book_id.to_string(), archive));
    }
    f(&mut open.as_mut().unwrap().1)
}

/// L17: the book's entries and their sizes, so the reader can load them one by one.
#[tauri::command]
pub fn book_entries(state: State<AppState>, book_id: String) -> CmdResult<Vec<(String, u64)>> {
    with_open_book(&state, &book_id, |archive| {
        let mut out = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i).map_err(|e| CommandError::Failed {
                message: e.to_string(),
            })?;
            if entry.is_file() {
                out.push((entry.name().to_string(), entry.size()));
            }
        }
        Ok(out)
    })
}

/// L17: one entry of the open book, read from the zip on demand.
#[tauri::command]
pub fn book_entry(
    state: State<AppState>,
    book_id: String,
    name: String,
) -> CmdResult<tauri::ipc::Response> {
    with_open_book(&state, &book_id, |archive| {
        read_entry(archive, &name).map(tauri::ipc::Response::new)
    })
}

/// One entry's bytes, capped at MAX_ENTRY_BYTES.
fn read_entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> CmdResult<Vec<u8>> {
    use std::io::Read;
    let failed = |e: String| CommandError::Failed { message: e };
    let entry = archive
        .by_name(name)
        .map_err(|e| failed(format!("{name}: {e}")))?;
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(failed(format!("{name} is too large")));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .take(MAX_ENTRY_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| failed(e.to_string()))?;
    Ok(bytes)
}

/// §6.4 memory: the book's images, audio and video are served to the WebView from
/// the zip through this URL scheme (`linen-book://localhost/<book id>/<entry>`),
/// so they never pass through JavaScript as blobs. Only media entries of the
/// book that is open now are served: never documents, styles or scripts, which
/// go through the content sanitiser (D-E1), and never another book.
pub const BOOK_SCHEME: &str = "linen-book";

/// The media type of a servable entry, from its extension; `None` refuses it.
pub fn media_type(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        _ => return None,
    })
}

/// Decode a %-encoded URL path segment; `None` for malformed input.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Split `/<book id>/<entry path>` into its parts.
pub fn parse_book_path(path: &str) -> Option<(String, String)> {
    let (id, entry) = path.strip_prefix('/')?.split_once('/')?;
    let entry = entry
        .split('/')
        .map(percent_decode)
        .collect::<Option<Vec<_>>>()?
        .join("/");
    Some((percent_decode(id)?, entry))
}

/// Serve one request on BOOK_SCHEME.
pub fn serve_book_media(
    state: &AppState,
    path: &str,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    use tauri::http::{Response, StatusCode};
    let reply = |status: StatusCode| {
        Response::builder()
            .status(status)
            .body(std::borrow::Cow::Borrowed(&[][..]))
            .unwrap()
    };
    let Some((book_id, entry)) = parse_book_path(path) else {
        return reply(StatusCode::BAD_REQUEST);
    };
    let Some(mime) = media_type(&entry) else {
        return reply(StatusCode::FORBIDDEN);
    };
    let mut open = state.open_book.lock().unwrap();
    let Some((open_id, archive)) = open.as_mut() else {
        return reply(StatusCode::NOT_FOUND);
    };
    if *open_id != book_id {
        return reply(StatusCode::NOT_FOUND);
    }
    match read_entry(archive, &entry) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", mime)
            .header("X-Content-Type-Options", "nosniff")
            .header("Content-Security-Policy", "sandbox; default-src 'none'")
            .body(std::borrow::Cow::Owned(bytes))
            .unwrap(),
        Err(_) => reply(StatusCode::NOT_FOUND),
    }
}

/// F8: the book's extracted chapter text for search, if it was saved for this
/// version of the file (a replaced file has a new content hash).
#[tauri::command]
pub fn search_text_get(state: State<AppState>, book_id: String) -> CmdResult<Vec<(u32, String)>> {
    let failed = |e: rusqlite::Error| CommandError::Failed {
        message: e.to_string(),
    };
    let store = state.store.lock().unwrap();
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT s.section, s.text FROM search_text s JOIN books b ON b.id = s.book_id
             WHERE s.book_id = ?1 AND s.content_hash = b.content_hash ORDER BY s.section",
        )
        .map_err(failed)?;
    let rows = stmt
        .query_map([&book_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(failed)?
        .collect::<Result<Vec<(u32, String)>, _>>()
        .map_err(failed)?;
    Ok(rows)
}

/// F8: save extracted chapter text for search, stamped with the file's content hash.
#[tauri::command]
pub fn search_text_put(
    state: State<AppState>,
    book_id: String,
    chapters: Vec<(u32, String)>,
) -> CmdResult<()> {
    let failed = |e: rusqlite::Error| CommandError::Failed {
        message: e.to_string(),
    };
    let mut store = state.store.lock().unwrap();
    let tx = store.conn_mut().transaction().map_err(failed)?;
    let hash: String = tx
        .query_row(
            "SELECT content_hash FROM books WHERE id = ?1",
            [&book_id],
            |r| r.get(0),
        )
        .map_err(failed)?;
    // Text from an older version of the file goes.
    tx.execute(
        "DELETE FROM search_text WHERE book_id = ?1 AND content_hash != ?2",
        rusqlite::params![book_id, hash],
    )
    .map_err(failed)?;
    for (section, text) in &chapters {
        tx.execute(
            "INSERT OR REPLACE INTO search_text (book_id, content_hash, section, text) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![book_id, hash, section, text],
        )
        .map_err(failed)?;
    }
    tx.commit().map_err(failed)
}

/// A9: the book's annotations.
#[tauri::command]
pub fn annotations_list(
    state: State<AppState>,
    book_id: String,
) -> CmdResult<Vec<crate::store::AnnotationRow>> {
    Ok(state.store.lock().unwrap().annotations(&book_id)?)
}

#[tauri::command]
pub fn annotation_save(
    state: State<AppState>,
    annotation: crate::store::AnnotationRow,
) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().save_annotation(&annotation)?)
}

/// A7: delete at once; Undo restores it.
#[tauri::command]
pub fn annotation_delete(state: State<AppState>, id: String) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().delete_annotation(&id)?)
}

#[tauri::command]
pub fn annotation_restore(state: State<AppState>, id: String) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().restore_annotation(&id)?)
}

/// E3, N6: the book's damaged spine items (zip paths), recorded at import.
#[tauri::command]
pub fn book_damage(state: State<AppState>, book_id: String) -> CmdResult<Vec<String>> {
    let failed = |e: rusqlite::Error| CommandError::Failed {
        message: e.to_string(),
    };
    let store = state.store.lock().unwrap();
    let mut stmt = store
        .conn()
        .prepare("SELECT item_href FROM book_damage WHERE book_id = ?1 ORDER BY item_href")
        .map_err(failed)?;
    let rows = stmt
        .query_map([&book_id], |r| r.get(0))
        .map_err(failed)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(failed)?;
    Ok(rows)
}

/// Per-book settings (S13): layout mode and the docked Navigator tab.
#[tauri::command]
pub fn book_settings_get(
    state: State<AppState>,
    book_id: String,
) -> CmdResult<Option<(String, Option<String>)>> {
    use rusqlite::OptionalExtension;
    Ok(state
        .store
        .lock()
        .unwrap()
        .conn()
        .query_row(
            "SELECT layout_mode, navigator_docked FROM book_settings WHERE book_id = ?1",
            [&book_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(StoreError::from)?)
}

#[tauri::command]
pub fn book_settings_set(
    state: State<AppState>,
    book_id: String,
    layout_mode: String,
    navigator_docked: Option<String>,
) -> CmdResult<()> {
    state
        .store
        .lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO book_settings (book_id, layout_mode, navigator_docked) VALUES (?1, ?2, ?3)
             ON CONFLICT(book_id) DO UPDATE SET layout_mode = excluded.layout_mode,
             navigator_docked = excluded.navigator_docked",
            rusqlite::params![book_id, layout_mode, navigator_docked],
        )
        .map_err(StoreError::from)?;
    Ok(())
}

#[tauri::command]
pub fn position_save(
    state: State<AppState>,
    book_id: String,
    cfi: String,
    fraction: f64,
) -> CmdResult<()> {
    Ok(state
        .store
        .lock()
        .unwrap()
        .save_position(&book_id, &cfi, fraction)?)
}

#[tauri::command]
pub fn position_get(state: State<AppState>, book_id: String) -> CmdResult<Option<(String, f64)>> {
    Ok(state.store.lock().unwrap().position(&book_id)?)
}

#[tauri::command]
pub fn setting_get(state: State<AppState>, key: String) -> CmdResult<Option<String>> {
    Ok(state.store.lock().unwrap().setting(&key)?)
}

#[tauri::command]
pub fn setting_set(state: State<AppState>, key: String, value: String) -> CmdResult<()> {
    Ok(state.store.lock().unwrap().set_setting(&key, &value)?)
}

/// Files opened before the store existed, and results the UI has not collected yet.
/// macOS delivers launch documents before `setup` runs, so opens are buffered.
#[derive(Default)]
pub struct PendingOpens {
    urls: Mutex<Vec<tauri::Url>>,
    results: Mutex<Vec<ImportResult>>,
}

/// Files opened from the OS (Finder “Open With”, Dock drop, double-click): import
/// them and tell the frontend, which opens the book and skips the library (E8).
pub fn handle_opened<R: Runtime>(app: &AppHandle<R>, urls: Vec<tauri::Url>) {
    let Some(state) = app.try_state::<AppState>() else {
        if let Some(pending) = app.try_state::<PendingOpens>() {
            pending.urls.lock().unwrap().extend(urls);
        }
        return;
    };
    let paths: Vec<PathBuf> = urls.iter().filter_map(|u| u.to_file_path().ok()).collect();
    log::info!("opened from the OS: {} file(s)", paths.len());
    if paths.is_empty() {
        return;
    }
    match import_paths(&state, &paths) {
        Ok(results) => {
            if let Some(pending) = app.try_state::<PendingOpens>() {
                pending.results.lock().unwrap().extend(results.clone());
            }
            let _ = app.emit("os-opened", results);
        }
        Err(e) => {
            let _ = app.emit("os-open-failed", e);
        }
    }
}

/// Import files that arrived before the store was ready (call at the end of setup).
pub fn flush_pending_opens<R: Runtime>(app: &AppHandle<R>) {
    let urls = match app.try_state::<PendingOpens>() {
        Some(p) => std::mem::take(&mut *p.urls.lock().unwrap()),
        None => return,
    };
    if !urls.is_empty() {
        handle_opened(app, urls);
    }
}

/// Results of OS opens the UI has not shown yet (for opens during launch).
#[tauri::command]
pub fn opened_take(pending: State<PendingOpens>) -> Vec<ImportResult> {
    std::mem::take(&mut *pending.results.lock().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// E5: a full disk surfaces as SaveFailed, and the same write succeeds once space returns.
    #[test]
    fn disk_full_is_a_save_failure_and_retry_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("linen.db")).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO books (id, content_hash, file_path, title, title_source, added_at)
                 VALUES ('b1', 'h', '/x', 'T', 'package', 0)",
                [],
            )
            .unwrap();
        let pages: i64 = store
            .conn()
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        // Simulate a full disk: no room for another page.
        store
            .conn()
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        let big = "x".repeat(64 * 1024);
        let err = store.set_setting("big", &big).unwrap_err();
        assert!(matches!(
            CommandError::from(err),
            CommandError::SaveFailed { .. }
        ));
        // Space returns: the retried write succeeds and nothing was lost.
        store
            .conn()
            .pragma_update(None, "max_page_count", 1_073_741_823)
            .unwrap();
        store.set_setting("big", &big).unwrap();
        assert_eq!(
            store.setting("big").unwrap().map(|v| v.len()),
            Some(big.len())
        );
    }
}

/// N5: on quit the reader saves its position before the app exits. The core asks
/// the frontend (`app-quitting`), which saves and calls `quit_ready`; a fallback
/// timer quits anyway if no answer comes.
#[derive(Default)]
pub struct QuitState(pub std::sync::atomic::AtomicBool);

#[tauri::command]
pub fn quit_ready<R: Runtime>(app: AppHandle<R>) {
    if let Some(q) = app.try_state::<QuitState>() {
        q.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    app.exit(0);
}

/// Handle an exit request: hold it once so the frontend can save. Returns true
/// when the exit should be prevented now.
pub fn hold_exit_for_save<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(q) = app.try_state::<QuitState>() else {
        return false;
    };
    if q.0.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return false; // already saved (or timed out): let it exit
    }
    let _ = app.emit("app-quitting", ());
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        handle.exit(0);
    });
    true
}

#[cfg(test)]
mod book_scheme_tests {
    use super::{media_type, parse_book_path};

    #[test]
    fn serves_only_media_types() {
        assert_eq!(media_type("OEBPS/plate-3.PNG"), Some("image/png"));
        assert_eq!(media_type("a/b.jpeg"), Some("image/jpeg"));
        assert_eq!(media_type("audio/x.mp3"), Some("audio/mpeg"));
        // Documents, styles, scripts, SVG and fonts always go through the sanitiser or decoder.
        for name in [
            "c.xhtml", "c.html", "s.css", "x.js", "i.svg", "f.otf", "f.woff2", "noext",
        ] {
            assert_eq!(media_type(name), None, "{name}");
        }
    }

    #[test]
    fn parses_book_and_entry() {
        assert_eq!(
            parse_book_path("/abc123/OEBPS/images/a%20b.png"),
            Some(("abc123".into(), "OEBPS/images/a b.png".into()))
        );
        assert_eq!(parse_book_path("/abc123"), None);
        assert_eq!(parse_book_path("/abc/%zz.png"), None);
    }
}
