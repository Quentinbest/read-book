// Settings changed in one window reach the others (G2): the Settings window
// saves a setting, then says so; the reader and the library apply it.

import { emit, listen } from '@tauri-apps/api/event'
import { ipc } from './ipc'

export const SETTINGS_CHANGED = 'settings-changed'

export interface SettingChange {
  key: string
  value: string
}

/** Save a setting and tell every window. */
export async function changeSetting(key: string, value: string): Promise<void> {
  await ipc.settingSet(key, value)
  await emit(SETTINGS_CHANGED, { key, value } satisfies SettingChange)
}

export function onSettingChanged(fn: (c: SettingChange) => void): Promise<() => void> {
  return listen<SettingChange>(SETTINGS_CHANGED, (e) => fn(e.payload))
}
