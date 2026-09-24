//! Phase 0 spike harness support (docs/spikes/). Compiled only with `--features spikes`.
//!
//! Run with `LINEN_SPIKE=<list>` set: the main window loads `spikes.html?run=<list>`,
//! the page runs the checks, reports raw results through `spike_report`, and exits.
//! A canary HTTP server on 127.0.0.1:8765 and the `spike_canary` command record any
//! request or IPC call that hostile book content manages to make (Spike E).

use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::{ipc::Response, AppHandle, Manager, Runtime};

pub const CANARY_ADDR: &str = "127.0.0.1:8765";

#[derive(Default, Serialize, Clone)]
pub struct CanaryLog {
    ipc: Vec<String>,
    http: Vec<String>,
}

pub struct SpikeState(Mutex<CanaryLog>);

fn repo_root() -> PathBuf {
    std::env::var_os("LINEN_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[tauri::command]
pub fn spike_read_corpus(name: String) -> Result<Response, String> {
    if !valid_name(&name) {
        return Err(format!("invalid corpus name: {name}"));
    }
    for dir in ["corpus/cache", "corpus/generated"] {
        let path = repo_root().join(dir).join(&name);
        if path.is_file() {
            return std::fs::read(&path)
                .map(Response::new)
                .map_err(|e| e.to_string());
        }
    }
    Err(format!("not in corpus: {name}"))
}

#[tauri::command]
pub fn spike_canary(state: tauri::State<SpikeState>, id: String, channel: Option<String>) {
    state
        .0
        .lock()
        .unwrap()
        .ipc
        .push(format!("{id} via {}", channel.unwrap_or_default()));
}

#[tauri::command]
pub fn spike_canary_log(state: tauri::State<SpikeState>) -> CanaryLog {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
pub fn spike_canary_clear(state: tauri::State<SpikeState>) {
    *state.0.lock().unwrap() = CanaryLog::default();
}

#[tauri::command]
pub fn spike_report(name: String, json: String) -> Result<String, String> {
    if !valid_name(&name) {
        return Err(format!("invalid report name: {name}"));
    }
    let dir = repo_root().join("docs/spikes/raw");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn spike_info() -> serde_json::Value {
    let plist = "/System/Library/Frameworks/WebKit.framework/Versions/A/Resources/Info.plist";
    let read = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    serde_json::json!({
        "os": read("sw_vers", &["-productVersion"]),
        "webkit": read("plutil", &["-extract", "CFBundleVersion", "raw", plist]),
        "cpu": read("sysctl", &["-n", "machdep.cpu.brand_string"]),
        "memory_bytes": read("sysctl", &["-n", "hw.memsize"]),
        "time": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
        // Run only the end-to-end checks whose id matches this pattern (for debugging).
        "only": std::env::var("LINEN_E2E_ONLY").ok(),
    })
}

#[tauri::command]
pub fn spike_log(line: String) {
    eprintln!("{line}");
}

#[tauri::command]
pub fn spike_exit<R: Runtime>(app: AppHandle<R>, code: i32) {
    app.exit(code);
}

fn start_canary_server<R: Runtime>(app: AppHandle<R>) {
    let listener = match TcpListener::bind(CANARY_ADDR) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("spikes: canary server failed to bind {CANARY_ADDR}: {e}");
            return;
        }
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            let mut reader = BufReader::new(&stream);
            let _ = reader.read_line(&mut line);
            let line = line.trim().to_string();
            if !line.is_empty() {
                app.state::<SpikeState>().0.lock().unwrap().http.push(line);
            }
            let mut s = &stream;
            let _ = s.write_all(
                b"HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
}

pub fn install<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.manage(SpikeState(Mutex::new(CanaryLog::default())))
}

/// Absolute path of a corpus file, for importing it through the product commands.
#[tauri::command]
pub fn spike_corpus_path(name: String) -> Result<String, String> {
    if !valid_name(&name) {
        return Err(format!("invalid corpus name: {name}"));
    }
    for dir in ["corpus/cache", "corpus/generated"] {
        let path = repo_root().join(dir).join(&name);
        if path.is_file() {
            return Ok(path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .display()
                .to_string());
        }
    }
    Err(format!("not in corpus: {name}"))
}

pub fn setup<R: Runtime>(app: &tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let Ok(run) = std::env::var("LINEN_SPIKE") else {
        return Ok(());
    };
    start_canary_server(app.handle().clone());
    let window = app
        .get_webview_window("main")
        .ok_or("main window missing")?;
    let url = window.url()?.join(&format!("spikes.html?run={run}"))?;
    window.navigate(url)?;

    // Automation safety net: never leave a hung harness running.
    let timeout: u64 = std::env::var("LINEN_SPIKE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1200);
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(timeout));
        eprintln!("spikes: timed out after {timeout} s");
        handle.exit(3);
    });
    Ok(())
}

/// `ps` elapsed time `[[dd-]hh:]mm:ss` in seconds.
fn etime_seconds(s: &str) -> Option<u64> {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, s),
    };
    let parts: Vec<u64> = rest
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let secs = parts.iter().fold(0, |acc, p| acc * 60 + p);
    Some(days * 86_400 + secs)
}

