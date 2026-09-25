// Settings… (⌘,) opens the Settings window, or brings it forward (G2).
import { LogicalPosition } from '@tauri-apps/api/dpi'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'

export async function openSettingsWindow(): Promise<void> {
  const existing = await WebviewWindow.getByLabel('settings')
  if (existing) {
    await existing.setFocus()
    return
  }
  new WebviewWindow('settings', {
    url: 'settings.html',
    title: 'Settings',
    width: 860,
    height: 640,
    minWidth: 720,
    minHeight: 480,
    titleBarStyle: 'overlay',
    hiddenTitle: true,
    trafficLightPosition: new LogicalPosition(20, 25),
  })
}
