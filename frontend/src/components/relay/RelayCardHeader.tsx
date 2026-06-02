import { A } from "@solidjs/router"
import { Show, createEffect, createSignal, onCleanup } from "solid-js"
import { getProfilePicture, type ProfileContent } from "applesauce-core/helpers/profile"
import type { Relay } from "@/lib/api"
import menuDotsIcon from "@/assets/menu-dots-2.svg"
import { shortenPubkey } from "@/lib/pubkey"
import { RELAY_DOMAIN } from "@/lib/subdomain"

const STATUS_STYLES: Record<string, string> = {
  active:   "bg-green-50 text-green-700 border-green-200",
  inactive: "bg-gray-100 text-gray-500 border-gray-200",
}

export function StatusBadge(props: { status: string }) {
  const styles = () => STATUS_STYLES[props.status] ?? "bg-gray-100 text-gray-500 border-gray-200"
  const label = () => props.status.replace(/_/g, " ")
  return (
    <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${styles()}`}>
      {label()}
    </span>
  )
}

// Presentational header for RelayDetailCard: icon/title/status, the WSS link, the
// optional tenant profile link, the sync_error banner, and the actions dropdown.
// The dropdown owns its own open/close UI state and document listeners; all relay
// mutations are surfaced as callbacks. Props are reactive only when read lazily,
// so access props.* (and accessor props like props.relay()) inside JSX, never
// destructure at the top.
type RelayCardHeaderProps = {
  relay: () => Relay
  showTenant?: boolean
  tenantProfile: () => ProfileContent | undefined
  editHref?: string
  deactivating?: boolean
  reactivating?: boolean
  onRequestDeactivate?: () => void
  onRequestReactivate?: () => void
}

export default function RelayCardHeader(props: RelayCardHeaderProps) {
  const r = () => props.relay()
  const metadata = () => props.tenantProfile()
  const [menuOpen, setMenuOpen] = createSignal(false)

  let menuContainerRef: HTMLDivElement | undefined

  createEffect(() => {
    if (!menuOpen()) return

    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node | null
      if (target && !menuContainerRef?.contains(target)) {
        setMenuOpen(false)
      }
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuOpen(false)
      }
    }

    document.addEventListener("mousedown", handleClickOutside)
    document.addEventListener("keydown", handleEscape)

    onCleanup(() => {
      document.removeEventListener("mousedown", handleClickOutside)
      document.removeEventListener("keydown", handleEscape)
    })
  })

  return (
    <>
      {/* Header */}
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-start gap-4 min-w-0">
          <Show when={r().info_icon}>
            <img src={r().info_icon} alt="" class="w-14 h-14 rounded-xl object-cover shrink-0 border border-gray-200" />
          </Show>
          <div class="min-w-0">
            <div class="flex items-center gap-3 flex-wrap">
              <h1 class="text-2xl font-bold text-gray-900">{r().info_name || r().subdomain}</h1>
              <StatusBadge status={r().status} />
            </div>
            <a
              href={`wss://${r().subdomain}.${RELAY_DOMAIN}`}
              class="text-sm text-blue-600 hover:underline break-all"
            >
              wss://{r().subdomain}.{RELAY_DOMAIN}
            </a>
            <Show when={props.showTenant}>
              <A href={`/admin/tenants/${r().tenant_pubkey}`} class="group mt-1.5 flex w-fit items-center gap-2 min-w-0">
                <Show
                  when={getProfilePicture(metadata())}
                  fallback={
                    <div class="h-6 w-6 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-[10px] text-gray-500">
                      {((metadata()?.name || metadata()?.display_name) || r().tenant_pubkey).slice(0, 1).toUpperCase()}
                    </div>
                  }
                >
                  <img src={getProfilePicture(metadata())} alt="" class="h-6 w-6 flex-shrink-0 rounded-full object-cover" />
                </Show>
                <span class="text-sm text-blue-600 group-hover:underline truncate">{(metadata()?.name || metadata()?.display_name) || shortenPubkey(r().tenant_pubkey)}</span>
              </A>
            </Show>
            <Show when={r().info_description.trim()}>
              <p class="mt-2 text-sm text-gray-600">{r().info_description}</p>
            </Show>
          </div>
        </div>

        <Show when={props.editHref && (props.onRequestDeactivate || props.onRequestReactivate)}>
          <div class="relative shrink-0" ref={menuContainerRef}>
            <button
              type="button"
              class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-gray-200 hover:bg-gray-50"
              aria-label="Open relay actions"
              onClick={() => setMenuOpen((open) => !open)}
            >
              <img src={menuDotsIcon} alt="" class="h-5 w-5" />
            </button>

            <div
              class="absolute right-0 mt-2 w-44 rounded-lg border border-gray-200 bg-white shadow-lg z-10 py-1 origin-top-right transition-all duration-150 ease-out"
              classList={{
                "opacity-100 scale-100": menuOpen(),
                "opacity-0 scale-95 pointer-events-none": !menuOpen(),
              }}
            >
              <A
                href={props.editHref!}
                class="block px-3 py-2 text-sm text-gray-700 hover:bg-gray-50"
                onClick={() => setMenuOpen(false)}
              >
                Edit Details
              </A>
              <Show when={r().status === "active" && props.onRequestDeactivate}>
                <button
                  type="button"
                  class="block w-full text-left px-3 py-2 text-sm text-red-600 hover:bg-red-50 disabled:opacity-50"
                  onClick={() => {
                    setMenuOpen(false)
                    props.onRequestDeactivate?.()
                  }}
                  disabled={props.deactivating}
                >
                  {props.deactivating ? "Deactivating..." : "Deactivate"}
                </button>
              </Show>
              <Show when={r().status === "inactive" && props.onRequestReactivate}>
                <button
                  type="button"
                  class="block w-full text-left px-3 py-2 text-sm text-blue-600 hover:bg-blue-50 disabled:opacity-50"
                  onClick={() => {
                    setMenuOpen(false)
                    props.onRequestReactivate?.()
                  }}
                  disabled={props.reactivating}
                >
                  {props.reactivating ? "Reactivating..." : "Reactivate"}
                </button>
              </Show>
            </div>
          </div>
        </Show>
      </div>

      <Show when={r().sync_error}>
        <div class="rounded-lg border border-red-200 bg-red-50 px-4 py-3">
          <p class="text-sm font-semibold text-red-800">Provisioning error</p>
          <p class="mt-1 text-sm text-red-700 font-mono break-all">{r().sync_error}</p>
        </div>
      </Show>
    </>
  )
}
