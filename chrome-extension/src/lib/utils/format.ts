/**
 * Display formatting shared by every Anda surface (chat, bookmarks, skills).
 *
 * Both helpers absorb the "no value / unparseable value" cases so callers can
 * interpolate the result straight into markup without guarding first.
 */

export type TimestampStyle = 'time' | 'dateTime' | 'full'

const timestampFormats: Record<Exclude<TimestampStyle, 'full'>, Intl.DateTimeFormatOptions> = {
  time: { hour: '2-digit', minute: '2-digit' },
  dateTime: { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }
}

/**
 * Renders an epoch-millisecond or date-string timestamp in the browser locale.
 * Returns '' for missing or unparseable values.
 *
 * - `time`: clock only, for messages inside one conversation.
 * - `dateTime`: month/day plus clock, for lists spanning several days.
 * - `full`: the locale's default date and time, for detail panes.
 */
export function formatTimestamp(
  value: string | number | null | undefined,
  style: TimestampStyle = 'dateTime'
): string {
  if (!value) {
    return ''
  }
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return ''
  }
  return style === 'full' ? date.toLocaleString() : date.toLocaleString([], timestampFormats[style])
}

/** Renders a byte count as B/KB/MB. Returns '' when the size is unknown. */
export function formatFileSize(size: number | null | undefined): string {
  if (size === null || size === undefined) {
    return ''
  }
  if (size < 1024) {
    return `${size} B`
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`
  }
  return `${(size / 1024 / 1024).toFixed(1)} MB`
}

const htmlEscapes: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;'
}

/**
 * Escapes text for interpolation into an HTML string. Only for the few places
 * that build markup by hand (print views, highlighted search snippets) — Svelte
 * already escapes `{value}` in a template.
 */
export function escapeHtml(value: string): string {
  return value.replace(/[&<>"]/g, (character) => htmlEscapes[character])
}
