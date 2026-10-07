// Install the generated menu model as the macOS app menu.

import { Menu, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'
import { menuModel } from '../lib/commands/menu'
import type { CommandRegistry } from '../lib/commands/registry'

export async function installMenuBar(registry: CommandRegistry) {
  const sep = () => PredefinedMenuItem.new({ item: 'Separator' })
  const model = menuModel(registry.available())
  const settings = registry.available().find((c) => c.id === 'app.settings')

  const appMenu = await Submenu.new({
    text: 'Linen',
    items: [
      await PredefinedMenuItem.new({ item: { About: null } }),
      await sep(),
      // macOS puts Settings… (⌘,) in the app menu.
      ...(settings
        ? [
            await MenuItem.new({
              id: settings.id,
              text: settings.title,
              accelerator: 'Cmd+,',
              action: () => registry.run(settings.id),
            }),
            await sep(),
          ]
        : []),
      await PredefinedMenuItem.new({ item: 'Services' }),
      await sep(),
      await PredefinedMenuItem.new({ item: 'Hide' }),
      await PredefinedMenuItem.new({ item: 'HideOthers' }),
      await PredefinedMenuItem.new({ item: 'ShowAll' }),
      await sep(),
      await PredefinedMenuItem.new({ item: 'Quit' }),
    ],
  })

  const submenus = []
  for (const m of model) {
    const items = []
    for (const it of m.items) {
      items.push(
        await MenuItem.new({
          id: it.id,
          text: it.title,
          accelerator: it.accelerator,
          action: () => registry.run(it.id),
        }),
      )
    }
    if (m.id === 'Edit') {
      // Text fields need the standard editing items (and their shortcuts) on macOS.
      items.push(await sep())
      for (const item of ['Cut', 'Copy', 'Paste', 'SelectAll'] as const) {
        items.push(await PredefinedMenuItem.new({ item }))
      }
    }
    if (m.id === 'Window') {
      items.push(await sep(), await PredefinedMenuItem.new({ item: 'Minimize' }))
    }
    submenus.push(await Submenu.new({ text: m.title, items }))
  }
  const menu = await Menu.new({ items: [appMenu, ...submenus] })
  await menu.setAsAppMenu()
}
