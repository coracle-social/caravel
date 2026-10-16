import { createSignal } from "solid-js"
import { getRelay, updateRelay, type Relay } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"

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
      setError(formatError(e, "Failed to save domain"))
    } finally {
      setSaving(false)
    }
  }

  // The server verifies domains in the background, so this rereads the relay.
  async function verifyDomain() {
    setVerifying(true)
    setError(undefined)
    try {
      const updated = await getRelay(relayId())
      mutate(updated)
    } catch (e: unknown) {
      setError(formatError(e, "Verification failed"))
    } finally {
      setVerifying(false)
    }
  }

  return { saving, verifying, error, saveDomain, verifyDomain }
}
