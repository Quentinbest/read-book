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
/// Item 32: the last version announced as “available” to an install that cannot update itself.
const ANNOUNCED: &str = "updateAnnounced";
/// Where “Download” goes: the release page of that version, on the update source (item 29).
const RELEASES: &str = "https://github.com/Quentinbest/read-book/releases/tag/v";
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
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let now = unix_now();
            let last = setting(&app).and_then(|s| s.parse().ok());
            if due(last, now) {
                match check_and_install(&app).await {
                    Ok(Checked::Installed) => {
                        set_setting(&app, &now.to_string());
                        // The update waits for Restart or the next launch. This version
                        // checks no more: each check would find, and install, it again.
                        return;
                    }
                    Ok(Checked::UpToDate) => set_setting(&app, &now.to_string()),
                    Ok(Checked::CannotInstall(version)) => {
                        set_setting(&app, &now.to_string());
                        // Item 32: say so once per version, with a link to download it.
                        let announced = setting_of(&app, ANNOUNCED);
                        if should_announce(announced.as_deref(), &version) {
                            let _ = app.emit("update-available", &version);
                            set_setting_of(&app, ANNOUNCED, &version);
                        }
                    }
                    // Offline, or a server error: tried again at the next tick, not in a day.
                    Err(e) => log::warn!("update check: {e}"),
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

enum Checked {
    UpToDate,
    Installed,
    /// An update exists but this account cannot replace the app (see `bundle_writable`).
    CannotInstall(String),
}

async fn check_and_install<R: Runtime>(app: &AppHandle<R>) -> Result<Checked, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(Checked::UpToDate);
    };
    let version = update.version.clone();
    if !bundle_writable() {
        log::info!(
            "update {version} available; the app bundle is not writable, so it is not installed"
        );
        return Ok(Checked::CannotInstall(version));
    }
    // Verified against the public key in tauri.conf.json before it is installed.
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("update-ready", version);
    Ok(Checked::Installed)
}

/// D6 “reading is never interrupted”: the updater replaces the app bundle, and
/// where this account cannot (a standard account, an app an administrator put in
/// /Applications, an app run from a read-only disk image) it falls back to asking
/// for an administrator's password. That dialog must not appear mid-book, so the
/// install is only tried when the bundle and its folder are writable.
#[cfg(target_os = "macos")]
fn bundle_writable() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    // …/Linen.app/Contents/MacOS/linen
    let Some(bundle) = exe.ancestors().nth(3) else {
        return false;
    };
    if bundle.extension() != Some(std::ffi::OsStr::new("app")) {
        return false;
    }
    bundle.parent().is_some_and(writable) && writable(bundle)
}

#[cfg(not(target_os = "macos"))]
fn bundle_writable() -> bool {
    true
}

#[cfg(target_os = "macos")]
fn writable(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// “Restart” on the update line: the new version starts; the reader saves first (N5).
#[tauri::command]
pub fn app_restart<R: Runtime>(app: AppHandle<R>) {
    crate::commands::request_quit(&app, true);
}

/// Item 32: whether to announce `version`, given the version announced last.
pub fn should_announce(announced: Option<&str>, version: &str) -> bool {
    announced != Some(version)
}

/// Item 32: “Download” on the update-available line. The core builds the address, so a
/// page can only ask for a release page on the update source.
#[tauri::command]
pub fn open_release_page(version: String) -> Result<(), String> {
    let ok = !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if !ok {
        return Err("not a version".into());
    }
    crate::native::open_external(format!("{RELEASES}{version}"))
}

fn setting<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    setting_of(app, SETTING)
}

fn set_setting<R: Runtime>(app: &AppHandle<R>, value: &str) {
    set_setting_of(app, SETTING, value)
}

fn setting_of<R: Runtime>(app: &AppHandle<R>, key: &str) -> Option<String> {
    let state = app.try_state::<AppState>()?;
    let store = state.store.lock().ok()?;
    store.setting(key).ok().flatten()
}

fn set_setting_of<R: Runtime>(app: &AppHandle<R>, key: &str, value: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut store) = state.store.lock() {
            let _ = store.set_setting(key, value);
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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

    #[test]
    fn an_available_update_is_announced_once_per_version() {
        assert!(should_announce(None, "0.2.0"));
        assert!(!should_announce(Some("0.2.0"), "0.2.0"));
        assert!(should_announce(Some("0.2.0"), "0.3.0"));
    }

    #[test]
    fn the_release_page_takes_only_a_version() {
        assert!(open_release_page("".into()).is_err());
        assert!(open_release_page("1.0/../../evil".into()).is_err());
        assert!(open_release_page("1.0?x=y".into()).is_err());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn only_a_writable_folder_counts_as_writable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        assert!(writable(dir.path()));
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(!writable(&locked));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!writable(&dir.path().join("missing")));
    }
}
