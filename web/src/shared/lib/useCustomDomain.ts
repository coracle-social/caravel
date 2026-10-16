import { createSignal } from "solid-js"
import type { Relay } from "@/shared/lib/api"
import { getRelay, updateRelay } from "@/shared/lib/api"

export default function useCustomDomain(relayId: () => string, mutate: (relay: Relay) => void) {
  const [saving, setSaving] = createSignal(false)
  const [verifying, setVerifying] = createSignal(false)
  const [error, setError] = createSignal<string>()

  async function saveDomain(domain: string) {
    setSaving(true)
    setError(undefined)
    try {
      const updated = await updateRelay(relayId(), { custom_domain: domain })
      mutate(updated)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to save domain")
    } finally {
      setSaving(false)
    }
  }

  // Verification runs in a background poller on the backend; reload the relay so
  // the UI picks up the latest custom_domain_verified state.
  async function verifyDomain() {
    setVerifying(true)
    setError(undefined)
    try {
      const updated = await getRelay(relayId())
      mutate(updated)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Verification failed")
    } finally {
      setVerifying(false)
    }
  }

  return { saving, verifying, error, saveDomain, verifyDomain }
}
