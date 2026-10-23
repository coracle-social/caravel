import { createEffect, Show, type Component, type JSX } from "solid-js"
import { useLocation, useNavigate } from "@solidjs/router"
import Toast from "@/shared/components/Toast"
import TenantShell from "@/tenant/components/TenantShell"
import { identity } from "@/shared/lib/state"

// The tenant dashboard's frame: its navigation on the signed-in pages, and the toast.
export default function Layout(props: { children?: JSX.Element }) {
  const location = useLocation()

  const usesShell = () => /^\/(relays|account)/.test(location.pathname)

  return (
    <div class="min-h-screen bg-gray-50">
      <Show when={!identity.loading && identity() && usesShell()} fallback={<main>{props.children}</main>}>
        <TenantShell>{props.children}</TenantShell>
      </Show>
      <Toast />
    </div>
  )
}

// Its children, shown only to a signed-in tenant, sending everyone else to the home page.
export function TenantGate(props: { children: JSX.Element }) {
  const navigate = useNavigate()

  createEffect(() => {
    if (!identity.loading && !identity()) navigate("/", { replace: true })
  })

  return <Show when={!identity.loading && identity()}>{props.children}</Show>
}

// A page shown only to a signed-in tenant.
export const requireTenant = (Page: Component): Component => () => (
  <TenantGate>
    <Page />
  </TenantGate>
)
