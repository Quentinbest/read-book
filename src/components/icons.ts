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
  chevron: 'M9 6l6 6-6 6',
  open: 'M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14',
  back: 'M10 7l-5 5 5 5M5 12h14',
  // Screens 03, 06, 07 (drawn there).
  highlights: 'M14 5l5 5-8 8H6v-5zM4 21h9',
  note: 'M5 5h14v10h-8l-6 4z',
  copy: 'M10.5 8.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2zM15.5 5.5V5a1.5 1.5 0 0 0-1.5-1.5H5A1.5 1.5 0 0 0 3.5 5v9A1.5 1.5 0 0 0 5 15.5h.5',
  trash: 'M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  // Screen 01: the sort menu's chevron and Open….
  'chevron-down': 'M6 9l6 6 6-6',
  plus: 'M12 5v14M5 12h14',
  // Screen 11: network permissions.
  globe:
    'M12 3.5a8.5 8.5 0 1 1 0 17 8.5 8.5 0 0 1 0-17zM3.5 12h17M12 3.5c2.4 2.3 3.5 5.2 3.5 8.5s-1.1 6.2-3.5 8.5c-2.4-2.3-3.5-5.2-3.5-8.5s1.1-6.2 3.5-8.5z',
  // 1.1 (approved 2026-10-01): Look Up (an open book); the library's grid and list views.
  dictionary:
    'M12 6.5C10 5 7 4.5 4 5v13c3-.5 6 0 8 1.5 2-1.5 5-2 8-1.5V5c-3-.5-6 0-8 1.5zM12 6.5v13',
  grid: 'M4.5 4.5h6v6h-6zM13.5 4.5h6v6h-6zM4.5 13.5h6v6h-6zM13.5 13.5h6v6h-6z',
  list: 'M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01',
} as const

export type IconName = keyof typeof ICONS
