import { createSignal } from "solid-js"
import { updateActiveTenant } from "@/lib/hooks"
import { createPortalSession } from "@/lib/api"
import { account } from "@/lib/state"

// Lightning/NWC save state machine, shared by the combined and focused setup
// dialogs. `onSaved` fires once the wallet URL is persisted.
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
      await updateActiveTenant({ nwc_url: url })
      setSaved(true)
      onSaved?.()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to save wallet connection")
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

// Card setup is a full-page redirect to the Stripe billing portal (which returns
// to wherever it was opened from), so there's no local "saved" state — only the
// in-flight redirect and any failure to open the portal.
export function useCardPortal() {
  const [redirecting, setRedirecting] = createSignal(false)
  const [error, setError] = createSignal("")

  async function openPortal() {
    setRedirecting(true)
    setError("")
    try {
      const { url } = await createPortalSession(account()!.pubkey, window.location.href)
      window.location.href = url
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to open billing portal")
      setRedirecting(false)
    }
  }

  function reset() {
    setError("")
  }

  return { redirecting, error, openPortal, reset }
}

export type CardPortal = ReturnType<typeof useCardPortal>
