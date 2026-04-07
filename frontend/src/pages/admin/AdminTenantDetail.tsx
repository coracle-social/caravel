import { useParams } from "@solidjs/router"
import { For, Show } from "solid-js"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import RelayListItem from "@/components/RelayListItem"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/components/useMinLoading"
import { useAdminTenant, useAdminTenantRelays } from "@/lib/hooks"

export default function AdminTenantDetail() {
  const params = useParams()
  const tenantId = () => params.id ?? ""
  const [tenant] = useAdminTenant(tenantId)
  const [relays] = useAdminTenantRelays(tenantId)
  const loading = useMinLoading(() => tenant.loading || relays.loading)

  const pastDueLabel = () => {
    const ts = tenant()?.past_due_at
    if (!ts) return null
    return new Date(ts * 1000).toLocaleString()
  }

  return (
    <PageContainer>
      <BackLink href="/admin/tenants" label="Tenants" />
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Tenant {params.id}</h1>
      <ResourceState loading={loading()} error={tenant.error || relays.error} loadingText="Loading tenant..." errorText="Failed to load tenant." class="mb-4" />
      <Show when={!loading()}>
        <div class="space-y-6">
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Status</h2>
            <Show when={tenant()}>
              {(t) => (
                <dl class="grid gap-y-3 text-sm">
                  <div class="flex gap-2">
                    <dt class="text-gray-500">Status:</dt>
                    <dd class="font-medium uppercase tracking-wide">{t().past_due_at ? "past due" : "active"}</dd>
                  </div>
                  <Show when={t().stripe_customer_id}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Stripe Customer:</dt>
                      <dd class="font-mono text-xs">{t().stripe_customer_id}</dd>
                    </div>
                  </Show>
                  <Show when={pastDueLabel()}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Past Due Since:</dt>
                      <dd class="text-red-600 font-medium">{pastDueLabel()}</dd>
                    </div>
                  </Show>
                  <Show when={t().nwc_error}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">NWC Error:</dt>
                      <dd class="text-red-600">{t().nwc_error}</dd>
                    </div>
                  </Show>
                </dl>
              )}
            </Show>
          </section>
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Relays</h2>
            <Show when={(relays()?.length ?? 0) > 0} fallback={<p class="text-gray-500">No relays.</p>}>
              <ul class="space-y-3">
                <For each={relays() ?? []}>
                  {(relay) => <RelayListItem relay={relay} href={`/admin/relays/${relay.id}`} />}
                </For>
              </ul>
            </Show>
          </section>
        </div>
      </Show>
    </PageContainer>
  )
}
