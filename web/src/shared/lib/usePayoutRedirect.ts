import { createSignal } from "solid-js"
import { openPayoutDashboard, openPayoutOnboarding } from "@/shared/lib/api"
import { account } from "@/shared/lib/state"

// Payout setup and the payout dashboard are full-page redirects to Stripe, so the
// only local state is the redirect in flight and any failure to start it.
export function usePayoutRedirect() {
  const [redirecting, setRedirecting] = createSignal(false)
  const [error, setError] = createSignal("")

  async function redirect(open: (pubkey: string) => Promise<{ url: string }>) {
    setRedirecting(true)
    setError("")
    try {
      const { url } = await open(account()!.pubkey)
      window.location.href = url
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to open Stripe")
      setRedirecting(false)
    }
  }

  return {
    redirecting,
    error,
    openSetup: () => redirect(pubkey => openPayoutOnboarding(pubkey, window.location.href)),
    openDashboard: () => redirect(openPayoutDashboard),
  }
}
