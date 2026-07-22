import { A, useLocation } from "@solidjs/router"
import { createMemo, createSignal, For, Show } from "solid-js"
import { useProfileMetadata, useProfilePicture, useTenantRelays, type Relay } from "@/lib/hooks"
import { account, identity, PLATFORM_LOGO, PLATFORM_NAME } from "@/lib/state"
import { fuzzySearch } from "@/lib/search"
import serverIcon from "@/assets/server.svg"
import Modal from "@/components/Modal"
import BillingPrompts from "@/components/BillingPrompts"
import CopyNpub from "@/components/CopyNpub"
import { shortenNpub } from "@/lib/pubkey"

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

function AdminIcon() {
  return (
    <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
    </svg>
  )
}

export default function AppShell(props: { children?: any }) {
  const location = useLocation()
  const picture = useProfilePicture(() => account()?.pubkey)
  const metadata = useProfileMetadata(() => account()?.pubkey)
  const [tenantRelays] = useTenantRelays()
  const [searchOpen, setSearchOpen] = createSignal(false)
  const [searchQuery, setSearchQuery] = createSignal("")
  const [adminOpen, setAdminOpen] = createSignal(false)

  const displayName = createMemo(() => metadata()?.name || metadata()?.display_name)
  const username = createMemo(() => {
    const pubkey = account()?.pubkey
    return displayName() || (pubkey ? shortenNpub(pubkey) : "")
  })
  const initial = createMemo(() => (displayName() || account()?.pubkey || "?").slice(0, 1).toUpperCase())
  const nip05 = createMemo(() => metadata()?.nip05)
  const searchedRelays = createMemo<Relay[]>(() => {
    const list = tenantRelays() ?? []
    const query = searchQuery().trim()

    return fuzzySearch(list, ["info_name", "subdomain"], query)
  })

  const myResources = [{ href: "/relays", label: "My Relays" }]
  const adminResources = [
    { href: "/admin/tenants", label: "Tenants" },
    { href: "/admin/relays", label: "Relays" },
    { href: "/admin/invoices", label: "Invoices" },
    { href: "/admin/moderation", label: "Moderation" },
  ]

  const navItemClass = (href: string) => {
    const active = location.pathname === href || location.pathname.startsWith(`${href}/`)
    return active
      ? "block rounded-lg bg-white/15 px-3 py-2 text-sm font-medium text-white"
      : "block rounded-lg px-3 py-2 text-sm text-white/80 hover:bg-white/10 hover:text-white"
  }

  const mobileNavItemClass = (href: string) => {
    const active = location.pathname === href || location.pathname.startsWith(`${href}/`)
    return active
      ? "block rounded-lg bg-gray-100 px-3 py-3 text-sm font-medium text-gray-900"
      : "block rounded-lg px-3 py-3 text-sm text-gray-700 hover:bg-gray-100"
  }

  const openSearchModal = () => setSearchOpen(true)
  const closeSearchModal = () => {
    setSearchOpen(false)
    setSearchQuery("")
  }
  const openAdminModal = () => setAdminOpen(true)
  const closeAdminModal = () => setAdminOpen(false)

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
          </ul>

          <Show when={identity()?.is_admin}>
            <div class="mt-8">
              <h2 class="px-3 text-xs font-semibold uppercase tracking-wider text-white/60">Administration</h2>
              <ul class="mt-2 space-y-1">
                <For each={adminResources}>{(item) => <li><A href={item.href} class={navItemClass(item.href)}>{item.label}</A></li>}</For>
              </ul>
            </div>
          </Show>
        </div>

        <div class="border-t border-white/15 p-4">
          <A href="/account" class="flex items-center gap-3 rounded-lg p-2 hover:bg-white/10">
            <Show
              when={picture()}
              fallback={
                <div class="h-10 w-10 rounded-full bg-white/15 text-sm font-medium text-white flex items-center justify-center">
                  {initial()}
                </div>
              }
            >
              <img src={picture()} alt="Profile" class="h-10 w-10 rounded-full object-cover" />
            </Show>
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
        <BillingPrompts variant="banner" />
        <main>{props.children}</main>
      </div>

      <nav
        class="fixed inset-x-0 bottom-0 z-20 border-t border-gray-200 bg-white md:hidden"
        onClick={() => {
          if (searchOpen()) closeSearchModal()
          if (adminOpen()) closeAdminModal()
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
            <Show when={identity()?.is_admin}>
              <button
                type="button"
                aria-label="Administration"
                class="rounded-lg p-2 text-gray-700 hover:bg-gray-100"
                onClick={(e) => {
                  e.stopPropagation()
                  openAdminModal()
                }}
              >
                <AdminIcon />
              </button>
            </Show>
          </div>
          <A href="/account" aria-label="Account settings" class="rounded-full">
            <Show
              when={picture()}
              fallback={<div class="h-9 w-9 rounded-full bg-gray-200 text-gray-600 flex items-center justify-center text-sm font-medium">{initial()}</div>}
            >
              <img src={picture()} alt="Profile" class="h-9 w-9 rounded-full object-cover" />
            </Show>
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
                        <p class="text-sm font-medium text-gray-900">{relay.info_name || relay.subdomain}</p>
                        <p class="text-xs text-gray-500">{relay.subdomain}.{relay.zooid_domain}</p>
                      </A>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </div>
      </Modal>

      <Modal
        open={adminOpen()}
        onClose={closeAdminModal}
        wrapperClass="fixed inset-x-0 top-0 bottom-16 z-30 flex items-end bg-black/40 p-0 md:hidden"
        panelClass="w-full overflow-hidden rounded-t-2xl bg-white"
      >
        <div class="flex items-center justify-between border-b border-gray-200 p-4">
          <h2 class="text-xs font-semibold uppercase tracking-wider text-gray-500">Administration</h2>
          <button
            type="button"
            class="rounded-lg px-3 py-2 text-sm text-gray-600 hover:bg-gray-100"
            onClick={closeAdminModal}
          >
            Close
          </button>
        </div>
        <ul class="space-y-1 p-4">
          <For each={adminResources}>
            {(item) => (
              <li>
                <A href={item.href} class={mobileNavItemClass(item.href)} onClick={closeAdminModal}>
                  {item.label}
                </A>
              </li>
            )}
          </For>
        </ul>
      </Modal>
    </div>
  )
}
