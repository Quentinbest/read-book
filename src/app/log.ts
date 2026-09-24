// Forward frontend errors and warnings to the app log (~/Library/Logs/app.linen.reader),
// so failures in the WebView are diagnosable without the Web Inspector.

import { error as logError, warn as logWarn } from '@tauri-apps/plugin-log'

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
  window.addEventListener(
    'error',
    (e) => void logError(`[webview] ${e.message} (${e.filename}:${e.lineno})`).catch(() => {}),
  )
  window.addEventListener(
    'unhandledrejection',
    (e) => void logError(`[webview] unhandled: ${text([e.reason])}`).catch(() => {}),
  )
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
