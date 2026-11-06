import type { RelayProfile } from "@/shared/lib/api"
import { PORTAL_PATH, PORTAL_RELAY } from "@/shared/lib/config"

// The relay this portal is served for, or under `vite dev`, the one `?relay=` names.
export function portalRelay(): RelayProfile | undefined {
  if (PORTAL_RELAY) return PORTAL_RELAY
  if (!import.meta.env.DEV) return undefined

  const id = new URLSearchParams(location.search).get("relay") ?? sessionStorage.getItem("portal.relay")
  if (!id) return undefined
  sessionStorage.setItem("portal.relay", id)
  return { relay_id: id, host: id, name: "", icon: "", description: "" }
}

// The portal's own path for a page, which sits under /portal under `vite dev`.
export const PORTAL_BASE = import.meta.env.DEV ? "/portal" : PORTAL_PATH
