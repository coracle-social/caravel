import type { RelayProfile } from "@/shared/lib/api"

// The configuration the server writes into each app's page, or under `vite dev`, VITE_* variables.
type Injected = {
  api_url?: string
  relay_domains?: string[]
  platform_name?: string
  platform_logo?: string
  flotilla_url?: string
  relay?: RelayProfile
}

function readInjected(): Injected {
  try {
    return JSON.parse(document.getElementById("config")?.textContent ?? "{}") as Injected
  } catch {
    return {}
  }
}

const injected = readInjected()

// The API's base url: `/api` on the platform's host, which is this page's origin
// under `vite dev`, where no server writes the configuration into the page.
export const API_URL = injected.api_url ?? `${location.origin}/api`

// The domain new relays are created under: the first the platform serves.
export const RELAY_DOMAIN = injected.relay_domains?.[0] ?? import.meta.env.VITE_RELAY_DOMAIN

export const PLATFORM_NAME = injected.platform_name ?? import.meta.env.VITE_PLATFORM_NAME ?? "Caravel"

export const PLATFORM_LOGO = injected.platform_logo ?? import.meta.env.VITE_PLATFORM_LOGO ?? "/caravel.png"

export const FLOTILLA_URL = injected.flotilla_url ?? import.meta.env.VITE_FLOTILLA_URL ?? "https://app.flotilla.social"

// The relay a member portal is served for.
export const PORTAL_RELAY = injected.relay
