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
pub fn extension_remove(
    state: State<AppState>,
    vault: State<Vaults>,
    id: String,
    delete_data: bool,
) -> CmdResult<()> {
    // LK6, LK7: an extension's keys go with it, whatever happens to its other data.
    let store = state.store.lock().unwrap();
    for s in registry::secrets_of(&store, &id)? {
        if let Err(e) = vault.0.delete(&id, &s.name) {
            log::warn!("extension {id}: a key could not be deleted: {e}");
        }
        registry::secret_forget(&store, &id, &s.name)?;
    }
    Ok(registry::remove(
        &store,
        &state.library.extensions_dir,
        &id,
        delete_data,
    )?)
}

// ---------------------------------------------------------------- Reading Lens Stage 2d

/// The start of the refusal when a saved key can't be read (src/extensions/host.svelte.ts).
const KEY_UNAVAILABLE: &str = "key-unavailable";

/// Where extension keys live: the Keychain, or memory in a test build's throwaway library.
pub struct Vaults(pub Box<dyn crate::extensions::secrets::Vault>);

impl Vaults {
    pub fn for_app() -> Self {
        #[cfg(feature = "spikes")]
        if std::env::var_os("LINEN_DATA_DIR").is_some() {
            return Vaults(Box::new(MemoryVault::default()));
        }
        Vaults(Box::new(crate::extensions::secrets::Keychain))
    }
}

/// Test builds: keys in memory, so the in-app checks never touch the person's Keychain.
#[cfg(feature = "spikes")]
#[derive(Default)]
pub struct MemoryVault(std::sync::Mutex<std::collections::HashMap<String, String>>);

/// Test builds: the memory vault refuses to read, as a locked Keychain would.
#[cfg(feature = "spikes")]
pub static VAULT_DENIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "spikes")]
impl crate::extensions::secrets::Vault for MemoryVault {
    fn get(&self, ext: &str, name: &str) -> Result<Option<String>, String> {
        if VAULT_DENIED.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("the Keychain is locked (test)".into());
        }
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&format!("{ext}/{name}"))
            .cloned())
    }
    fn set(&self, ext: &str, name: &str, value: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(format!("{ext}/{name}"), value.into());
        Ok(())
    }
    fn delete(&self, ext: &str, name: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(&format!("{ext}/{name}"));
        Ok(())
    }
}

/// LK6: the reader allowed a host in Linen's sheet.
#[tauri::command]
pub fn extension_grant(state: State<AppState>, id: String, host: String) -> CmdResult<()> {
    Ok(registry::grant_optional(
        &state.store.lock().unwrap(),
        &state.library.extensions_dir,
        &id,
        &host,
    )?)
}

/// LK6: Settings › Extensions › Remove beside an allowed host.
#[tauri::command]
pub fn extension_revoke(state: State<AppState>, id: String, host: String) -> CmdResult<()> {
    Ok(registry::revoke_optional(
        &state.store.lock().unwrap(),
        &state.library.extensions_dir,
        &id,
        &host,
    )?)
}

/// LK7: an extension's keys, by name and host (EP4); never their values.
#[tauri::command]
pub fn extension_secrets(
    state: State<AppState>,
    id: String,
) -> CmdResult<Vec<registry::SecretRow>> {
    Ok(registry::secrets_of(&state.store.lock().unwrap(), &id)?)
}

/// LK7: whether a key with this name is saved (the only thing an extension may learn).
#[tauri::command]
pub fn extension_secret_has(
    state: State<AppState>,
    vault: State<Vaults>,
    id: String,
    name: String,
) -> CmdResult<bool> {
    let known = registry::secrets_of(&state.store.lock().unwrap(), &id)?
        .iter()
        .any(|s| s.name == name);
    Ok(known
        && vault
            .0
            .get(&id, &name)
            .map_err(|message| CommandError::Failed { message })?
            .is_some())
}

#[tauri::command]
pub fn extension_secret_delete(
    state: State<AppState>,
    vault: State<Vaults>,
    id: String,
    name: String,
) -> CmdResult<()> {
    vault
        .0
        .delete(&id, &name)
        .map_err(|message| CommandError::Failed { message })?;
    Ok(registry::secret_forget(
        &state.store.lock().unwrap(),
        &id,
        &name,
    )?)
}

