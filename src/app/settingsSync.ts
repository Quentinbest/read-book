// Settings changed in one window reach the others (G2): the Settings window
// saves a setting, then says so; the reader and the library apply it.

import { emit, listen } from '@tauri-apps/api/event'
import { ipc } from './ipc'

export const SETTINGS_CHANGED = 'settings-changed'

export interface SettingChange {
  key: string
  value: string
  /** The window that made the change: it has applied it already. */
  source?: string
}

/**
 * Who changed it, so a window ignores its own changes coming back (an old echo could
 * undo a newer change). One per page; the Settings page keeps its own.
 */
export const SOURCE = crypto.randomUUID()

/** Save a setting and tell every window. */
export async function changeSetting(key: string, value: string, source = SOURCE): Promise<void> {
  await ipc.settingSet(key, value)
  await emit(SETTINGS_CHANGED, { key, value, source } satisfies SettingChange)
}

export function onSettingChanged(
  fn: (c: SettingChange) => void,
  source = SOURCE,
): Promise<() => void> {
  return listen<SettingChange>(SETTINGS_CHANGED, (e) => {
    if (e.payload.source !== source) fn(e.payload)
  })
}
