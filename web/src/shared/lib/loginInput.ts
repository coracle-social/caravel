// Reading what someone types into the login form.

export type Tab = "nip07" | "nip46" | "key"

// What's wrong with a pasted signer link, if anything. A nostrconnect:// link names a client, not a signer (NIP-46).
export function validateBunkerUri(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return "Enter a bunker link"
  if (trimmed.startsWith("nostrconnect://")) return "That link is for your signer app to open; paste the bunker:// link it gives you"
  if (!trimmed.startsWith("bunker://")) return "Link must start with bunker://"

  try {
    new URL(trimmed)
  } catch {
    return "That doesn't look like a valid link"
  }
  return null
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
