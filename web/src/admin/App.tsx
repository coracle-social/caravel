import { Navigate, Router, Route } from "@solidjs/router"
import Layout, { requireAdmin } from "@/shared/components/Layout"
import RelayEdit from "@/shared/views/RelayEdit"
import AdminTenantList from "@/admin/pages/AdminTenantList"
import AdminTenantDetail from "@/admin/pages/AdminTenantDetail"
import AdminRelayList from "@/admin/pages/AdminRelayList"
import AdminRelayDetail from "@/admin/pages/AdminRelayDetail"
import AdminInvoiceList from "@/admin/pages/AdminInvoiceList"
import AdminInvoiceDetail from "@/admin/pages/AdminInvoiceDetail"
import AdminModeration from "@/admin/pages/AdminModeration"

export default function App() {
  return (
    <Router root={Layout}>
      <Route path="/admin" component={() => <Navigate href="/admin/tenants" />} />
      <Route path="/admin/tenants" component={requireAdmin(AdminTenantList)} />
      <Route path="/admin/tenants/:id" component={requireAdmin(AdminTenantDetail)} />
      <Route path="/admin/relays" component={requireAdmin(AdminRelayList)} />
      <Route path="/admin/relays/:id" component={requireAdmin(AdminRelayDetail)} />
      <Route path="/admin/relays/:id/edit" component={requireAdmin(() => <RelayEdit basePath="/admin/relays" title="Edit Relay (Admin)" />)} />
      <Route path="/admin/invoices" component={requireAdmin(AdminInvoiceList)} />
      <Route path="/admin/invoices/:id" component={requireAdmin(AdminInvoiceDetail)} />
      <Route path="/admin/moderation" component={requireAdmin(AdminModeration)} />
    </Router>
  )
}
