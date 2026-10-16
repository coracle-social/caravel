// Reading what someone types into the login form.

export type Tab = "nip07" | "nip46" | "key"

// What's wrong with a pasted signer link, if anything.
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

// A pasted signer link as a bunker:// url, rewriting a nostrconnect:// one.
export function normalizeBunkerUrl(value: string): string {
  const trimmed = value.trim()
  if (!trimmed) return ""

  if (trimmed.startsWith("nostrconnect://")) {
    const url = new URL(trimmed)
    const remote = url.host || url.pathname.replace(/^\/+/, "")
    const relays = url.searchParams.getAll("relay")
    const secret = url.searchParams.get("secret")
    const params = new URLSearchParams()
    for (const relay of relays) params.append("relay", relay)
    if (secret) params.set("secret", secret)
    return `bunker://${remote}?${params.toString()}`
  }

  return trimmed
}

// How to log in with the keys entered: an ncryptsec with its password, or else an nsec.
export type KeyLoginPlan =
  | { kind: "ncryptsec"; ncryptsec: string; password: string }
  | { kind: "nsec"; key: string }
  | { kind: "error"; message: string }

export function decideKeyLogin(input: { ncryptsec: string; nsec: string; password: string }): KeyLoginPlan {
  const ncryptsec = input.ncryptsec.trim()
  if (ncryptsec) {
    if (!input.password.trim()) {
      return { kind: "error", message: "Password is required for ncryptsec" }
    }
    return { kind: "ncryptsec", ncryptsec, password: input.password }
  }

  const key = input.nsec.trim()
  if (!key) return { kind: "error", message: "Enter an nsec or ncryptsec key" }
  return { kind: "nsec", key }
}
