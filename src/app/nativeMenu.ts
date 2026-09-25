// Native pop-up menus (A10, E7, the library's sort menu). The e2e harness cannot
// dismiss a native menu, so under test the entries are recorded instead.

import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem } from '@tauri-apps/api/menu'
import { testHooks } from './testHooks'

export type MenuEntry = { label: string; run: () => void; checked?: boolean } | null

export async function popUpMenu(entries: MenuEntry[]): Promise<void> {
  const actions = entries.filter((x) => x !== null)
  if (testHooks) {
    testHooks.contextMenu = {
      labels: actions.map((x) => x.label),
      run: (label) => actions.find((x) => x.label === label)?.run(),
    }
    return
  }
  const items = await Promise.all(
    entries.map((x) =>
      x === null
        ? PredefinedMenuItem.new({ item: 'Separator' })
        : x.checked !== undefined
          ? CheckMenuItem.new({ text: x.label, checked: x.checked, action: x.run })
          : MenuItem.new({ text: x.label, action: x.run }),
    ),
  )
  await (await Menu.new({ items })).popup()
}
