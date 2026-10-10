// Settings… (⌘,) opens the Settings window, or brings it forward (G2).
import { LogicalPosition } from '@tauri-apps/api/dpi'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { t } from '../lib/strings'

/** `options`: open on that extension's options page (Reading Lens LK8). */
export async function openSettingsWindow(options?: string): Promise<void> {
  const existing = await WebviewWindow.getByLabel('settings')
  if (existing) {
    await existing.setFocus()
    if (options) await existing.emit('open-extension-options', options)
    return
  }
  new WebviewWindow('settings', {
    url: options ? `settings.html#options=${encodeURIComponent(options)}` : 'settings.html',
    title: t.prefs.title,
    width: 860,
    height: 640,
    minWidth: 720,
    minHeight: 480,
    titleBarStyle: 'overlay',
    hiddenTitle: true,
    trafficLightPosition: new LogicalPosition(20, 25),
  })
}
