//! Native signals a WebView does not expose (plan §10.5).

/// T6: is a screen reader running? On macOS this is VoiceOver. Single-key
/// shortcuts switch off while it runs (§2.8), and so do page-turn motion (V9).
/// N10: open an external link in the system browser. Only http(s) and mailto are
/// accepted; anything else (file:, custom schemes, app launch URLs) is refused.
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err(format!("refused to open a {} link", parsed.scheme()));
    }
    let ns = objc2_foundation::NSString::from_str(parsed.as_str());
    let ns_url = objc2_foundation::NSURL::URLWithString(&ns).ok_or("not a URL")?;
    let opened = objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&ns_url);
    if opened {
        Ok(())
    } else {
        Err("the system could not open the link".into())
    }
}

/// D7-WebKit: open System Settings at Software Update, where Safari (and so the
/// system WebKit) is updated. A fixed URL: the page cannot open other settings panes.
#[tauri::command]
pub fn open_software_update() -> Result<(), String> {
    let ns = objc2_foundation::NSString::from_str(
        "x-apple.systempreferences:com.apple.preferences.softwareupdate",
    );
    let ns_url = objc2_foundation::NSURL::URLWithString(&ns).ok_or("not a URL")?;
    if objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&ns_url) {
        Ok(())
    } else {
        Err("the system could not open Software Update".into())
    }
}

/// N9 Copy (and later selection Copy): put plain text on the general pasteboard.
/// Native, so it does not depend on WebKit's user-gesture rules for the clipboard API.
#[tauri::command]
pub fn copy_text(text: String) -> Result<(), String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    let s = objc2_foundation::NSString::from_str(&text);
    // SAFETY: NSPasteboardTypeString is a static framework constant.
    let ok = board.setString_forType(&s, unsafe { NSPasteboardTypeString });
    if ok {
        Ok(())
    } else {
        Err("the pasteboard refused the text".into())
    }
}

#[tauri::command]
pub fn screen_reader_running() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        NSWorkspace::sharedWorkspace().isVoiceOverEnabled()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// L-4: the reader's preferred languages, most preferred first (e.g. "zh-Hant-TW").
/// `NSLocale` follows the per-app language set in System Settings, which
/// `navigator.languages` in WKWebView may not (docs/spikes/i18n-spike.md).
#[tauri::command]
pub fn preferred_languages() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSLocale;
        NSLocale::preferredLanguages()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// macOS virtual key codes (ANSI positions) for the `KeyboardEvent.code` values
/// Linen binds or shows (T7).
#[cfg(target_os = "macos")]
const MAC_KEYS: &[(&str, u16)] = &[
    ("KeyA", 0x00),
    ("KeyS", 0x01),
    ("KeyD", 0x02),
    ("KeyF", 0x03),
    ("KeyH", 0x04),
    ("KeyG", 0x05),
    ("KeyZ", 0x06),
    ("KeyX", 0x07),
    ("KeyC", 0x08),
    ("KeyV", 0x09),
    ("KeyB", 0x0B),
    ("KeyQ", 0x0C),
    ("KeyW", 0x0D),
    ("KeyE", 0x0E),
    ("KeyR", 0x0F),
    ("KeyY", 0x10),
    ("KeyT", 0x11),
    ("Digit1", 0x12),
    ("Digit2", 0x13),
    ("Digit3", 0x14),
    ("Digit4", 0x15),
    ("Digit6", 0x16),
    ("Digit5", 0x17),
    ("Equal", 0x18),
    ("Digit9", 0x19),
    ("Digit7", 0x1A),
    ("Minus", 0x1B),
    ("Digit8", 0x1C),
    ("Digit0", 0x1D),
    ("BracketRight", 0x1E),
    ("KeyO", 0x1F),
    ("KeyU", 0x20),
    ("BracketLeft", 0x21),
    ("KeyI", 0x22),
    ("KeyP", 0x23),
    ("KeyL", 0x25),
    ("KeyJ", 0x26),
    ("Quote", 0x27),
    ("KeyK", 0x28),
    ("Semicolon", 0x29),
    ("Backslash", 0x2A),
    ("Comma", 0x2B),
    ("Slash", 0x2C),
    ("KeyN", 0x2D),
    ("KeyM", 0x2E),
    ("Period", 0x2F),
    ("Backquote", 0x32),
];

#[cfg(target_os = "macos")]
mod carbon {
    use std::ffi::c_void;
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        pub static kTISPropertyUnicodeKeyLayoutData: *const c_void;
        pub fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
        pub fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *const c_void;
        pub fn LMGetKbdType() -> u8;
        #[allow(clippy::too_many_arguments)]
        pub fn UCKeyTranslate(
            layout: *const c_void,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            key_translate_options: u32,
            dead_key_state: *mut u32,
            max_string_length: usize,
            actual_string_length: *mut usize,
            unicode_string: *mut u16,
        ) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        pub fn CFRelease(cf: *const c_void);
    }
}

