//! D6 (approved 2026-09-26): Tauri's updater, checked once a day. An update is signed
//! with the Linen update key and served from GitHub Releases. It downloads and installs
//! in the background; the page shows a quiet “Update ready · Restart” line, and
//! otherwise the new version starts at the next launch. Reading is never interrupted.
//!
//! The check runs here, in the core, so no page gets the updater's permissions.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

use crate::commands::AppState;

const SETTING: &str = "updateCheckedAt";
const DAY: u64 = 24 * 60 * 60;
/// How often the loop looks at the clock (a Mac asleep overnight still checks on waking).
const TICK: Duration = Duration::from_secs(60 * 60);

/// Whether a check is due, given the last one (Unix seconds) and now.
pub fn due(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(t) => now.saturating_sub(t) >= DAY || t > now,
    }
}

/// Start the daily check. Development and test builds never update themselves.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    if cfg!(debug_assertions) || cfg!(feature = "spikes") {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Not in the first minute: launch is for opening the book.
        tokio_sleep(Duration::from_secs(60)).await;
        loop {
            let now = unix_now();
            let last = setting(&app).and_then(|s| s.parse().ok());
            if due(last, now) {
                if let Err(e) = check_and_install(&app).await {
                    log::warn!("update check: {e}");
                }
                set_setting(&app, &now.to_string());
            }
            tokio_sleep(TICK).await;
        }
    });
}

async fn check_and_install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let version = update.version.clone();
    // Verified against the public key in tauri.conf.json before it is installed.
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("update-ready", version);
    Ok(())
}

/// “Restart” on the update line: the new version starts; the reader saves first (N5).
#[tauri::command]
pub fn app_restart<R: Runtime>(app: AppHandle<R>) {
    app.request_restart();
}

fn setting<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let state = app.try_state::<AppState>()?;
    let store = state.store.lock().ok()?;
    store.setting(SETTING).ok().flatten()
}

fn set_setting<R: Runtime>(app: &AppHandle<R>, value: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut store) = state.store.lock() {
            let _ = store.set_setting(SETTING, value);
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn tokio_sleep(d: Duration) {
    // tauri's runtime is tokio; a blocking sleep on a spawned blocking task keeps
    // this crate free of a direct tokio dependency.
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_due_once_a_day() {
        assert!(due(None, 1000));
        assert!(!due(Some(1000), 1000 + DAY - 1));
        assert!(due(Some(1000), 1000 + DAY));
        // A clock set back does not stop checks forever.
        assert!(due(Some(5000), 1000));
    }
}
