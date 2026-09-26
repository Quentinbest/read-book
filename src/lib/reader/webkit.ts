// D7-WebKit: the reader engine (foliate-js's paginator) uses a regex lookbehind,
// which WebKit parses only from Safari 16.4. macOS 13 shipped with 16.0–16.3, so on
// an un-updated Mac a book would open to a blank page. The app asks first.

/** Whether this WebView can run the reader engine. */
export function readerEngineSupported(): boolean {
  try {
    new RegExp('(?<=a)b')
    return true
  } catch {
    return false
  }
}
