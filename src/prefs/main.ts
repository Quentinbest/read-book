// The Settings window (G2, provisional; Screen 11): its own page and window.
import { mount } from 'svelte'
import Preferences from './Preferences.svelte'
import '../app/base.css'
import { installThemeCss } from '../app/theme'
import { installErrorLogging } from '../app/log'

installErrorLogging()
installThemeCss()
mount(Preferences, { target: document.getElementById('app')! })
