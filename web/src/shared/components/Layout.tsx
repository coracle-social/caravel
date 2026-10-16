import { createEffect, Show } from "solid-js"
import { useLocation, useNavigate } from "@solidjs/router"
import type { Component } from "solid-js"
import AppShell from "@/shared/components/AppShell"
import { isAdminPath } from "@/shared/components/AppLink"
import Toast from "@/shared/components/Toast"
import { identity } from "@/shared/lib/state"

// The frame both dashboards share: navigation once signed in, and the toast.
export default function Layout(props: { children?: any }) {
  const location = useLocation()

  const usesAppShell = () => location.pathname.match(/^\/(relays|account|admin)/)

  return (
    <div class="min-h-screen bg-gray-50">
      <Show when={!identity.loading && identity() && usesAppShell()} fallback={<main>{props.children}</main>}>
        <AppShell>{props.children}</AppShell>
      </Show>
      <Toast />
    </div>
  )
}

// A page shown only while `condition` holds, sending everyone else to the home page.
function requireCondition(Page: Component, condition: () => boolean): Component {
  return () => {
    const navigate = useNavigate()
    const location = useLocation()

    createEffect(() => {
      if (identity.loading || condition()) return
      // The admin dashboard is another app, so leaving it is a page load.
      if (isAdminPath(location.pathname)) window.location.replace("/")
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
