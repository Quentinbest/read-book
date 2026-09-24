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
    let mut store = state.store.lock().unwrap();
    let mut results = Vec::new();
    for path in paths {
        let outcome = import_book(&mut store, &state.library, path)?;
        results.push(ImportResult {
            path: path.to_string_lossy().into_owned(),
            outcome,
        });
    }
    Ok(results)
}

#[tauri::command]
pub fn library_import(state: State<AppState>, paths: Vec<String>) -> CmdResult<Vec<ImportResult>> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    import_paths(&state, &paths)
}

/// The book file's bytes for the reader. Only files inside the library are served.
#[tauri::command]
pub fn book_bytes(state: State<AppState>, book_id: String) -> CmdResult<tauri::ipc::Response> {
    let path: String = state
        .store
        .lock()
        .unwrap()
        .conn()
        .query_row(
            "SELECT file_path FROM books WHERE id = ?1",
            [&book_id],
            |r| r.get(0),
        )
        .map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })?;
    let path = PathBuf::from(path);
    let books_dir = state
        .library
        .books_dir
        .canonicalize()
        .map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })?;
    let resolved = path.canonicalize().map_err(|e| CommandError::Failed {
        message: e.to_string(),
    })?;
    if !resolved.starts_with(&books_dir) {
        return Err(CommandError::Failed {
            message: "book file is outside the library".into(),
        });
    }
    std::fs::read(&resolved)
        .map(tauri::ipc::Response::new)
        .map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })
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
