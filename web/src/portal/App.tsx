import { Route, Router } from "@solidjs/router"
import AccountPage from "@/portal/pages/AccountPage"
import InvitePage from "@/portal/pages/InvitePage"
import JoinPage from "@/portal/pages/JoinPage"
import NotFound from "@/portal/pages/NotFound"
import RelayPage from "@/portal/pages/RelayPage"
import { PORTAL_BASE } from "@/portal/lib/relay"

export default function App() {
  return (
    <Router base={PORTAL_BASE}>
      <Route path="/" component={RelayPage} />
      <Route path="/join" component={JoinPage} />
      <Route path="/invites/:code" component={InvitePage} />
      <Route path="/account" component={AccountPage} />
      <Route path="*" component={NotFound} />
    </Router>
  )
}
