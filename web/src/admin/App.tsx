import { Navigate, Router, Route } from "@solidjs/router"
import { ADMIN_BASE } from "@/shared/lib/config"
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
    <Router root={Layout} base={ADMIN_BASE}>
      <Route path="/" component={() => <Navigate href="/tenants" />} />
      <Route path="/tenants" component={AdminTenantList} />
      <Route path="/tenants/:id" component={AdminTenantDetail} />
      <Route path="/relays" component={AdminRelayList} />
      <Route path="/relays/:id" component={AdminRelayDetail} />
      <Route path="/relays/:id/edit" component={() => <RelayEdit basePath="/relays" title="Edit Relay (Admin)" />} />
      <Route path="/invoices" component={AdminInvoiceList} />
      <Route path="/invoices/:id" component={AdminInvoiceDetail} />
      <Route path="/moderation" component={AdminModeration} />
    </Router>
  )
}
