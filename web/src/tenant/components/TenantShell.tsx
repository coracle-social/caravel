import { A, useLocation } from "@solidjs/router"
import { createMemo, createSignal, For, Show, type JSX } from "solid-js"
import { account } from "@/shared/lib/account"
import { canonicalHost, relayName, type Relay } from "@/shared/lib/api"
import { PLATFORM_LOGO, PLATFORM_NAME } from "@/shared/lib/config"
import { profileName, useProfileMetadata } from "@/shared/lib/profiles"
import { useTenantRelays } from "@/shared/lib/relays"
import { identity } from "@/shared/lib/state"
import { fuzzySearch } from "@/shared/lib/search"
import serverIcon from "@/shared/assets/server.svg"
import Avatar from "@/shared/components/Avatar"
import Modal from "@/shared/components/Modal"
import BillingPrompts from "@/shared/components/BillingPrompts"
import CopyNpub from "@/shared/components/CopyNpub"
import { shortenNpub } from "@/shared/lib/pubkey"

function SearchIcon() {
  return (
    <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="11" cy="11" r="8" />
      <path d="M21 21l-4.3-4.3" />
    </svg>
  )
}

function RelayIcon() {
  return <img src={serverIcon} alt="" aria-hidden="true" class="h-5 w-5" />
}

