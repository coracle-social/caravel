import { useParams } from "@solidjs/router"
import { For, Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import RelayListItem from "@/components/RelayListItem"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/lib/useMinLoading"
import { useAdminTenant, useAdminTenantRelays, useProfileMetadata } from "@/lib/hooks"
import { shortenPubkey } from "@/lib/pubkey"

export default function AdminTenantDetail() {
  const params = useParams()
  const tenantId = () => params.id ?? ""
  const [tenant] = useAdminTenant(tenantId)
  const [relays] = useAdminTenantRelays(tenantId)
  const loading = useMinLoading(() => tenant.loading || relays.loading)
  const metadata = useProfileMetadata(tenantId)

  const churnedLabel = () => {
    const ts = tenant()?.churned_at
    if (!ts) return null
    return new Date(ts * 1000).toLocaleString()
  }

  return (
    <PageContainer>
      <BackLink href="/admin/tenants" label="Tenants" />
      <div class="flex items-center gap-4 mb-6 py-2">
        <Show
          when={getProfilePicture(metadata())}
          fallback={
            <div class="h-14 w-14 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-gray-500 text-lg">
              {((metadata()?.name || metadata()?.display_name) || tenantId()).slice(0, 1).toUpperCase()}
            </div>
          }
        >
          <img src={getProfilePicture(metadata())} alt="Profile" class="h-14 w-14 flex-shrink-0 rounded-full object-cover" />
        </Show>
        <div class="min-w-0">
          <h1 class="text-2xl font-bold text-gray-900 truncate">{(metadata()?.name || metadata()?.display_name) || shortenPubkey(tenantId())}</h1>
          <p class="text-xs text-gray-500 break-all">{tenantId()}</p>
        </div>
      </div>
      <ResourceState loading={loading()} error={tenant.error || relays.error} loadingText="Loading tenant..." errorText="Failed to load tenant." class="mb-4" />
      <Show when={!loading()}>
        <div class="space-y-6">
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Status</h2>
            <Show when={tenant()}>
              {(t) => (
                <dl class="grid gap-y-3 text-sm">
                  <div class="flex gap-2 items-center">
                    <dt class="text-gray-500">Status:</dt>
                    <dd>
                      <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${t().churned_at ? "bg-red-50 text-red-700 border-red-200" : "bg-green-50 text-green-700 border-green-200"}`}>
                        {t().churned_at ? "delinquent" : "active"}
                      </span>
                    </dd>
                  </div>
                  <Show when={t().stripe_customer_id}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Stripe Customer:</dt>
                      <dd class="font-mono">{t().stripe_customer_id}</dd>
                    </div>
                  </Show>
                  <Show when={churnedLabel()}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Delinquent Since:</dt>
                      <dd class="text-red-600 font-medium">{churnedLabel()}</dd>
                    </div>
                  </Show>
                  <Show when={t().nwc_error}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">NWC Error:</dt>
                      <dd class="text-red-600">{t().nwc_error}</dd>
                    </div>
                  </Show>
                  <Show when={t().stripe_error}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Stripe Error:</dt>
                      <dd class="text-red-600">{t().stripe_error}</dd>
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
