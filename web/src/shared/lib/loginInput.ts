// Pure decision module for Login's input handling. No signers are constructed
// here (that's effectful/async and stays in the component) — only the input
// normalization, the key-material validation ladder, and the initial-tab choice.
// Keeping these pure makes them testable independently of the DOM/signer stack.

export type Tab = "nip07" | "nip46" | "key"

// Normalize a pasted signer link into a bunker:// URI. nostrconnect:// links are
// rewritten to the equivalent bunker:// form; anything else is passed through
// trimmed. Pure string transform.
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

// Discriminated plan for key-material login, encoding the validation ladder:
// an ncryptsec requires a password; otherwise an nsec is required; otherwise it's
// an error. The component constructs the actual signer/account on the data
// branches and throws on the error branch.
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

// Default login tab: prefer the extension when one is present, otherwise the
// remote signer tab.
export function initialLoginTab(hasExtension: boolean): Tab {
  return hasExtension ? "nip07" : "nip46"
}
