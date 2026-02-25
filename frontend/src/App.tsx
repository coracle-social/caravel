import { createEffect } from "solid-js"
import { Router, Route, useLocation } from "@solidjs/router"
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
  return (
    <Router root={Layout}>
      <Route path="/" component={Home} />
      <Route path="/login" component={Login} />
      <Route path="/relays" component={RelayList} />
      <Route path="/relays/new" component={RelayNew} />
      <Route path="/relays/:id" component={RelayDetail} />
      <Route path="/relays/:id/edit" component={RelayEdit} />
      <Route path="/account" component={Account} />
      <Route path="/admin/tenants" component={AdminTenants} />
      <Route path="/admin/tenants/:id" component={AdminTenantDetail} />
      <Route path="/admin/relays" component={AdminRelays} />
      <Route path="/admin/relays/:id" component={AdminRelayDetail} />
      <Route path="/admin/relays/:id/edit" component={AdminRelayEdit} />
    </Router>
  )
}
