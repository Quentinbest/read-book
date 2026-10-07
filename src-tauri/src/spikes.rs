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

/// Stage 4 (docs/i18n-plan.md): `LINEN_LOCALE=<tag>` runs the page in that language;
/// its captures and reports go to i18n-out/visual/<tag>/, away from the baselines.
fn run_locale() -> Option<String> {
    std::env::var("LINEN_LOCALE")
        .ok()
        .filter(|l| l != "en" && valid_name(l))
}

/// Where a run's captures (`docs/visual/app`) or reports (`docs/spikes/raw`) go.
fn output_dir(default: &str) -> PathBuf {
    match run_locale() {
        Some(l) => repo_root().join("i18n-out/visual").join(l),
        None => repo_root().join(default),
    }
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
    let dir = output_dir("docs/spikes/raw");
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

/// i18n spike (docs/spikes/i18n-spike.md): what the main bundle offers macOS,
/// and the language AppKit picks for it.
#[tauri::command]
pub fn spike_bundle_languages() -> serde_json::Value {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSBundle;
        let bundle = NSBundle::mainBundle();
        let list = |a: objc2::rc::Retained<
            objc2_foundation::NSArray<objc2_foundation::NSString>,
        >| { a.iter().map(|s| s.to_string()).collect::<Vec<_>>() };
        serde_json::json!({
            "localizations": list(bundle.localizations()),
            "preferredLocalizations": list(bundle.preferredLocalizations()),
            "developmentLocalization": bundle.developmentLocalization().map(|s| s.to_string()),
            "bundleIdentifier": bundle.bundleIdentifier().map(|s| s.to_string()),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        serde_json::Value::Null
    }
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

/// Spike G: install a probe extension from `src-tauri/tests/fixtures/ext/<id>` into
/// the library's Extensions folder (throwaway data folders only).
#[tauri::command]
pub fn spike_install_ext(
    state: tauri::State<crate::commands::AppState>,
    id: String,
) -> Result<(), String> {
    if !crate::extensions::valid_id(&id) {
        return Err(format!("invalid id {id}"));
    }
    let from = repo_root().join("src-tauri/tests/fixtures/ext").join(&id);
    let to = state.library.extensions_dir.join(&id);
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(&from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Phase 7: install a package without the compatibility check (see registry).
#[tauri::command]
pub fn spike_install_unchecked(
    state: tauri::State<crate::commands::AppState>,
    path: String,
) -> Result<String, String> {
    let mut store = state.store.lock().unwrap();
    crate::extensions::registry::install_unchecked(
        &mut store,
        &state.library.extensions_dir,
        std::path::Path::new(&path),
    )
    .map(|m| m.id)
    .map_err(|e| e.to_string())
}

/// §6.4 cold start: every file in a corpus folder (the 500-book fixture).
#[tauri::command]
pub fn spike_corpus_dir(name: String) -> Result<Vec<String>, String> {
    if !valid_name(&name) {
        return Err(format!("invalid corpus name: {name}"));
    }
    let dir = repo_root().join("corpus/generated").join(&name);
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "epub"))
        .map(|p| p.display().to_string())
        .collect();
    out.sort();
    Ok(out)
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
    let window = app
        .get_webview_window("main")
        .ok_or("main window missing")?;
    // LINEN_SPACE=N moves the harness to desktop N, off the screen being used
    // (optional since the owner's 2026-09-29 instruction; unset: the current desktop).
    // The window starts hidden (tauri.spikes.conf.json), moves, then shows.
    let desktop = std::env::var("LINEN_SPACE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok());
    #[cfg(target_os = "macos")]
    if let Some(n) = desktop {
        match desktops::move_window(&window, n) {
            Ok(space) => eprintln!("spikes: window moved to desktop {n} (space {space})"),
            Err(e) => {
                eprintln!("spikes: could not move the window to desktop {n}: {e}");
                return Err(e.into());
            }
        }
    }
    let _ = desktop;
    window.show()?;
    let Ok(run) = std::env::var("LINEN_SPIKE") else {
        return Ok(());
    };
    start_canary_server(app.handle().clone());
    let locale = run_locale()
        .map(|l| format!("&locale={l}"))
        .unwrap_or_default();
    let url = window
        .url()?
        .join(&format!("spikes.html?run={run}{locale}"))?;
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
    let mut footprint_total = 0u64;
    let mut parts = Vec::new();
    let mut footprints = Vec::new();
    for (pid, age, rss, comm) in &rows {
        let ours = *pid == me || (comm.contains("com.apple.WebKit") && *age <= my_age);
        if ours {
            let name = comm.rsplit('/').next().unwrap_or(comm);
            total_kb += rss;
            parts.push(format!("{name} {} MB", rss / 1024));
            let fp = phys_footprint(*pid).unwrap_or(0);
            footprint_total += fp;
            footprints.push(format!("{name} {} MB", fp >> 20));
        }
    }
    Ok(serde_json::json!({
        // §6.4 names resident memory (RSS). Under memory pressure macOS compresses
        // pages out of RSS, so the physical footprint (what Activity Monitor calls
        // Memory; it includes compressed pages) is reported alongside.
        "total_mb": total_kb / 1024,
        "processes": parts,
        "footprint_mb": footprint_total >> 20,
        "footprints": footprints,
    }))
}

/// A process's physical footprint in bytes (same user only).
fn phys_footprint(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: proc_pid_rusage fills a rusage_info_v2 of the size the flavor names.
    let ok = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr() as *mut libc::rusage_info_t,
        )
    };
    // SAFETY: zero-initialised and, on success, filled by the kernel.
    (ok == 0).then(|| unsafe { info.assume_init() }.ri_phys_footprint)
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
    let dir = output_dir("docs/visual/app");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}.png"));
    // The window server's own capture of this one window: it works wherever the
    // window is, including another desktop (LINEN_SPACE), where screencapture fails.
    // It can refuse once in a while (the window between frames): try a few times.
    let mut result = capture::window_png(number as u32, &path);
    for _ in 0..5 {
        if result.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        result = capture::window_png(number as u32, &path);
    }
    result?;
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
    fn CGEventCreateMouseEvent(
        source: *const std::ffi::c_void,
        kind: u32,
        point: CGPoint,
        button: u32,
    ) -> *mut std::ffi::c_void;
    fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
    fn CGEventGetLocation(event: *mut std::ffi::c_void) -> CGPoint;
    fn CGEventSetLocation(event: *mut std::ffi::c_void, point: CGPoint);
    fn CGEventPostToPid(pid: i32, event: *mut std::ffi::c_void);
    fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// kCGHIDEventTap: events enter the system where a real mouse's do.
const HID_TAP: u32 = 0;

/// A point in the window (CSS px, origin top-left) on the screen.
fn screen_point<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    x: f64,
    y: f64,
) -> Result<CGPoint, String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let origin = window.inner_position().map_err(|e| e.to_string())?;
    Ok(CGPoint {
        x: origin.x as f64 / scale + x,
        y: origin.y as f64 / scale + y,
    })
}

