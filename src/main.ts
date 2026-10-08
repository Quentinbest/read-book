import { mount } from 'svelte'
import './app/base.css'
import { installThemeCss } from './app/theme'
import { installErrorLogging } from './app/log'
import { initLocale } from './app/locale'

installErrorLogging()
installThemeCss()
// L-5: the language is set before the UI is imported, so nothing that reads `t`
// while it loads sees English first.
await initLocale()
const { default: App } = await import('./App.svelte')

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app
