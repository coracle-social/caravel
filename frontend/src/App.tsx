import { createEffect, createResource, Show } from "solid-js"
import { Router, Route, useLocation, useNavigate } from "@solidjs/router"
import type { Component } from "solid-js"
import Navbar from "./components/Navbar"
import Home from "./pages/Home"
import Login from "./pages/Login"
import RelayList from "./pages/relays/RelayList"
import RelayNew from "./pages/relays/RelayNew"
import RelayDetail from "./pages/relays/RelayDetail"
import RelayEdit from "./pages/relays/RelayEdit"
import Account from "./pages/Account"
import AdminTenants from "./pages/admin/AdminTenants"
import AdminTenantDetail from "./pages/admin/AdminTenantDetail"
import AdminRelays from "./pages/admin/AdminRelays"
import AdminRelayDetail from "./pages/admin/AdminRelayDetail"
import AdminRelayEdit from "./pages/admin/AdminRelayEdit"
import { useActiveAccount } from "./lib/nostr"
import { adminCheck as fetchAdminCheck } from "./lib/api"

function Layout(props: { children?: any }) {
  const location = useLocation()

  createEffect(() => {
    // Reinitialize Preline components on route change
    location.pathname
    window.HSStaticMethods?.autoInit()
  })

  return (
    <div class="min-h-screen bg-gray-50">
      <Navbar />
      <main>{props.children}</main>
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
      const [adminCheck] = createResource(
        () => account()?.id,
        () => fetchAdminCheck(),
      )

      createEffect(() => {
        if (!account()) navigate("/login", { replace: true })
      })

      return (
        <Show when={account()}>
          <Show when={!adminCheck.loading} fallback={<div class="max-w-4xl mx-auto px-4 py-8 text-gray-500">Checking admin access...</div>}>
            <Show
              when={!adminCheck.error && !!adminCheck()?.is_admin}
              fallback={<div class="max-w-4xl mx-auto px-4 py-8 text-red-600">You do not have admin access.</div>}
            >
              <Page />
            </Show>
          </Show>
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
      <Route path="/admin/tenants" component={withAdminAuth(AdminTenants)} />
      <Route path="/admin/tenants/:id" component={withAdminAuth(AdminTenantDetail)} />
      <Route path="/admin/relays" component={withAdminAuth(AdminRelays)} />
      <Route path="/admin/relays/:id" component={withAdminAuth(AdminRelayDetail)} />
      <Route path="/admin/relays/:id/edit" component={withAdminAuth(AdminRelayEdit)} />
    </Router>
  )
}
