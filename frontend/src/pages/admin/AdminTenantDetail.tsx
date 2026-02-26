import { useParams, A } from "@solidjs/router"
import { createResource, createSignal, For, Show } from "solid-js"
import { adminGetTenant, adminUpdateTenantStatus } from "../../lib/api"

export default function AdminTenantDetail() {
  const params = useParams()
  const tenantId = () => params.id ?? ""
  const [detail, { refetch }] = createResource(tenantId, adminGetTenant)
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")

  async function setStatus(status: string) {
    if (busy()) return
    setBusy(true)
    setError("")
    try {
      await adminUpdateTenantStatus(tenantId(), status)
      await refetch()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update tenant")
    } finally {
      setBusy(false)
    }
  }

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <div class="flex items-center gap-2 mb-6">
        <A href="/admin/tenants" class="text-gray-500 hover:text-gray-700">← Tenants</A>
      </div>
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Tenant {params.id}</h1>

      <Show when={detail.loading}>
        <p class="text-gray-500 mb-4">Loading tenant...</p>
      </Show>
      <Show when={detail.error && !detail.loading}>
        <p class="text-red-600 mb-4">Failed to load tenant.</p>
      </Show>

      <div class="space-y-6">
        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold mb-4">Status</h2>
          <Show when={detail()}>
            {(d) => (
              <div class="space-y-3">
                <p class="text-sm text-gray-700">
                  Current: <span class="font-medium uppercase tracking-wide">{d().tenant.status}</span>
                </p>
                <div class="flex gap-2">
                  <button
                    class="py-2 px-4 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 disabled:opacity-50"
                    onClick={() => void setStatus("active")}
                    disabled={busy()}
                  >
                    Activate
                  </button>
                  <button
                    class="py-2 px-4 border border-red-300 text-red-600 rounded-lg hover:bg-red-50 disabled:opacity-50"
                    onClick={() => void setStatus("deactivated")}
                    disabled={busy()}
                  >
                    Deactivate
                  </button>
                </div>
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
                          <p class="font-medium text-gray-900">{relay.name}</p>
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
        <Show when={error()}>
          <p class="text-sm text-red-600">{error()}</p>
        </Show>
      </div>
    </div>
  )
}