/// What the key dialog says (from the page, in the reader's language).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct KeyDialog {
    pub title: String,
    pub message: String,
    pub save: String,
    pub cancel: String,
}

/// LK7: Linen's own native dialog with a secure field. The key goes from it to the
/// Keychain without passing through any web page. True when a key was saved.
#[tauri::command]
pub async fn extension_secret_request(
    app: tauri::AppHandle,
    id: String,
    name: String,
    host: String,
    label: String,
    dialog: KeyDialog,
) -> CmdResult<bool> {
    use tauri::Manager;
    if !crate::extensions::secrets::valid_name(&name) {
        return Err(CommandError::Failed {
            message: format!("“{name}” is not a key name"),
        });
    }
    let state = app.state::<AppState>();
    {
        // The host must be one the reader allowed (LK6) or the manifest required.
        let store = state.store.lock().unwrap();
        let granted = registry::list(&store, &state.library.extensions_dir)?
            .into_iter()
            .find(|e| e.manifest.id == id)
            .map(|e| e.granted)
            .unwrap_or_default();
        if !granted.contains(&format!("network:{host}")) {
            return Err(CommandError::Failed {
                message: format!("{host} is not allowed for this extension"),
            });
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(crate::native::ask_secret(&dialog));
    })
    .map_err(|e| CommandError::Failed {
        message: e.to_string(),
    })?;
    let Some(value) = tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| CommandError::Failed {
            message: e.to_string(),
        })?
    else {
        return Ok(false);
    };
    let vault = app.state::<Vaults>();
    vault
        .0
        .set(&id, &name, &value)
        .map_err(|message| CommandError::Failed { message })?;
    registry::secret_record(&state.store.lock().unwrap(), &id, &name, &host, &label)?;
    Ok(true)
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

/// LK7: which saved key goes into a request, and how.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FetchAuth {
    pub secret: String,
    pub scheme: crate::extensions::secrets::Scheme,
}

/// §7.2: fetch for an extension, only to the hosts it was granted (read here, from the store).
#[tauri::command]
// Each field of the page's request is its own argument (as Tauri passes them).
#[allow(clippy::too_many_arguments)]
pub async fn extension_net_fetch(
    state: State<'_, AppState>,
    vault: State<'_, Vaults>,
    id: String,
    url: String,
    method: String,
    body: Option<String>,
    headers: Option<Vec<(String, String)>>,
    auth: Option<FetchAuth>,
) -> CmdResult<FetchResponse> {
    let granted = registry::list(&state.store.lock().unwrap(), &state.library.extensions_dir)?
        .into_iter()
        .find(|e| e.manifest.id == id && e.enabled && !e.suspended)
        .map(|e| e.granted)
        .ok_or(CommandError::Failed {
            message: format!("{id} is not running"),
        })?;
    // LK7: the key named, from the Keychain, with the host it was saved for.
    let key = match auth {
        None => None,
        Some(a) => {
            let row = registry::secrets_of(&state.store.lock().unwrap(), &id)?
                .into_iter()
                .find(|s| s.name == a.secret)
                .ok_or(CommandError::Failed {
                    message: format!("no key named {}", a.secret),
                })?;
            // LK7-keychain-denied: a locked Keychain, denied access or an item gone
            // (Spike J) is one refusal the host recognises; nothing is sent.
            let value = vault
                .0
                .get(&id, &a.secret)
                .ok()
                .flatten()
                .ok_or(CommandError::Failed {
                    message: format!("{KEY_UNAVAILABLE}: {}", a.secret),
                })?;
            Some(registry::RequestKey {
                scheme: a.scheme,
                host: row.host,
                value,
            })
        }
    };
    let headers = headers.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        registry::net_fetch(&granted, &url, &method, body.as_deref(), &headers, key)
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
    // N5: the reader saves first, as for any quit.
    crate::commands::request_quit(&app, true);
    Ok(())
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
