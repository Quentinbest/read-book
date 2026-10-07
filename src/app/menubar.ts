// Install the generated menu model as the macOS app menu. Every item is named from
// the catalogue, AppKit's standard ones too (L-1; muda names those in English).

import {
  Menu,
  MenuItem,
  PredefinedMenuItem,
  type PredefinedMenuItemOptions,
  Submenu,
} from '@tauri-apps/api/menu'
import { menuModel } from '../lib/commands/menu'
import type { CommandRegistry } from '../lib/commands/registry'
import { t } from '../lib/strings'

/** The app's name, which is not translated (L-10). */
const APP = 'Linen'

export async function installMenuBar(registry: CommandRegistry) {
  const sep = () => PredefinedMenuItem.new({ item: 'Separator' })
  const m = t.menus
  const standard = (item: PredefinedMenuItemOptions['item'], text: string) =>
    PredefinedMenuItem.new({ item, text })
  const model = menuModel(registry.available())
  const settings = registry.available().find((c) => c.id === 'app.settings')

  const appMenu = await Submenu.new({
    text: APP,
    items: [
      await standard({ About: null }, m.about(APP)),
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
      await standard('Services', m.services),
      await sep(),
      await standard('Hide', m.hide(APP)),
      await standard('HideOthers', m.hideOthers),
      await standard('ShowAll', m.showAll),
      await sep(),
      await standard('Quit', m.quit(APP)),
    ],
  })

  const submenus = []
  for (const menu of model) {
    const items = []
    for (const it of menu.items) {
      items.push(
        await MenuItem.new({
          id: it.id,
          text: it.title,
          accelerator: it.accelerator,
          action: () => registry.run(it.id),
        }),
      )
    }
    if (menu.id === 'Edit') {
      // Text fields need the standard editing items (and their shortcuts) on macOS.
      items.push(
        await sep(),
        await standard('Cut', m.cut),
        await standard('Copy', m.copy),
        await standard('Paste', m.paste),
        await standard('SelectAll', m.selectAll),
      )
    }
    if (menu.id === 'Window') {
      items.push(await sep(), await standard('Minimize', m.minimize))
    }
    submenus.push(await Submenu.new({ text: menu.title, items }))
  }
  const bar = await Menu.new({ items: [appMenu, ...submenus] })
  await bar.setAsAppMenu()
}
