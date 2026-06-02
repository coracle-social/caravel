export const formatUsd = (cents: number) => `$${(cents / 100).toFixed(2)}`

export const formatPeriod = (startSecs?: number, endSecs?: number) =>
  (!startSecs || !endSecs) ? "" : `${new Date(startSecs * 1000).toLocaleDateString()} – ${new Date(endSecs * 1000).toLocaleDateString()}`
