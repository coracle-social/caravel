import { A } from "@solidjs/router"
import Fuse from "fuse.js"
import { createMemo, createSignal, For, Show } from "solid-js"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"
import useMinLoading from "../../components/useMinLoading"
import { useAdminRelays } from "../../lib/hooks"

export default function AdminRelayList() {
  const [query, setQuery] = createSignal("")
  const [relays] = useAdminRelays()
  const loading = useMinLoading(() => relays.loading)

  const filtered = createMemo(() => {
    const list = relays() ?? []
    const q = query().trim()

    if (!q) return list

    return new Fuse(list, {
      keys: ["info_name", "subdomain", "tenant"],
      threshold: 0.35,
      ignoreLocation: true,
    }).search(q).map((result) => result.item)
  })

  return (
    <PageContainer>
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-gray-900 py-2">Relays</h1>
      </div>
      <div class="relative mb-6">
        <span class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-gray-400">
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="8" />
            <path d="M21 21l-4.3-4.3" />
          </svg>
        </span>
        <input
          type="search"
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          placeholder="Search relays..."
          class="w-full border border-gray-300 rounded-lg py-2 pl-10 pr-3 focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      </div>

      <ResourceState
        loading={loading()}
        error={relays.error}
        loadingText="Loading relays..."
        errorText="Failed to load relays."
      />
      <Show when={!loading()}>
        <Show when={(filtered().length ?? 0) > 0} fallback={<p class="py-20 text-center text-gray-500">No relays found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(relay) => (
                <li>
                  <A href={`/admin/relays/${relay.id}`} class="block border border-gray-200 rounded-lg p-4 bg-white hover:border-gray-300">
                    <div class="flex items-center justify-between gap-3">
                      <div>
                        <p class="font-medium text-gray-900">{relay.info_name || relay.subdomain}</p>
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
      </Show>
    </PageContainer>
  )
}
