import { createSignal } from "solid-js"
import { account } from "@/shared/lib/account"
import { updateTenant } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"

// Saving a Nostr Wallet Connect url for the signed-in tenant. `onSaved` runs once it is stored.
export function useNwcSetup(onSaved?: () => void) {
  const [nwcUrl, setNwcUrl] = createSignal("")
  const [saving, setSaving] = createSignal(false)
  const [saved, setSaved] = createSignal(false)
  const [error, setError] = createSignal("")

  async function save() {
    const url = nwcUrl().trim()
    if (!url) return
    setSaving(true)
    setError("")
    try {
      await updateTenant(account()!.pubkey, { nwc_url: url })
      setSaved(true)
      onSaved?.()
    } catch (e) {
      setError(formatError(e, "Failed to save wallet connection"))
    } finally {
      setSaving(false)
    }
  }

  function reset() {
    setNwcUrl("")
    setSaved(false)
    setError("")
  }

  return { nwcUrl, setNwcUrl, saving, saved, error, save, reset }
}

export type NwcSetup = ReturnType<typeof useNwcSetup>
