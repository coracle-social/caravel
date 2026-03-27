import { createEffect, Show } from "solid-js"
import { Router, Route, useLocation, useNavigate } from "@solidjs/router"
import type { Component } from "solid-js"
import AppShell from "./components/AppShell"
import Home from "./pages/Home"
import RelayList from "./pages/relays/RelayList"
import RelayNew from "./pages/relays/RelayNew"
import RelayDetail from "./pages/relays/RelayDetail"
import RelayEdit from "./pages/relays/RelayEdit"
import Account from "./pages/Account"
import AdminTenantList from "./pages/admin/AdminTenantList"
import AdminTenantDetail from "./pages/admin/AdminTenantDetail"
import AdminRelayList from "./pages/admin/AdminRelayList"
import AdminRelayDetail from "./pages/admin/AdminRelayDetail"
import AdminRelayEdit from "./pages/admin/AdminRelayEdit"
import { identity } from "./lib/hooks"

function Layout(props: { children?: any }) {
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
    </div>
  )
}

export default function App() {
  const requireCondition = (Page: Component, condition: () => boolean): Component => {
    return () => {
      const navigate = useNavigate()

      createEffect(() => {
        if (!identity.loading && !condition()) navigate("/", { replace: true })
      })

      return (
        <Show when={!identity.loading && condition()}>
          <Page />
        </Show>
      )
    }
  }

  const requireAdmin = (Page: Component) => requireCondition(Page, () => Boolean(identity()?.is_admin))
  const requireTenant = (Page: Component) => requireCondition(Page, () => Boolean(identity()))

  return (
    <Router root={Layout}>
      <Route path="/" component={Home} />
      <Route path="/relays" component={requireTenant(RelayList)} />
      <Route path="/relays/new" component={requireTenant(RelayNew)} />
      <Route path="/relays/:id" component={requireTenant(RelayDetail)} />
      <Route path="/relays/:id/edit" component={requireTenant(RelayEdit)} />
      <Route path="/account" component={requireTenant(Account)} />
      <Route path="/admin/tenants" component={requireAdmin(AdminTenantList)} />
      <Route path="/admin/tenants/:id" component={requireAdmin(AdminTenantDetail)} />
      <Route path="/admin/relays" component={requireAdmin(AdminRelayList)} />
      <Route path="/admin/relays/:id" component={requireAdmin(AdminRelayDetail)} />
      <Route path="/admin/relays/:id/edit" component={requireAdmin(AdminRelayEdit)} />
    </Router>
  )
}
