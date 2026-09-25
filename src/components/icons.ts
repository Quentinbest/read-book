// Outline icons on a 24 px grid, 1.6 px strokes, filled only when active (V6).
// Each value is SVG path data.

export const ICONS = {
  library: 'M15 5l-7 7 7 7',
  contents: 'M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01',
  search: 'M11 4.5a6.5 6.5 0 1 1 0 13 6.5 6.5 0 0 1 0-13zM20 20l-4.3-4.3',
  notes: 'M6 4h9l3 3v13H6zM9 10h6M9 14h6',
  more: 'M6 12h.01M12 12h.01M18 12h.01',
  close: 'M6 6l12 12M18 6L6 18',
  warning: 'M12 4 21 20H3zM12 10v4.5M12 17.5h.01',
  open: 'M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14',
  back: 'M10 7l-5 5 5 5M5 12h14',
} as const

export type IconName = keyof typeof ICONS
