import { createSignal } from "solid-js"
import type { RedirectUrl } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"

// Sending the browser to a page the API names, such as one of Stripe's.
export function useRedirect() {
  const [redirecting, setRedirecting] = createSignal(false)
  const [error, setError] = createSignal("")

  async function go(open: () => Promise<RedirectUrl>, failure: string) {
    setRedirecting(true)
    setError("")
    try {
      window.location.href = (await open()).url
    } catch (e) {
      setError(formatError(e, failure))
      setRedirecting(false)
    }
  }

  return { redirecting, error, go, reset: () => setError("") }
}

export type Redirect = ReturnType<typeof useRedirect>
