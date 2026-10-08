// The Settings window (G2, provisional; Screen 11): its own page and window.
import { mount } from 'svelte'
import '../app/base.css'
import { installThemeCss } from '../app/theme'
import { installErrorLogging } from '../app/log'
import { initLocale } from '../app/locale'

installErrorLogging()
installThemeCss()
// L-5: the language is set before the UI is imported (as src/main.ts).
await initLocale()
const { default: Preferences } = await import('./Preferences.svelte')
mount(Preferences, { target: document.getElementById('app')! })