/// §6.4 memory budget: resident memory of this app and the WebKit helper
/// processes it started (WebContent, Networking, GPU run as separate XPC
/// processes, not children, so they are matched by name and start time).
#[tauri::command]
pub fn spike_memory() -> Result<serde_json::Value, String> {
    let me = std::process::id();
    let out = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,etime=,rss=,comm="])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let rows: Vec<(u32, u64, u64, String)> = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let elapsed = etime_seconds(it.next()?)?;
            let rss = it.next()?.parse().ok()?;
            let comm = it.collect::<Vec<_>>().join(" ");
            Some((pid, elapsed, rss, comm))
        })
        .collect();
    let my_age = rows
        .iter()
        .find(|r| r.0 == me)
        .map(|r| r.1)
        .ok_or("own process not found")?;
    let mut total_kb = 0u64;
    let mut parts = Vec::new();
    for (pid, age, rss, comm) in &rows {
        let ours = *pid == me || (comm.contains("com.apple.WebKit") && *age <= my_age);
        if ours {
            total_kb += rss;
            parts.push(format!(
                "{} {} MB",
                comm.rsplit('/').next().unwrap_or(comm),
                rss / 1024
            ));
        }
    }
    Ok(serde_json::json!({ "total_mb": total_kb / 1024, "processes": parts }))
}

/// Capture this app's own window (and nothing else on screen) to
/// docs/visual/app/<name>.png, for the visual baselines (plan §6.1).
#[tauri::command]
pub fn spike_capture<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    name: String,
) -> Result<String, String> {
    if !valid_name(&name) {
        return Err(format!("invalid name: {name}"));
    }
    let ptr = window.ns_window().map_err(|e| e.to_string())? as usize;
    // SAFETY: Tauri hands out the window's own NSWindow; windowNumber is a plain getter.
    let number = unsafe { (*(ptr as *const objc2_app_kit::NSWindow)).windowNumber() };
    let dir = repo_root().join("docs/visual/app");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}.png"));
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{number}")])
        .arg(&path)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("screencapture failed: {status}"));
    }
    Ok(path.display().to_string())
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventCreateScrollWheelEvent(
        source: *const std::ffi::c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        ...
    ) -> *mut std::ffi::c_void;
    fn CGEventSetLocation(event: *mut std::ffi::c_void, point: CGPoint);
    fn CGEventPostToPid(pid: i32, event: *mut std::ffi::c_void);
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// Post real scroll-wheel events to this app only (not the rest of the system), at a
/// point in the window, so end-to-end tests exercise AppKit and WebKit scrolling.
/// `pixels`: continuous (trackpad-like) deltas in points; otherwise wheel lines.
#[tauri::command]
pub fn spike_scroll_wheel<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    x: f64,
    y: f64,
    delta: i32,
    count: u32,
    pixels: bool,
) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let origin = window.inner_position().map_err(|e| e.to_string())?;
    let point = CGPoint {
        x: origin.x as f64 / scale + x,
        y: origin.y as f64 / scale + y,
    };
    let pid = std::process::id() as i32;
    std::thread::spawn(move || {
        for _ in 0..count {
            // SAFETY: CoreGraphics creates the event; it is posted to this process and released.
            unsafe {
                let event =
                    CGEventCreateScrollWheelEvent(std::ptr::null(), u32::from(!pixels), 1, delta);
                if event.is_null() {
                    return;
                }
                CGEventSetLocation(event, point);
                CGEventPostToPid(pid, event);
                CFRelease(event);
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
    Ok(())
}
