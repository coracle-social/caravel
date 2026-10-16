import { createSignal } from "solid-js"

// The member's session token, kept in this browser so a later visit to this
// relay's portal needs no sign-in.
const KEY = "caravel.portal.session"

function read() {
  try {
    return localStorage.getItem(KEY) ?? undefined
  } catch {
    return undefined
  }
}

const [token, setToken] = createSignal(read())

export const session = token

export function signIn(next: string) {
  setToken(next)
  try {
    localStorage.setItem(KEY, next)
  } catch {
    // The session still lasts for this page.
  }
}

export function signOut() {
  setToken(undefined)
  try {
    localStorage.removeItem(KEY)
  } catch {
    // Nothing was stored.
  }
}
