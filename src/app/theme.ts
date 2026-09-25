// Apply the theme tokens as CSS custom properties (V1). Auto follows the system
// between Paper and Night. An extension's theme pack (P9) is applied as inline
// custom properties, since the stylesheet holds only the built-in themes.

import { themeCss, themeVariables, type Theme } from '../lib/theme/tokens'

export type ThemeChoice = 'paper' | 'sepia' | 'night' | 'auto' | `ext:${string}`

let installed = false
let customKeys: string[] = []

export function installThemeCss() {
  if (installed) return
  const style = document.createElement('style')
  style.id = 'linen-theme'
  style.textContent = themeCss()
  document.head.prepend(style)
  installed = true
}

/** `custom` is the theme pack's theme when `choice` names one (`ext:<id>/<theme>`). */
export function applyTheme(choice: ThemeChoice, custom?: Theme | null) {
  installThemeCss()
  const root = document.documentElement
  for (const k of customKeys) root.style.removeProperty(k)
  customKeys = []
  if (choice.startsWith('ext:') && custom) {
    for (const [k, v] of Object.entries(themeVariables(custom))) {
      root.style.setProperty(k, v)
      customKeys.push(k)
    }
    root.style.colorScheme = custom.scheme
    root.dataset.theme = 'extension'
    return
  }
  root.style.removeProperty('color-scheme')
  // A theme pack that is gone or refused falls back to Auto.
  root.dataset.theme = choice.startsWith('ext:') ? 'auto' : choice
}