/// Post a mouse event of `kind` (a CGEventType) at `point`, through the HID tap.
fn post_mouse(kind: u32, point: CGPoint) {
    // SAFETY: CoreGraphics creates the event; it is posted and released.
    unsafe {
        let event = CGEventCreateMouseEvent(std::ptr::null(), kind, point, 0);
        if !event.is_null() {
            CGEventPost(HID_TAP, event);
            CFRelease(event);
        }
    }
}

/// Post real scroll-wheel events at a point in the window, so end-to-end tests exercise
/// AppKit and WebKit scrolling. `pixels`: continuous (trackpad-like) deltas in points;
/// otherwise wheel lines. By default they go to this app only (not the rest of the
/// system); AppKit's monitor sees them, but WebKit scrolls only what is under the real
/// cursor. `hid`: move the cursor there and post them as a real wheel would arrive.
#[tauri::command]
pub fn spike_scroll_wheel<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    x: f64,
    y: f64,
    delta: i32,
    count: u32,
    pixels: bool,
    hid: Option<bool>,
) -> Result<(), String> {
    let point = screen_point(&window, x, y)?;
    let hid = hid.unwrap_or(false);
    if hid {
        post_mouse(MOUSE_MOVED, point);
    }
    let pid = std::process::id() as i32;
    std::thread::spawn(move || {
        for _ in 0..count {
            // SAFETY: CoreGraphics creates the event; it is posted and released.
            unsafe {
                let event =
                    CGEventCreateScrollWheelEvent(std::ptr::null(), u32::from(!pixels), 1, delta);
                if event.is_null() {
                    return;
                }
                CGEventSetLocation(event, point);
                if hid {
                    CGEventPost(HID_TAP, event);
                } else {
                    CGEventPostToPid(pid, event);
                }
                CFRelease(event);
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
    Ok(())
}

/// kCGEventMouseMoved, kCGEventLeftMouseDown, kCGEventLeftMouseUp
const MOUSE_MOVED: u32 = 5;
const MOUSE_DOWN: u32 = 1;
const MOUSE_UP: u32 = 2;

/// A real mouse move, or a left click, at a point in the window (CSS px, origin
/// top-left). Posted through the HID tap, so the system cursor moves and WebKit's own
/// hit testing sends the pointer to the page or a book's frame. (Moves posted to the
/// process alone never reach WebKit: AppKit tracks the real cursor.)
#[tauri::command]
pub fn spike_mouse<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    x: f64,
    y: f64,
    click: bool,
) -> Result<(), String> {
    let point = screen_point(&window, x, y)?;
    post_mouse(MOUSE_MOVED, point);
    if click {
        std::thread::sleep(Duration::from_millis(30));
        post_mouse(MOUSE_DOWN, point);
        std::thread::sleep(Duration::from_millis(60));
        post_mouse(MOUSE_UP, point);
    }
    Ok(())
}

/// A real left-button drag from one point in the window to another (CSS px, origin
/// top-left), in steps, through the HID tap: what a person resizing the window by its
/// corner does. AppKit's minimum size limits this, not a programmatic resize.
#[tauri::command]
pub fn spike_mouse_drag<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    from: (f64, f64),
    to: (f64, f64),
) -> Result<(), String> {
    const DRAGGED: u32 = 6; // kCGEventLeftMouseDragged
    let start = screen_point(&window, from.0, from.1)?;
    let end = screen_point(&window, to.0, to.1)?;
    post_mouse(MOUSE_MOVED, start);
    std::thread::sleep(Duration::from_millis(100));
    post_mouse(MOUSE_DOWN, start);
    const STEPS: u32 = 20;
    for i in 1..=STEPS {
        let t = f64::from(i) / f64::from(STEPS);
        std::thread::sleep(Duration::from_millis(16));
        post_mouse(
            DRAGGED,
            CGPoint {
                x: start.x + (end.x - start.x) * t,
                y: start.y + (end.y - start.y) * t,
            },
        );
    }
    std::thread::sleep(Duration::from_millis(100));
    post_mouse(MOUSE_UP, end);
    Ok(())
}

/// What this process's AppKit makes of the scroll-bar setting: `preferredScrollerStyle`
/// (0 legacy, 1 overlay) and the `AppleShowScrollBars` default it reads (absent:
/// Automatically).
#[tauri::command]
pub fn spike_scroller_style() -> String {
    use objc2::{class, msg_send};
    use objc2_foundation::{NSString, NSUserDefaults};
    // SAFETY: a class getter on NSScroller.
    let style: isize = unsafe { msg_send![class!(NSScroller), preferredScrollerStyle] };
    let key = NSString::from_str("AppleShowScrollBars");
    let shown = NSUserDefaults::standardUserDefaults()
        .stringForKey(&key)
        .map_or_else(|| "absent".to_string(), |v| v.to_string());
    format!("preferredScrollerStyle {style}, AppleShowScrollBars {shown}")
}

/// Where the cursor was before a check moved it.
static SAVED_CURSOR: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// Remember the system cursor's place (`restore: false`), or put it back there.
#[tauri::command]
pub fn spike_cursor(restore: bool) {
    let mut saved = SAVED_CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: CoreGraphics calls on an event it creates and releases.
    unsafe {
        if restore {
            if let Some((x, y)) = saved.take() {
                CGWarpMouseCursorPosition(CGPoint { x, y });
            }
        } else {
            let event = CGEventCreate(std::ptr::null());
            if !event.is_null() {
                let p = CGEventGetLocation(event);
                *saved = Some((p.x, p.y));
                CFRelease(event);
            }
        }
    }
}

/// Phase 7: read back a file an extension exported (test exports only).
#[tauri::command]
pub fn spike_read_file(path: String) -> Result<String, String> {
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if !path.starts_with("/tmp/") || !name.starts_with("linen-export-") {
        return Err("only test exports".into());
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// D1: the crash log's text (empty if there is none), so tests can check what reached it.
#[tauri::command]
pub fn spike_crash_log(log: tauri::State<crate::crashlog::CrashLog>) -> String {
    std::fs::read_to_string(log.file()).unwrap_or_default()
}

/// The general pasteboard's plain text, so tests can check Copy.
#[tauri::command]
pub fn spike_read_pasteboard() -> String {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    // SAFETY: a static framework constant.
    NSPasteboard::generalPasteboard()
        .stringForType(unsafe { NSPasteboardTypeString })
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// macOS desktops (Spaces). There is no public API for putting a window on another
/// desktop; the window server's calls below are what window managers use. Spike
/// builds only, for the harness's own window.
#[cfg(target_os = "macos")]
mod desktops {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    use tauri::Runtime;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSMainConnectionID() -> i32;
        fn CGSCopyManagedDisplaySpaces(cid: i32) -> *mut AnyObject;
        fn CGSMoveWindowsToManagedSpace(cid: i32, windows: *mut AnyObject, space: u64);
        fn CGSCopySpacesForWindows(cid: i32, mask: i32, windows: *mut AnyObject) -> *mut AnyObject;
    }

    unsafe fn get(dict: *mut AnyObject, key: &str) -> *mut AnyObject {
        let key = NSString::from_str(key);
        msg_send![dict, objectForKey: &*key]
    }

    /// The window-server ids of the user desktops on the main display, in order.
    fn user_desktops() -> Vec<u64> {
        unsafe {
            let cid = CGSMainConnectionID();
            let displays = CGSCopyManagedDisplaySpaces(cid);
            if displays.is_null() {
                return vec![];
            }
            let mut out = vec![];
            let count: usize = msg_send![displays, count];
            if count > 0 {
                let main: *mut AnyObject = msg_send![displays, objectAtIndex: 0usize];
                let spaces = get(main, "Spaces");
                let n: usize = if spaces.is_null() {
                    0
                } else {
                    msg_send![spaces, count]
                };
                for i in 0..n {
                    let space: *mut AnyObject = msg_send![spaces, objectAtIndex: i];
                    let kind: i64 = msg_send![get(space, "type"), longLongValue];
                    let id: u64 = msg_send![get(space, "ManagedSpaceID"), unsignedLongLongValue];
                    if kind == 0 {
                        out.push(id);
                    }
                }
            }
            let _: () = msg_send![displays, release];
            out
        }
    }

    /// Put the window on the user's desktop `n` (1-based) without switching to it.
    pub fn move_window<R: Runtime>(
        window: &tauri::WebviewWindow<R>,
        n: usize,
    ) -> Result<u64, String> {
        let spaces = user_desktops();
        let space = *spaces
            .get(n.checked_sub(1).ok_or("desktops count from 1")?)
            .ok_or(format!(
                "there is no desktop {n} (this Mac has {})",
                spaces.len()
            ))?;
        let ns_window = window.ns_window().map_err(|e| e.to_string())? as *mut AnyObject;
        unsafe {
            let cid = CGSMainConnectionID();
            let number: isize = msg_send![ns_window, windowNumber];
            let boxed: *mut AnyObject = msg_send![class!(NSNumber), numberWithInteger: number];
            let list: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: boxed];
            CGSMoveWindowsToManagedSpace(cid, list, space);
            // Check where it went (mask 7: every kind of space).
            let now = CGSCopySpacesForWindows(cid, 7, list);
            let found: bool = if now.is_null() {
                false
            } else {
                let count: usize = msg_send![now, count];
                let mut hit = false;
                for i in 0..count {
                    let v: *mut AnyObject = msg_send![now, objectAtIndex: i];
                    let id: u64 = msg_send![v, unsignedLongLongValue];
                    hit |= id == space;
                }
                let _: () = msg_send![now, release];
                hit
            };
            if !found {
                return Err(format!("the window is not on space {space} after the move"));
            }
        }
        Ok(space)
    }
}

/// Capture one of this app's windows to a PNG, on any desktop (spike builds only).
mod capture {
    use std::ffi::c_void;
    use std::path::Path;

    type CFTypeRef = *const c_void;
    /// Include the whole window, not clipped by what is on screen.
    const IGNORE_GLOBAL_CLIP_SHAPE: u32 = 1 << 11;
    /// Nominal resolution: one pixel per point, as the baselines were taken.
    const NOMINAL_RESOLUTION: u32 = 1 << 9;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSMainConnectionID() -> i32;
        fn CGSHWCaptureWindowList(
            cid: i32,
            windows: *const u32,
            count: u32,
            options: u32,
        ) -> CFTypeRef;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: CFTypeRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
        fn CFURLCreateFromFileSystemRepresentation(
            alloc: CFTypeRef,
            buffer: *const u8,
            length: isize,
            is_directory: bool,
        ) -> CFTypeRef;
        fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const i8, encoding: u32) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
    }
    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        fn CGImageDestinationCreateWithURL(
            url: CFTypeRef,
            kind: CFTypeRef,
            count: usize,
            options: CFTypeRef,
        ) -> CFTypeRef;
        fn CGImageDestinationAddImage(dest: CFTypeRef, image: CFTypeRef, properties: CFTypeRef);
        fn CGImageDestinationFinalize(dest: CFTypeRef) -> bool;
    }

    pub fn window_png(window: u32, path: &Path) -> Result<(), String> {
        let bytes = path.to_string_lossy().into_owned();
        // SAFETY: plain CoreFoundation / ImageIO calls on objects created and released here.
        unsafe {
            let images = CGSHWCaptureWindowList(
                CGSMainConnectionID(),
                &window,
                1,
                IGNORE_GLOBAL_CLIP_SHAPE | NOMINAL_RESOLUTION,
            );
            if images.is_null() || CFArrayGetCount(images) < 1 {
                if !images.is_null() {
                    CFRelease(images);
                }
                return Err(format!(
                    "the window server could not capture window {window}"
                ));
            }
            let image = CFArrayGetValueAtIndex(images, 0);
            let url = CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
                false,
            );
            let png =
                CFStringCreateWithCString(std::ptr::null(), c"public.png".as_ptr(), 0x0800_0100);
            let dest = CGImageDestinationCreateWithURL(url, png, 1, std::ptr::null());
            let ok = !dest.is_null() && {
                CGImageDestinationAddImage(dest, image, std::ptr::null());
                CGImageDestinationFinalize(dest)
            };
            for cf in [dest, png, url, images] {
                if !cf.is_null() {
                    CFRelease(cf);
                }
            }
            if ok {
                Ok(())
            } else {
                Err(format!("could not write {}", path.display()))
            }
        }
    }
}
