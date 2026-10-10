// Reading Lens Stage 2d (LK6, LK7): what an extension may ask Linen for, in either
// window: a host from its optionalPermissions, granted in Linen's own sheet, and a
// key, typed into Linen's native dialog and kept in the Keychain. The extension
// never sees a key; it may only ask whether one is saved.

import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { testHooks } from '../../app/testHooks'
import { t } from '../strings'
import type { InstalledExtension } from './types'

export interface HostRequest {
  extId: string
  name: string
  host: string
  /** The extension's own words, plain text, shown as theirs. */
  purpose: string
}

/** Shows Linen's sheet; true when the reader allowed the host. */
export type AskHost = (r: HostRequest) => Promise<boolean>

const plain = (s: unknown, max: number) =>
  String(s ?? '')
    .split('')
    .map((c) => (c.charCodeAt(0) < 32 ? ' ' : c))
    .join('')
    .trim()
    .slice(0, max)

/** LK6: a host the extension lists as optional, granted only if the reader allows it. */
export async function requestHost(
  x: InstalledExtension,
  host: string,
  purpose: unknown,
  ask: AskHost | null,
): Promise<boolean> {
  const permission = `network:${host}`
  if (x.granted.includes(permission)) return true
  if (!(x.manifest.optionalPermissions ?? []).includes(permission))
    throw new Error(`${host} is not in optionalPermissions`)
  if (!ask) throw new Error('no window can ask the reader now')
  const allowed = await ask({
    extId: x.manifest.id,
    name: x.manifest.name,
    host,
    purpose: plain(purpose, 200),
  })
  if (!allowed) return false
  await invoke('extension_grant', { id: x.manifest.id, host })
  await emit('extensions-changed')
  return true
}

export const hasHost = (x: InstalledExtension, host: string) =>
  x.granted.includes(`network:${host}`)

/** LK7: a key for a host the extension may connect to, typed into Linen's dialog. */
export async function requestSecret(
  x: InstalledExtension,
  name: string,
  host: string,
  label: unknown,
): Promise<boolean> {
  if (!hasHost(x, host)) throw new Error(`${host} is not allowed for this extension`)
  // The harness cannot type into a modal NSAlert; it hands the key over instead.
  if (testHooks?.secretFor) {
    const value = testHooks.secretFor(x.manifest.id, name)
    if (value === null) return false
    await invoke('spike_secret_save', { id: x.manifest.id, name, host, value })
    return true
  }
  return invoke<boolean>('extension_secret_request', {
    id: x.manifest.id,
    name,
    host,
    label: plain(label, 60),
    dialog: {
      title: t.extAccess.keyTitle(host),
      message: t.extAccess.keyMessage(x.manifest.name, host),
      save: t.extAccess.save,
      cancel: t.extAccess.cancel,
    },
  })
}

export const hasSecret = (id: string, name: string) =>
  invoke<boolean>('extension_secret_has', { id, name })
