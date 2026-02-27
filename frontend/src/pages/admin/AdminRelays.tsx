import { A } from "@solidjs/router"
import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import { adminListRelays } from "../../lib/api"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"

export default function AdminRelays() {
  const [query, setQuery] = createSignal("")
  const [relays] = createResource(adminListRelays)

  const filtered = createMemo(() => {
    const q = query().trim().toLowerCase()
    return (relays() ?? []).filter((relay) => {
      if (!q) return true
      return (
        relay.name.toLowerCase().includes(q)
        || relay.subdomain.toLowerCase().includes(q)
        || relay.tenant.toLowerCase().includes(q)
      )
    })
  })

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6">All Relays</h1>
      <input
        type="search"
        value={query()}
        onInput={(e) => setQuery(e.currentTarget.value)}
        placeholder="Search relays..."
        class="w-full border border-gray-300 rounded-lg px-3 py-2 mb-6 focus:outline-none focus:ring-2 focus:ring-blue-500"
      />

      <ResourceState
        loading={relays.loading}
        error={relays.error}
        loadingText="Loading relays..."
        errorText="Failed to load relays."
      />
      <Show when={(filtered().length ?? 0) > 0} fallback={<p class="text-gray-500">No relays found.</p>}>
        <ul class="space-y-3">
          <For each={filtered()}>
            {(relay) => (
              <li>
                <A href={`/admin/relays/${relay.id}`} class="block border border-gray-200 rounded-lg p-4 bg-white hover:border-gray-300">
                  <div class="flex items-center justify-between gap-3">
                    <div>
                      <p class="font-medium text-gray-900">{relay.name}</p>
                      <p class="text-xs text-gray-500">{relay.subdomain}.spaces.coracle.social</p>
                      <p class="text-xs text-gray-500 break-all mt-1">Tenant: {relay.tenant}</p>
                    </div>
                    <p class="text-xs uppercase tracking-wide text-gray-500">{relay.status}</p>
                  </div>
                </A>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </PageContainer>
  )
}
