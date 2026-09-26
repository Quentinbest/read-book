// Forward frontend errors and warnings to the app log (~/Library/Logs/app.linen.reader),
// so failures in the WebView are diagnosable without the Web Inspector. The debug log
// exists in development builds only; uncaught errors also go to the crash log (D1),
// which every build keeps on this Mac.

import { invoke } from '@tauri-apps/api/core'
import { error as logError, warn as logWarn } from '@tauri-apps/plugin-log'

/** At most this many crash-log entries per window, so a failing loop cannot fill the disk. */
const CRASH_NOTES = 20
let notes = 0
function crashNote(message: string) {
  if (notes++ >= CRASH_NOTES) return
  void invoke('crash_log_note', { message: `${location.pathname}: ${message}` }).catch(() => {})
}

const text = (args: unknown[]) =>
  args
    .map((a) =>
      a instanceof Error
        ? `${a.name}: ${a.message}\n${a.stack ?? ''}`
        : typeof a === 'string'
          ? a
          : JSON.stringify(a),
    )
    .join(' ')

export function installErrorLogging() {
  window.addEventListener('error', (e) => {
    const where = `${e.message} (${e.filename}:${e.lineno})`
    void logError(`[webview] ${where}`).catch(() => {})
    // WebKit's notice that a resize observer deferred work to the next frame: not an error.
    if (/^ResizeObserver loop/.test(e.message)) return
    crashNote(e.error instanceof Error ? text([e.error]) : where)
  })
  window.addEventListener('unhandledrejection', (e) => {
    void logError(`[webview] unhandled: ${text([e.reason])}`).catch(() => {})
    crashNote(`unhandled: ${text([e.reason])}`)
  })
  const origError = console.error.bind(console)
  const origWarn = console.warn.bind(console)
  console.error = (...args: unknown[]) => {
    origError(...args)
    void logError(`[webview] ${text(args)}`).catch(() => {})
  }
  console.warn = (...args: unknown[]) => {
    origWarn(...args)
    void logWarn(`[webview] ${text(args)}`).catch(() => {})
  }
}
