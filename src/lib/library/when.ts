// When a book was last opened, as the library says it (Screen 01):
// “today at 21:40” (Continue reading), “yesterday”, “3 days ago”, then a date,
// in the UI language (L-1).

import { t } from '../strings'
import { formatDate, formatDaysAgo } from '../strings/format'

const DAY = 86_400_000

function startOfDay(ms: number): number {
  const d = new Date(ms)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}

/** Whole calendar days between two times (0 = the same day). */
export function daysAgo(then: number, now: number): number {
  return Math.round((startOfDay(now) - startOfDay(then)) / DAY)
}

export function openedAgo(then: number, now: number): string {
  const days = daysAgo(then, now)
  if (days < 7) return formatDaysAgo(Math.max(0, days))
  const d = new Date(then)
  return formatDate(then, {
    day: 'numeric',
    month: 'short',
    year: d.getFullYear() === new Date(now).getFullYear() ? undefined : 'numeric',
  })
}

/** Continue reading: “Opened today at 21:40”. */
export function openedLine(then: number, now: number): string {
  const days = daysAgo(then, now)
  if (days > 1) return t.library.openedAgo(openedAgo(then, now))
  const time = formatDate(then, { hour: '2-digit', minute: '2-digit' })
  return t.library.openedAt(formatDaysAgo(Math.max(0, days)), time)
}
