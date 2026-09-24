// Component gallery (dev only; never in the production build). Served by the
// Vite dev server at /gallery.html?theme=paper|sepia|night for the axe checks
// (plan Phase 1 Done-when) and for visual review.
import { mount } from 'svelte'
import '../app/base.css'
import { applyTheme, type ThemeChoice } from '../app/theme'
import Gallery from './Gallery.svelte'

applyTheme((new URLSearchParams(location.search).get('theme') as ThemeChoice) ?? 'paper')
mount(Gallery, { target: document.getElementById('app')! })
