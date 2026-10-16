// The configuration the server writes into each app's page, or under `vite dev`,
// where there is no server page, the build's VITE_* variables.

type Injected = {
  api_url?: string
  relay_domains?: string[]
  platform_name?: string
  platform_logo?: string
  flotilla_url?: string
  relay?: PortalRelay
}

// The relay a member portal is served for.
export type PortalRelay = {
  id: string
  host: string
  name: string
  icon: string
  description: string
}

function readInjected(): Injected {
  try {
    return JSON.parse(document.getElementById("config")?.textContent ?? "{}") as Injected
  } catch {
    return {}
  }
}

const injected = readInjected()

export const API_URL = injected.api_url ?? import.meta.env.VITE_API_URL

// The domain new relays are created under: the first the platform serves.
export const RELAY_DOMAIN = injected.relay_domains?.[0] ?? import.meta.env.VITE_RELAY_DOMAIN

export const PLATFORM_NAME = injected.platform_name ?? import.meta.env.VITE_PLATFORM_NAME ?? "Caravel"

export const PLATFORM_LOGO = injected.platform_logo ?? import.meta.env.VITE_PLATFORM_LOGO ?? "/caravel.png"

export const FLOTILLA_URL = injected.flotilla_url ?? import.meta.env.VITE_FLOTILLA_URL ?? "https://app.flotilla.social"

export const PORTAL_RELAY = injected.relay
