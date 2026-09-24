//! Native signals a WebView does not expose (plan §10.5).

/// T6: is a screen reader running? On macOS this is VoiceOver. Single-key
/// shortcuts switch off while it runs (§2.8), and so do page-turn motion (V9).
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
