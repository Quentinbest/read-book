// Apply the theme tokens as CSS custom properties (V1). Auto follows the system
// between Paper and Night.

import { themeCss } from '../lib/theme/tokens'

export type ThemeChoice = 'paper' | 'sepia' | 'night' | 'auto'

let installed = false

export function installThemeCss() {
  if (installed) return
  const style = document.createElement('style')
  style.id = 'linen-theme'
  style.textContent = themeCss()
  document.head.prepend(style)
  installed = true
}

export function applyTheme(choice: ThemeChoice) {
  installThemeCss()
  document.documentElement.dataset.theme = choice
}
