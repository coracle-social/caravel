import { createEffect, Show } from "solid-js"
import { useLocation, useNavigate } from "@solidjs/router"
import type { Component } from "solid-js"
import AppShell from "@/shared/components/AppShell"
import Toast from "@/shared/components/Toast"
import { identity } from "@/shared/lib/state"

// The frame the tenant and admin dashboards share: their navigation once signed
// in, and the toast.
export default function Layout(props: { children?: any }) {
  const location = useLocation()

  const usesAppShell = () => location.pathname.match(/^\/(relays|account|admin)/)

  createEffect(() => {
    // Reinitialize Preline components on route change
    location.pathname
    window.HSStaticMethods?.autoInit()
  })

  return (
    <div class="min-h-screen bg-gray-50">
      <Show when={!identity.loading && identity() && usesAppShell()} fallback={<main>{props.children}</main>}>
        <AppShell>{props.children}</AppShell>
      </Show>
      <Toast />
    </div>
  )
}

// A page shown only while `condition` holds, sending everyone else to the tenant
// dashboard's home page.
export function requireCondition(Page: Component, condition: () => boolean): Component {
  return () => {
    const navigate = useNavigate()
    const location = useLocation()

    createEffect(() => {
      if (identity.loading || condition()) return
      // The admin dashboard is another app, so leaving it is a page load.
      if (location.pathname.startsWith("/admin")) window.location.replace("/")
      else navigate("/", { replace: true })
    })

    return (
      <Show when={!identity.loading && condition()}>
        <Page />
      </Show>
    )
  }
}

export const requireAdmin = (Page: Component) => requireCondition(Page, () => Boolean(identity()?.is_admin))

export const requireTenant = (Page: Component) => requireCondition(Page, () => Boolean(identity()))
