import { A, useLocation } from "@solidjs/router"
import { createMemo, For, type JSX } from "solid-js"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import Avatar from "@/shared/components/Avatar"
import { account } from "@/shared/lib/account"
import { signOut } from "@/shared/lib/api"
import { PLATFORM_LOGO, PLATFORM_NAME } from "@/shared/lib/config"
import { profileName, useProfileMetadata } from "@/shared/lib/profiles"
import { shortenNpub } from "@/shared/lib/pubkey"
import { ADMIN_BASE, ADMIN_URL, pathWithin } from "@/shared/lib/config"

const SECTIONS = [
  { href: "/tenants", label: "Tenants" },
  { href: "/relays", label: "Relays" },
  { href: "/invoices", label: "Invoices" },
  { href: "/moderation", label: "Moderation" },
]

// End the admin's session and return to the admin dashboard's sign-in.
export async function signOutOfAdmin() {
  await signOut()
  localStorage.clear()
  window.location.href = ADMIN_URL
}

// The admin dashboard's frame: a dark bar across the top with its sections and the
// signed-in admin's menu, which wraps its sections into a second row on phones.
export default function AdminShell(props: { children: JSX.Element }) {
  const location = useLocation()
  const metadata = useProfileMetadata(() => account()?.pubkey)
  const name = createMemo(() => profileName(metadata()) || shortenNpub(account()?.pubkey ?? ""))

  const sectionClass = (href: string) =>
    pathWithin(ADMIN_BASE, location.pathname) === href || pathWithin(ADMIN_BASE, location.pathname).startsWith(`${href}/`)
      ? "shrink-0 rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-medium text-white"
      : "shrink-0 rounded-md px-3 py-1.5 text-sm text-zinc-400 hover:bg-zinc-900 hover:text-white"

  const sections = () => <For each={SECTIONS}>{(section) => <A href={section.href} class={sectionClass(section.href)}>{section.label}</A>}</For>

  return (
    <div class="min-h-screen bg-zinc-100">
      <header class="sticky top-0 z-20 bg-zinc-950 text-zinc-100 shadow">
        <div class="mx-auto flex h-14 max-w-4xl items-center gap-6 px-4">
          <A href="/tenants" class="flex shrink-0 items-center gap-2 font-semibold">
            <img src={PLATFORM_LOGO} alt="" class="h-6 w-6 rounded" />
            <span class="hidden sm:inline">{PLATFORM_NAME}</span>
            <span class="rounded bg-amber-400 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-zinc-950">Admin</span>
          </A>
          <nav class="hidden items-center gap-1 md:flex">{sections()}</nav>
          <div class="ml-auto min-w-0">
            <ActionsMenu
              label="Account"
              triggerClass="flex min-w-0 items-center gap-2 rounded-lg px-2 py-1 hover:bg-zinc-900"
              trigger={
                <>
                  <Avatar profile={metadata()} pubkey={account()?.pubkey ?? "?"} alt="" class="h-7 w-7 shrink-0" fallbackClass="bg-zinc-700 text-xs font-medium text-zinc-100" />
                  <span class="hidden max-w-[12rem] truncate text-sm text-zinc-300 sm:block">{name()}</span>
                  <svg class="h-4 w-4 shrink-0 text-zinc-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M6 9l6 6 6-6" />
                  </svg>
                </>
              }
            >
              {(close) => (
                <ActionsMenuItem
                  onClick={() => {
                    close()
                    void signOutOfAdmin()
                  }}
                >
                  Sign out
                </ActionsMenuItem>
              )}
            </ActionsMenu>
          </div>
        </div>
        <nav class="mx-auto flex max-w-4xl gap-1 overflow-x-auto px-4 pb-2 md:hidden">{sections()}</nav>
      </header>
      <main>{props.children}</main>
    </div>
  )
}
