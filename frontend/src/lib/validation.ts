export function isKnownPlanId(planId: string, plans: { id: string }[]): boolean {
  return plans.some(p => p.id === planId)
}

export function validateBunkerUri(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return "Enter a bunker or nostrconnect link"

  if (trimmed.startsWith("nostrconnect://") || trimmed.startsWith("bunker://")) {
    try {
      new URL(trimmed)
    } catch {
      return "That doesn't look like a valid link"
    }
    return null
  }

  return "Link must start with bunker:// or nostrconnect://"
}
