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
              <p class="text-sm text-gray-700">Current: <span class="font-medium uppercase tracking-wide">tenant</span></p>
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
