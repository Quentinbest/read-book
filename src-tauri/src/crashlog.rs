//! D1 (provisional, pending item 26): no telemetry. Crashes and uncaught errors are
//! written to a log on this Mac only (~/Library/Logs/app.linen.reader/crash.log);
//! Settings › About reveals it, for the reader to attach to an email if they choose.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;

/// The log grows to this size, then starts again (the previous one kept as crash.old.log).
const MAX_BYTES: u64 = 1 << 20;
/// One uncaught error from the page is cut to this many bytes.
const MAX_NOTE: usize = 4096;

pub struct CrashLog(pub PathBuf);

impl CrashLog {
    pub fn file(&self) -> PathBuf {
        self.0.join("crash.log")
    }
}

/// Where the log lives. Test and spike runs with a throwaway data folder keep it there.
pub fn dir<R: tauri::Runtime>(app: &tauri::App<R>) -> PathBuf {
    use tauri::Manager;
    #[cfg(any(debug_assertions, feature = "spikes"))]
    if let Some(data) = std::env::var_os("LINEN_DATA_DIR") {
        return PathBuf::from(data).join("Logs");
    }
    app.path()
        .app_log_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("Linen"))
}

/// Append one entry, rotating a full log first. Errors are ignored: logging never fails the app.
pub fn append(dir: &Path, kind: &str, body: &str) {
    let _ = std::fs::create_dir_all(dir);
    let file = dir.join("crash.log");
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&file, dir.join("crash.old.log"));
    }
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    else {
        return;
    };
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(
        f,
        "--- {} · {kind} · Linen {} · macOS {}\n{}\n",
        utc(secs),
        env!("CARGO_PKG_VERSION"),
        macos_version(),
        body.trim_end()
    );
}

/// Record panics (with a backtrace) before the default hook runs.
pub fn install_panic_hook(dir: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let body = format!(
            "thread '{}' {info}\n{}",
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        append(&dir, "panic", &body);
        default(info);
    }));
}

/// An uncaught error or rejection in a window (sent by the page).
#[tauri::command]
pub fn crash_log_note(log: State<CrashLog>, message: String) {
    let mut end = message.len().min(MAX_NOTE);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    append(&log.0, "uncaught error", &message[..end]);
}

/// Whether there is anything to show.
#[tauri::command]
pub fn crash_log_exists(log: State<CrashLog>) -> bool {
    log.file().is_file()
}

/// Reveal the log in Finder.
#[tauri::command]
pub fn crash_log_show(log: State<CrashLog>) -> Result<(), String> {
    let file = log.file();
    if !file.is_file() {
        return Err("there is no crash log".into());
    }
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&file)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn macos_version() -> String {
    std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "?".into())
}

/// `YYYY-MM-DD hh:mm:ss UTC` from Unix seconds (civil-from-days, no date crate).
fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(1_790_386_056), "2026-09-26 01:27:36 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 UTC");
    }

    #[test]
    fn entries_append_and_a_full_log_starts_again() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), "panic", "first");
        append(dir.path(), "uncaught error", "second");
        let text = std::fs::read_to_string(dir.path().join("crash.log")).unwrap();
        assert!(text.contains("· panic ·") && text.contains("first"));
        assert!(text.contains("second"));
        std::fs::write(
            dir.path().join("crash.log"),
            vec![b'x'; (MAX_BYTES + 1) as usize],
        )
        .unwrap();
        append(dir.path(), "panic", "third");
        let text = std::fs::read_to_string(dir.path().join("crash.log")).unwrap();
        assert!(text.contains("third") && !text.contains("xxx"));
        assert!(dir.path().join("crash.old.log").is_file());
    }
}
