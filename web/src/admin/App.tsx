import { Navigate, Router, Route } from "@solidjs/router"
import Layout from "@/admin/components/Layout"
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
      <Route path="/admin/tenants" component={AdminTenantList} />
      <Route path="/admin/tenants/:id" component={AdminTenantDetail} />
      <Route path="/admin/relays" component={AdminRelayList} />
      <Route path="/admin/relays/:id" component={AdminRelayDetail} />
      <Route path="/admin/relays/:id/edit" component={() => <RelayEdit basePath="/admin/relays" title="Edit Relay (Admin)" />} />
      <Route path="/admin/invoices" component={AdminInvoiceList} />
      <Route path="/admin/invoices/:id" component={AdminInvoiceDetail} />
      <Route path="/admin/moderation" component={AdminModeration} />
    </Router>
  )
}
