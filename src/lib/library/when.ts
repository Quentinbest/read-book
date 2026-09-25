// When a book was last opened, as the library says it (Screen 01):
// “today at 21:40” (Continue reading), “yesterday”, “3 days ago”, then a date.

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
  if (days <= 0) return 'today'
  if (days === 1) return 'yesterday'
  if (days < 7) return `${days} days ago`
  const d = new Date(then)
  return d.toLocaleDateString('en-GB', {
    day: 'numeric',
    month: 'short',
    year: d.getFullYear() === new Date(now).getFullYear() ? undefined : 'numeric',
  })
}

/** Continue reading: “Opened today at 21:40”. */
export function openedLine(then: number, now: number): string {
  const time = new Date(then).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })
  const days = daysAgo(then, now)
  if (days <= 0) return `Opened today at ${time}`
  if (days === 1) return `Opened yesterday at ${time}`
  return `Opened ${openedAgo(then, now)}`
}
