import { Router, Route } from "@solidjs/router"
import Layout, { requireTenant } from "@/shared/components/Layout"
import Home from "@/tenant/pages/Home"
import RelayList from "@/tenant/pages/relays/RelayList"
import RelayNew from "@/tenant/pages/relays/RelayNew"
import RelayDetail from "@/tenant/pages/relays/RelayDetail"
import RelayEdit from "@/tenant/pages/relays/RelayEdit"
import Account from "@/tenant/pages/Account"
import { account, eventStore, pool } from "@/shared/lib/state"
import { NostrProvider } from "@/shared/lib/nostr"

export default function App() {
  return (
    <NostrProvider value={{ account, eventStore, pool }}>
      <Router root={Layout}>
        <Route path="/" component={Home} />
        <Route path="/relays" component={requireTenant(RelayList)} />
        <Route path="/relays/new" component={requireTenant(RelayNew)} />
        <Route path="/relays/:id" component={requireTenant(RelayDetail)} />
        <Route path="/relays/:id/edit" component={requireTenant(RelayEdit)} />
        <Route path="/account" component={requireTenant(Account)} />
      </Router>
    </NostrProvider>
  )
}