/// T7: the character each physical key produces in the current keyboard layout,
/// keyed by `KeyboardEvent.code`, for shortcut labels. Bindings never change;
/// only the labels follow the layout. Runs on the main thread (sync command),
/// as the Text Input Sources API requires.
#[tauri::command]
pub fn keyboard_layout_labels() -> std::collections::HashMap<String, String> {
    let mut labels = std::collections::HashMap::new();
    #[cfg(target_os = "macos")]
    unsafe {
        use carbon::*;
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return labels;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        if !data.is_null() {
            let layout = CFDataGetBytePtr(data) as *const std::ffi::c_void;
            const KEY_ACTION_DISPLAY: u16 = 3;
            const NO_DEAD_KEYS: u32 = 1;
            for (code, vk) in MAC_KEYS {
                let mut dead: u32 = 0;
                let mut len: usize = 0;
                let mut buf = [0u16; 4];
                let status = UCKeyTranslate(
                    layout,
                    *vk,
                    KEY_ACTION_DISPLAY,
                    0,
                    LMGetKbdType() as u32,
                    NO_DEAD_KEYS,
                    &mut dead,
                    buf.len(),
                    &mut len,
                    buf.as_mut_ptr(),
                );
                if status == 0 && len > 0 {
                    let s = String::from_utf16_lossy(&buf[..len]);
                    if !s.trim().is_empty() {
                        labels.insert((*code).to_string(), s);
                    }
                }
            }
        }
        CFRelease(source);
    }
    labels
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn key_table_has_unique_codes_and_positions() {
        let codes: std::collections::HashSet<_> = MAC_KEYS.iter().map(|(c, _)| c).collect();
        let vks: std::collections::HashSet<_> = MAC_KEYS.iter().map(|(_, v)| v).collect();
        assert_eq!(codes.len(), MAC_KEYS.len());
        assert_eq!(vks.len(), MAC_KEYS.len());
    }

    /// Reads the real layout; run with `cargo test -- --ignored` on a Mac.
    #[test]
    #[ignore]
    fn reads_the_current_layout() {
        let labels = keyboard_layout_labels();
        eprintln!("{labels:?}");
        assert!(labels.len() >= 40);
    }
}

/// T8, S11: where the chrome's reveal zones must give way to the OS.
#[derive(serde::Serialize)]
pub struct ScreenEdges {
    /// The Dock hides itself and slides in at `dock_edge` (at the bottom, the bottom zone
    /// leaves out the Dock's strip).
    pub dock_autohide: bool,
    /// "bottom", "left" or "right".
    pub dock_edge: String,
    /// The window is in macOS full screen: the menu bar slides in at the top.
    pub fullscreen: bool,
    /// Height of the menu bar in points (the top zone starts below it in full screen).
    pub menu_bar_height: f64,
}

#[tauri::command]
pub fn screen_edges(window: tauri::WebviewWindow) -> ScreenEdges {
    let fullscreen = window.is_fullscreen().unwrap_or(false);
    #[cfg(target_os = "macos")]
    {
        use objc2::AnyThread;
        use objc2_foundation::{NSString, NSUserDefaults};
        let dock = NSUserDefaults::initWithSuiteName(
            NSUserDefaults::alloc(),
            Some(&NSString::from_str("com.apple.dock")),
        );
        let (autohide, edge) = match dock {
            Some(d) => (
                d.boolForKey(&NSString::from_str("autohide")),
                d.stringForKey(&NSString::from_str("orientation"))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "bottom".into()),
            ),
            None => (false, "bottom".into()),
        };
        // The menu bar is 24 pt, or 37 pt on displays with a camera housing; take the safe height.
        let menu_bar_height = window
            .current_monitor()
            .ok()
            .flatten()
            .map(|m| {
                if m.size().height as f64 / m.scale_factor() > 1100.0 {
                    37.0
                } else {
                    24.0
                }
            })
            .unwrap_or(24.0);
        ScreenEdges {
            dock_autohide: autohide,
            dock_edge: edge,
            fullscreen,
            menu_bar_height,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        ScreenEdges {
            dock_autohide: false,
            dock_edge: "bottom".into(),
            fullscreen,
            menu_bar_height: 0.0,
        }
    }
}

/// Screens 02 and 03: the window buttons sit in the reader's top bar and hide
/// with it, so the immersive page shows nothing but the book.
#[tauri::command]
pub fn set_window_controls(window: tauri::WebviewWindow, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let Ok(ptr) = window.ns_window() else { return };
        let ptr = ptr as usize;
        let _ = window.run_on_main_thread(move || {
            use objc2_app_kit::{NSWindow, NSWindowButton};
            // SAFETY: Tauri hands out the window's own NSWindow pointer, used on the main thread.
            let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
            for kind in [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ] {
                if let Some(button) = ns_window.standardWindowButton(kind) {
                    button.setHidden(!visible);
                }
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, visible);
}

/// P7: is ⇧ held right now? Read at launch: holding it starts in safe mode.
#[cfg(target_os = "macos")]
pub fn shift_held() -> bool {
    use objc2::{class, msg_send};
    // NSEventModifierFlagShift = 1 << 17
    let flags: usize = unsafe { msg_send![class!(NSEvent), modifierFlags] };
    flags & (1 << 17) != 0
}

#[cfg(not(target_os = "macos"))]
pub fn shift_held() -> bool {
    false
}

/// Reading Lens LK7: a native dialog with a secure field, so a key never passes through
/// a web page. Runs on the main thread; returns the key, or None when cancelled or empty.
#[cfg(target_os = "macos")]
pub fn ask_secret(d: &crate::ext_commands::KeyDialog) -> Option<String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSSecureTextField};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    let mtm = MainThreadMarker::new()?;
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&d.title));
    alert.setInformativeText(&NSString::from_str(&d.message));
    alert.addButtonWithTitle(&NSString::from_str(&d.save));
    alert.addButtonWithTitle(&NSString::from_str(&d.cancel));
    let field = NSSecureTextField::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(300.0, 24.0)),
    );
    alert.setAccessoryView(Some(&field));
    alert.window().setInitialFirstResponder(Some(&field));
    let answer = alert.runModal();
    if answer != NSAlertFirstButtonReturn {
        return None;
    }
    let value = field.stringValue().to_string();
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(not(target_os = "macos"))]
pub fn ask_secret(_d: &crate::ext_commands::KeyDialog) -> Option<String> {
    None
}
