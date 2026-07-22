export const formatUsd = (cents: number) => `$${(cents / 100).toFixed(2)}`

export const formatPeriod = (startSecs?: number, endSecs?: number) =>
  (!startSecs || !endSecs) ? "" : `${new Date(startSecs * 1000).toLocaleDateString()} – ${new Date(endSecs * 1000).toLocaleDateString()}`

const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 60 * 60],
  ["month", 30 * 24 * 60 * 60],
  ["day", 24 * 60 * 60],
  ["hour", 60 * 60],
  ["minute", 60],
]

export const formatTimeAgo = (secs: number) => {
  const elapsed = Math.max(Math.floor(Date.now() / 1000) - secs, 0)
  const formatter = new Intl.RelativeTimeFormat(undefined, {numeric: "auto"})

  for (const [unit, unitSecs] of RELATIVE_UNITS) {
    if (elapsed >= unitSecs) return formatter.format(-Math.floor(elapsed / unitSecs), unit)
  }

  return formatter.format(-elapsed, "second")
}

export const formatBytes = (bytes: number) => {
  const units = ["B", "KB", "MB", "GB"]
  let value = bytes
  let unit = 0

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }

  return `${unit === 0 ? value : value.toFixed(1)} ${units[unit]}`
}
