import { PORTAL_RELAY, type PortalRelay } from "@/shared/lib/config"

// The relay this portal is served for, from the page the server wrote. Under
// `vite dev` there is no such page, so a `?relay=` id stands in for it.
export function portalRelay(): PortalRelay | undefined {
  if (PORTAL_RELAY) return PORTAL_RELAY
  if (!import.meta.env.DEV) return undefined

  const id = new URLSearchParams(location.search).get("relay") ?? sessionStorage.getItem("portal.relay")
  if (!id) return undefined
  sessionStorage.setItem("portal.relay", id)
  return { id, host: id, name: "", icon: "", description: "" }
}

// The portal's own path for a page, which sits under /portal under `vite dev`.
export const PORTAL_BASE = import.meta.env.DEV ? "/portal" : ""