// The tenant dashboard's frame once signed in: a dark sidebar with their relays and
// account, a bar along the bottom on phones, and the billing banner.
export default function TenantShell(props: { children: JSX.Element }) {
  const location = useLocation()
  const metadata = useProfileMetadata(() => account()?.pubkey)
  const [tenantRelays] = useTenantRelays()
  const [searchOpen, setSearchOpen] = createSignal(false)
  const [searchQuery, setSearchQuery] = createSignal("")

  const displayName = createMemo(() => profileName(metadata()))
  const username = createMemo(() => {
    const pubkey = account()?.pubkey
    return displayName() || (pubkey ? shortenNpub(pubkey) : "")
  })
  const nip05 = createMemo(() => metadata()?.nip05)
  const searchedRelays = createMemo<Relay[]>(() => {
    const list = tenantRelays() ?? []
    const query = searchQuery().trim()

    return fuzzySearch(list, ["info_name", "subdomain"], query)
  })

  const myResources = [{ href: "/relays", label: "My Relays" }]

  const navItemClass = (href: string) => {
    const active = location.pathname === href || location.pathname.startsWith(`${href}/`)
    return active
      ? "block rounded-lg bg-white/15 px-3 py-2 text-sm font-medium text-white"
      : "block rounded-lg px-3 py-2 text-sm text-white/80 hover:bg-white/10 hover:text-white"
  }

  const openSearchModal = () => setSearchOpen(true)
  const closeSearchModal = () => {
    setSearchOpen(false)
    setSearchQuery("")
  }

  return (
    <div class="min-h-screen bg-gray-50">
      <aside class="hidden md:flex fixed inset-y-0 left-0 w-[260px] bg-slate-900 text-white flex-col z-10">
        <A href="/" class="flex items-center gap-3 border-b border-white/15 px-7 py-5 text-lg font-bold text-white hover:bg-white/10">
          <img src={PLATFORM_LOGO} alt={PLATFORM_NAME} class="h-7 w-7 rounded" />
          <span class="truncate">{PLATFORM_NAME}</span>
        </A>
        <div class="flex-1 px-4 py-6">
          <h2 class="px-3 text-xs font-semibold uppercase tracking-wider text-white/60">Resources</h2>
          <ul class="mt-2 space-y-1">
            <For each={myResources}>{(item) => <li><A href={item.href} class={navItemClass(item.href)}>{item.label}</A></li>}</For>
            <Show when={identity()?.is_admin}>
              {/* The admin dashboard is another app, so the router mustn't take this link. */}
              <li><a href="/admin" rel="external" class={navItemClass("/admin")}>Administration</a></li>
            </Show>
          </ul>
        </div>

        <div class="border-t border-white/15 p-4">
          <A href="/account" class="flex items-center gap-3 rounded-lg p-2 hover:bg-white/10">
            <Avatar profile={metadata()} pubkey={account()?.pubkey ?? "?"} alt="Profile" class="h-10 w-10" fallbackClass="bg-white/15 text-sm font-medium text-white" />
            <div class="min-w-0">
              <Show
                when={displayName()}
                fallback={<CopyNpub pubkey={account()?.pubkey ?? ""} class="text-sm font-medium text-white" />}
              >
                <p class="truncate text-sm font-medium text-white">{username()}</p>
              </Show>
              <p class="truncate text-xs text-white/60">{nip05()}</p>
            </div>
          </A>
        </div>
      </aside>

      <div class="md:pl-[260px] min-h-screen pb-20 md:pb-0">
        <BillingPrompts />
        <main>{props.children}</main>
      </div>

      <nav
        class="fixed inset-x-0 bottom-0 z-20 border-t border-gray-200 bg-white md:hidden"
        onClick={() => {
          if (searchOpen()) closeSearchModal()
        }}
      >
        <div class="flex h-16 items-center justify-between px-6">
          <div class="flex items-center gap-3">
            <button
              type="button"
              aria-label="Search"
              class="rounded-lg p-2 text-gray-700 hover:bg-gray-100"
              onClick={(e) => {
                e.stopPropagation()
                openSearchModal()
              }}
            >
              <SearchIcon />
            </button>
            <A href="/relays" aria-label="Relays" class="rounded-lg p-2 text-gray-700 hover:bg-gray-100">
              <RelayIcon />
            </A>
          </div>
          <A href="/account" aria-label="Account settings" class="rounded-full">
            <Avatar profile={metadata()} pubkey={account()?.pubkey ?? "?"} alt="Profile" class="h-9 w-9" fallbackClass="bg-gray-200 text-gray-600 text-sm font-medium" />
          </A>
        </div>
      </nav>

      <Modal
        open={searchOpen()}
        onClose={closeSearchModal}
        wrapperClass="fixed inset-x-0 top-0 bottom-16 z-30 flex items-end bg-black/40 p-0 md:hidden"
        panelClass="min-h-[40vh] max-h-[80vh] w-full overflow-hidden rounded-t-2xl bg-white"
      >
        <div class="border-b border-gray-200 p-4">
          <div class="flex items-center gap-2">
            <div class="relative flex-1">
              <span class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-gray-400">
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <circle cx="11" cy="11" r="8" />
                  <path d="M21 21l-4.3-4.3" />
                </svg>
              </span>
              <input
                type="search"
                value={searchQuery()}
                onInput={(e) => setSearchQuery(e.currentTarget.value)}
                placeholder="Search your relays"
                class="w-full rounded-lg border border-gray-300 py-2 pl-10 pr-3"
                autofocus
              />
            </div>
            <button
              type="button"
              class="rounded-lg px-3 py-2 text-sm text-gray-600 hover:bg-gray-100"
              onClick={closeSearchModal}
            >
              Close
            </button>
          </div>
        </div>

        <div class="max-h-[calc(80vh-73px)] overflow-y-auto p-4">
          <Show when={!tenantRelays.loading} fallback={<p class="text-sm text-gray-500">Loading relays...</p>}>
            <Show when={(searchedRelays().length ?? 0) > 0} fallback={<p class="text-sm text-gray-500">No relays found.</p>}>
              <ul class="space-y-2">
                <For each={searchedRelays()}>
                  {(relay) => (
                    <li>
                      <A
                        href={`/relays/${relay.id}`}
                        class="block rounded-lg border border-gray-200 bg-white p-3"
                        onClick={closeSearchModal}
                      >
                        <p class="text-sm font-medium text-gray-900">{relayName(relay)}</p>
                        <p class="text-xs text-gray-500">{canonicalHost(relay)}</p>
                      </A>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </div>
      </Modal>

    </div>
  )
}
