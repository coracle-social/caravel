import { Navigate, Router, Route } from "@solidjs/router"
import Layout, { requireAdmin } from "@/shared/components/Layout"
import AdminTenantList from "@/admin/pages/AdminTenantList"
import AdminTenantDetail from "@/admin/pages/AdminTenantDetail"
import AdminRelayList from "@/admin/pages/AdminRelayList"
import AdminRelayDetail from "@/admin/pages/AdminRelayDetail"
import AdminRelayEdit from "@/admin/pages/AdminRelayEdit"
import AdminInvoiceList from "@/admin/pages/AdminInvoiceList"
import AdminInvoiceDetail from "@/admin/pages/AdminInvoiceDetail"
import AdminModeration from "@/admin/pages/AdminModeration"
import { account, eventStore, pool } from "@/shared/lib/state"
import { NostrProvider } from "@/shared/lib/nostr"

export default function App() {
  return (
    <NostrProvider value={{ account, eventStore, pool }}>
      <Router root={Layout}>
        <Route path="/admin" component={() => <Navigate href="/admin/tenants" />} />
        <Route path="/admin/tenants" component={requireAdmin(AdminTenantList)} />
        <Route path="/admin/tenants/:id" component={requireAdmin(AdminTenantDetail)} />
        <Route path="/admin/relays" component={requireAdmin(AdminRelayList)} />
        <Route path="/admin/relays/:id" component={requireAdmin(AdminRelayDetail)} />
        <Route path="/admin/relays/:id/edit" component={requireAdmin(AdminRelayEdit)} />
        <Route path="/admin/invoices" component={requireAdmin(AdminInvoiceList)} />
        <Route path="/admin/invoices/:id" component={requireAdmin(AdminInvoiceDetail)} />
        <Route path="/admin/moderation" component={requireAdmin(AdminModeration)} />
      </Router>
    </NostrProvider>
  )
}
