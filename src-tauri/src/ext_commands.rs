//! Tauri commands for extensions (Phase 7). The extension host in the app's own
//! page calls these on an extension's behalf, after its own permission check;
//! extension code itself can reach none of them (Spike G). The core checks again
//! where it matters: `net.fetch` reads the granted hosts from the store, storage
//! is keyed by the extension id and capped.

use crate::commands::{AppState, CommandError};
use crate::extensions::manifest::Manifest;
use crate::extensions::registry::{self, ExtError, FetchResponse, Inspection, Installed};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, Runtime, State};

type CmdResult<T> = Result<T, CommandError>;

impl From<ExtError> for CommandError {
    fn from(e: ExtError) -> Self {
        CommandError::Failed {
            message: e.to_string(),
        }
    }
}

/// P7: safe mode for this launch (⇧ held at launch, or “Restart without extensions”).
pub struct SafeMode(pub AtomicBool);

/// The built-in extensions shipped with the app (G7: listed as “Built-in”).
pub const BUILTINS: &[registry::Builtin] = &[registry::Builtin {
    id: "app.linen.markdown-export",
    files: &[
        (
            "manifest.json",
            include_str!("extensions/builtin/markdown-export/manifest.json"),
        ),
        (
            "main.js",
            include_str!("extensions/builtin/markdown-export/main.js"),
        ),
    ],
}];

#[tauri::command]
pub fn extensions_list(state: State<AppState>) -> CmdResult<Vec<Installed>> {
    Ok(registry::list(
        &state.store.lock().unwrap(),
        &state.library.extensions_dir,
    )?)
}

#[tauri::command]
pub fn extension_inspect(state: State<AppState>, path: String) -> CmdResult<Inspection> {
    Ok(registry::inspect(
        &state.store.lock().unwrap(),
        &PathBuf::from(path),
    )?)
}

#[tauri::command]
pub fn extension_install(
    state: State<AppState>,
    path: String,
    granted: Vec<String>,
) -> CmdResult<Manifest> {
    let mut store = state.store.lock().unwrap();
    Ok(registry::install(
        &mut store,
        &state.library.extensions_dir,
        &PathBuf::from(path),
        &granted,
    )?)
}

#[tauri::command]
pub fn extension_set_enabled(state: State<AppState>, id: String, enabled: bool) -> CmdResult<()> {
    Ok(registry::set_enabled(
        &state.store.lock().unwrap(),
        &id,
        enabled,
    )?)
}

#[tauri::command]
pub fn extension_remove(state: State<AppState>, id: String, delete_data: bool) -> CmdResult<()> {
    Ok(registry::remove(
        &state.store.lock().unwrap(),
        &state.library.extensions_dir,
        &id,
        delete_data,
    )?)
}

/// P6: the host reports a crash or a timeout; true when the extension is now suspended.
#[tauri::command]
pub fn extension_crashed(state: State<AppState>, id: String) -> CmdResult<bool> {
    Ok(registry::crashed(&state.store.lock().unwrap(), &id)?)
}

#[tauri::command]
pub fn extension_restart(state: State<AppState>, id: String) -> CmdResult<()> {
    Ok(registry::restart(&state.store.lock().unwrap(), &id)?)
}

#[tauri::command]
pub fn extension_storage_get(
    state: State<AppState>,
    id: String,
    key: String,
) -> CmdResult<Option<String>> {
    Ok(
        registry::storage_get(&state.store.lock().unwrap(), &id, &key)?
            .map(|v| String::from_utf8_lossy(&v).into_owned()),
    )
}

#[tauri::command]
pub fn extension_storage_set(
    state: State<AppState>,
    id: String,
    key: String,
    value: String,
) -> CmdResult<()> {
    Ok(registry::storage_set(
        &mut state.store.lock().unwrap(),
        &id,
        &key,
        value.as_bytes(),
    )?)
}

#[tauri::command]
pub fn extension_storage_delete(state: State<AppState>, id: String, key: String) -> CmdResult<()> {
    Ok(registry::storage_delete(
        &state.store.lock().unwrap(),
        &id,
        &key,
    )?)
}

#[tauri::command]
pub fn extension_storage_keys(state: State<AppState>, id: String) -> CmdResult<Vec<String>> {
    Ok(registry::storage_keys(&state.store.lock().unwrap(), &id)?)
}

/// §7.2: fetch for an extension, only to the hosts it was granted (read here, from the store).
#[tauri::command]
pub async fn extension_net_fetch(
    state: State<'_, AppState>,
    id: String,
    url: String,
    method: String,
    body: Option<String>,
) -> CmdResult<FetchResponse> {
    let granted = registry::list(&state.store.lock().unwrap(), &state.library.extensions_dir)?
        .into_iter()
        .find(|e| e.manifest.id == id && e.enabled && !e.suspended)
        .map(|e| e.granted)
        .ok_or(CommandError::Failed {
            message: format!("{id} is not running"),
        })?;
    tauri::async_runtime::spawn_blocking(move || {
        registry::net_fetch(&granted, &url, &method, body.as_deref())
    })
    .await
    .map_err(|e| CommandError::Failed {
        message: e.to_string(),
    })?
    .map_err(Into::into)
}

#[tauri::command]
pub fn app_safe_mode(safe: State<SafeMode>) -> bool {
    safe.0.load(Ordering::Relaxed)
}

/// P7: “Restart without extensions” (Screen 11): the next launch is in safe mode.
#[tauri::command]
pub fn app_restart_safe<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    let state = app.state::<AppState>();
    state
        .store
        .lock()
        .unwrap()
        .set_setting("safeModeOnce", "1")?;
    app.restart();
}

/// Whether this launch is in safe mode: ⇧ held, LINEN_SAFE_MODE=1, or a restart asked for it.
pub fn launch_in_safe_mode(state: &AppState) -> bool {
    let mut store = state.store.lock().unwrap();
    let once = store.setting("safeModeOnce").ok().flatten().as_deref() == Some("1");
    if once {
        let _ = store.set_setting("safeModeOnce", "0");
    }
    once || std::env::var("LINEN_SAFE_MODE").as_deref() == Ok("1") || crate::native::shift_held()
}
