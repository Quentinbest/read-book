//! 1.1 dictionary peek (PROVISIONAL): definitions from the dictionaries installed on
//! this Mac (Dictionary Services), so a looked-up word never leaves it (D1). “Open in
//! Dictionary” hands the word to Dictionary.app.

/// The longest text looked up: a phrase, not a passage.
const MAX_CHARS: usize = 80;

fn clean(text: &str) -> Option<String> {
    let word: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CHARS)
        .collect();
    (!word.is_empty()).then_some(word)
}

#[cfg(target_os = "macos")]
mod dcs {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct CFRange {
        pub location: isize,
        pub length: isize,
    }
    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        /// Returns a +1 CFStringRef, or null when no dictionary has the text.
        pub fn DCSCopyTextDefinition(
            dictionary: *const c_void,
            text: *const c_void,
            range: CFRange,
        ) -> *const c_void;
    }
}

/// A plain-text definition of `text`, or None.
#[tauri::command]
pub fn look_up(text: String) -> Option<String> {
    let word = clean(&text)?;
    definition(&word)
}

#[cfg(target_os = "macos")]
fn definition(word: &str) -> Option<String> {
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    let ns = NSString::from_str(word);
    let range = dcs::CFRange {
        location: 0,
        length: ns.length() as isize,
    };
    // SAFETY: `ns` outlives the call (NSString and CFString are toll-free bridged);
    // the result follows the Copy rule, so it is taken over (+1) and released on drop.
    let out = unsafe {
        let ptr = dcs::DCSCopyTextDefinition(std::ptr::null(), Retained::as_ptr(&ns).cast(), range);
        if ptr.is_null() {
            return None;
        }
        Retained::from_raw(ptr as *mut NSString)?
    };
    let s = out.to_string();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

#[cfg(not(target_os = "macos"))]
fn definition(_word: &str) -> Option<String> {
    None
}

/// Open the word in Dictionary.app (`dict:` URL). Only the cleaned text is passed.
#[tauri::command]
pub fn open_dictionary(text: String) -> Result<(), String> {
    let word = clean(&text).ok_or("nothing to look up")?;
    #[cfg(target_os = "macos")]
    {
        let url = format!("dict://{}", encode(&word));
        let ns = objc2_foundation::NSString::from_str(&url);
        let ns_url = objc2_foundation::NSURL::URLWithString(&ns).ok_or("not a URL")?;
        if objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&ns_url) {
            return Ok(());
        }
        Err("Dictionary could not be opened".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = word;
        Err("no dictionary on this system".into())
    }
}

/// Percent-encode everything but unreserved characters (RFC 3986).
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_a_phrase_not_a_passage() {
        assert_eq!(clean("  white\n whale "), Some("white whale".into()));
        assert_eq!(clean(" \n"), None);
        assert_eq!(clean(&"a".repeat(500)).unwrap().len(), MAX_CHARS);
    }

    #[test]
    fn the_dictionary_url_carries_only_the_word() {
        assert_eq!(encode("naïve café"), "na%C3%AFve%20caf%C3%A9");
        assert_eq!(encode("a/b?c#d"), "a%2Fb%3Fc%23d");
    }

    #[test]
    fn nothing_is_a_definition_of_nothing() {
        assert_eq!(look_up("   ".into()), None);
        // A word may or may not have a definition, depending on the installed dictionaries.
        if let Some(d) = look_up("whale".into()) {
            assert!(!d.is_empty());
        }
    }
}
