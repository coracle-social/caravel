import { useParams, A } from "@solidjs/router"
import { createResource, For, Show } from "solid-js"
import { adminGetTenant } from "../../lib/api"
import BackLink from "../../components/BackLink"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"
import useMinLoading from "../../components/useMinLoading"

export default function AdminTenantDetail() {
  const params = useParams()
  const tenantId = () => params.id ?? ""
  const [detail] = createResource(tenantId, adminGetTenant)
  const loading = useMinLoading(() => detail.loading)

  return (
    <PageContainer>
      <BackLink href="/admin/tenants" label="Tenants" />
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Tenant {params.id}</h1>

      <ResourceState
        loading={loading()}
        error={detail.error}
        loadingText="Loading tenant..."
        errorText="Failed to load tenant."
        class="mb-4"
      />

      <Show when={!loading()}>
        <div class="space-y-6">
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Status</h2>
            <Show when={detail()}>
              {(d) => (
                <div class="space-y-3">
                  <p class="text-sm text-gray-700">
                    Current: <span class="font-medium uppercase tracking-wide">tenant</span>
                  </p>
                </div>
              )}
            </Show>
          </section>

          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Relays</h2>
            <Show when={(detail()?.relays.length ?? 0) > 0} fallback={<p class="text-gray-500">No relays.</p>}>
              <ul class="space-y-3">
                <For each={detail()?.relays ?? []}>
                  {(relay) => (
                    <li>
                      <A href={`/admin/relays/${relay.id}`} class="block rounded-lg border border-gray-200 p-3 hover:border-gray-300">
                        <div class="flex items-center justify-between gap-2">
                          <div>
                            <p class="font-medium text-gray-900">{relay.info_name || relay.subdomain}</p>
                            <p class="text-xs text-gray-500">{relay.subdomain}.spaces.coracle.social</p>
                          </div>
                          <span class="text-xs uppercase tracking-wide text-gray-500">{relay.status}</span>
                        </div>
                      </A>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </section>
        </div>
      </Show>
    </PageContainer>
  )
}
