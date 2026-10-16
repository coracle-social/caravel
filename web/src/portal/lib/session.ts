import { createSignal } from "solid-js"
import { request } from "@/shared/lib/http"

// The member's session token, kept so a later visit needs no sign-in.
const KEY = "caravel.portal.session"

function read() {
  try {
    return localStorage.getItem(KEY) ?? undefined
  } catch {
    return undefined
  }
}

const [session, setSession] = createSignal(read())

export { session }

export function signIn(next: string) {
  setSession(next)
  try {
    localStorage.setItem(KEY, next)
  } catch {
    // The session still lasts for this page.
  }
}

// Forget the session here, and end it on the server.
export function signOut() {
  const token = session()
  setSession(undefined)
  try {
    localStorage.removeItem(KEY)
  } catch {
    // Nothing was stored.
  }
  if (token) {
    void request("/session", { method: "DELETE", headers: { Authorization: `Bearer ${token}` } }).catch(() => undefined)
  }
}
