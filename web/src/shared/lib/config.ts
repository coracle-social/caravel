import type { RelayProfile } from "@/shared/lib/api"

// The configuration the server writes into each app's page, or under `vite dev`, VITE_* variables.
type Injected = {
  api_url?: string
  tenant_url?: string
  admin_url?: string
  portal_path?: string
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

// Each section's url. Under `vite dev`, where no server writes the configuration
// into the page, they sit under this page's origin as the dev server routes them.
export const API_URL = injected.api_url ?? `${location.origin}/api`

export const TENANT_URL = injected.tenant_url ?? location.origin

export const ADMIN_URL = injected.admin_url ?? `${location.origin}/admin`

const pathOf = (url: string) => new URL(url).pathname.replace(/\/$/, "")

// The paths the dashboards are served under, which their routers are based at.
export const TENANT_BASE = pathOf(TENANT_URL)

export const ADMIN_BASE = pathOf(ADMIN_URL)

// The path each relay's member portal is under, on the relay's own hosts.
export const PORTAL_PATH = injected.portal_path ?? "/members"

// A page's path within the app based at `base`.
export const pathWithin = (base: string, pathname: string) => pathname.slice(base.length) || "/"

// The domain new relays are created under: the first the platform serves.
export const RELAY_DOMAIN = injected.relay_domains?.[0] ?? import.meta.env.VITE_RELAY_DOMAIN

export const PLATFORM_NAME = injected.platform_name ?? import.meta.env.VITE_PLATFORM_NAME ?? "Caravel"

export const PLATFORM_LOGO = injected.platform_logo ?? import.meta.env.VITE_PLATFORM_LOGO ?? "/caravel.png"

export const FLOTILLA_URL = injected.flotilla_url ?? import.meta.env.VITE_FLOTILLA_URL ?? "https://app.flotilla.social"

// The relay a member portal is served for.
export const PORTAL_RELAY = injected.relay
