import { createEffect, Show } from "solid-js"
import { Router, Route, useLocation, useNavigate } from "@solidjs/router"
import type { Component } from "solid-js"
import AppShell from "./components/AppShell"
import Home from "./pages/Home"
import Login from "./pages/Login"
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
import { useActiveAccount } from "./lib/nostr"

function Layout(props: { children?: any }) {
  const location = useLocation()
  const account = useActiveAccount()

  const usesAppShell = () => {
    const path = location.pathname
    return path.startsWith("/relays") || path.startsWith("/account") || path.startsWith("/admin")
  }

  createEffect(() => {
    // Reinitialize Preline components on route change
    location.pathname
    window.HSStaticMethods?.autoInit()
  })

  return (
    <div class="min-h-screen bg-gray-50">
      <Show when={account() && usesAppShell()} fallback={<main>{props.children}</main>}>
        <AppShell>{props.children}</AppShell>
      </Show>
    </div>
  )
}

export default function App() {
  const withTenantAuth = (Page: Component): Component => {
    return () => {
      const navigate = useNavigate()
      const account = useActiveAccount()

      createEffect(() => {
        if (!account()) navigate("/login", { replace: true })
      })

      return <Show when={account()}><Page /></Show>
    }
  }

  const withAdminAuth = (Page: Component): Component => {
    return () => {
      const navigate = useNavigate()
      const account = useActiveAccount()

      createEffect(() => {
        if (!account()) navigate("/login", { replace: true })
      })

      return (
        <Show when={account()}>
          <Page />
        </Show>
      )
    }
  }

  return (
    <Router root={Layout}>
      <Route path="/" component={Home} />
      <Route path="/login" component={Login} />
      <Route path="/relays" component={withTenantAuth(RelayList)} />
      <Route path="/relays/new" component={withTenantAuth(RelayNew)} />
      <Route path="/relays/:id" component={withTenantAuth(RelayDetail)} />
      <Route path="/relays/:id/edit" component={withTenantAuth(RelayEdit)} />
      <Route path="/account" component={withTenantAuth(Account)} />
      <Route path="/admin/tenants" component={withAdminAuth(AdminTenantList)} />
      <Route path="/admin/tenants/:id" component={withAdminAuth(AdminTenantDetail)} />
      <Route path="/admin/relays" component={withAdminAuth(AdminRelayList)} />
      <Route path="/admin/relays/:id" component={withAdminAuth(AdminRelayDetail)} />
      <Route path="/admin/relays/:id/edit" component={withAdminAuth(AdminRelayEdit)} />
    </Router>
  )
}
