import { Router, Route } from "@solidjs/router"
import { TENANT_BASE } from "@/shared/lib/config"
import Layout, { requireTenant, TenantGate } from "@/tenant/components/Layout"
import RelayEdit from "@/shared/views/RelayEdit"
import Home from "@/tenant/pages/Home"
import RelayList from "@/tenant/pages/relays/RelayList"
import RelayNew from "@/tenant/pages/relays/RelayNew"
import RelayDetail from "@/tenant/pages/relays/RelayDetail"
import RelayMembers from "@/tenant/pages/relays/RelayMembers"
import RelayRooms from "@/tenant/pages/relays/RelayRooms"
import RelayRoom from "@/tenant/pages/relays/RelayRoom"
import RelayReports from "@/tenant/pages/relays/RelayReports"
import RelayContent from "@/tenant/pages/relays/RelayContent"
import RelayBans from "@/tenant/pages/relays/RelayBans"
import RelayFrame from "@/tenant/components/RelayFrame"
import Account from "@/tenant/pages/Account"

export default function App() {
  return (
    <Router root={Layout} base={TENANT_BASE}>
      <Route path="/" component={Home} />
      <Route path="/relays" component={requireTenant(RelayList)} />
      <Route path="/relays/new" component={requireTenant(RelayNew)} />
      <Route path="/relays/:id/edit" component={requireTenant(RelayEdit)} />
      <Route
        path="/relays/:id"
        component={(props) => (
          <TenantGate>
            <RelayFrame {...props} />
          </TenantGate>
        )}
      >
        <Route path="/" component={RelayDetail} />
        <Route path="/members" component={RelayMembers} />
        <Route path="/rooms" component={RelayRooms} />
        <Route path="/rooms/:room" component={RelayRoom} />
        <Route path="/reports" component={RelayReports} />
        <Route path="/content" component={RelayContent} />
        <Route path="/bans" component={RelayBans} />
      </Route>
      <Route path="/account" component={requireTenant(Account)} />
    </Router>
  )
}
