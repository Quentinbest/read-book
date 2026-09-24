import { mount } from 'svelte'
import App from './App.svelte'
import './app/base.css'
import { installThemeCss } from './app/theme'

installThemeCss()

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app
